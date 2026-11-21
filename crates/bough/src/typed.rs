//! The prelude on the typed element layer (Integration Card 4).
//!
//! A façade element value converts ([`Typed`]) to a value of `bough-x`'s element layer (`el.rs`): its type is its
//! records and its layout words, its operands are expressions stored in record words, and `World<V, B>` mounts it
//! once, settles it by differentiating the columns that read a changed block word, and re-describes it by a
//! typed compare. Nothing here is a node tree: there is no per-element allocation and no erased handler.
//!
//! * A box is a `bough_lay::LBox` with one record ([`PBox`]), painted or not (alpha 0).
//! * One line of text is one record ([`PLine`]) naming a run of the lowered runtime's run store.
//! * `sig!(field)` of a word-sized type is [`Dy`]: the block word assigned to the field's proven offset.
//! * `hover_bg` is [`HSel`]: a selection on the one hidden hover word, set by a handler on the box's record.
//! * A handler is a typed component on the record ([`HClick`], [`HEv`]); the owner is reached by type
//!   ([`Reach`]): the root view directly, a list row's item by the list's proven offset and the item's slot.
//! * `.scroll(sig!(..))` and `list(sig!(..))` are a space moved by the offset's block word; rows by exposure.
//!
//! A view whose element type has no conversion ([`MaybeTyped::OK`] is false), or whose description asks for
//! something a typed row cannot hold (a wrapped paragraph, a 48th bound word), is not admitted: [`App::new`]
//! says so by name and the caller drives it through `bough::lower` (the same world type, no legacy runtime).
use crate::app::{Config, View};
use crate::color::Color;
use crate::elem::TextSrc;
use crate::input::{Click, Drag, Ev, Focusable, Hover, HoverBg, Key, Mods, On, OnKey, Press};
use crate::layout::{LBox, Para};
use crate::list::List;
use crate::listview::{LList, ListRows};
use crate::lower::canvas::{Canvas, Images};
use crate::lower::provenance::count;
use crate::lower::text::TextSys;
use crate::signal::{Signal, WMap, Wired};
use crate::template::ScrollBox;
use bough_lay::{Rec1, Shape};
use bough_x::draw::{Draws, Prims};
use bough_x::el::{Boxy, Flow, Hd, Paints, Part, Scroll, Seg};
use bough_x::event::{Event, Handler, kind};
use bough_x::expr::{APP_WORDS, Code, Color as XColor, Cx as ECx, Expr, StageId, Turn, U};
use bough_x::prim::{P_BOX, P_TEXT, Prim};
use bough_x::sink::{Cx, Geo, Leaf, Mk, Row, Sink};
use bough_x::store::{Arch, Backend, ColRef, NO_WORD, Rect, Site, Tiles};
use bough_x::world::World;
use std::any::TypeId;
use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::rc::Rc;

/// The hidden block word that says which `hover_bg` record is hovered.
const HOVER: usize = APP_WORDS - 1;
const NONE: u32 = u32::MAX;
/// Every application word: what a dynamic operand may read.
const APP: u64 = (1 << APP_WORDS) - 1;

// ---- what a description shares -------------------------------------------------------------------------------

struct SlotEnt {
    off: u32,
    read: unsafe fn(*const u8) -> u32,
    write: unsafe fn(*mut u8, u32),
}

/// What conversions consult while a view is being described: the run store, the block words assigned so far,
/// the row being described, refusals.
struct Shared {
    text: Rc<RefCell<TextSys>>,
    slots: RefCell<Vec<SlotEnt>>,
    refused: RefCell<Vec<&'static str>>,
    scale: Cell<f32>,
    root: TypeId,
    /// The list row being described: (list offset in its owner, slot index, slot generation, row + 1); 0: none.
    row: Cell<(u32, u32, u32, u32)>,
    /// `hover_bg` boxes described so far in this row (or in the fixed part).
    ord: Cell<u32>,
    describes: Cell<u64>,
}
thread_local! {
    static CX: Cell<*const Shared> = const { Cell::new(std::ptr::null()) };
}
fn with<T>(f: impl FnOnce(&Shared) -> T) -> T {
    let p = CX.with(Cell::get);
    assert!(!p.is_null(), "a typed conversion outside a description");
    // SAFETY: `enter` set the pointer to a `Shared` that outlives the call it wraps and cleared it after
    // (also on unwind); a conversion runs only inside that call, on this thread.
    f(unsafe { &*p })
}
fn enter<T>(sh: &Shared, f: impl FnOnce() -> T) -> T {
    struct Back(*const Shared);
    impl Drop for Back {
        fn drop(&mut self) {
            CX.with(|c| c.set(self.0));
        }
    }
    let _back = Back(CX.with(|c| c.replace(sh)));
    f()
}
fn refuse(what: &'static str) {
    with(|sh| {
        let mut r = sh.refused.borrow_mut();
        if !r.contains(&what) {
            r.push(what);
        }
    })
}

/// A value that is one block word.
pub trait Wordy: Copy + 'static {
    fn to_w(self) -> u32;
    fn from_w(w: u32) -> Self;
}
macro_rules! wordy {
    ($($t:ty: $to:expr, $from:expr;)*) => {$(
        impl Wordy for $t {
            #[inline(always)]
            fn to_w(self) -> u32 { ($to)(self) }
            #[inline(always)]
            fn from_w(w: u32) -> Self { ($from)(w) }
        }
    )*};
}
wordy! {
    u32: |v| v, |w| w;
    i32: |v: i32| v as u32, |w| w as i32;
    f32: f32::to_bits, f32::from_bits;
    bool: |v| v as u32, |w| w != 0;
    u8: |v| v as u32, |w| w as u8;
    u16: |v| v as u32, |w| w as u16;
    Color: |v: Color| v.0, Color;
}

