//! Layout over a heap tree: the same flex function as a fixed shape ([`crate::shape::Flat`]), with no limit on
//! the number of elements, of children of one container, or of wrapping leaves.
//!
//! A fixed shape is a row whose elements are known when the program is built: 96 of them at most, 32 children
//! per container, on the stack. A description that is data (the façade lowered: `bough::lower`) has neither
//! bound, so its elements live in a `Vec` in pre-order and its flex items in a `Vec` per container. This is the
//! general fallback the fixed shape's limits refuse into; nothing here is generated per element type.
//!
//! Every element gets a rect (position `k` of the output is element `k`), whether or not it paints: handlers,
//! clips and lists need the rect of a box that draws nothing.
//!
//! A subtree is asked for its size several times by its ancestors (basis, minimum, cross size, placement). With
//! no bound on depth that would multiply, so each element keeps its last answers by question
//! ([`Tree::memo`]): a question asked again is a lookup.
use crate::el::{Block, Measure, leaf_size};
use crate::flex::{Item, Kids, Lay, Out, Pass, Spec, place_kids, rect, solve};
use crate::shape::{K_BOX, K_LINE, Wrap};
use crate::tree::Node;
use crate::tree::standin::Line;
use crate::val::{Av, Cx, V};
use core::cell::RefCell;

/// One element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct El {
    pub spec: Spec,
    /// [`K_BOX`], [`K_LINE`] or [`crate::shape::K_WRAP`].
    pub kind: u32,
    /// Children (direct), for a box.
    pub count: u32,
    /// The index after this element's subtree.
    pub end: u32,
    /// A line's width and height in device pixels.
    pub a: f32,
    pub b: f32,
    /// A wrapping leaf's index in [`Tree::wraps`].
    pub wrap: u32,
}

type Q = ([Option<V>; 2], [Option<V>; 2], [Av; 2], bool);
const MEMO: usize = 6;

/// Elements in pre-order.
#[derive(Default)]
pub struct Tree {
    pub els: Vec<El>,
    pub wraps: Vec<Box<dyn Wrap>>,
    /// Per element: up to `MEMO` answered questions (a ring), in one allocation.
    memo: RefCell<Vec<(u8, [Option<(Q, [V; 2])>; MEMO])>>,
    /// Size questions answered from the memo, and answered by solving (a count band for the gates).
    pub hits: core::cell::Cell<u64>,
    pub misses: core::cell::Cell<u64>,
}

impl Tree {
    pub fn new() -> Tree {
        Tree::default()
    }
    /// Open a box; close it with [`Tree::close`] after its children.
    pub fn open(&mut self, spec: Spec) -> usize {
        self.els.push(El {
            spec,
            kind: K_BOX,
            count: 0,
            end: 0,
            a: 0.0,
            b: 0.0,
            wrap: 0,
        });
        self.els.len() - 1
    }
    /// Close the box opened at `i`, which has `count` direct children.
    pub fn close(&mut self, i: usize, count: u32) {
        self.els[i].count = count;
        self.els[i].end = self.els.len() as u32;
    }
    /// A leaf of known size (device pixels).
    pub fn line(&mut self, spec: Spec, w: f32, h: f32) -> usize {
        let end = self.els.len() as u32 + 1;
        self.els.push(El {
            spec,
            kind: K_LINE,
            count: 0,
            end,
            a: w,
            b: h,
            wrap: 0,
        });
        self.els.len() - 1
    }
    /// A leaf whose content wraps.
    pub fn wrap(&mut self, spec: Spec, w: Box<dyn Wrap>) -> usize {
        let end = self.els.len() as u32 + 1;
        self.els.push(El {
            spec,
            kind: crate::shape::K_WRAP,
            count: 0,
            end,
            a: 0.0,
            b: 0.0,
            wrap: self.wraps.len() as u32,
        });
        self.wraps.push(w);
        self.els.len() - 1
    }
    fn at(&self, i: usize) -> At<'_> {
        At { t: self, i }
    }
    fn ready(&self) {
        let mut m = self.memo.borrow_mut();
        m.clear();
        m.resize(self.els.len(), (0, [None; MEMO]));
    }
    /// Lay the subtree at `root` out as a window root of `avail` device pixels: `out[k - root]` is element `k`.
    pub fn place_root<O: Out + ?Sized>(
        &self,
        root: usize,
        s: f32,
        u: &bough_expr::U,
        avail: [f32; 2],
        out: &mut O,
    ) {
        self.ready();
        crate::place_root(&self.at(root), s, u, avail, out);
    }
    /// Lay the subtree at `root` out as a row of `width` device pixels (its height is its content's). Returns
    /// the height.
    pub fn place_row<O: Out + ?Sized>(
        &self,
        root: usize,
        s: f32,
        u: &bough_expr::U,
        width: f32,
        out: &mut O,
    ) -> i32 {
        self.ready();
        crate::place_row(&self.at(root), s, u, width, out).0
    }
    /// The height of the subtree at `root` as a row of `width`, with no rects.
    pub fn size_row(&self, root: usize, s: f32, u: &bough_expr::U, width: f32) -> i32 {
        self.ready();
        crate::size_row(&self.at(root), s, u, width).0
    }
}

