//! `FlatApp`: a prelude view on the flat core.
use crate::app::{Config, View};
use crate::elem::Element;
use crate::input::{Button, Ev, Key, Mods};
use crate::lower::canvas::{Canvas, Images};
use crate::lower::text::TextSys;
use crate::lower::{Core, Doc, Hnd, Kind, LayMemo, Lower, NO, Owner, Scene};
use bough_flat::store::ROOT;
use bough_flat::{self as flat, ExtPaint, FlatWorld, FrameInfo, Prop, RecId, RecordDesc, Slot, SpaceId, Surface, TextSrc, Val};
use bough_x::prim::{P_BOX, P_IMAGE, P_TEXT, Prim};
use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

fn fr(r: bough_x::store::Rect) -> flat::Rect {
    flat::Rect::new(r.x0, r.y0, r.x1, r.y1)
}
fn xr(r: flat::Rect) -> bough_x::store::Rect {
    bough_x::store::Rect { x0: r.x0, y0: r.y0, x1: r.x1, y1: r.y1 }
}
fn words(p: &Prim) -> [u32; 4] {
    [p.kind, p.a, p.b, (p.ox as i16 as u16 as u32) << 16 | p.oy as i16 as u16 as u32]
}

/// Paints the records the flat core does not paint itself (shaped lines, images) with the lowered runtime's
/// canvas, straight into the flat surface's pixels.
struct CanvasExt<'a>(&'a mut Canvas);
impl ExtPaint for CanvasExt<'_> {
    fn paint(&mut self, s: &mut Surface, rect: flat::Rect, clip: flat::Rect, color: u32, opacity: u32, w: [u32; 4]) {
        let c = &mut *self.0;
        assert!(s.org == (0, 0) && s.w == c.w && s.h == c.h, "shaped text is painted on the window surface (the prelude describes no backdrop layer)");
        std::mem::swap(&mut c.px, &mut s.px);
        let whole = c.clip;
        c.clip = xr(clip);
        let p = Prim { kind: w[0], color, a: w[1], b: w[2], alpha: opacity, ox: (w[3] >> 16) as u16 as i16 as i32, oy: w[3] as u16 as i16 as i32, ..Prim::default() };
        c.one(xr(rect), xr(clip), &p);
        c.clip = whole;
        std::mem::swap(&mut c.px, &mut s.px);
    }
}

/// One segment of a scene (a run of records in one coordinate space) as the flat world holds it.
struct SegMap {
    port: RecId,
    space: SpaceId,
    sx: Slot<i32>,
    sy: Slot<i32>,
    recs: Vec<RecId>,
    /// What each record was last described as.
    last: Vec<(bough_x::store::Rect, Prim)>,
}

/// How far ahead the description-to-world step looks for a record that moved up or down a run (rows leaving
/// and entering a scrolled list).
const LOOKAHEAD: usize = 96;

/// Counters of the description-to-world step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SyncStats {
    pub syncs: u64,
    /// Records compared with their description.
    pub compared: u64,
    /// Property writes that found a difference.
    pub patches: u64,
    pub inserted: u64,
    pub removed: u64,
}

/// The non-generic half of [`FlatApp`]: the world, and the scene-to-world step.
pub struct Host {
    pub world: FlatWorld,
    canvas: Canvas,
    segs: Vec<SegMap>,
    pub sync: SyncStats,
}

