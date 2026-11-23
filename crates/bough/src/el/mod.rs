//! The author surface of `designs/reactivity/final` (02 §1–4, 03 §2, 04 §2–3): one concrete, frame-borrowed
//! element currency, [`El`], and a [`View`] whose `view` returns it.
//!
//! ```ignore
//! struct Counter { count: Signal<u32> }
//! impl Counter { fn inc(&mut self) { *self.count += 1; } }
//! impl View for Counter {
//!     fn view(&self) -> El<'_> {
//!         row((text(self.count.map(|n| format!("Count: {n}"))), button("+").on_click(Counter::inc))).gap(8)
//!     }
//! }
//! ```
//!
//! * Every builder and setter returns `El<'f>`, so `if`, `match` and `Option` arms unify without ceremony.
//! * A property port takes a constant, a borrowed `Signal` field (`.bg(&self.tint)`), or a signal mapped
//!   through a closure (`self.n.map(f)`): a signal field of the view becomes a direct port, and publishing it
//!   patches the element without rerunning `view` (`plug.rs`).
//! * Children: an `El`, a tuple, an `Option`, a `Vec`, or an iterator (`column(rows.iter().map(row))`).
//! * Routes are methods: `.on_click(Counter::inc)`, `.on_key(Key::Escape, Pane::close)`,
//!   `.on_key(Key::Minus, Pane::split.with(Axis::Horizontal))`.
//!
//! An `El` is a cursor into a per-thread draft (`arena.rs`) that lives for one description; the kernel walks
//! it into the retained records (`walk.rs`). Identity is parent × ordinal × kind (L3): a child whose kind
//! changes at its position is remounted there, and only there; an iterator's items keep their position.
mod arena;
pub mod plug;
mod walk;

use crate::color::{rgb, Color};
use crate::input::{Ev, Key, RouteFn};
use crate::layout::{Align, Dim, Justify, Spec, Styled};
use arena::*;
use plug::{IntoProp, Len, Seal, Text};
use std::any::TypeId;
use std::marker::PhantomData;
use std::ptr::{self, NonNull};

pub use arena::stats as arena_stats;
pub(crate) use arena::Walk;

/// Retire this thread's description (what the end of a walk does): for benchmarks of `view` alone.
#[doc(hidden)]
pub fn rewind_for_bench() {
    drop(Walk::enter());
}
pub use plug::{Map, Source};

/// The element currency: a cursor to one element of the description being written. Move-only, `!Send`,
/// covariant in `'f`, and valid for the one description it was written in: kept past it, every use panics.
#[must_use = "an element does nothing until it is returned from `view` or placed in a container"]
pub struct El<'f> {
    node: NonNull<DNode>,
    turn: u64,
    _f: PhantomData<(&'f (), *const ())>,
}

/// A view: a plain struct whose fields are its state, described by `view` (02 §1). Every `View` is also the
/// kernel's view (`App`, `Application::run` and the test drivers take it unchanged).
pub trait View: Sized + 'static {
    fn view(&self) -> El<'_>;
    /// The one effect site: what crossed this view's boundaries this turn (04 §1).
    fn changed(&mut self, _d: crate::changed::Changed<'_, Self>) {}
}

impl<T: View> crate::app::View for T {
    #[inline]
    fn view(&self) -> impl crate::elem::Element<Self> + '_ {
        View::view(self)
    }
    #[inline]
    fn changed(&mut self, d: crate::changed::Changed<'_, Self>) {
        View::changed(self, d)
    }
}

