//! The lowered runtime's driver: a view, its world, its surfaces, its input state.
use super::canvas::{Canvas, Images};
use super::provenance::count;
use super::text::TextSys;
use super::{Core, Doc, Hnd, Kind, Lower, NO, Owner, Scene};
use crate::app::{Config, View};
use crate::elem::Element;
use crate::input::{Button, Ev, Key, Mods};
use bough_x::expr::U;
use bough_x::prim::Prim;
use bough_x::sink::Sink;
use bough_x::store::{Rect, Space, Tiles};
use bough_x::world::World;
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The view as the world sees it: the façade view, and the scene its last description came to. Describing it
/// to a sink replays the scene's segments as scopes of free rows.
pub struct Lowered<V> {
    pub v: V,
    scene: RefCell<Scene>,
    /// The most segments any description had: a later description with fewer still names the rest (empty),
    /// so their rows are released.
    segs: Cell<usize>,
}
static NO_BLOCK: U = U { w: [0; 64] };
const NOWHERE: Space = Space {
    clip: Rect {
        x0: 0,
        y0: 0,
        x1: 0,
        y1: 0,
    },
    base: (0, 0),
    scroll: bough_x::store::NO_WORD,
};

impl<V: View> bough_x::sink::View for Lowered<V> {
    fn block(&self) -> &U {
        &NO_BLOCK
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        let scene = self.scene.borrow();
        let n = scene.segs.len().max(self.segs.get());
        self.segs.set(n);
        for k in 0..n {
            match scene.segs.get(k) {
                Some(g) => s.scope(g.space, |s| s.free(g.prims.len() as u32, |i| g.prims[i])),
                None => s.scope(NOWHERE, |s| {
                    s.free(0, |_| (Rect::default(), Prim::default()))
                }),
            }
        }
    }
}

/// Count bands the gates assert exactly.
#[derive(Clone, Copy, Default, Debug)]
pub struct Stats {
    pub describes: u64,
    /// One-record patches (hover paints) that needed no description.
    pub patches: u64,
    pub frames: u64,
    /// Frames that found nothing damaged.
    pub idle_frames: u64,
    pub tiles_repaired: u64,
    /// With `profile` set: thread instructions spent in the view and its lowering, in layout and emission, and
    /// in the world's compare pass (where a turn's cost is).
    pub profile: bool,
    pub stage: [u64; 3],
}

struct Held {
    doc: u32,
    node: u32,
    at: (f32, f32),
}

pub struct App<V: View> {
    lv: Lowered<V>,
    world: World<Lowered<V>, Canvas>,
    text: Rc<RefCell<TextSys>>,
    images: Rc<RefCell<Images>>,
    docs: Vec<Doc>,
    handlers: Vec<Hnd>,
    owners: Vec<Owner>,
    refused: Vec<&'static str>,
    lists: super::list::Lists,
    lay: super::LayMemo,
    size_pt: (f32, f32),
    scale: f32,
    /// The surfaces, each with the damage it has not repaired yet.
    ring: Vec<(Canvas, Tiles)>,
    next: usize,
    shown: usize,
    ptr: Option<(f32, f32)>,
    hover: Vec<(u32, u32)>,
    held: Option<Held>,
    focus: Option<(u32, u32)>,
    pub stats: Stats,
}

fn resolve<'x>(owners: &[Owner], id: u32, root: &'x mut dyn Any) -> Option<&'x mut dyn Any> {
    if id == 0 {
        return Some(root);
    }
    let o = &owners[id as usize - 1];
    let p = resolve(owners, o.parent, root)?;
    (o.project)(p)
}