#[derive(Clone, Copy)]
struct At<'t> {
    t: &'t Tree,
    i: usize,
}
struct Under<'t>(At<'t>);

impl Kids for Under<'_> {
    type St = Vec<Item>;
    const LEAVES: usize = 0;
    const WRAPS: bool = true;
    const VARIES: bool = true;
    fn st(&self) -> Vec<Item> {
        vec![Item::NONE; self.0.t.els[self.0.i].count as usize]
    }
    fn pass<P: Pass>(&self, st: &mut Vec<Item>, p: &mut P) {
        let mut j = self.0.i + 1;
        for it in st.iter_mut() {
            p.kid(&At { t: self.0.t, i: j }, it);
            j = self.0.t.els[j].end as usize;
        }
    }
    fn each<F: FnMut(&Item)>(st: &Vec<Item>, f: &mut F) {
        st.iter().for_each(f)
    }
    fn each_mut<F: FnMut(&mut Item)>(st: &mut Vec<Item>, f: &mut F) {
        st.iter_mut().for_each(f)
    }
    fn empty(&self) -> bool {
        self.0.t.els[self.0.i].count == 0
    }
    fn trees(&self, _: &mut Vec<Node>) {}
}

struct ByRef<'a>(&'a dyn Wrap);
impl Measure for ByRef<'_> {
    const WRAPS: bool = true;
    fn inline(&self) -> (f32, f32) {
        self.0.inline()
    }
    fn block(&self, w: f32) -> Block {
        self.0.block(w)
    }
}