/// The block word of the signal `w` and its value now. `NONE` (a constant of this description) unless the
/// signal's owner is the root view and no list row is being described.
fn slot_of<T: Wordy, O: 'static>(w: &Wired<'_, T, O>) -> (u32, u32) {
    unsafe fn read<T: Wordy>(p: *const u8) -> u32 {
        // SAFETY: the caller's contract: `p` is a `Signal<T>` field of a live owner.
        unsafe { (**(p as *const Signal<T>)).to_w() }
    }
    unsafe fn write<T: Wordy>(p: *mut u8, v: u32) {
        // SAFETY: as `read`, and the owner is exclusively borrowed.
        unsafe { (*(p as *mut Signal<T>)).set_silent(T::from_w(v)) }
    }
    let val = w.get().to_w();
    with(|sh| {
        if TypeId::of::<O>() != sh.root || sh.row.get().3 != 0 {
            return (NONE, val);
        }
        let mut slots = sh.slots.borrow_mut();
        if let Some(i) = slots.iter().position(|e| e.off == w.off) {
            return (i as u32, val);
        }
        if slots.len() == HOVER {
            drop(slots);
            refuse("more than 47 word-sized bound signals");
            return (NONE, val);
        }
        slots.push(SlotEnt {
            off: w.off,
            read: read::<T>,
            write: write::<T>,
        });
        (slots.len() as u32 - 1, val)
    })
}

// ---- expressions ------------------------------------------------------------------------------------------------

/// A bound signal through its projection: the word at `slot` of the block (or `val`, a constant), then `f`.
pub struct Dy<T, F> {
    slot: u32,
    val: u32,
    f: F,
    _t: PhantomData<fn(&T)>,
}
impl<T, F: Copy> Clone for Dy<T, F> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T, F: Copy> Copy for Dy<T, F> {}
impl<T: Wordy, F: Fn(&T) -> Color + Copy + 'static> Expr for Dy<T, F> {
    type Out = XColor;
    type Stage = Turn;
    const READS: u64 = APP;
    const CODE: &'static Code = &Code::Cpu(&[]);
    #[inline(always)]
    fn eval(&self, cx: &ECx) -> XColor {
        let w = if self.slot == NONE {
            self.val
        } else {
            cx.u.w[self.slot as usize]
        };
        XColor((self.f)(&T::from_w(w)).0)
    }
}

/// `hover_bg`: `hot` while the hover word names this record, else the base colour.
#[derive(Clone, Copy)]
pub struct HSel<C> {
    id: u32,
    hot: u32,
    base: C,
}
impl<C: Expr<Out = XColor>> Expr for HSel<C> {
    type Out = XColor;
    type Stage = Turn;
    const READS: u64 = C::READS | 1 << HOVER;
    const CODE: &'static Code = &Code::Cpu(&[]);
    #[inline(always)]
    fn eval(&self, cx: &ECx) -> XColor {
        if cx.u.w[HOVER] == self.id {
            XColor(self.hot)
        } else {
            self.base.eval(cx)
        }
    }
}

/// A colour operand of the façade as a colour expression.
pub trait TColor<O>: Sized {
    type E: Expr<Out = XColor> + Copy + 'static;
    fn expr(self) -> Self::E;
}
impl<O> TColor<O> for Color {
    type E = XColor;
    #[inline(always)]
    fn expr(self) -> XColor {
        XColor(self.0)
    }
}
impl<'a, O: 'static> TColor<O> for Wired<'a, Color, O> {
    type E = Dy<Color, fn(&Color) -> Color>;
    fn expr(self) -> Self::E {
        let (slot, val) = slot_of(&self);
        Dy {
            slot,
            val,
            f: |c| *c,
            _t: PhantomData,
        }
    }
}
impl<'a, T: Wordy, O: 'static, F: Fn(&T) -> Color + Copy + 'static> TColor<O>
    for WMap<'a, T, O, F>
{
    type E = Dy<T, F>;
    fn expr(self) -> Self::E {
        let (slot, val) = slot_of(&self.w);
        Dy {
            slot,
            val,
            f: self.f,
            _t: PhantomData,
        }
    }
}

// ---- archetypes ---------------------------------------------------------------------------------------------------

/// A box's record: its colour expression and its constants.
#[derive(Clone, Copy)]
pub struct PBox<C> {
    c: C,
    /// 1/256 device pixels.
    radius: u32,
    border: u32,
    border_color: u32,
    alpha: u32,
}
/// A box's paint as the element layer takes it.
#[derive(Clone, Copy)]
pub struct BoxP<C>(PBox<C>);
impl<W, C: Expr<Out = XColor> + Copy + 'static> Paints<W> for BoxP<C> {
    type A = PBox<C>;
    #[inline(always)]
    fn arch(&self) -> PBox<C> {
        self.0
    }
}
impl<C: Expr<Out = XColor> + Copy + 'static> Arch for PBox<C> {
    const READS: u64 = C::READS;
    const NAME: &'static str = "box";
    const STAGE: StageId = C::STAGE;
    #[inline(always)]
    fn same_at(&self, o: &Self, u: &U) -> bool {
        (self.radius, self.border, self.border_color, self.alpha)
            == (o.radius, o.border, o.border_color, o.alpha)
            && self.c.eval(&ECx::of(u)) == o.c.eval(&ECx::of(u))
    }
    #[inline(always)]
    fn changed(&self, old: &U, new: &U) -> bool {
        self.alpha != 0 && self.c.eval(&ECx::of(old)) != self.c.eval(&ECx::of(new))
    }
    #[inline(always)]
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
        b.draws(col, u)
    }
}
impl<C: Expr<Out = XColor> + Copy + 'static> Draws for PBox<C> {
    #[inline(always)]
    fn draw<P: Prims>(&self, r: Rect, cut: Rect, u: &U, p: &mut P) {
        if self.alpha != 0 {
            p.prim(
                r,
                cut,
                &Prim {
                    kind: P_BOX,
                    color: self.c.eval(&ECx::of(u)).0,
                    radius: self.radius,
                    border: self.border,
                    border_color: self.border_color,
                    alpha: self.alpha,
                    ..Prim::default()
                },
            )
        }
    }
}