impl Host {
    fn new(w: i32, h: i32, text: Rc<RefCell<TextSys>>, images: Rc<RefCell<Images>>) -> Host {
        Host { world: FlatWorld::new(w, h), canvas: Canvas::new(w, h, text, images), segs: Vec::new(), sync: SyncStats::default() }
    }
    fn desc(space: SpaceId, r: flat::Rect, p: &Prim) -> RecordDesc {
        let d = RecordDesc { opacity: Val::Const(p.alpha), ..RecordDesc::rect(space, r, p.color) };
        match p.kind {
            P_BOX => RecordDesc { radius: Val::Const(p.radius), border_w: Val::Const(p.border), border: Val::Const(p.border_color), ..d },
            P_TEXT | P_IMAGE => RecordDesc { kind: flat::Kind::Ext, text: Some(TextSrc::Ext(words(p))), ..d },
            k => panic!("the flat bridge does not know record kind {k}"),
        }
    }
    /// Bring the world's records in line with a scene: equal records cost a compare, different ones a patch,
    /// and a run that grew or shrank keeps its common head and tail.
    fn sync_scene(&mut self, scene: &Scene) {
        self.sync.syncs += 1;
        let w = &mut self.world;
        for (k, g) in scene.segs.iter().enumerate() {
            let clip = fr(g.space.clip);
            let (sx, sy) = (clip.x0 - g.space.base.0, clip.y0 - g.space.base.1);
            if k == self.segs.len() {
                // the segment's space: an invisible port holding its clip, and its origin as two state words
                let port = w.add_record(RecordDesc { kind: flat::Kind::None, ..RecordDesc::rect(ROOT, clip, 0u32) });
                let (sxs, sys) = (w.slot(sx), w.slot(sy));
                let space = w.add_space(ROOT, Some(port), true, sxs, sys);
                self.segs.push(SegMap { port, space, sx: sxs, sy: sys, recs: Vec::new(), last: Vec::new() });
            }
            let m = &self.segs[k];
            w.patch_rect(m.port, clip);
            w.set(m.sx, sx);
            w.set(m.sy, sy);
            let space = m.space;
            // what a record of this run is described before, if the run has nothing after the point
            let after = self.segs[k + 1..].iter().find_map(|s| s.recs.first().copied());
            let m = &mut self.segs[k];
            let (old_ids, old) = (std::mem::take(&mut m.recs), std::mem::take(&mut m.last));
            m.recs.reserve(g.prims.len());
            let (mut i, mut j) = (0, 0);
            let patch = |w: &mut FlatWorld, id: RecId, r: &bough_x::store::Rect, p: &Prim| -> u64 {
                let mut n = w.patch_rect(id, fr(*r)) as u64 + w.patch(id, Prop::Fill, p.color) as u64 + w.patch(id, Prop::Opacity, p.alpha) as u64;
                if p.kind == P_BOX {
                    n += w.patch(id, Prop::Radius, p.radius) as u64 + w.patch(id, Prop::BorderW, p.border) as u64 + w.patch(id, Prop::Border, p.border_color) as u64;
                } else {
                    n += w.patch_ext(id, words(p)) as u64;
                }
                n
            };
            while j < g.prims.len() {
                let new = &g.prims[j];
                self.sync.compared += 1;
                if i < old.len() && old[i] == *new {
                    m.recs.push(old_ids[i]);
                    (i, j) = (i + 1, j + 1);
                    continue;
                }
                // the same record further down the old run: what is between left
                if let Some(d) = old[i.min(old.len())..].iter().take(LOOKAHEAD).position(|o| o == new) {
                    for id in &old_ids[i..i + d] {
                        w.remove(*id);
                        self.sync.removed += 1;
                    }
                    i += d;
                    continue;
                }
                // the old record further down the new run: what is between entered
                let enters = match old.get(i) {
                    Some(o) => g.prims[j..].iter().take(LOOKAHEAD).position(|n| n == o),
                    None => Some(g.prims.len() - j),
                };
                if let Some(d) = enters {
                    let anchor = old_ids.get(i).copied().or(after);
                    for (r, p) in &g.prims[j..j + d] {
                        let desc = Self::desc(space, fr(*r), p);
                        m.recs.push(match anchor {
                            Some(a) => w.insert_before(a, desc),
                            None => w.add_record(desc),
                        });
                        self.sync.inserted += 1;
                    }
                    j += d;
                    continue;
                }
                // neither: the record at this place changed (or was replaced by one of another kind)
                if old[i].1.kind == new.1.kind {
                    self.sync.patches += patch(w, old_ids[i], &new.0, &new.1);
                    m.recs.push(old_ids[i]);
                } else {
                    m.recs.push(w.insert_before(old_ids[i], Self::desc(space, fr(new.0), &new.1)));
                    w.remove(old_ids[i]);
                    self.sync.inserted += 1;
                    self.sync.removed += 1;
                }
                (i, j) = (i + 1, j + 1);
            }
            for id in &old_ids[i..] {
                w.remove(*id);
                self.sync.removed += 1;
            }
            m.last = old;
            m.last.clear();
            m.last.extend_from_slice(&g.prims);
        }
        for m in &mut self.segs[scene.segs.len()..] {
            for id in m.recs.drain(..) {
                w.remove(id);
                self.sync.removed += 1;
            }
            m.last.clear();
            w.patch_rect(m.port, flat::Rect::EMPTY);
        }
    }
    fn frame(&mut self) -> FrameInfo {
        self.world.frame_with(&mut CanvasExt(&mut self.canvas))
    }
}

