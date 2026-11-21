//! The façade on the typed core (Integration Card 3): `bough::prelude` elements lowered onto `World<V, B>`.
//!
//! An element value has two protocols. `Element::mount` / `update` drive the legacy runtime (`rt.rs`), which
//! `bough::raw::App` still is. [`Element::lower`](crate::elem::Element::lower) describes the same element as
//! data: a node in a tree with layout words (`bough-lay`'s heap tree), a generic paint record
//! ([`bough_x::prim::Prim`]), erased handlers and a parent column. [`App`] lays the tree out with `bough-lay`'s
//! one flex function, emits the records as free rows into a [`bough_x::world::World`], and paints through
//! [`canvas::Canvas`]. No `Rt` is constructed on this path ([`provenance`] counts it).
//!
//! A turn is: run the handlers, describe the view again, compare against the columns (the world's
//! re-description), repaint the damaged tiles. A hover that only swaps a paint (`hover_bg`) patches one record.
//!
//! What this path does not lower yet refuses by name at description time ([`Lower::refuse`], reported by
//! [`App::refused`]) instead of drawing something else; the list is in the lane report.
pub mod app;
pub mod canvas;
pub mod provenance;
pub mod text;

pub use app::App;

use crate::input::Ev;
use crate::layout::Spec;
use bough_lay::heap::Tree;
use bough_x::prim::{P_BOX, P_IMAGE, P_TEXT, Prim};
use bough_x::store::{Rect, Space};
use std::any::Any;
use std::marker::PhantomData;
use std::rc::Rc;

pub(crate) const NO: u32 = u32::MAX;

/// What a handler hears.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    Click,
    Press,
    Hover,
    Key,
    Drag,
}

pub(crate) struct Hnd {
    pub kind: Kind,
    pub owner: u32,
    pub f: Box<dyn Fn(&mut dyn Any, &Ev)>,
}

/// How an owner is reached from its parent owner (owner 0 is the root view).
pub(crate) struct Owner {
    pub parent: u32,
    pub project: Box<dyn for<'x> Fn(&'x mut dyn Any) -> Option<&'x mut dyn Any>>,
}

/// What a node paints.
#[derive(Clone)]
pub(crate) enum Paint {
    None,
    Box {
        color: u32,
        radius: f32,
        border: Option<(f32, u32)>,
    },
    /// One shaped line: (run, generation), size in device pixels, colour.
    Line {
        run: (u32, u32),
        size: f32,
        color: u32,
    },
    /// Wrapped words, broken at the width layout gives.
    Para {
        para: Rc<text::Para>,
        size: f32,
        color: u32,
    },
    Image {
        id: u32,
    },
}

/// A scrolled port: its content is another document, moved by an offset the view owns.
pub(crate) struct Scroll {
    pub doc: u32,
    /// The offset now, in points.
    pub at: f32,
    pub line: f32,
    pub owner: u32,
    pub set: Box<dyn Fn(&mut dyn Any, f32)>,
}

pub(crate) struct Node {
    pub parent: u32,
    pub paint: Paint,
    /// Handlers: a range of the core's handler list.
    pub hnd: (u32, u32),
    pub hover_bg: Option<u32>,
    pub focusable: bool,
    /// The pointer stops here (a painted box, or one with handlers); otherwise it falls through to what is under.
    pub solid: bool,
    pub alpha: u32,
    /// Paint offset in device pixels (the layout place does not move).
    pub shift: (i32, i32),
    pub scroll: Option<Scroll>,
    pub source: &'static str,
}

/// A layout tree and its nodes, element `k` of the tree being node `k`.
pub(crate) struct Doc {
    pub tree: Tree,
    pub nodes: Vec<Node>,
    /// Open boxes: (node, children so far).
    stack: Vec<(usize, u32)>,
}

/// The non-generic part of a description in progress.
pub struct Core<'a> {
    pub(crate) scale: f32,
    pub(crate) docs: Vec<Doc>,
    cur: usize,
    pub(crate) handlers: Vec<Hnd>,
    pub(crate) owners: Vec<Owner>,
    owner: u32,
    pub(crate) text: &'a mut text::TextSys,
    pub(crate) images: &'a mut canvas::Images,
    pub(crate) refused: Vec<&'static str>,
}

/// The sink an element lowers itself into, for owner `V`.
pub struct Lower<'c, 'a, V> {
    pub(crate) core: &'c mut Core<'a>,
    _v: PhantomData<fn(&mut V)>,
}