/// One line of text: a run of the run store, where its baseline is in the box, its colour expression.
#[derive(Clone, Copy)]
pub struct PLine<C> {
    run: (u32, u32),
    base: i32,
    c: C,
}
impl<C: Expr<Out = XColor> + Copy + 'static> Arch for PLine<C> {
    const READS: u64 = C::READS;
    const NAME: &'static str = "line";
    const STAGE: StageId = C::STAGE;
    #[inline(always)]
    fn same_at(&self, o: &Self, u: &U) -> bool {
        (self.run, self.base) == (o.run, o.base)
            && self.c.eval(&ECx::of(u)) == o.c.eval(&ECx::of(u))
    }
    #[inline(always)]
    fn changed(&self, old: &U, new: &U) -> bool {
        self.c.eval(&ECx::of(old)) != self.c.eval(&ECx::of(new))
    }
    #[inline(always)]
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
        b.draws(col, u)
    }
}
impl<C: Expr<Out = XColor> + Copy + 'static> Draws for PLine<C> {
    /// The line is cut to its layout box (the record's rect is what a change of it damages).
    #[inline(always)]
    fn draw<P: Prims>(&self, r: Rect, cut: Rect, u: &U, p: &mut P) {
        p.prim(
            r,
            cut,
            &Prim {
                kind: P_TEXT,
                color: self.c.eval(&ECx::of(u)).0,
                a: self.run.0,
                b: self.run.1,
                alpha: 255,
                ox: 0,
                oy: self.base,
                ..Prim::default()
            },
        )
    }
}

/// One line of text as an element: a leaf of known size, one record.
#[derive(Clone, Copy)]
pub struct TLine<C> {
    spec: bough_lay::Spec,
    w: f32,
    lh: f32,
    rec: PLine<C>,
}
impl<C> Shape for TLine<C> {
    const LEAVES: usize = 1;
    const WRAPS: bool = false;
    const VARIES: bool = false;
    const HK: bough_lay::Hk = bough_lay::Hk::Fixed;
    #[inline(always)]
    fn emit<'a, S: bough_lay::shape::Sink<'a>>(&'a self, s: &mut S) {
        s.node(bough_lay::shape::Nd::pack(
            &self.spec,
            bough_lay::shape::K_LINE,
            true,
            0,
            self.w,
            self.lh,
            [0; 2],
        ));
    }
}
impl<W, C: Expr<Out = XColor> + Copy + 'static> Row<W> for TLine<C> {
    const LEAVES: u32 = 1;
    #[inline(always)]
    fn visit<L: Leaf<W>>(&self, l: &mut L) {
        <PLine<C> as Row<W>>::visit(&self.rec, l)
    }
    #[inline(always)]
    unsafe fn write(&self, sites: *const Site, slot: u32) {
        // SAFETY: the caller's contract, for this one record.
        unsafe { <PLine<C> as Row<W>>::write(&self.rec, sites, slot) }
    }
    #[inline(always)]
    unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
        // SAFETY: as `write`.
        unsafe { <PLine<C> as Row<W>>::patch(&self.rec, sites, slot, mk, cx) }
    }
}
impl<W, C: Expr<Out = XColor> + Copy + 'static> Part<W> for TLine<C> {
    const HOLES: bool = false;
    #[inline(always)]
    fn holes<S: Sink<W>>(&mut self, _: Option<&[Rect]>, k: &mut usize, _: &mut S) {
        *k += 1;
    }
}

/// What an element type with no conversion converts to (never constructed).
#[derive(Clone, Copy)]
pub struct Nope;
impl Shape for Nope {
    const LEAVES: usize = 0;
    const WRAPS: bool = false;
    const VARIES: bool = false;
    const HK: bough_lay::Hk = bough_lay::Hk::Fixed;
    fn emit<'a, S: bough_lay::shape::Sink<'a>>(&'a self, _: &mut S) {}
}
impl<W> Row<W> for Nope {
    const LEAVES: u32 = 0;
    fn visit<L: Leaf<W>>(&self, _: &mut L) {}
    unsafe fn write(&self, _: *const Site, _: u32) {}
    unsafe fn patch(&self, _: *const Site, _: u32, _: &Mk, _: &mut Cx<'_>) {}
}
impl<W> Part<W> for Nope {
    const HOLES: bool = false;
    fn holes<S: Sink<W>>(&mut self, _: Option<&[Rect]>, _: &mut usize, _: &mut S) {}
}

// ---- owners ---------------------------------------------------------------------------------------------------------

/// The view as the world sees it.
pub struct Ty<V> {
    pub v: V,
    sh: Rc<Shared>,
    hover: Cell<u32>,
    /// A handler of the author's ran in the event being dispatched.
    ran: Cell<bool>,
    /// The block as the world may write it (a wheel over a scrolled box writes the offset's word here).
    shadow: U,
}
static NO_BLOCK: U = U { w: [0; 64] };
impl<V: View> Ty<V> {
    fn fill(&self, out: &mut U) {
        let base = &self.v as *const V as *const u8;
        for (i, e) in self.sh.slots.borrow().iter().enumerate() {
            // SAFETY: an entry is made only from a `Wired<'_, T, V>` of the root view type (`slot_of` checks
            // the owner's type), which only `sig!` constructs: `off` is `offset_of!(V, path)` of a field the
            // macro pins to `Signal<T>`, and `read` was instantiated for that `T`. `self.v` is that `V`.
            out.w[i] = unsafe { (e.read)(base.add(e.off as usize)) };
        }
        out.w[HOVER] = self.hover.get();
    }
    /// Words the world wrote into the shadow block (a scroll offset) go back to their signals.
    fn sync(&mut self) {
        let base = &mut self.v as *mut V as *mut u8;
        for (i, e) in self.sh.slots.borrow().iter().enumerate() {
            // SAFETY: as `fill`; `self` is exclusively borrowed, so the field is not otherwise borrowed.
            unsafe {
                if (e.read)(base.add(e.off as usize)) != self.shadow.w[i] {
                    (e.write)(base.add(e.off as usize), self.shadow.w[i]);
                }
            }
        }
    }
}
impl<V: View> bough_x::sink::View for Ty<V> {
    fn block(&self) -> &U {
        &NO_BLOCK
    }
    fn gathered<'a>(&'a self, _: &[bough_x::expr::Slot], out: &'a mut U) -> &'a U {
        self.fill(out);
        out
    }
    fn block_mut(&mut self) -> Option<&mut U> {
        let mut u = U::default();
        self.fill(&mut u);
        self.shadow = u;
        Some(&mut self.shadow)
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        enter(&self.sh, || {
            self.sh.row.set((0, 0, 0, 0));
            self.sh.ord.set(0);
            if S::FIXED {
                self.sh.describes.set(self.sh.describes.get() + 1);
                self.sh.text.borrow_mut().begin_pass();
            }
            drive(root_el(&self.v), s)
        })
    }
}

/// How a handler's owner is reached from the root view.
pub trait Reach<V: View>: 'static {
    type O: 'static;
    fn reach<'x>(t: &'x mut Ty<V>, at: (u32, u32, u32)) -> Option<&'x mut Self::O>;
}
/// The root view itself.
pub struct Root;
impl<V: View> Reach<V> for Root {
    type O = V;
    #[inline(always)]
    fn reach<'x>(t: &'x mut Ty<V>, _: (u32, u32, u32)) -> Option<&'x mut V> {
        Some(&mut t.v)
    }
}
/// An item of a `List<T>` field of the root view: by the list's offset and the item's slot.
pub struct InList<T>(PhantomData<fn() -> T>);
impl<V: View, T: 'static> Reach<V> for InList<T> {
    type O = T;
    fn reach<'x>(t: &'x mut Ty<V>, (off, idx, generation): (u32, u32, u32)) -> Option<&'x mut T> {
        // SAFETY: `off` came from an `LWired<'_, T, V>` (only `sig!` makes one): `offset_of!(V, path)` of a field
        // the macro pins to `List<T>`; `t.v` is that `V`, exclusively borrowed.
        let l = unsafe {
            &mut *(&mut t.v as *mut V as *mut u8)
                .add(off as usize)
                .cast::<List<T>>()
        };
        l.edit_slot(crate::list::Slot::raw(idx, generation))
    }
}