/// Count bands the gates assert.
#[derive(Clone, Copy, Default, Debug)]
pub struct Stats {
    pub describes: u64,
    /// One-record patches (hover paints) that needed no description.
    pub patches: u64,
    pub frames: u64,
    pub idle_frames: u64,
}

struct Held {
    doc: u32,
    node: u32,
    at: (f32, f32),
}

fn resolve<'x>(owners: &[Owner], id: u32, root: &'x mut dyn Any) -> Option<&'x mut dyn Any> {
    if id == 0 {
        return Some(root);
    }
    let o = &owners[id as usize - 1];
    let p = resolve(owners, o.parent, root)?;
    (o.project)(p)
}

/// A `bough::prelude` view running on the flat core. The surface is `lower::App`'s: construct, `turn`, the
/// pointer and key calls, `frame`, `check`.
pub struct FlatApp<V: View> {
    v: V,
    pub host: Host,
    text: Rc<RefCell<TextSys>>,
    images: Rc<RefCell<Images>>,
    scene: Scene,
    docs: Vec<Doc>,
    handlers: Vec<Hnd>,
    owners: Vec<Owner>,
    lists: crate::lower::list::Lists,
    displayed_lists: Vec<crate::lower::list::DisplayedList>,
    lay: LayMemo,
    size_pt: (f32, f32),
    scale: f32,
    ptr: Option<(f32, f32)>,
    hover: Vec<(u32, u32)>,
    held: Option<Held>,
    focus: Option<(u32, u32)>,
    pub stats: Stats,
    pub last: FrameInfo,
}