impl<'f> El<'f> {
    #[inline]
    fn new(kind: u8, spec: Spec) -> El<'f> {
        let (node, turn) = arena::node(kind, spec);
        El { node, turn, _f: PhantomData }
    }
    /// The node, checked to be of this description.
    #[inline(always)]
    #[track_caller]
    #[allow(clippy::mut_from_ref)]
    fn n(&self) -> &mut DNode {
        // SAFETY: node memory is never freed (`arena.rs`), so reading the turn number is always defined; when
        // it matches, the node is this handle's own and this handle (move-only) is the only one that names it.
        let n = unsafe { &mut *self.node.as_ptr() };
        if n.turn != self.turn {
            stale()
        }
        n
    }
    #[inline(always)]
    #[track_caller]
    #[allow(clippy::mut_from_ref)]
    fn kind_is(&self, k: u8, what: &str) -> &mut DNode {
        let n = self.n();
        if n.kind != k {
            wrong_kind(what, n.kind, k)
        }
        n
    }
    #[inline(always)]
    #[track_caller]
    #[allow(clippy::mut_from_ref)]
    fn ext(&self) -> &mut Ext {
        let n = self.n();
        if n.ext.is_null() {
            n.ext = arena::put(Ext::NEW);
        }
        // SAFETY: `ext` points into this turn's bump store and belongs to this node.
        unsafe { &mut *n.ext }
    }
    /// The node, for the walk; checked.
    #[inline]
    #[track_caller]
    pub(crate) fn raw(self) -> NonNull<DNode> {
        self.n();
        self.node
    }

    // ------------------------------------------------------------------- layout (every element)

    /// Width in points: a length, or a `f32` signal / mapped source (the box restyles when it publishes).
    #[track_caller]
    pub fn w<P: IntoProp<'f, f32>>(self, p: P) -> Self {
        let pl = p.plug::<crate::boxops::ApplyBoxW>(Seal);
        if pl.w.is_null() {
            self.n().spec.w = Dim::Pt(pl.v);
        } else {
            self.kind_is(K_BOX, "a width source").ops |= OP_W;
            let x = self.ext();
            (x.w, x.ww) = (pl.v, pl.w);
        }
        self
    }
    /// Height in points (as [`w`](Self::w)).
    #[track_caller]
    pub fn h<P: IntoProp<'f, f32>>(self, p: P) -> Self {
        let pl = p.plug::<crate::boxops::ApplyBoxH>(Seal);
        if pl.w.is_null() {
            self.n().spec.h = Dim::Pt(pl.v);
        } else {
            self.kind_is(K_BOX, "a height source").ops |= OP_H;
            let x = self.ext();
            (x.h, x.wh) = (pl.v, pl.w);
        }
        self
    }
    pub fn size(self, w: impl Len, h: impl Len) -> Self {
        let n = self.n();
        (n.spec.w, n.spec.h) = (Dim::Pt(w.pt()), Dim::Pt(h.pt()));
        self
    }
    /// Width / height as a fraction of the parent (1.0 = 100%).
    pub fn w_pct(self, f: f32) -> Self {
        self.n().spec.w = Dim::Pct(f);
        self
    }
    pub fn h_pct(self, f: f32) -> Self {
        self.n().spec.h = Dim::Pct(f);
        self
    }
    pub fn min_w(self, v: impl Len) -> Self {
        self.n().spec.min_w = v.pt();
        self
    }
    pub fn min_h(self, v: impl Len) -> Self {
        self.n().spec.min_h = v.pt();
        self
    }
    pub fn pad(self, l: impl Len, t: impl Len, r: impl Len, b: impl Len) -> Self {
        self.n().spec.pad = [l.pt(), t.pt(), r.pt(), b.pt()];
        self
    }
    pub fn p(self, v: impl Len) -> Self {
        let v = v.pt();
        self.n().spec.pad = [v; 4];
        self
    }
    pub fn px(self, v: impl Len) -> Self {
        let n = self.n();
        (n.spec.pad[0], n.spec.pad[2]) = (v.pt(), v.pt());
        self
    }
    pub fn py(self, v: impl Len) -> Self {
        let n = self.n();
        (n.spec.pad[1], n.spec.pad[3]) = (v.pt(), v.pt());
        self
    }
    pub fn margin(self, l: impl Len, t: impl Len, r: impl Len, b: impl Len) -> Self {
        self.n().spec.margin = [l.pt(), t.pt(), r.pt(), b.pt()];
        self
    }
    pub fn gap(self, v: impl Len) -> Self {
        self.n().spec.gap = v.pt();
        self
    }
    pub fn grow(self, v: f32) -> Self {
        self.n().spec.grow = v;
        self
    }
    pub fn shrink(self, v: f32) -> Self {
        self.n().spec.shrink = v;
        self
    }
    pub fn items(self, a: Align) -> Self {
        self.n().spec.items = Some(a);
        self
    }
    pub fn align_self(self, a: Align) -> Self {
        self.n().spec.self_align = Some(a);
        self
    }
    pub fn justify(self, j: Justify) -> Self {
        self.n().spec.justify = Some(j);
        self
    }
    /// Centre the children on both axes.
    pub fn center(self) -> Self {
        self.items(Align::Center).justify(Justify::Center)
    }
    /// `overflow: clip`: the box's content overflow does not reach its parent's layout.
    pub fn clip_overflow(self) -> Self {
        self.n().spec.clip = true;
        self
    }
    /// `position: relative` (the containing block of `abs` children).
    pub fn relative(self) -> Self {
        self.n().spec.relative = true;
        self
    }
    /// Absolutely positioned at (x, y) points from the parent's padding box.
    pub fn abs(self, x: impl Len, y: impl Len) -> Self {
        self.n().spec.set_abs(Some((x.pt(), y.pt())));
        self
    }
    /// Apply a layout recipe (`const BAR: Flex = Flex::row().align(Align::Center).gap(8)`).
    #[track_caller]
    pub fn layout(self, f: crate::ui::Flex) -> Self {
        let n = self.kind_is(K_BOX, "a layout recipe");
        n.spec.container = true;
        n.spec.column = f.column;
        n.spec.gap = f.gap;
        if f.align.is_some() {
            n.spec.items = f.align;
        }
        if f.justify.is_some() {
            n.spec.justify = f.justify;
        }
        self
    }
    /// `f(self)`: apply a mixin in a chain (`column((..)).apply(card)`, 02 §2).
    pub fn apply<R>(self, f: impl FnOnce(Self) -> R) -> R {
        f(self)
    }

    // ------------------------------------------------------------------- boxes

    /// The box's fill: a colour, a `Color` signal field, or a mapped source.
    #[track_caller]
    pub fn bg<P: IntoProp<'f, Color>>(self, p: P) -> Self {
        let pl = p.plug::<crate::elem::ApplyBg>(Seal);
        let n = self.kind_is(K_BOX, "a background");
        (n.paint, n.ink) = (pl.v.0, pl.w);
        n.flags |= F_DRAW;
        self
    }
    #[track_caller]
    pub fn rounded(self, r: impl Len) -> Self {
        let n = self.kind_is(K_BOX, "a corner radius");
        n.radius = r.pt();
        n.flags |= F_DRAW;
        self
    }
    #[track_caller]
    pub fn border(self, w: impl Len, c: Color) -> Self {
        let n = self.kind_is(K_BOX, "a border");
        (n.bw, n.bc) = (w.pt(), c.0);
        n.flags |= F_DRAW | F_BORDER;
        self
    }
    /// Paint opacity (0..=1) of a painted box: a constant, an `f32` signal field, or a mapped source.
    #[track_caller]
    pub fn opacity<P: IntoProp<'f, f32>>(self, p: P) -> Self {
        let pl = p.plug::<crate::boxops::ApplyOpacity>(Seal);
        self.kind_is(K_BOX, "an opacity").ops |= OP_ALPHA;
        let x = self.ext();
        (x.alpha, x.wa) = (pl.v, pl.w);
        self
    }
    /// Paint offset along x (points) of a painted box: its paint moves, its layout place does not.
    #[track_caller]
    pub fn translate_x<P: IntoProp<'f, f32>>(self, p: P) -> Self {
        let pl = p.plug::<crate::boxops::ApplyTx>(Seal);
        self.kind_is(K_BOX, "a translation").ops |= OP_TX;
        let x = self.ext();
        (x.tx, x.wtx) = (pl.v, pl.w);
        self
    }
    /// Paint offset along y (points); see [`translate_x`](Self::translate_x).
    #[track_caller]
    pub fn translate_y<P: IntoProp<'f, f32>>(self, p: P) -> Self {
        let pl = p.plug::<crate::boxops::ApplyTy>(Seal);
        self.kind_is(K_BOX, "a translation").ops |= OP_TY;
        let x = self.ext();
        (x.ty, x.wty) = (pl.v, pl.w);
        self
    }
    /// One more child in flow: the one child verb (`div()` is born empty; `row`/`column` take theirs).
    #[track_caller]
    pub fn child(self, c: impl ChildSeq<'f>) -> Self {
        let n = self.kind_is(K_BOX, "a child");
        c.into_children(n, Seal);
        self
    }
    /// The style while the pointer is over the element: `.hover(|s| s.bg(theme.surface_hover))`.
    #[track_caller]
    pub fn hover(self, f: impl FnOnce(Hover) -> Hover) -> Self {
        self.kind_is(K_BOX, "a hover style");
        let h = f(Hover { bg: None });
        if let Some(c) = h.bg {
            let x = self.ext();
            x.hover = c.0;
            x.flags |= X_HOVER;
        }
        self
    }

    // ------------------------------------------------------------------- text

    /// The text's ink: a colour, a `Color` signal field, or a mapped source (03 §5: the one inking port).
    #[track_caller]
    pub fn fg<P: IntoProp<'f, Color>>(self, p: P) -> Self {
        let pl = p.plug::<crate::boxops::ApplyTextColor>(Seal);
        let n = self.kind_is(K_TEXT, "an ink colour");
        (n.paint, n.ink) = (pl.v.0, pl.w);
        self
    }
    /// Font size and line height, points.
    #[track_caller]
    pub fn font(self, size: impl Len, line_height: impl Len) -> Self {
        let n = self.kind_is(K_TEXT, "a font size");
        (n.size, n.lh) = (size.pt(), line_height.pt());
        self
    }

    // ------------------------------------------------------------------- routes

    /// Activation (press and release over the element): `.on_click(Counter::inc)`.
    pub fn on_click<M>(self, r: impl Route<M>) -> Self {
        self.route(A_CLICK, Key::Escape, r)
    }
    /// The primary button pressed over the element.
    pub fn on_press<M>(self, r: impl Route<M>) -> Self {
        self.route(A_PRESS, Key::Escape, r)
    }
    /// The pointer entered (`ev.on`) or left the element.
    pub fn on_hover<M>(self, r: impl Route<M>) -> Self {
        self.route(A_HOVER, Key::Escape, r)
    }
    /// Pointer travel while pressed (`ev.dx`, `ev.dy` since the press; `ev.on` false at release).
    pub fn on_drag<M>(self, r: impl Route<M>) -> Self {
        self.route(A_DRAG, Key::Escape, r)
    }
    /// `key` pressed while the element, or an element inside it, has focus. Makes the element focusable.
    pub fn on_key<M>(self, key: Key, r: impl Route<M>) -> Self {
        self.route(A_KEY, key, r)
    }
    /// Any key no element nearer the focus took (`ev.key`). Makes the element focusable.
    pub fn on_any_key<M>(self, r: impl Route<M>) -> Self {
        self.route(A_ANY_KEY, Key::Escape, r)
    }
    /// Tab can move focus here.
    pub fn focusable(self) -> Self {
        self.ext().flags |= X_FOCUS;
        self
    }
    #[track_caller]
    fn route<M>(self, kind: u8, key: Key, r: impl Route<M>) -> Self {
        let (tramp, recipe, recv, recv_name) = r.lower(Seal);
        let x = self.ext();
        x.acts = arena::put(ActReq { next: x.acts, tramp, recipe, recv, recv_name, key, kind });
        self
    }
}