fn fev<V>(t: &Ty<V>, e: &bough_x::event::Ev) -> Ev {
    use bough_x::event::{Button as B, Key as K};
    let s = t.sh.scale.get();
    let key = e.key.map(|k| match k {
        K::Char(c) => Key::Char(c),
        K::Tab => Key::Tab,
        K::Backspace => Key::Backspace,
        K::Delete => Key::Delete,
        K::Enter => Key::Enter,
        K::Escape => Key::Escape,
        K::Left => Key::Left,
        K::Right => Key::Right,
        K::Up => Key::Up,
        K::Down => Key::Down,
        K::Home => Key::Home,
        K::End => Key::End,
        K::PageUp => Key::PageUp,
        K::PageDown => Key::PageDown,
    });
    let button = match e.button {
        B::Primary => crate::input::Button::Primary,
        B::Secondary => crate::input::Button::Secondary,
        B::Middle => crate::input::Button::Middle,
    };
    Ev {
        x: e.x / s,
        y: e.y / s,
        dx: e.dx / s,
        dy: e.dy / s,
        key,
        mods: Mods {
            shift: e.mods.shift,
            ctrl: e.mods.ctrl,
            alt: e.mods.alt,
            cmd: e.mods.cmd,
        },
        button,
        on: e.on,
    }
}

/// `on_click(Owner::method)` on a record.
pub struct HClick<F, R>(F, (u32, u32, u32), PhantomData<fn() -> R>);
impl<F: Copy, R> Clone for HClick<F, R> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<F: Copy, R> Copy for HClick<F, R> {}
impl<V: View, R: Reach<V>, A, F: Fn(&mut R::O) + Copy + 'static> Handler<Ty<V>, A>
    for HClick<F, R>
{
    const KINDS: u32 = kind::CLICK;
    #[inline]
    fn handle(&self, t: &mut Ty<V>, _: &A, _: &Event) -> bool {
        if let Some(o) = R::reach(t, self.1) {
            count(|c| c.dispatch += 1);
            (self.0)(o);
            t.ran.set(true);
        }
        true
    }
}
/// A handler that takes the event: press, hover, key, drag (`K` is the kinds it hears).
pub struct HEv<F, R, const K: u32>(F, (u32, u32, u32), PhantomData<fn() -> R>);
impl<F: Copy, R, const K: u32> Clone for HEv<F, R, K> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<F: Copy, R, const K: u32> Copy for HEv<F, R, K> {}
impl<V: View, R: Reach<V>, A, F: Fn(&mut R::O, &Ev) + Copy + 'static, const K: u32>
    Handler<Ty<V>, A> for HEv<F, R, K>
{
    const KINDS: u32 = K;
    #[inline]
    fn handle(&self, t: &mut Ty<V>, _: &A, e: &Event) -> bool {
        let ev = fev(t, &e.ev);
        if let Some(o) = R::reach(t, self.1) {
            count(|c| c.dispatch += 1);
            (self.0)(o, &ev);
            t.ran.set(true);
        }
        true
    }
}
/// `hover_bg`'s handler: the hover word names the record the pointer entered.
#[derive(Clone, Copy)]
pub struct HSet;
impl<V: View, C: Expr<Out = XColor> + Copy + 'static> Handler<Ty<V>, PBox<HSel<C>>> for HSet {
    const KINDS: u32 = kind::HOVER;
    #[inline]
    fn handle(&self, t: &mut Ty<V>, a: &PBox<HSel<C>>, e: &Event) -> bool {
        if e.ev.on {
            t.hover.set(a.c.id);
        } else if t.hover.get() == a.c.id {
            t.hover.set(0);
        }
        true
    }
}