impl<V: View> FlatApp<V> {
    /// A window of `cfg.width_pt` x `cfg.height_pt` points at `cfg.scale` with `cfg.font` as the UI face. The
    /// view is described and mounted; nothing is painted until `frame`.
    pub fn new(view: V, cfg: Config) -> FlatApp<V> {
        let (w, h) = ((cfg.width_pt * cfg.scale).round() as i32, (cfg.height_pt * cfg.scale).round() as i32);
        let text = Rc::new(RefCell::new(TextSys::new(&cfg.font)));
        let images = Rc::new(RefCell::new(Images::default()));
        let mut app = FlatApp {
            v: view,
            host: Host::new(w, h, text.clone(), images.clone()),
            text,
            images,
            scene: Scene::default(),
            docs: Vec::new(),
            handlers: Vec::new(),
            owners: Vec::new(),
            lists: Default::default(),
            displayed_lists: Vec::new(),
            lay: Default::default(),
            size_pt: (cfg.width_pt, cfg.height_pt),
            scale: cfg.scale,
            ptr: None,
            hover: Vec::new(),
            held: None,
            focus: None,
            stats: Stats::default(),
            last: FrameInfo::default(),
        };
        app.describe();
        app.host.sync_scene(&app.scene);
        app
    }
    fn px(&self) -> (i32, i32) {
        ((self.size_pt.0 * self.scale).round() as i32, (self.size_pt.1 * self.scale).round() as i32)
    }
    fn display_lists(&mut self, on: bool) {
        for entry in &self.displayed_lists {
            if let Some(owner) = resolve(&self.owners, entry.owner, &mut self.v) {
                (entry.display)(owner, entry.offset, on);
            }
        }
    }
    fn describe(&mut self) {
        for round in 0.. {
            if !self.describe_once() || round == 3 {
                break;
            }
        }
        self.rehover();
    }
    fn describe_once(&mut self) -> bool {
        self.display_lists(false);
        let mut displayed_lists = std::mem::take(&mut self.displayed_lists);
        displayed_lists.clear();
        self.stats.describes += 1;
        let (w, h) = self.px();
        let (mut text, mut images) = (self.text.borrow_mut(), self.images.borrow_mut());
        text.begin_pass();
        let mut core = Core::new(self.scale, (w, h), &self.lists, &mut text, &mut images);
        core.displayed_lists = Some(displayed_lists);
        View::view(&self.v).lower(&mut Lower::new(&mut core));
        assert!(core.refused.is_empty(), "the lowering does not describe these elements yet: {:?}", core.refused);
        let scene = crate::lower::finish(&core, w, h, &mut self.lay);
        let Core { docs, handlers, owners, displayed_lists, rows, .. } = core;
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
        (self.docs, self.handlers, self.owners) = (docs, handlers, owners);
        self.scene = scene;
        drop((text, images));
        self.displayed_lists = displayed_lists.unwrap_or_default();
        self.display_lists(true);
        again
    }
    /// After a description: who is under the pointer now, and their hover paints into the scene.
    fn rehover(&mut self) {
        self.hover = match self.ptr {
            Some((x, y)) => self.chain(x, y),
            None => Vec::new(),
        };
        for &(d, n) in &self.hover {
            if let Some(c) = self.docs[d as usize].nodes[n as usize].hover_bg {
                if let Some((s, i)) = self.scene.hit(d, n).and_then(|h| h.prim) {
                    self.scene.segs[s as usize].prims[i as usize].1.color = c;
                }
            }
        }
    }
    /// After handlers ran: the effect site, a description, and the world brought in line with it.
    fn settle(&mut self) {
        let pubs = crate::changed::Pubs::default();
        self.v.changed(crate::changed::Changed { pubs: &pubs, readers: &[], _v: std::marker::PhantomData });
        self.describe();
        self.host.sync_scene(&self.scene);
    }
    /// Run `f` on the state, then bring the window up to date with it.
    pub fn turn(&mut self, f: impl FnOnce(&mut V)) {
        f(&mut self.v);
        self.settle();
    }
    pub fn view(&self) -> &V {
        &self.v
    }
    /// Repaint what is damaged. Returns whether anything was.
    pub fn frame(&mut self) -> bool {
        self.stats.frames += 1;
        self.last = self.host.frame();
        self.stats.idle_frames += self.last.idle as u64;
        !self.last.idle
    }
    /// The surface's pixels (premultiplied RGBA8).
    pub fn presented_pixels(&self) -> Vec<u8> {
        self.host.world.surface().bytes()
    }
    /// The scene painted directly by the lowered runtime's canvas, record by record, with no stores: an oracle
    /// that shares no code with the flat raster or the damage engine.
    pub fn immediate_pixels(&self) -> Vec<u8> {
        let (w, h) = self.px();
        let mut c = Canvas::new(w, h, self.text.clone(), self.images.clone());
        for g in &self.scene.segs {
            for (r, p) in &g.prims {
                c.one(r.shift(g.space.base), g.space.clip, p);
            }
        }
        c.bytes()
    }
    /// Present a frame and hold it to the oracles: the retained surface equals a full redraw of the flat
    /// stores, equals the same state mounted into a new world, equals the scene painted directly by the
    /// lowered runtime's canvas.
    pub fn check(&mut self) -> Result<(), String> {
        self.stats.frames += 1;
        let Host { world, canvas, .. } = &mut self.host;
        self.last = world.check(&mut CanvasExt(canvas))?;
        let (got, want) = (self.presented_pixels(), self.immediate_pixels());
        if got != want {
            let n = got.chunks_exact(4).zip(want.chunks_exact(4)).filter(|(a, b)| a != b).count();
            return Err(format!("flat surface != the lowered canvas's immediate paint ({n} pixels)"));
        }
        Ok(())
    }
    pub fn write_png(&self, path: &std::path::Path) -> std::io::Result<()> {
        let (w, h) = self.px();
        crate::lower::app::png(&self.presented_pixels(), w, h, path)
    }
    pub fn record_count(&self) -> usize {
        self.scene.segs.iter().map(|g| g.prims.len()).sum()
    }
    pub fn size_pt(&self) -> (f32, f32) {
        self.size_pt
    }
    /// The window is `w` x `h` points now.
    pub fn resize(&mut self, w_pt: f32, h_pt: f32) {
        self.size_pt = (w_pt, h_pt);
        let (w, h) = self.px();
        self.host.world.resize(w, h);
        self.host.canvas = Canvas::new(w, h, self.text.clone(), self.images.clone());
        self.describe();
        self.host.sync_scene(&self.scene);
    }