impl<V: View> App<V> {
    /// A window of `cfg.width_pt` x `cfg.height_pt` points at `cfg.scale`, with `cfg.font` as the UI face and
    /// `cfg.ring` surfaces (0: four). The view is described and mounted; nothing is painted until `frame`.
    pub fn new(view: V, cfg: Config) -> App<V> {
        let (w, h) = (
            (cfg.width_pt * cfg.scale).round() as i32,
            (cfg.height_pt * cfg.scale).round() as i32,
        );
        let text = Rc::new(RefCell::new(TextSys::new(&cfg.font)));
        let images = Rc::new(RefCell::new(Images::default()));
        let mut world = World::<Lowered<V>, Canvas>::new(w, h, 0);
        world.set_scale(cfg.scale);
        world.tick(1);
        let n = if cfg.ring == 0 { 4 } else { cfg.ring };
        let mut app = App {
            lv: Lowered {
                v: view,
                scene: RefCell::new(Scene::default()),
                segs: Cell::new(0),
            },
            world,
            text,
            images,
            docs: Vec::new(),
            handlers: Vec::new(),
            owners: Vec::new(),
            refused: Vec::new(),
            lists: Default::default(),
            lay: Default::default(),
            size_pt: (cfg.width_pt, cfg.height_pt),
            scale: cfg.scale,
            ring: Vec::new(),
            next: 0,
            shown: 0,
            ptr: None,
            hover: Vec::new(),
            held: None,
            focus: None,
            stats: Stats::default(),
        };
        app.surfaces(n);
        app.describe();
        app.world.mount(&app.lv);
        count(|c| c.world_mount += 1);
        app
    }
    fn px(&self) -> (i32, i32) {
        (
            (self.size_pt.0 * self.scale).round() as i32,
            (self.size_pt.1 * self.scale).round() as i32,
        )
    }
    fn surfaces(&mut self, n: usize) {
        let (w, h) = self.px();
        self.ring = (0..n)
            .map(|_| {
                let mut t = Tiles::new(w, h);
                t.mark_all();
                (Canvas::new(w, h, self.text.clone(), self.images.clone()), t)
            })
            .collect();
        self.next = 0;
        self.shown = 0;
    }
    /// Run the view and lay it out: the scene, the handlers, the hit list. No world pass.
    ///
    /// A virtual list places its rows from heights measured by earlier descriptions. If this one measured a
    /// row at another height than it assumed, it is run again with the measurement (at most four times: each
    /// round can only add measurements of rows the round before exposed).
    fn describe(&mut self) {
        for round in 0.. {
            if !self.describe_once() || round == 3 {
                break;
            }
        }
        self.rehover();
    }
    /// One description. Returns whether a list row was measured at a height the description did not assume.
    fn describe_once(&mut self) -> bool {
        count(|c| c.describe += 1);
        self.stats.describes += 1;
        let (w, h) = self.px();
        let (mut text, mut images) = (self.text.borrow_mut(), self.images.borrow_mut());
        text.begin_pass();
        let mut core = Core::new(self.scale, (w, h), &self.lists, &mut text, &mut images);
        let t0 = if self.stats.profile {
            bough_sys::perf::thread_instructions()
        } else {
            0
        };
        View::view(&self.lv.v).lower(&mut Lower::new(&mut core));
        let t1 = if self.stats.profile {
            bough_sys::perf::thread_instructions()
        } else {
            0
        };
        assert!(
            core.refused.is_empty(),
            "the lowered runtime does not lower these elements yet: {:?}",
            core.refused
        );
        let scene = super::finish(&core, w, h, &mut self.lay);
        if self.stats.profile {
            self.stats.stage[0] += t1 - t0;
            self.stats.stage[1] += bough_sys::perf::thread_instructions() - t1;
        }
        let Core {
            docs,
            handlers,
            owners,
            refused,
            rows,
            ..
        } = core;
        let mut again = false;
        for r in rows {
            let rect = scene.rects[r.doc as usize][r.node as usize];
            let st = self.lists.entry(r.key).or_default();
            if st.h.len() <= r.slot as usize {
                st.h.resize(r.slot as usize + 1, 0);
            }
            let hgt = (rect[3] - rect[1]).max(1);
            again |= st.h[r.slot as usize] != hgt;
            st.h[r.slot as usize] = hgt;
        }
        (self.docs, self.handlers, self.owners, self.refused) = (docs, handlers, owners, refused);
        *self.lv.scene.borrow_mut() = scene;
        again
    }
    /// After a description: who is under the pointer now, and their hover paints into the scene.
    fn rehover(&mut self) {
        self.hover = match self.ptr {
            Some((x, y)) => self.chain(x, y),
            None => Vec::new(),
        };
        let mut scene = self.lv.scene.borrow_mut();
        for &(d, n) in &self.hover {
            if let Some(c) = self.docs[d as usize].nodes[n as usize].hover_bg {
                if let Some(h) = scene.hit(d, n) {
                    if let Some((s, i)) = h.prim {
                        scene.segs[s as usize].prims[i as usize].1.color = c;
                    }
                }
            }
        }
    }
    /// After handlers ran: the effect site, a description, the world's compare.
    fn settle(&mut self) {
        let pubs = crate::changed::Pubs::default();
        self.lv.v.changed(crate::changed::Changed {
            pubs: &pubs,
            readers: &[],
            _v: std::marker::PhantomData,
        });
        self.describe();
        let t0 = if self.stats.profile {
            bough_sys::perf::thread_instructions()
        } else {
            0
        };
        self.world.redescribe(&self.lv);
        if self.stats.profile {
            self.stats.stage[2] += bough_sys::perf::thread_instructions() - t0;
        }
    }
    /// Run `f` on the state, then bring the window up to date with it.
    pub fn turn(&mut self, f: impl FnOnce(&mut V)) {
        f(&mut self.lv.v);
        self.settle();
    }
    /// Read the state.
    pub fn view(&self) -> &V {
        &self.lv.v
    }