// ---- conversions ------------------------------------------------------------------------------------------------------

/// A façade element as an element of the typed layer, for a world of root view `V` and an owner reached by `R`.
pub trait Typed<V: View, R: Reach<V>>: Sized {
    type El: Part<Ty<V>> + Shape;
    fn typed(self) -> Self::El;
}
/// The children of a box.
pub trait TKids<V: View, R: Reach<V>>: Sized {
    type K: Part<Ty<V>> + bough_lay::shape::ShapeKids;
    fn kids(self) -> Self::K;
}
impl<V: View, R: Reach<V>> TKids<V, R> for () {
    type K = ();
    fn kids(self) {}
}
macro_rules! kids_tuple {
    ($($t:ident),+) => {
        impl<V: View, R: Reach<V>, $($t: TKids<V, R>),+> TKids<V, R> for ($($t,)+) {
            type K = ($($t::K,)+);
            #[allow(non_snake_case)]
            fn kids(self) -> Self::K {
                let ($($t,)+) = self;
                ($($t.kids(),)+)
            }
        }
    };
}
kids_tuple!(A);
kids_tuple!(A, B);
kids_tuple!(A, B, C);
kids_tuple!(A, B, C, D);
kids_tuple!(A, B, C, D, E);
kids_tuple!(A, B, C, D, E, F);
kids_tuple!(A, B, C, D, E, F, G);
kids_tuple!(A, B, C, D, E, F, G, H);
/// An element is a child.
macro_rules! kid {
    ([$($g:tt)*] $t:ty) => {
        impl<$($g)* V: View, R: Reach<V>> TKids<V, R> for $t
        where
            $t: Typed<V, R>,
        {
            type K = <$t as Typed<V, R>>::El;
            fn kids(self) -> Self::K {
                self.typed()
            }
        }
    };
}
kid!([Bg, Ch,] LBox<Bg, Ch, ()>);
kid!([S, C,] Para<S, C>);
kid!([E, O,] On<E, O>);
kid!(['a, O2, Bg, Ch,] ScrollBox<'a, O2, Bg, Ch>);
kid!(['a, S, O2,] LList<'a, S, O2>);

type TBox<E, K> = bough_lay::LBox<Rec1<BoxP<E>>, K>;

fn pbox<C>(c: C, draw: bool, radius: f32, border: Option<(f32, Color)>) -> BoxP<C> {
    let s = with(|sh| sh.scale.get());
    let (bw, bc) = border.map_or((0, 0), |(w, c)| (((w * s).max(1.0) * 256.0) as u32, c.0));
    BoxP(PBox {
        c,
        radius: (radius * s * 256.0) as u32,
        border: bw,
        border_color: bc,
        alpha: if draw { 255 } else { 0 },
    })
}
fn tbox<E, K>(spec: &crate::layout::Spec, paint: BoxP<E>, kids: K) -> TBox<E, K> {
    let s = with(|sh| sh.scale.get());
    let mut b = bough_lay::column(kids);
    b.spec = crate::lower::lay_spec(spec, s);
    b.bg(paint)
}

impl<V: View, R: Reach<V>, Bg: TColor<R::O>, Ch: TKids<V, R>> Typed<V, R> for LBox<Bg, Ch, ()> {
    type El = TBox<Bg::E, Ch::K>;
    fn typed(self) -> Self::El {
        tbox(
            &self.spec,
            pbox(self.bg.expr(), self.draw, self.radius, self.border),
            self.ch.kids(),
        )
    }
}

impl<V: View, R: Reach<V>, S: TextSrc<R::O>, C: TColor<R::O>> Typed<V, R> for Para<S, C> {
    type El = TLine<C::E>;
    fn typed(self) -> Self::El {
        let (scale, text) = with(|sh| (sh.scale.get(), sh.text.clone()));
        if self.wrap {
            refuse("para (wrapped text)");
        }
        let (size, lh) = (self.size * scale, self.lh * scale);
        let (run, w, base) = self.s.with_str(|t| {
            let mut ts = text.borrow_mut();
            let run = ts.run(t, size);
            (
                run,
                ts.get(run.0, run.1).map_or(0.0, |r| r.w).ceil(),
                ts.baseline(size, lh),
            )
        });
        TLine {
            spec: crate::lower::lay_spec(&self.spec, scale),
            w,
            lh,
            rec: PLine {
                run,
                base,
                c: self.color.expr(),
            },
        }
    }
}

fn at_row() -> (u32, u32, u32) {
    with(|sh| {
        let r = sh.row.get();
        (r.0, r.1, r.2)
    })
}
impl<V: View, R: Reach<V>, E: Typed<V, R>, F: Fn(&mut R::O) + Copy + 'static> Typed<V, R>
    for On<E, Click<F>>
where
    E::El: Boxy<Ty<V>>,
    Hd<E::El, HClick<F, R>>: Part<Ty<V>> + Shape,
{
    type El = Hd<E::El, HClick<F, R>>;
    fn typed(self) -> Self::El {
        Hd(self.0.typed(), HClick(self.1.0, at_row(), PhantomData))
    }
}
macro_rules! on_ev {
    ($op:ident, $k:expr) => {
        impl<V: View, R: Reach<V>, E: Typed<V, R>, F: Fn(&mut R::O, &Ev) + Copy + 'static>
            Typed<V, R> for On<E, $op<F>>
        where
            E::El: Boxy<Ty<V>>,
            Hd<E::El, HEv<F, R, { $k }>>: Part<Ty<V>> + Shape,
        {
            type El = Hd<E::El, HEv<F, R, { $k }>>;
            fn typed(self) -> Self::El {
                Hd(self.0.typed(), HEv(self.1.0, at_row(), PhantomData))
            }
        }
    };
}
on_ev!(Press, kind::PRESS);
on_ev!(Hover, kind::HOVER);
on_ev!(OnKey, kind::KEY | kind::FOCUSABLE);
on_ev!(Drag, kind::DRAG);
impl<V: View, R: Reach<V>, E: Typed<V, R>> Typed<V, R> for On<E, Focusable>
where
    E::El: Boxy<Ty<V>>,
    Hd<E::El, bough_x::event::Focusable>: Part<Ty<V>> + Shape,
{
    type El = Hd<E::El, bough_x::event::Focusable>;
    fn typed(self) -> Self::El {
        Hd(self.0.typed(), bough_x::event::Focusable)
    }
}
impl<V: View, R: Reach<V>, Bg: TColor<R::O>, Ch: TKids<V, R>> Typed<V, R>
    for On<LBox<Bg, Ch, ()>, HoverBg>
{
    type El = Hd<TBox<HSel<Bg::E>, Ch::K>, HSet>;
    fn typed(self) -> Self::El {
        let b = self.0;
        // an id that is the same in every description of the same state: the row, and the order within it
        let id = with(|sh| {
            let n = sh.ord.get() + 1;
            sh.ord.set(n);
            (sh.row.get().3 << 10 | n & 0x3ff) + 1
        });
        let c = HSel {
            id,
            hot: self.1.0.0,
            base: b.bg.expr(),
        };
        Hd(
            tbox(&b.spec, pbox(c, true, b.radius, b.border), b.ch.kids()),
            HSet,
        )
    }
}

impl<'a, V: View, R: Reach<V>, Bg: TColor<R::O>, Ch: TKids<V, R>> Typed<V, R>
    for ScrollBox<'a, R::O, Bg, Ch>
where
    TBox<XColor, Ch::K>: Seg<Ty<V>>,
{
    type El = Scroll<Rec1<BoxP<Bg::E>>, TBox<XColor, Ch::K>>;
    fn typed(self) -> Self::El {
        let (slot, _) = slot_of(&self.sig);
        if slot == NONE {
            refuse("a scrolled box whose offset is not a signal of the root view");
        }
        let s = with(|sh| sh.scale.get());
        let p = self.port;
        let c = self.content;
        Scroll {
            spec: crate::lower::lay_spec(&p.spec, s),
            paint: Rec1(pbox(p.bg.expr(), p.draw, p.radius, p.border)),
            content: tbox(&c.spec, pbox(XColor(0), false, 0.0, None), c.ch.kids()),
            word: slot as u8,
        }
    }
}

/// The rows of a `list(..)`: row `r` is item `r`'s view, converted for an owner reached through the list.
pub struct TRows<'a, V, T> {
    list: &'a List<T>,
    off: u32,
    _v: PhantomData<fn(&V)>,
}
/// An item type's view as a row of a world of root view `V`.
pub trait RowView<V: View>: View {
    type El<'a>: Part<Ty<V>> + Shape + IsOk
    where
        Self: 'a;
    const OK: bool;
    fn row_el(&self) -> Self::El<'_>;
}
impl<V: View, T: View> RowView<V> for T {
    type El<'a> = impl Part<Ty<V>> + Shape + IsOk + 'a;
    const OK: bool = <Self::El<'static> as IsOk>::IS;
    fn row_el(&self) -> Self::El<'_> {
        maybe::<V, InList<T>, _>(self.view())
    }
}
impl<'a, V: View, T: View> Seg<Ty<V>> for TRows<'a, V, T> {
    fn seg<S: Sink<Ty<V>>>(&mut self, s: &mut S, f: &mut Flow) {
        let (list, off) = (self.list, self.off);
        let n = list.len();
        let geo = if f.lay {
            Some(Geo {
                x: 0,
                y: f.y,
                w: f.w,
            })
        } else {
            None
        };
        f.y += s.rows(n as u32, geo, |r| {
            assert!(r < n);
            let slot = list.slot(r).expect("a row under the list's length");
            with(|sh| {
                sh.row
                    .set((off, slot.index(), slot.generation(), r as u32 + 1));
                sh.ord.set(0);
            });
            let el = list.get(r).expect("a row under the list's length").row_el();
            with(|sh| sh.row.set((0, 0, 0, 0)));
            el
        });
    }
}
impl<'a, V: View, R: Reach<V>, T: View> Typed<V, R> for LList<'a, ListRows<'a, T, R::O>, R::O> {
    type El = Scroll<Rec1<BoxP<XColor>>, TRows<'a, V, T>>;
    fn typed(self) -> Self::El {
        if !<T as RowView<V>>::OK {
            refuse("a list whose row view has no typed conversion");
        }
        if !self.virt {
            refuse("a list that is not a viewport (every row in flow)");
        }
        if self.spec.gap != 0.0 || self.spec.pad != [0.0; 4] {
            refuse("a list with a gap or padding");
        }
        let s = with(|sh| sh.scale.get());
        let word = match &self.scroll {
            Some(w) => {
                let (slot, _) = slot_of(w);
                if slot == NONE {
                    refuse("a list whose offset is not a signal of the root view");
                }
                slot as u8
            }
            None => NO_WORD,
        };
        let mut spec = self.spec;
        spec.container = false;
        Scroll {
            spec: crate::lower::lay_spec(&spec, s),
            paint: Rec1(pbox(
                XColor(self.bg.map_or(0, |c| c.0)),
                self.bg.is_some(),
                self.radius,
                None,
            )),
            content: TRows {
                list: self.src.list.list,
                off: self.src.list.off,
                _v: PhantomData,
            },
            word,
        }
    }
}