#[cold]
#[inline(never)]
#[track_caller]
fn stale() -> ! {
    panic!("an `El` was used after the description it was written in (an element lives for one `view`)")
}

fn kind_name(kind: u8) -> &'static str {
    match kind {
        K_BOX => "a box",
        K_TEXT => "a text",
        K_INPUT => "a text input",
        K_IMAGE => "an image",
        _ => "a group",
    }
}

#[cold]
#[inline(never)]
#[track_caller]
fn wrong_kind(what: &str, kind: u8, wants: u8) -> ! {
    panic!("{what} is set on {}, not on {}", kind_name(wants), kind_name(kind))
}

/// What `.hover(|s| ..)` sets.
pub struct Hover {
    bg: Option<Color>,
}
impl Hover {
    pub fn bg(mut self, c: Color) -> Hover {
        self.bg = Some(c);
        self
    }
}

impl Styled for El<'_> {
    fn spec_mut(&mut self) -> &mut Spec {
        &mut self.n().spec
    }
}

// =============================================================================== builders

/// The styled-box primitive, born empty: add content with `.child(x)`, arrange it with `.layout(recipe)`.
pub fn div<'f>() -> El<'f> {
    El::new(K_BOX, Spec::new(true, true))
}
/// Children top to bottom.
pub fn column<'f>(c: impl ChildSeq<'f>) -> El<'f> {
    let e = El::new(K_BOX, Spec::new(true, true));
    c.into_children(e.n(), Seal);
    e
}
/// Children left to right.
pub fn row<'f>(c: impl ChildSeq<'f>) -> El<'f> {
    let e = El::new(K_BOX, Spec::new(true, false));
    c.into_children(e.n(), Seal);
    e
}
/// A leaf rectangle of `w` x `h` points (paint it with `.bg`).
pub fn block<'f>(w: impl Len, h: impl Len) -> El<'f> {
    let mut s = Spec::new(false, false);
    (s.w, s.h) = (Dim::Pt(w.pt()), Dim::Pt(h.pt()));
    El::new(K_BOX, s)
}