impl Lay for At<'_> {
    const LEAVES: usize = 0;
    const WRAPS: bool = true;
    fn spec(&self) -> &Spec {
        &self.t.els[self.i].spec
    }
    fn size(
        &self,
        cx: &mut Cx,
        known: [Option<V>; 2],
        parent: [Option<V>; 2],
        avail: [Av; 2],
        content_only: bool,
    ) -> [V; 2] {
        let q: Q = (known, parent, avail, content_only);
        if let Some((_, a)) = self.t.memo.borrow()[self.i]
            .1
            .iter()
            .flatten()
            .find(|e| e.0 == q)
        {
            let a = *a;
            self.t.hits.set(self.t.hits.get() + 1);
            return a;
        }
        self.t.misses.set(self.t.misses.get() + 1);
        let e = &self.t.els[self.i];
        let a = if e.kind == K_BOX {
            solve(
                &e.spec,
                &Under(*self),
                cx,
                known,
                parent,
                avail,
                content_only,
                false,
            )
            .outer
        } else if e.kind == K_LINE {
            leaf_size(
                &e.spec,
                &Line(e.a, e.b),
                cx,
                known,
                parent,
                avail,
                content_only,
            )
        } else {
            leaf_size(
                &e.spec,
                &ByRef(&*self.t.wraps[e.wrap as usize]),
                cx,
                known,
                parent,
                avail,
                content_only,
            )
        };
        let mut m = self.t.memo.borrow_mut();
        let (next, slot) = &mut m[self.i];
        slot[*next as usize % MEMO] = Some((q, a));
        *next = next.wrapping_add(1);
        a
    }
    fn reported(&self, cx: &mut Cx, size: [V; 2]) -> [V; 2] {
        if self.t.els[self.i].kind == K_BOX {
            crate::flex::reported(self.spec(), cx, size)
        } else {
            size
        }
    }
    fn place<O: Out + ?Sized>(
        &self,
        cx: &mut Cx,
        size: [V; 2],
        org: [V; 2],
        out: &mut O,
        k0: usize,
    ) -> usize {
        let e = &self.t.els[self.i];
        out.put(k0, rect(cx, size, org));
        let mut k = k0 + 1;
        if e.kind == K_BOX && e.count != 0 {
            let av = [Av::Def(size[0]), Av::Def(size[1])];
            let mut sol = solve(
                &e.spec,
                &Under(*self),
                cx,
                [Some(size[0]), Some(size[1])],
                [None; 2],
                av,
                false,
                true,
            );
            k = place_kids(&Under(*self), cx, &mut sol, org, out, k);
        }
        // an element's subtree takes exactly its elements' positions
        debug_assert_eq!(k - k0, e.end as usize - self.i);
        k
    }
    fn tree(&self) -> Node {
        unimplemented!("a heap tree is laid out by the flex function directly")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flex::Dim;

    #[test]
    fn a_column_of_a_thousand_children_past_every_fixed_limit() {
        let mut t = Tree::new();
        let mut root = Spec::new(true);
        root.w = Dim::Pct(1.0);
        let r = t.open(root);
        for _ in 0..1000 {
            let mut row = Spec::new(false);
            row.grow = 0.0;
            let b = t.open(row);
            for _ in 0..40 {
                let mut c = Spec::new(false);
                c.grow = 1.0;
                c.h = Dim::Pt(10.0);
                t.line(c, 4.0, 20.0);
            }
            t.close(b, 40);
        }
        t.close(r, 1000);
        let mut out = vec![[0i32; 4]; t.els.len()];
        t.place_root(
            0,
            2.0,
            &bough_expr::U::default(),
            [800.0, 600.0],
            &mut out[..],
        );
        assert_eq!(out[0], [0, 0, 800, 20_000]);
        // row 999 is 20 px tall (10 pt at scale 2) at y = 999 * 20; its 40 children share 800 px
        let last_row = 1 + 999 * 41;
        assert_eq!(out[last_row], [0, 19_980, 800, 20_000]);
        assert_eq!(out[last_row + 40], [780, 19_980, 800, 20_000]);
    }

    #[test]
    fn a_tree_sixteen_deep_is_asked_a_bounded_number_of_times() {
        let mut t = Tree::new();
        let mut opens = Vec::new();
        for d in 0..16 {
            let mut s = Spec::new(d % 2 == 0);
            s.pad = [1.0; 4];
            if d == 0 {
                (s.w, s.h) = (Dim::Pct(1.0), Dim::Pct(1.0));
            }
            opens.push(t.open(s));
            t.line(Spec::new(false), 10.0, 10.0);
        }
        t.line(Spec::new(false), 30.0, 12.0);
        for (d, &i) in opens.iter().enumerate().rev() {
            let _ = d;
            t.close(i, 2);
        }
        let mut out = vec![[0i32; 4]; t.els.len()];
        t.place_root(
            0,
            1.0,
            &bough_expr::U::default(),
            [400.0, 300.0],
            &mut out[..],
        );
        // without the memo the questions multiply per level; with it they stay linear in the elements
        assert!(
            t.misses.get() < 40 * t.els.len() as u64,
            "{} solves for {} elements",
            t.misses.get(),
            t.els.len()
        );
        assert_eq!(out[0], [0, 0, 400, 300]);
    }
}