/// Whether an element type converts, and its conversion: every type answers (the default is no).
pub trait MaybeTyped<V: View, R: Reach<V>>: Sized {
    const OK: bool;
    type El: Part<Ty<V>> + Shape;
    fn maybe(self) -> Self::El;
}
impl<V: View, R: Reach<V>, E> MaybeTyped<V, R> for E {
    default const OK: bool = false;
    default type El = Nope;
    default fn maybe(self) -> Self::El {
        unreachable!("an element type with no typed conversion is driven through `bough::lower`")
    }
}
impl<V: View, R: Reach<V>, E: Typed<V, R>> MaybeTyped<V, R> for E {
    const OK: bool = true;
    type El = E::El;
    fn maybe(self) -> E::El {
        self.typed()
    }
}
/// Whether a converted element type is a conversion at all ([`Nope`] is not).
pub trait IsOk {
    const IS: bool;
}
impl<T> IsOk for T {
    default const IS: bool = true;
}
impl IsOk for Nope {
    const IS: bool = false;
}
fn maybe<V: View, R: Reach<V>, E: MaybeTyped<V, R>>(e: E) -> E::El {
    e.maybe()
}
fn root_el<V: View>(v: &V) -> impl Part<Ty<V>> + Shape + '_ {
    maybe::<V, Root, _>(v.view())
}
fn root_ok<V: View>(v: &V) -> bool {
    fn ok<V: View, E: MaybeTyped<V, Root>>(_: &E) -> bool {
        E::OK
    }
    ok::<V, _>(&v.view())
}