fn text_node<'f, P: IntoProp<'f, Text>>(t: P, wrap: bool) -> El<'f> {
    let pl = t.plug::<crate::elem::ApplyText>(Seal);
    let e = El::new(K_TEXT, Spec::new(false, false));
    let n = e.n();
    (n.text, n.len, n.flags, n.tw) = (pl.v.ptr, pl.v.len, pl.v.flags | if wrap { F_WRAP } else { 0 }, pl.w);
    (n.size, n.lh, n.paint) = (14.0, 20.0, rgb(0x000000).0);
    e
}
/// One line of text, as wide as its text: a string, an integer, a `Signal<String>` field, or a mapped source
/// (`self.count.map(|n| format!("{n}"))`; a signal makes it a direct port).
pub fn text<'f, P: IntoProp<'f, Text>>(t: P) -> El<'f> {
    text_node(t, false)
}
/// A paragraph that wraps at its width (set `.w(..)`, or it fills the available width).
pub fn para<'f, P: IntoProp<'f, Text>>(t: P) -> El<'f> {
    text_node(t, true)
}

const BUTTON: Color = rgb(0x2f6bff);
const BUTTON_HOVER: Color = rgb(0x4a80ff);

/// A push button: its label in a padded, rounded box. Give it a route with `.on_click(Owner::method)`.
pub fn button<'f, P: IntoProp<'f, Text>>(label: P) -> El<'f> {
    row(text(label).fg(rgb(0xffffff))).center().px(14).py(6).bg(BUTTON).rounded(6).hover(|s| s.bg(BUTTON_HOVER))
}