    // ---- frames ----------------------------------------------------------------------------------------------

    /// Present: the next surface of the ring is repaired where it is stale and becomes the shown one. Returns
    /// whether anything was repainted.
    pub fn frame(&mut self) -> bool {
        self.stats.frames += 1;
        let world = &mut self.world;
        if world.tiles.any() {
            for (_, pending) in &mut self.ring {
                world.tiles.each(|r| pending.mark(r));
            }
            world.tiles.clear();
        }
        let k = self.next;
        self.next = (k + 1) % self.ring.len();
        self.shown = k;
        let (canvas, pending) = &mut self.ring[k];
        if !pending.any() {
            self.stats.idle_frames += 1;
            return false;
        }
        self.stats.tiles_repaired += pending.count() as u64;
        count(|c| c.paint += 1);
        let world = &self.world;
        canvas.repair(pending, |c| world.paint(c));
        pending.clear();
        true
    }
    /// The shown surface's pixels (premultiplied RGBA8).
    pub fn presented_pixels(&self) -> Vec<u8> {
        self.ring[self.shown].0.bytes()
    }
    /// A full redraw of the stores into a new surface.
    pub fn reference_pixels(&self) -> Vec<u8> {
        let (w, h) = self.px();
        let mut c = Canvas::new(w, h, self.text.clone(), self.images.clone());
        self.world.paint(&mut c);
        c.bytes()
    }
    /// The scene painted directly, record by record, with no stores: the immediate-mode oracle.
    pub fn immediate_pixels(&self) -> Vec<u8> {
        let (w, h) = self.px();
        let mut c = Canvas::new(w, h, self.text.clone(), self.images.clone());
        for g in &self.lv.scene.borrow().segs {
            for (r, p) in &g.prims {
                c.one(r.shift(g.space.base), g.space.clip, p);
            }
        }
        c.bytes()
    }
    /// The same view state mounted into a new world and painted: the fresh-mount oracle.
    pub fn fresh_pixels(&self) -> Vec<u8> {
        let (w, h) = self.px();
        let mut world = World::<Lowered<V>, Canvas>::new(w, h, 0);
        world.set_scale(self.scale);
        world.tick(1);
        let segs = self.lv.segs.replace(0);
        world.mount(&self.lv);
        self.lv.segs.set(segs);
        let mut c = Canvas::new(w, h, self.text.clone(), self.images.clone());
        world.paint(&mut c);
        c.bytes()
    }
    /// Present a frame and hold it to the four oracles: the retained surface (repaired at this ring's age)
    /// equals a full redraw of the stores, equals the scene painted directly with no stores, equals the same
    /// state mounted into a new world. `Err` names the first pair that differs and by how many pixels.
    pub fn check(&mut self) -> Result<(), String> {
        self.frame();
        let diff = |a: &[u8], b: &[u8]| {
            a.chunks_exact(4)
                .zip(b.chunks_exact(4))
                .filter(|(x, y)| x != y)
                .count()
        };
        let (retained, full) = (self.presented_pixels(), self.reference_pixels());
        if retained != full {
            return Err(format!(
                "retained != full redraw ({} pixels, ring of {})",
                diff(&retained, &full),
                self.ring.len()
            ));
        }
        let now = self.immediate_pixels();
        if full != now {
            return Err(format!(
                "stores != immediate ({} pixels)",
                diff(&full, &now)
            ));
        }
        let fresh = self.fresh_pixels();
        if full != fresh {
            return Err(format!(
                "retained world != fresh mount ({} pixels)",
                diff(&full, &fresh)
            ));
        }
        Ok(())
    }
    /// The shown surface over white, as a PNG file (visual fixtures and debugging).
    pub fn write_png(&self, path: &std::path::Path) -> std::io::Result<()> {
        let (w, h) = self.px();
        let px = self.presented_pixels();
        let mut rgb = Vec::with_capacity(px.len() / 4 * 3);
        for p in px.chunks_exact(4) {
            let ia = 255 - p[3] as u32;
            rgb.extend(
                [p[0] as u32 + ia, p[1] as u32 + ia, p[2] as u32 + ia].map(|c| c.min(255) as u8),
            );
        }
        let mut enc = png::Encoder::new(
            std::io::BufWriter::new(std::fs::File::create(path)?),
            w as u32,
            h as u32,
        );
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().map_err(std::io::Error::other)?;
        wr.write_image_data(&rgb).map_err(std::io::Error::other)
    }
    /// Records the world holds, live ones (the O(exposed) count band).
    pub fn record_count(&self) -> usize {
        self.lv
            .scene
            .borrow()
            .segs
            .iter()
            .map(|g| g.prims.len())
            .sum()
    }
    /// Records, columns and bytes of the world's stores as the world counts them.
    pub fn world_stats(&self) -> (usize, usize, usize) {
        self.world.root().stats()
    }
    /// (mounts, in-place resizes) of the world.
    pub fn lifecycle(&self) -> (u32, u32) {
        self.world.lifecycle()
    }
    /// (documents laid out, documents whose layout was kept from the last description).
    pub fn layouts(&self) -> (u64, u64) {
        (self.lay.laid, self.lay.kept)
    }
    pub fn text_stats(&self) -> super::text::TextStats {
        self.text.borrow().stats
    }
    pub fn refused(&self) -> &[&'static str] {
        &self.refused
    }
    /// Layout boxes (points: x, y, w, h) of the nodes whose element type name contains `what`, in description
    /// order; and of every node the pointer stops at with `interactive`.
    pub fn boxes(&self, what: &str) -> Vec<[f32; 4]> {
        let scene = self.lv.scene.borrow();
        scene
            .hits
            .iter()
            .filter(|h| {
                self.docs[h.doc as usize].nodes[h.node as usize]
                    .source
                    .contains(what)
            })
            .map(|h| {
                [
                    h.rect.x0,
                    h.rect.y0,
                    h.rect.x1 - h.rect.x0,
                    h.rect.y1 - h.rect.y0,
                ]
                .map(|v| v as f32 / self.scale)
            })
            .collect()
    }
    /// The boxes of nodes that carry a handler (points).
    pub fn interactive(&self) -> Vec<[f32; 4]> {
        let scene = self.lv.scene.borrow();
        scene
            .hits
            .iter()
            .filter(|h| self.docs[h.doc as usize].nodes[h.node as usize].hnd.1 != 0)
            .map(|h| {
                [
                    h.rect.x0,
                    h.rect.y0,
                    h.rect.x1 - h.rect.x0,
                    h.rect.y1 - h.rect.y0,
                ]
                .map(|v| v as f32 / self.scale)
            })
            .collect()
    }
    /// The focused node's box, if any (points).
    pub fn focused(&self) -> Option<[f32; 4]> {
        let (d, n) = self.focus?;
        let scene = self.lv.scene.borrow();
        let h = scene.hit(d, n)?;
        Some(
            [
                h.rect.x0,
                h.rect.y0,
                h.rect.x1 - h.rect.x0,
                h.rect.y1 - h.rect.y0,
            ]
            .map(|v| v as f32 / self.scale),
        )
    }
    pub fn size_pt(&self) -> (f32, f32) {
        self.size_pt
    }