/// Drive a converted element: its layout words through the heap tree (no element or child limit), its records
/// as one row, then its scrolled boxes. A pass after a turn takes no fixed rows and reaches only the lists.
fn drive<W, E: Part<W> + Shape, S: Sink<W>>(mut e: E, s: &mut S) {
    const {
        assert!(
            <E as Row<W>>::LEAVES as usize == <E as Shape>::LEAVES,
            "an element's layout has one rect per record"
        )
    };
    if !S::FIXED {
        return e.holes(None, &mut 0, s);
    }
    let env = s.env();
    let tree = bough_lay::heap::Tree::of_shape(&e);
    let mut out = vec![[0i32; 4]; tree.els.len()];
    tree.place_root(0, env.scale, env.u, env.size, &mut out[..]);
    let rects: Vec<Rect> = out
        .iter()
        .map(|r| Rect {
            x0: r[0],
            y0: r[1],
            x1: r[2],
            y1: r[3],
        })
        .collect();
    s.row(|| (&rects[..], bough_x::el::ByRef(&e)));
    e.holes(Some(&rects), &mut 0, s);
}

// ---- the application ------------------------------------------------------------------------------------------------

/// Count bands the gates assert exactly.
#[derive(Clone, Copy, Default, Debug)]
pub struct Stats {
    pub frames: u64,
    pub idle_frames: u64,
    pub tiles_repaired: u64,
}

/// A view on the typed path: its world, its surfaces.
pub struct App<V: View> {
    ty: Ty<V>,
    world: World<Ty<V>, Canvas>,
    text: Rc<RefCell<TextSys>>,
    images: Rc<RefCell<Images>>,
    size_pt: (f32, f32),
    scale: f32,
    once: bool,
    ring: Vec<(Canvas, Tiles)>,
    next: usize,
    shown: usize,
    pub stats: Stats,
}