/// A one-line text input bound two-way to a `Signal<String>` field of the view.
pub fn text_input<'f>(s: &'f crate::signal::Signal<String>) -> El<'f> {
    let e = El::new(K_INPUT, Spec::new(false, false));
    let n = e.n();
    (n.text, n.size, n.lh) = (s as *const _ as *const u8, 240.0, 32.0);
    e
}
impl<'f> El<'f> {
    /// A text input's size in points.
    #[track_caller]
    pub fn input_size(self, w: impl Len, h: impl Len) -> Self {
        let n = self.kind_is(K_INPUT, "an input size");
        (n.size, n.lh) = (w.pt(), h.pt());
        self
    }
}

/// A raster image (PNG bytes) in flow, sized by layout (by default its pixel size, in points).
pub fn image<'f>(png: &'static [u8]) -> El<'f> {
    let b = crate::ui::image(png);
    let e = El::new(K_IMAGE, b.spec);
    e.n().text = arena::put(b) as *const u8;
    e
}

// =============================================================================== children

/// What a container takes as its children (02 §4): an [`El`], a tuple of children, an `Option`, a `Vec`, an
/// array, or an iterator (`.map(..)`, `.filter_map(..)`, `.chain(..)`, ...; any other through `each(iter)`).
/// Fixed forms (an `El`, a tuple) keep their positions; `Option`, `Vec`, arrays and iterators are dynamic parts
/// that realign by position.
pub trait ChildSeq<'f>: Sized {
    /// Append to `parent`'s children.
    #[doc(hidden)]
    #[allow(private_interfaces)]
    fn into_children(self, parent: &mut DNode, _: Seal);
    /// As one item of a dynamic part: one node.
    #[doc(hidden)]
    #[allow(private_interfaces)]
    fn into_item(self, _: Seal) -> *mut DNode {
        let f = El::new(K_FRAG, Spec::new(false, false));
        let n = f.n();
        self.into_children(n, Seal);
        n
    }
}