    // ---- input (the lowered runtime's rules) ----

    fn chain(&self, x: f32, y: f32) -> Vec<(u32, u32)> {
        let (px, py) = ((x * self.scale) as i32, (y * self.scale) as i32);
        let inside = |r: bough_x::store::Rect| px >= r.x0 && px < r.x1 && py >= r.y0 && py < r.y1;
        let top = self.scene.hits.iter().rposition(|h| inside(h.rect) && inside(h.clip) && self.docs[h.doc as usize].nodes[h.node as usize].solid);
        let mut out = Vec::new();
        let mut at = top.map_or(NO, |t| t as u32);
        while at != NO {
            let h = &self.scene.hits[at as usize];
            out.push((h.doc, h.node));
            at = h.parent;
        }
        out
    }
    fn fire(&mut self, at: (u32, u32), kind: Kind, e: &Ev) -> bool {
        if self.docs.get(at.0 as usize).is_none_or(|d| d.nodes.len() <= at.1 as usize) {
            return false;
        }
        let (a, n) = self.docs[at.0 as usize].nodes[at.1 as usize].hnd;
        let mut ran = false;
        for h in &self.handlers[a as usize..(a + n) as usize] {
            if h.kind == kind {
                if let Some(o) = resolve(&self.owners, h.owner, &mut self.v) {
                    (h.f)(o, e);
                    ran = true;
                }
            }
        }
        ran
    }
    fn hears(&self, at: (u32, u32), kind: Kind) -> bool {
        let (a, n) = self.docs[at.0 as usize].nodes[at.1 as usize].hnd;
        self.handlers[a as usize..(a + n) as usize].iter().any(|h| h.kind == kind)
    }
    fn ev(&self, x: f32, y: f32) -> Ev {
        Ev { x, y, dx: 0.0, dy: 0.0, key: None, mods: Mods::default(), button: Button::Primary, on: true }
    }
    pub fn pointer_move(&mut self, x: f32, y: f32) {
        self.ptr = Some((x, y));
        let now = self.chain(x, y);
        let mut ran = false;
        if let Some(h) = &self.held {
            let at = (h.doc, h.node);
            let e = Ev { dx: x - h.at.0, dy: y - h.at.1, ..self.ev(x, y) };
            ran |= self.fire(at, Kind::Drag, &e);
        }
        if now != self.hover {
            let was = std::mem::take(&mut self.hover);
            for &n in was.iter().filter(|n| !now.contains(n)) {
                ran |= self.fire(n, Kind::Hover, &Ev { on: false, ..self.ev(x, y) });
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
        for (set, on) in [(was, false), (now, true)] {
            let other = if on { was } else { now };
            for &at in set.iter().filter(|n| !other.contains(n)) {
                let node = &self.docs[at.0 as usize].nodes[at.1 as usize];
                let (Some(hover), crate::lower::Paint::Box { color, .. }) = (node.hover_bg, &node.paint) else {
                    continue;
                };
                let Some((s, i)) = self.scene.hit(at.0, at.1).and_then(|h| h.prim) else {
                    continue;
                };
                let c = if on { hover } else { *color };
                self.scene.segs[s as usize].prims[i as usize].1.color = c;
                let m = &mut self.host.segs[s as usize];
                m.last[i as usize].1.color = c;
                let id = m.recs[i as usize];
                self.stats.patches += self.host.world.patch(id, Prop::Fill, c) as u64;
            }
        }
    }
    pub fn pointer_leave(&mut self) {
        self.ptr = None;
        let was = std::mem::take(&mut self.hover);
        let mut ran = false;
        for &n in &was {
            ran |= self.fire(n, Kind::Hover, &Ev { on: false, ..self.ev(-1.0, -1.0) });
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
        self.focus = chain.iter().copied().find(|&(d, n)| self.docs[d as usize].nodes[n as usize].focusable);
        let e = Ev { button, ..self.ev(x, y) };
        let mut ran = false;
        if let Some(&at) = chain.iter().find(|&&n| self.hears(n, Kind::Press) || self.hears(n, Kind::Click) || self.hears(n, Kind::Drag)) {
            if button == Button::Primary {
                self.held = Some(Held { doc: at.0, node: at.1, at: (x, y) });
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
        if at.0 as usize >= self.docs.len() || at.1 as usize >= self.docs[at.0 as usize].nodes.len() {
            return;
        }
        let mut ran = self.fire(at, Kind::Drag, &Ev { dx: x - h.at.0, dy: y - h.at.1, on: false, ..self.ev(x, y) });
        if self.chain(x, y).contains(&at) {
            ran |= self.fire(at, Kind::Click, &self.ev(x, y));
        }
        if ran {
            self.settle();
        }
    }
    pub fn key_down(&mut self, key: Key, mods: Mods) {
        let Some(focus) = self.focus else { return };
        let e = Ev { key: Some(key), mods, ..self.ev(0.0, 0.0) };
        let mut at = self.scene.hits.iter().position(|h| (h.doc, h.node) == focus).map_or(NO, |i| i as u32);
        while at != NO {
            let h = &self.scene.hits[at as usize];
            let (n, parent) = ((h.doc, h.node), h.parent);
            if self.fire(n, Kind::Key, &e) {
                return self.settle();
            }
            at = parent;
        }
    }
    /// The wheel at (x, y) points turned by `dy` lines: the innermost scrolled port under the pointer moves.
    pub fn wheel(&mut self, x: f32, y: f32, _dx: f32, dy: f32) {
        for (d, n) in self.chain(x, y) {
            let Some(sc) = &self.docs[d as usize].nodes[n as usize].scroll else {
                continue;
            };
            let r = self.scene.rects[d as usize][n as usize];
            let (content, port) = (self.scene.heights[sc.doc as usize], r[3] - r[1]);
            let max = ((content - port).max(0)) as f32 / self.scale;
            let to = (sc.at + dy * sc.line).clamp(0.0, max);
            if to != sc.at {
                if let Some(o) = resolve(&self.owners, sc.owner, &mut self.v) {
                    (sc.set)(o, to);
                }
                self.settle();
            }
            return;
        }
    }
}