fn dim(d: crate::layout::Dim, s: f32) -> bough_lay::Dim {
    match d.parts() {
        (1, v) => bough_lay::Dim::Pt(v),
        (2, v) => bough_lay::Dim::Pct(v),
        (3, v) => bough_lay::Dim::Pt(v / s),
        _ => bough_lay::Dim::Auto,
    }
}
fn align(a: crate::layout::Align) -> bough_lay::Align {
    match a {
        // (baseline alignment is not in bough-lay yet: the items start-align; listed in the report)
        crate::layout::Align::Start | crate::layout::Align::Baseline => bough_lay::Align::Start,
        crate::layout::Align::End => bough_lay::Align::End,
        crate::layout::Align::Center => bough_lay::Align::Center,
        crate::layout::Align::Stretch => bough_lay::Align::Stretch,
    }
}
fn justify(j: crate::layout::Justify) -> bough_lay::Justify {
    use crate::layout::Justify as J;
    match j {
        J::Start => bough_lay::Justify::Start,
        J::End => bough_lay::Justify::End,
        J::Center => bough_lay::Justify::Center,
        J::SpaceBetween => bough_lay::Justify::SpaceBetween,
        J::SpaceAround => bough_lay::Justify::SpaceAround,
        J::SpaceEvenly => bough_lay::Justify::SpaceEvenly,
    }
}
/// The façade's spec as bough-lay's (points; `Px` lengths become points at this scale).
pub(crate) fn lay_spec(s: &Spec, scale: f32) -> bough_lay::Spec {
    bough_lay::Spec {
        w: dim(s.w, scale),
        h: dim(s.h, scale),
        min: [s.min_w, s.min_h],
        pad: s.pad,
        margin: s.margin,
        gap: s.gap,
        grow: s.grow,
        shrink: s.shrink,
        column: s.column,
        items: s.items.map(align),
        self_align: s.self_align.map(align),
        justify: s.justify.map(justify),
        abs: s.abs(),
    }
}

impl Node {
    fn new(parent: u32, source: &'static str) -> Node {
        Node {
            parent,
            paint: Paint::None,
            hnd: (0, 0),
            hover_bg: None,
            focusable: false,
            solid: false,
            alpha: 255,
            shift: (0, 0),
            scroll: None,
            source,
        }
    }
}

impl<'a> Core<'a> {
    pub(crate) fn new(
        scale: f32,
        text: &'a mut text::TextSys,
        images: &'a mut canvas::Images,
    ) -> Core<'a> {
        Core {
            scale,
            docs: vec![Doc {
                tree: Tree::new(),
                nodes: Vec::new(),
                stack: Vec::new(),
            }],
            cur: 0,
            handlers: Vec::new(),
            owners: Vec::new(),
            owner: 0,
            text,
            images,
            refused: Vec::new(),
        }
    }
    fn doc(&mut self) -> &mut Doc {
        &mut self.docs[self.cur]
    }
    fn parent(&mut self) -> u32 {
        let d = self.doc();
        match d.stack.last_mut() {
            Some((p, n)) => {
                *n += 1;
                *p as u32
            }
            None => {
                assert!(
                    d.nodes.is_empty(),
                    "a description has one root element per document (wrap siblings in a box)"
                );
                NO
            }
        }
    }
    /// Open a box; its children are what is lowered until [`Core::close`].
    pub(crate) fn open(&mut self, spec: &Spec, source: &'static str) -> usize {
        let (parent, s) = (self.parent(), self.scale);
        let d = self.doc();
        let i = d.tree.open(lay_spec(spec, s));
        d.nodes.push(Node::new(parent, source));
        d.stack.push((i, 0));
        i
    }
    pub(crate) fn close(&mut self, i: usize) {
        let d = self.doc();
        let (j, n) = d.stack.pop().expect("an open box");
        assert_eq!(i, j, "boxes close in the order they opened");
        d.tree.close(i, n);
    }
    /// A leaf of known size in device pixels.
    pub(crate) fn line(&mut self, spec: &Spec, w: f32, h: f32, source: &'static str) -> usize {
        let (parent, s) = (self.parent(), self.scale);
        let d = self.doc();
        let i = d.tree.line(lay_spec(spec, s), w, h);
        d.nodes.push(Node::new(parent, source));
        i
    }
    pub(crate) fn wrap(
        &mut self,
        spec: &Spec,
        w: Box<dyn bough_lay::Wrap>,
        source: &'static str,
    ) -> usize {
        let (parent, s) = (self.parent(), self.scale);
        let d = self.doc();
        let i = d.tree.wrap(lay_spec(spec, s), w);
        d.nodes.push(Node::new(parent, source));
        i
    }
    pub(crate) fn node(&mut self, i: usize) -> &mut Node {
        &mut self.doc().nodes[i]
    }
    /// The index the next node of the current document will have.
    pub(crate) fn next(&self) -> usize {
        self.docs[self.cur].nodes.len()
    }
    /// Lower `f` into a new document (a scrolled port's content); returns the document.
    pub(crate) fn sub(&mut self, f: impl FnOnce(&mut Core<'a>)) -> u32 {
        let back = self.cur;
        self.docs.push(Doc {
            tree: Tree::new(),
            nodes: Vec::new(),
            stack: Vec::new(),
        });
        self.cur = self.docs.len() - 1;
        let id = self.cur as u32;
        f(self);
        assert!(self.docs[id as usize].stack.is_empty());
        self.cur = back;
        id
    }
}