impl<V: View> App<V> {
    /// Mount `view` on the typed path, or say why it is not admitted (the caller then uses `bough::lower::App`).
    pub fn new(view: V, cfg: Config) -> Result<App<V>, (V, Vec<&'static str>)> {
        if !root_ok(&view) {
            return Err((
                view,
                vec!["the view's element type has no typed conversion"],
            ));
        }
        let (w, h) = (
            (cfg.width_pt * cfg.scale).round() as i32,
            (cfg.height_pt * cfg.scale).round() as i32,
        );
        let text = Rc::new(RefCell::new(TextSys::new(&cfg.font)));
        let images = Rc::new(RefCell::new(Images::default()));
        let sh = Rc::new(Shared {
            text: text.clone(),
            slots: RefCell::new(Vec::new()),
            refused: RefCell::new(Vec::new()),
            scale: Cell::new(cfg.scale),
            root: TypeId::of::<V>(),
            row: Cell::new((0, 0, 0, 0)),
            ord: Cell::new(0),
            describes: Cell::new(0),
        });
        let mut world = World::<Ty<V>, Canvas>::new(w, h, APP_WORDS);
        world.set_scale(cfg.scale);
        world.tick(1);
        let mut app = App {
            ty: Ty {
                v: view,
                sh,
                hover: Cell::new(0),
                ran: Cell::new(false),
                shadow: U::default(),
            },
            world,
            text,
            images,
            size_pt: (cfg.width_pt, cfg.height_pt),
            scale: cfg.scale,
            once: cfg.mount_once,
            ring: Vec::new(),
            next: 0,
            shown: 0,
            stats: Stats::default(),
        };
        app.surfaces(if cfg.ring == 0 { 4 } else { cfg.ring });
        app.world.mount(&app.ty);
        count(|c| c.world_mount += 1);
        let refused = app.ty.sh.refused.borrow().clone();
        if !refused.is_empty() {
            return Err((app.ty.v, refused));
        }
        Ok(app)
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
        (self.next, self.shown) = (0, 0);
    }
    /// Typed descriptions so far (mount passes and re-descriptions): the count band of the gates.
    pub fn describes(&self) -> u64 {
        self.ty.sh.describes.get()
    }
    pub fn view(&self) -> &V {
        &self.ty.v
    }
    /// After author code ran: the façade's contract is a re-description (a typed compare of what is exposed);
    /// under `Config::mount_once` only the block is compared.
    fn after(&mut self, ran: bool) {
        self.ty.ran.set(false);
        if ran && !self.once {
            self.world.redescribe(&self.ty);
            let refused = self.ty.sh.refused.borrow();
            assert!(
                refused.is_empty(),
                "a view admitted to the typed path described what it does not hold: {refused:?}"
            );
        } else {
            self.world.settle(&self.ty);
        }
    }
    /// Run `f` on the state, then bring the window up to date with it.
    pub fn turn(&mut self, f: impl FnOnce(&mut V)) {
        f(&mut self.ty.v);
        self.after(true);
    }
    fn dev(&self, x: f32, y: f32) -> (i32, i32) {
        ((x * self.scale) as i32, (y * self.scale) as i32)
    }
    pub fn pointer_move(&mut self, x: f32, y: f32) {
        let (px, py) = self.dev(x, y);
        self.world.pointer(&mut self.ty, px, py);
        self.after(self.ty.ran.get());
    }
    pub fn pointer_down(&mut self, x: f32, y: f32, button: crate::input::Button) {
        let (px, py) = self.dev(x, y);
        self.world
            .pointer_down(&mut self.ty, px, py, xbutton(button));
        self.after(self.ty.ran.get());
    }
    pub fn pointer_up(&mut self, x: f32, y: f32, button: crate::input::Button) {
        let (px, py) = self.dev(x, y);
        self.world.pointer_up(&mut self.ty, px, py, xbutton(button));
        self.after(self.ty.ran.get());
    }
    pub fn key_down(&mut self, key: Key, mods: Mods) {
        use bough_x::event::Key as K;
        let k = match key {
            Key::Char(c) => K::Char(c),
            Key::Tab => K::Tab,
            Key::Backspace => K::Backspace,
            Key::Delete => K::Delete,
            Key::Enter => K::Enter,
            Key::Escape => K::Escape,
            Key::Left => K::Left,
            Key::Right => K::Right,
            Key::Up => K::Up,
            Key::Down => K::Down,
            Key::Home => K::Home,
            Key::End => K::End,
            Key::PageUp => K::PageUp,
            Key::PageDown => K::PageDown,
        };
        let m = bough_x::event::Mods {
            shift: mods.shift,
            ctrl: mods.ctrl,
            alt: mods.alt,
            cmd: mods.cmd,
        };
        self.world.key_down(&mut self.ty, k, m);
        self.after(self.ty.ran.get());
    }
    /// The wheel at (x, y) points turned by `dy` lines: the space under it moves by its offset's word, which
    /// goes back to the view's signal. No author code runs, so nothing is described.
    pub fn wheel(&mut self, x: f32, y: f32, dx: f32, dy: f32) {
        let (px, py) = self.dev(x, y);
        self.world.wheel(&mut self.ty, px, py, dx, dy);
        self.ty.sync();
        self.after(self.ty.ran.get());
    }
    /// The window is `w` x `h` points now: the world is kept and the view described at the new size.
    pub fn resize(&mut self, w_pt: f32, h_pt: f32) {
        self.size_pt = (w_pt, h_pt);
        let (w, h) = self.px();
        self.world.resize(w, h);
        let n = self.ring.len();
        self.surfaces(n);
        self.world.redescribe(&self.ty);
    }
    /// Where a typed description's instructions go, measured apart (thread instructions, bracket subtracted):
    /// (running the view and converting its fixed part with no sink; one whole re-description; the same with
    /// every list row's conversion and compare skipped because nothing is exposed is not measurable from here,
    /// so the third number is a re-description of an unchanged view, for comparison with a changed one).
    pub fn profile(&mut self) -> (u64, u64) {
        let ot = bough_sys::perf::tinstr_overhead();
        let t0 = bough_sys::perf::thread_instructions();
        enter(&self.ty.sh, || {
            self.ty.sh.row.set((0, 0, 0, 0));
            self.ty.sh.ord.set(0);
            let e = root_el(&self.ty.v);
            std::hint::black_box(&e);
        });
        let t1 = bough_sys::perf::thread_instructions();
        self.world.redescribe(&self.ty);
        let t2 = bough_sys::perf::thread_instructions();
        ((t1 - t0).saturating_sub(ot), (t2 - t1).saturating_sub(ot))
    }
    pub fn lifecycle(&self) -> (u32, u32) {
        self.world.lifecycle()
    }
    pub fn world_stats(&self) -> (usize, usize, usize) {
        self.world.root().stats()
    }

    /// Present: the next surface of the ring is repaired where it is stale. Returns whether anything was
    /// repainted.
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
    pub fn presented_pixels(&self) -> Vec<u8> {
        self.ring[self.shown].0.bytes()
    }
    fn canvas(&self) -> Canvas {
        let (w, h) = self.px();
        Canvas::new(w, h, self.text.clone(), self.images.clone())
    }
    /// A full redraw of the stores.
    pub fn reference_pixels(&self) -> Vec<u8> {
        let mut c = self.canvas();
        self.world.paint(&mut c);
        c.bytes()
    }
    /// The view painted record by record as it is described, with no stores.
    pub fn immediate_pixels(&self) -> Vec<u8> {
        let (w, h) = self.px();
        let mut c = self.canvas();
        let d = self.ty.sh.describes.get();
        bough_x::sink::View::view(
            &self.ty,
            &mut bough_x::soft::Immediate::on(&mut c, &self.world.shown, w, h),
        );
        self.ty.sh.describes.set(d);
        c.bytes()
    }
    /// The same state mounted into a new world.
    pub fn fresh_pixels(&self) -> Vec<u8> {
        let (w, h) = self.px();
        let mut world = World::<Ty<V>, Canvas>::new(w, h, APP_WORDS);
        world.set_scale(self.scale);
        world.tick(self.world.now().max(1));
        let d = self.ty.sh.describes.get();
        world.mount(&self.ty);
        self.ty.sh.describes.set(d);
        let mut c = self.canvas();
        world.paint(&mut c);
        c.bytes()
    }
    /// Present a frame and hold it to the four oracles (as `bough::lower::App::check`).
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
    pub fn write_png(&self, path: &std::path::Path) -> std::io::Result<()> {
        let (w, h) = self.px();
        crate::lower::app::png(&self.presented_pixels(), w, h, path)
    }
}
fn xbutton(b: crate::input::Button) -> bough_x::event::Button {
    match b {
        crate::input::Button::Primary => bough_x::event::Button::Primary,
        crate::input::Button::Secondary => bough_x::event::Button::Secondary,
        crate::input::Button::Middle => bough_x::event::Button::Middle,
    }
}