    // ---- resize ------------------------------------------------------------------------------------------------

    /// The window is `w` x `h` points now. The world, its focus, a held press and the view's scroll offsets are
    /// kept: the tree is described at the new size and compared with the columns, not mounted again.
    pub fn resize(&mut self, w_pt: f32, h_pt: f32) {
        self.size_pt = (w_pt, h_pt);
        let (w, h) = self.px();
        self.world.resize(w, h);
        let n = self.ring.len();
        self.surfaces(n);
        self.describe();
        self.world.redescribe(&self.lv);
    }

    // ---- input -------------------------------------------------------------------------------------------------

    /// The nodes under a point (points), innermost first, then its lexical ancestors.
    fn chain(&self, x: f32, y: f32) -> Vec<(u32, u32)> {
        let (px, py) = ((x * self.scale) as i32, (y * self.scale) as i32);
        let scene = self.lv.scene.borrow();
        let inside = |r: Rect| px >= r.x0 && px < r.x1 && py >= r.y0 && py < r.y1;
        let top = scene.hits.iter().rposition(|h| {
            inside(h.rect)
                && inside(h.clip)
                && self.docs[h.doc as usize].nodes[h.node as usize].solid
        });
        let mut out = Vec::new();
        let mut at = top.map_or(NO, |t| t as u32);
        while at != NO {
            let h = &scene.hits[at as usize];
            out.push((h.doc, h.node));
            at = h.parent;
        }
        out
    }
    /// Run the handlers of kind `kind` on node `at`; returns whether any ran.
    fn fire(&mut self, at: (u32, u32), kind: Kind, e: &Ev) -> bool {
        // a node of a description that has since been replaced by a smaller one (a press held across turns)
        if self.docs.get(at.0 as usize).is_none_or(|d| d.nodes.len() <= at.1 as usize) {
            return false;
        }
        let (a, n) = self.docs[at.0 as usize].nodes[at.1 as usize].hnd;
        let mut ran = false;
        for h in &self.handlers[a as usize..(a + n) as usize] {
            if h.kind == kind {
                if let Some(o) = resolve(&self.owners, h.owner, &mut self.lv.v) {
                    count(|c| c.dispatch += 1);
                    (h.f)(o, e);
                    ran = true;
                }
            }
        }
        ran
    }
    fn hears(&self, at: (u32, u32), kind: Kind) -> bool {
        let (a, n) = self.docs[at.0 as usize].nodes[at.1 as usize].hnd;
        self.handlers[a as usize..(a + n) as usize]
            .iter()
            .any(|h| h.kind == kind)
    }
    fn ev(&self, x: f32, y: f32) -> Ev {
        Ev {
            x,
            y,
            dx: 0.0,
            dy: 0.0,
            key: None,
            mods: Mods::default(),
            button: Button::Primary,
            on: true,
        }
    }
    pub fn pointer_move(&mut self, x: f32, y: f32) {
        self.ptr = Some((x, y));
        let now = self.chain(x, y);
        let mut ran = false;
        if let Some(h) = &self.held {
            let at = (h.doc, h.node);
            let e = Ev {
                dx: x - h.at.0,
                dy: y - h.at.1,
                ..self.ev(x, y)
            };
            ran |= self.fire(at, Kind::Drag, &e);
        }
        if now != self.hover {
            let was = std::mem::take(&mut self.hover);
            for &n in was.iter().filter(|n| !now.contains(n)) {
                ran |= self.fire(
                    n,
                    Kind::Hover,
                    &Ev {
                        on: false,
                        ..self.ev(x, y)
                    },
                );
            }
            for &n in now.iter().rev().filter(|n| !was.contains(n)) {
                ran |= self.fire(n, Kind::Hover, &self.ev(x, y));
            }
            if !ran {
                // only paints changed: one record each, no description
                self.hover_paint(&was, &now);
                self.hover = now;
                return;
            }
            self.hover = was;
        }
        if ran {
            self.settle();
        }
    }
    /// Swap the hover paints of the nodes that left and entered: each a one-record patch.
    fn hover_paint(&mut self, was: &[(u32, u32)], now: &[(u32, u32)]) {
        let mut scene = self.lv.scene.borrow_mut();
        let mut set =
            |at: (u32, u32), on: bool, world: &mut World<Lowered<V>, Canvas>, stats: &mut Stats| {
                let node = &self.docs[at.0 as usize].nodes[at.1 as usize];
                let (Some(hover), super::Paint::Box { color, .. }) = (node.hover_bg, &node.paint)
                else {
                    return;
                };
                let Some((s, i)) = scene
                    .hits
                    .iter()
                    .find(|h| (h.doc, h.node) == at)
                    .and_then(|h| h.prim)
                else {
                    return;
                };
                let p = &mut scene.segs[s as usize].prims[i as usize].1;
                p.color = if on { hover } else { *color };
                count(|c| c.patch += 1);
                stats.patches += 1;
                #[cfg(not(feature = "mutant_lower_hover"))]
                world.free_set(s as usize, i, *p);
                let _ = &world;
            };
        for &n in was.iter().filter(|n| !now.contains(n)) {
            set(n, false, &mut self.world, &mut self.stats);
        }
        for &n in now.iter().filter(|n| !was.contains(n)) {
            set(n, true, &mut self.world, &mut self.stats);
        }
    }
    pub fn pointer_leave(&mut self) {
        self.ptr = None;
        let was = std::mem::take(&mut self.hover);
        let mut ran = false;
        for &n in &was {
            ran |= self.fire(
                n,
                Kind::Hover,
                &Ev {
                    on: false,
                    ..self.ev(-1.0, -1.0)
                },
            );
        }
        if ran {
            self.settle();
        } else {
            self.hover_paint(&was, &[]);
        }
    }
    pub fn pointer_down(&mut self, x: f32, y: f32, button: Button) {
        self.pointer_move(x, y);
        let chain = self.chain(x, y);
        // focus goes to the nearest focusable ancestor, or nowhere
        self.focus = chain
            .iter()
            .copied()
            .find(|&(d, n)| self.docs[d as usize].nodes[n as usize].focusable);
        let e = Ev {
            button,
            ..self.ev(x, y)
        };
        let mut ran = false;
        // the press is the innermost node that hears a press, a click or a drag (lexical ancestry: bubbling)
        if let Some(&at) = chain.iter().find(|&&n| {
            self.hears(n, Kind::Press) || self.hears(n, Kind::Click) || self.hears(n, Kind::Drag)
        }) {
            if button == Button::Primary {
                self.held = Some(Held {
                    doc: at.0,
                    node: at.1,
                    at: (x, y),
                });
            }
            ran |= self.fire(at, Kind::Press, &e);
        }
        if ran {
            self.settle();
        }
    }
    pub fn pointer_up(&mut self, x: f32, y: f32, button: Button) {
        let Some(h) = self.held.take() else { return };
        if button != Button::Primary {
            self.held = Some(h);
            return;
        }
        let at = (h.doc, h.node);
        let mut ran = false;
        if at.0 as usize >= self.docs.len() || at.1 as usize >= self.docs[at.0 as usize].nodes.len()
        {
            return;
        }
        ran |= self.fire(
            at,
            Kind::Drag,
            &Ev {
                dx: x - h.at.0,
                dy: y - h.at.1,
                on: false,
                ..self.ev(x, y)
            },
        );
        if self.chain(x, y).contains(&at) {
            ran |= self.fire(at, Kind::Click, &self.ev(x, y));
        }
        if ran {
            self.settle();
        }
    }
    pub fn key_down(&mut self, key: Key, mods: Mods) {
        if key == Key::Tab {
            return self.tab(mods.shift);
        }
        let Some(focus) = self.focus else { return };
        let e = Ev {
            key: Some(key),
            mods,
            ..self.ev(0.0, 0.0)
        };
        // the focused node, then its lexical ancestors: the first that hears keys takes it
        let mut at = {
            let scene = self.lv.scene.borrow();
            scene
                .hits
                .iter()
                .position(|h| (h.doc, h.node) == focus)
                .map_or(NO, |i| i as u32)
        };
        while at != NO {
            let (n, parent) = {
                let scene = self.lv.scene.borrow();
                let h = &scene.hits[at as usize];
                ((h.doc, h.node), h.parent)
            };
            if self.fire(n, Kind::Key, &e) {
                return self.settle();
            }
            at = parent;
        }
    }
    fn tab(&mut self, back: bool) {
        let stops: Vec<(u32, u32)> = {
            let scene = self.lv.scene.borrow();
            scene
                .hits
                .iter()
                .map(|h| (h.doc, h.node))
                .filter(|&(d, n)| self.docs[d as usize].nodes[n as usize].focusable)
                .collect()
        };
        if stops.is_empty() {
            return;
        }
        let at = self.focus.and_then(|f| stops.iter().position(|&s| s == f));
        let n = stops.len();
        self.focus = Some(match (at, back) {
            (None, false) => stops[0],
            (None, true) => stops[n - 1],
            (Some(i), false) => stops[(i + 1) % n],
            (Some(i), true) => stops[(i + n - 1) % n],
        });
    }
    /// The wheel at (x, y) points turned by `dy` lines: the innermost scrolled port under the pointer moves, as
    /// far as its content goes.
    pub fn wheel(&mut self, x: f32, y: f32, _dx: f32, dy: f32) {
        let chain = self.chain(x, y);
        for (d, n) in chain {
            let Some(sc) = &self.docs[d as usize].nodes[n as usize].scroll else {
                continue;
            };
            let (content, port) = {
                let scene = self.lv.scene.borrow();
                let r = scene.rects[d as usize][n as usize];
                (scene.heights[sc.doc as usize], r[3] - r[1])
            };
            let max = ((content - port).max(0)) as f32 / self.scale;
            let to = (sc.at - dy * sc.line).clamp(0.0, max);
            if to != sc.at {
                if let Some(o) = resolve(&self.owners, sc.owner, &mut self.lv.v) {
                    count(|c| c.dispatch += 1);
                    (sc.set)(o, to);
                }
                self.settle();
            }
            return;
        }
    }
}