impl<'c, 'a, V: 'static> Lower<'c, 'a, V> {
    pub(crate) fn new(core: &'c mut Core<'a>) -> Self {
        Lower {
            core,
            _v: PhantomData,
        }
    }
    /// Device pixels per point.
    pub fn scale(&self) -> f32 {
        self.core.scale
    }
    /// An element this path does not lower: recorded by name, and the description is refused as a whole
    /// (`App` panics with the list) rather than shown without it.
    pub fn refuse(&mut self, what: &'static str) {
        if !self.core.refused.contains(&what) {
            self.core.refused.push(what);
        }
    }
    /// A handler of the node `node` of the current document, for this owner.
    pub(crate) fn handler(&mut self, node: usize, kind: Kind, f: impl Fn(&mut V, &Ev) + 'static) {
        let owner = self.core.owner;
        let at = self.core.handlers.len() as u32;
        self.core.handlers.push(Hnd {
            kind,
            owner,
            f: Box::new(move |o: &mut dyn Any, e: &Ev| {
                // an owner of another type is a stale handler (its node belongs to a description that was
                // replaced while an event was in flight): it is not run
                if let Some(v) = o.downcast_mut::<V>() {
                    f(v, e)
                }
            }),
        });
        let n = self.core.node(node);
        n.solid = true;
        if n.hnd.1 == 0 {
            n.hnd = (at, 1);
        } else {
            assert_eq!(
                n.hnd.0 + n.hnd.1,
                at,
                "a node's handlers are described together"
            );
            n.hnd.1 += 1;
        }
    }
    pub(crate) fn owner(&self) -> u32 {
        self.core.owner
    }
}

// ---- the scene: what a description comes to -------------------------------------------------------------------

/// A run of records in one coordinate space, in paint order.
pub(crate) struct Seg {
    pub space: Space,
    pub prims: Vec<(Rect, Prim)>,
}

/// A node as the pointer and the keyboard see it.
pub(crate) struct Hit {
    /// On screen (device pixels), and what it is cut to.
    pub rect: Rect,
    pub clip: Rect,
    pub parent: u32,
    pub doc: u32,
    pub node: u32,
    /// The node's own record, if it paints a box: (segment, slot), and its described colour.
    pub prim: Option<(u32, u32)>,
}

#[derive(Default)]
pub(crate) struct Scene {
    pub segs: Vec<Seg>,
    pub hits: Vec<Hit>,
    /// Layout rects per document, element by element, relative to the document's origin (device pixels).
    pub rects: Vec<Vec<[i32; 4]>>,
    /// Per document: content height in device pixels.
    pub heights: Vec<i32>,
}

struct Emit<'s> {
    docs: &'s [Doc],
    text: &'s text::TextSys,
    scale: f32,
    scene: Scene,
}