#[inline]
fn append(parent: &mut DNode, c: *mut DNode) {
    if parent.last.is_null() {
        parent.first = c;
    } else {
        // SAFETY: `last` is a node of this description, linked by this function.
        unsafe { (*parent.last).next = c };
    }
    parent.last = c;
}

impl<'f> ChildSeq<'f> for El<'f> {
    #[inline]
    #[track_caller]
    #[allow(private_interfaces)]
    fn into_children(self, parent: &mut DNode, _: Seal) {
        append(parent, self.raw().as_ptr());
    }
    #[inline]
    #[track_caller]
    #[allow(private_interfaces)]
    fn into_item(self, _: Seal) -> *mut DNode {
        self.raw().as_ptr()
    }
}
impl<'f> ChildSeq<'f> for () {
    #[inline]
    #[allow(private_interfaces)]
    fn into_children(self, _: &mut DNode, _: Seal) {}
}

macro_rules! tuple_children {
    ($($t:ident),+) => {
        impl<'f, $($t: ChildSeq<'f>),+> ChildSeq<'f> for ($($t,)+) {
            #[allow(non_snake_case, private_interfaces)]
            #[inline]
            fn into_children(self, parent: &mut DNode, _: Seal) {
                let ($($t,)+) = self;
                $($t.into_children(parent, Seal);)+
            }
        }
    };
}
tuple_children!(A);
tuple_children!(A, B);
tuple_children!(A, B, C);
tuple_children!(A, B, C, D);
tuple_children!(A, B, C, D, E);
tuple_children!(A, B, C, D, E, F);
tuple_children!(A, B, C, D, E, F, G);
tuple_children!(A, B, C, D, E, F, G, H);
tuple_children!(A, B, C, D, E, F, G, H, I);
tuple_children!(A, B, C, D, E, F, G, H, I, J);
tuple_children!(A, B, C, D, E, F, G, H, I, J, K);
tuple_children!(A, B, C, D, E, F, G, H, I, J, K, L);

/// A group (a dynamic part) holding `items`.
#[inline]
fn group<'f, C: ChildSeq<'f>>(parent: &mut DNode, items: impl Iterator<Item = C>) {
    let g = El::new(K_GROUP, Spec::new(false, false));
    let gn = g.n();
    for c in items {
        append(gn, c.into_item(Seal));
    }
    append(parent, gn);
}

impl<'f, C: ChildSeq<'f>> ChildSeq<'f> for Option<C> {
    #[inline]
    #[allow(private_interfaces)]
    fn into_children(self, parent: &mut DNode, _: Seal) {
        group(parent, self.into_iter());
    }
}
impl<'f, C: ChildSeq<'f>> ChildSeq<'f> for Vec<C> {
    #[inline]
    #[allow(private_interfaces)]
    fn into_children(self, parent: &mut DNode, _: Seal) {
        group(parent, self.into_iter());
    }
}
impl<'f, C: ChildSeq<'f>, const N: usize> ChildSeq<'f> for [C; N] {
    #[inline]
    #[allow(private_interfaces)]
    fn into_children(self, parent: &mut DNode, _: Seal) {
        group(parent, self.into_iter());
    }
}
impl<'f, I: Iterator<Item: ChildSeq<'f>>> ChildSeq<'f> for crate::elem::Each<I> {
    #[inline]
    #[allow(private_interfaces)]
    fn into_children(self, parent: &mut DNode, _: Seal) {
        group(parent, self.0);
    }
}

macro_rules! iter_children {
    ($($p:ident)::+ <$($g:ident),*> $(, const $c:ident)?) => {
        impl<'f, $($g,)* $(const $c: usize)?> ChildSeq<'f> for $($p)::+<$($g,)* $($c)?>
        where
            $($p)::+<$($g,)* $($c)?>: Iterator<Item: ChildSeq<'f>>,
        {
            #[inline]
            #[allow(private_interfaces)]
            fn into_children(self, parent: &mut DNode, _: Seal) {
                group(parent, self);
            }
        }
    };
}
iter_children!(std::iter::Map<I, F>);
iter_children!(std::iter::FilterMap<I, F>);
iter_children!(std::iter::Filter<I, P>);
iter_children!(std::iter::Chain<A, B>);
iter_children!(std::iter::Take<I>);
iter_children!(std::iter::Skip<I>);
iter_children!(std::iter::Rev<I>);
iter_children!(std::iter::StepBy<I>);
iter_children!(std::iter::TakeWhile<I, P>);
iter_children!(std::iter::SkipWhile<I, P>);
iter_children!(std::iter::MapWhile<I, P>);
iter_children!(std::iter::Once<T>);
iter_children!(std::iter::Empty<T>);
iter_children!(std::iter::FromFn<F>);
iter_children!(std::iter::Zip<A, B>);
iter_children!(std::vec::IntoIter<T>);
iter_children!(std::option::IntoIter<T>);
iter_children!(std::array::IntoIter<T>, const N);