impl Emit<'_> {
    fn seg(&mut self, space: Space) -> usize {
        match self.scene.segs.last() {
            Some(s) if s.space == space => {}
            _ => self.scene.segs.push(Seg {
                space,
                prims: Vec::new(),
            }),
        }
        self.scene.segs.len() - 1
    }
    /// Emit document `d` into `space`: its records in pre-order (paint order), its nodes as hits.
    fn doc(&mut self, d: usize, space: Space, parent_hit: u32) {
        let doc = &self.docs[d];
        let base = self.scene.hits.len() as u32;
        for (k, n) in doc.nodes.iter().enumerate() {
            let r = self.scene.rects[d][k];
            let r = Rect {
                x0: r[0] + n.shift.0,
                y0: r[1] + n.shift.1,
                x1: r[2] + n.shift.0,
                y1: r[3] + n.shift.1,
            };
            let s = self.seg(space);
            let mut prim = None;
            match &n.paint {
                Paint::None => {}
                Paint::Box {
                    color,
                    radius,
                    border,
                } => {
                    prim = Some((s as u32, self.scene.segs[s].prims.len() as u32));
                    let (bw, bc) = border.map_or((0, 0), |(w, c)| {
                        (((w * self.scale).max(1.0) * 256.0) as u32, c)
                    });
                    self.scene.segs[s].prims.push((
                        r,
                        Prim {
                            kind: P_BOX,
                            color: *color,
                            radius: (radius * self.scale * 256.0) as u32,
                            border: bw,
                            border_color: bc,
                            alpha: n.alpha,
                            ..Prim::default()
                        },
                    ));
                }
                Paint::Line { run, size, color } => {
                    let lh = (r.y1 - r.y0) as f32;
                    let base_y = self.text.baseline(*size, lh);
                    self.line(s, *run, *color, n.alpha, (r.x0, r.y0 + base_y));
                }
                Paint::Para { para, size, color } => {
                    let width = (r.x1 - r.x0) as f32;
                    let base_y = self.text.baseline(*size, para.lh);
                    let mut words = Vec::new();
                    para.lines(width, |line, w, x| words.push((line, w, x)));
                    for (line, w, x) in words {
                        let word = &para.words[w];
                        if word.run != NO {
                            let y = r.y0 + (line as f32 * para.lh).round() as i32 + base_y;
                            self.line(
                                s,
                                (word.run, word.generation),
                                *color,
                                n.alpha,
                                (r.x0 + x.round() as i32, y),
                            );
                        }
                    }
                }
                Paint::Image { id } => self.scene.segs[s].prims.push((
                    r,
                    Prim {
                        kind: P_IMAGE,
                        a: *id,
                        alpha: n.alpha,
                        ..Prim::default()
                    },
                )),
            }
            let org = space.base;
            let parent = if n.parent == NO {
                parent_hit
            } else {
                base + n.parent
            };
            let at = self.scene.hits.len() as u32;
            self.scene.hits.push(Hit {
                rect: r.shift(org),
                clip: space.clip,
                parent,
                doc: d as u32,
                node: k as u32,
                prim,
            });
            if let Some(sc) = &n.scroll {
                // the content: a space cut to the port, moved by the offset; then back to this space
                let port = r.shift(org).and(space.clip);
                let off = (sc.at * self.scale).round() as i32;
                let inner = Space {
                    clip: port,
                    base: (r.x0 + org.0, r.y0 + org.1 - off),
                    scroll: bough_x::store::NO_WORD,
                };
                self.doc(sc.doc as usize, inner, at);
            }
        }
    }
    /// One line record: its rect is the run's ink box, its origin the pen origin on the baseline.
    fn line(&mut self, s: usize, run: (u32, u32), color: u32, alpha: u32, pen: (i32, i32)) {
        let Some(r) = self.text.get(run.0, run.1) else {
            return;
        };
        if r.glyphs.is_empty() || r.ink[2] <= r.ink[0] {
            return;
        }
        let rect = Rect {
            x0: pen.0 + r.ink[0],
            y0: pen.1 + r.ink[1],
            x1: pen.0 + r.ink[2],
            y1: pen.1 + r.ink[3],
        };
        self.scene.segs[s].prims.push((
            rect,
            Prim {
                kind: P_TEXT,
                color,
                a: run.0,
                b: run.1,
                alpha,
                ox: -r.ink[0],
                oy: -r.ink[1],
                ..Prim::default()
            },
        ));
    }
}

/// Lay every document out and emit the scene for a window of `w` x `h` device pixels.
pub(crate) fn finish(core: &Core<'_>, w: i32, h: i32) -> Scene {
    let u = bough_x::expr::U::default();
    let mut scene = Scene {
        rects: vec![Vec::new(); core.docs.len()],
        heights: vec![0; core.docs.len()],
        ..Scene::default()
    };
    // documents in creation order: a port is described (and so laid out) before its content
    let mut widths = vec![0f32; core.docs.len()];
    for (d, doc) in core.docs.iter().enumerate() {
        let mut out = vec![[0i32; 4]; doc.nodes.len()];
        if !doc.nodes.is_empty() {
            if d == 0 {
                doc.tree
                    .place_root(0, core.scale, &u, [w as f32, h as f32], &mut out[..]);
                scene.heights[0] = h;
            } else {
                scene.heights[d] = doc
                    .tree
                    .place_row(0, core.scale, &u, widths[d], &mut out[..]);
            }
        }
        for (k, n) in doc.nodes.iter().enumerate() {
            if let Some(sc) = &n.scroll {
                widths[sc.doc as usize] = (out[k][2] - out[k][0]) as f32;
            }
        }
        scene.rects[d] = out;
    }
    let mut e = Emit {
        docs: &core.docs,
        text: core.text,
        scale: core.scale,
        scene,
    };
    e.doc(0, Space::root(w, h), NO);
    e.scene
}