// =============================================================================== routes

/// A route (04 §2–3): a method of the view (`Counter::inc`, `fn(&mut Self)`), one that takes the event
/// (`fn(&mut Self, &Ev)`), a closure of either shape that captures at most 16 bytes, or a method bound to a
/// value with [`With::with`]. The receiver type is checked against the mounting view.
pub trait Route<M>: Sized {
    #[doc(hidden)]
    #[allow(private_interfaces)]
    fn lower(self, _: Seal) -> (RouteFn, [u8; 16], TypeId, &'static str);
}

#[inline]
fn recipe<F: Copy>(f: F) -> [u8; 16] {
    const { assert!(size_of::<F>() <= 16, "a route captures at most 16 bytes: bind a value with `Owner::method.with(value)`, or keep it in the view's state") };
    let mut r = [0u8; 16];
    // SAFETY: `size_of::<F>() <= 16` (checked at compile time); an unaligned write into a byte array.
    unsafe { ptr::write_unaligned(r.as_mut_ptr() as *mut F, f) };
    r
}

impl<R: 'static, F: Fn(&mut R) + Copy + 'static> Route<fn(&mut R)> for F {
    #[inline]
    #[allow(private_interfaces)]
    fn lower(self, _: Seal) -> (RouteFn, [u8; 16], TypeId, &'static str) {
        (crate::input::call_plain::<R, F>, recipe(self), TypeId::of::<R>(), std::any::type_name::<R>())
    }
}
impl<R: 'static, F: Fn(&mut R, &Ev) + Copy + 'static> Route<fn(&mut R, &Ev)> for F {
    #[inline]
    #[allow(private_interfaces)]
    fn lower(self, _: Seal) -> (RouteFn, [u8; 16], TypeId, &'static str) {
        (crate::input::call_ev::<R, F>, recipe(self), TypeId::of::<R>(), std::any::type_name::<R>())
    }
}

/// A method bound to a value: `Pane::split.with(Axis::Horizontal)` (the capture, made explicit and owned).
#[derive(Clone, Copy)]
pub struct Bound<F, A> {
    f: F,
    a: A,
}
/// `.with(value)` on a method taking `(&mut Self, value)`.
pub trait With<A>: Sized {
    fn with(self, a: A) -> Bound<Self, A>;
}
impl<F, A> With<A> for F {
    #[inline]
    fn with(self, a: A) -> Bound<F, A> {
        Bound { f: self, a }
    }
}
impl<R: 'static, A: Copy + 'static, F: Fn(&mut R, A) + Copy + 'static> Route<(fn(&mut R, A),)> for Bound<F, A> {
    #[inline]
    #[allow(private_interfaces)]
    fn lower(self, _: Seal) -> (RouteFn, [u8; 16], TypeId, &'static str) {
        let (f, a) = (self.f, self.a);
        Route::<fn(&mut R)>::lower(move |r: &mut R| f(r, a), Seal)
    }
}

// =============================================================================== tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "used after the description")]
    fn a_kept_element_panics_by_name() {
        let kept: El<'static> = div();
        drop(arena::Walk::enter());
        let _ = kept.p(4);
    }

    #[test]
    fn arms_unify_and_children_link_in_order() {
        let pick = |b: bool| -> El<'static> { if b { text("a") } else { div().bg(rgb(0xff0000)) } };
        let e = row((pick(true), pick(false), Some(text("c")), (0..3).map(text)));
        let n = e.n();
        let mut kinds = Vec::new();
        let mut c = n.first;
        while !c.is_null() {
            // SAFETY: nodes of this description.
            unsafe {
                kinds.push((*c).kind);
                c = (*c).next;
            }
        }
        assert_eq!(kinds, [K_TEXT, K_BOX, K_GROUP, K_GROUP]);
        drop(arena::Walk::enter());
    }
}
