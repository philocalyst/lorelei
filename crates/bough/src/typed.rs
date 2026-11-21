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
use bough_x::probe::{mark, ph};
use bough_x::sink::{Cx, Edit, Geo, Journal, Leaf, Mk, Row, Sink};
use bough_x::store::{Arch, Backend, ColRef, NO_WORD, Rect, Site, Tiles};
use bough_x::world::World;
use std::any::TypeId;
use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::rc::Rc;

#[cfg(feature = "phase_probe")]
thread_local! {
    /// Thread instructions charged to each phase, and the counter at the last mark.
    static PHASES: Cell<([u64; ph::N], u64, u64)> = const { Cell::new(([0; ph::N], 0, 0)) };
}
#[cfg(feature = "phase_probe")]
fn phase_hook(p: u32) {
    let now = bough_sys::perf::thread_instructions();
    PHASES.with(|c| {
        let (mut a, last, over) = c.get();
        a[p as usize] += (now - last).saturating_sub(over);
        // the hook's own work after reading the counter is charged to the next phase: read again
        c.set((a, bough_sys::perf::thread_instructions(), over));
    })
}

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

#[doc(hidden)]
/// What conversions consult while a view is being described: the run store, the block words assigned so far,
/// the row being described, refusals.
pub struct Shared {
    text: Rc<RefCell<TextSys>>,
    slots: RefCell<Vec<SlotEnt>>,
    refused: RefCell<Vec<&'static str>>,
    scale: Cell<f32>,
    root: TypeId,
    owner: Cell<*const u8>,
    /// The list row being described: (list offset in its owner, slot index, slot generation, row + 1); 0: none.
    row: Cell<(u32, u32, u32, u32)>,
    row_site: Cell<u32>,
    /// `hover_bg` boxes described so far in this row (or in the fixed part).
    ord: Cell<u32>,
    describes: Cell<u64>,
    /// What the last description said of each text of the fixed part, in description order (the tape), and
    /// the text the description under way is at.
    tape: RefCell<Vec<TapeEnt>>,
    ti: Cell<usize>,
    /// What the last description said of every other operand of the fixed part: the layout specs as the
    /// author gave them, and the paint operands as words (`Up`).
    specs: RefCell<Vec<crate::layout::Spec>>,
    ops: RefCell<Vec<u32>>,
    /// The layout words of the fixed part as last solved, with the rects they gave.
    lay: RefCell<LayMemo>,
    /// The lists of the view in description order: each one's journal, fed from the `List`'s own log.
    lists: RefCell<Vec<ListEnt>>,
    retired_lists: RefCell<Vec<ListEnt>>,
    next_list_tag: Cell<u32>,
    li: Cell<usize>,
    /// A pass that consults nothing kept (the oracles: an immediate description, a fresh mount).
    plain: Cell<bool>,
    /// A pass in which every exposed row is described and compared (so every run shown is asked for again
    /// before the run store collects).
    full: Cell<bool>,
    pass: Cell<PassStats>,
}

/// What one description did (count bands of the gates).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PassStats {
    /// List rows described (converted from the item's view).
    pub rows: u32,
    /// Times the fixed part's layout was solved (0: the kept rects were the answer).
    pub layouts: u32,
    /// Texts looked up in the run store by their bytes (a hash; 0 for a text the tape answered).
    pub lookups: u32,
    /// Texts the tape answered.
    pub taped: u32,
}

struct TapeEnt {
    text: Box<str>,
    size: u32,
    lh: u32,
    run: (u32, u32),
    w: f32,
    base: i32,
}
#[derive(Default)]
struct LayMemo {
    words: Vec<bough_lay::shape::Nd>,
    rects: Vec<Rect>,
    env: (u32, [u32; 2]),
}
struct ListEnt {
    off: u32,
    /// Boxed: list stores and rows hold its address between passes.
    journal: Box<Journal>,
    /// The list's log of this turn as journal edits (`ops::<T>`), and the end of its turn.
    ops: unsafe fn(*const u8, &mut Journal),
    end: unsafe fn(*mut u8, u32),
    /// The list's log from entry `from` on, published for `View::changed` (`list_pubs::<T>`); returns the
    /// log's length.
    pubs: unsafe fn(*const u8, usize, &mut crate::changed::Pubs) -> usize,
    /// How much of this turn's log `View::changed` has been shown.
    shown: usize,
    /// What the list's head holds while it is this list: a `List` put in the field's place (assigned, swapped)
    /// has another head, and every row of it is another row.
    tag: u32,
}
/// The log of the `List<T>` at `p` since its last `end_turn`, as edits of `j`. An edited item is an update of
/// the row it is at, if the list reading `j` exposes it (`Journal::window`).
///
/// SAFETY: `p` is a live `List<T>`, not mutably borrowed.
unsafe fn list_ops<T>(p: *const u8, j: &mut Journal) {
    use crate::list::Op;
    // SAFETY: the caller's contract.
    let l = unsafe { &*(p as *const List<T>) };
    let (log, _) = l.log();
    for op in log {
        match *op {
            Op::Insert { at, .. } => j.push(Edit::Insert(at)),
            Op::Remove { at, .. } => j.push(Edit::Remove(at)),
            Op::Move { from, to } => j.push(Edit::Shift(from.min(to))),
            Op::Permute { at, .. } => j.push(Edit::Shift(at)),
            Op::Edit { slot } => {
                let order = l.order_slots();
                let (lo, hi) = j.window();
                let (lo, hi) = (
                    (lo as usize).min(order.len()),
                    (hi as usize).min(order.len()),
                );
                if let Some(i) = order[lo..hi].iter().position(|&s| s == slot) {
                    j.push(Edit::Update((lo + i) as u32));
                }
            }
        }
    }
}
/// The log of the `List<T>` at `p` from entry `from` on, as a publication of the turn (what `View::changed`
/// reads through `Changed::list`). Returns the log's length.
///
/// SAFETY: `p` is a live `List<T>`, not mutably borrowed.
unsafe fn list_pubs<T>(p: *const u8, from: usize, pubs: &mut crate::changed::Pubs) -> usize {
    // SAFETY: the caller's contract.
    let l = unsafe { &*(p as *const List<T>) };
    let (log, _) = l.log();
    pubs.list(p as usize, &log[from.min(log.len())..]);
    log.len()
}
/// The end of the turn for the `List<T>` at `p`: its log is dropped and its head is `tag` (non-zero: displayed,
/// so it keeps a log; 0: not displayed, no log kept).
///
/// SAFETY: `p` is a live `List<T>`, exclusively borrowed.
unsafe fn list_end<T>(p: *mut u8, tag: u32) {
    // SAFETY: the caller's contract.
    let l = unsafe { &mut *(p as *mut List<T>) };
    l.end_turn();
    l.head().set(tag);
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
fn enter<T>(sh: &Shared, owner: *const u8, f: impl FnOnce() -> T) -> T {
    struct Back<'a> {
        previous: *const Shared,
        owner: &'a Cell<*const u8>,
        previous_owner: *const u8,
    }
    impl Drop for Back<'_> {
        fn drop(&mut self) {
            self.owner.set(self.previous_owner);
            CX.with(|c| c.set(self.previous));
        }
    }
    let _back = Back {
        previous: CX.with(|c| c.replace(sh)),
        owner: &sh.owner,
        previous_owner: sh.owner.replace(owner),
    };
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
    // any word a signal may be bound to: never the hover word
    const READS: u64 = APP & !(1 << HOVER);
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
///
/// A selection in the operators lane's sense (`expr::Sel`): the id is a record constant, so when only the
/// hover word changed the records that can show something else are the two whose id is its old and its new
/// value. A box whose base colour is a constant declares that ([`PBox`]'s `KEYED`), and the world's key index
/// answers the turn with those two records instead of every exposed one.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct HSel<C> {
    id: u32,
    hot: u32,
    base: C,
}
// SAFETY: `repr(C)`: two `u32`s, then a `Words` operand (whole words at alignment 4, or zero-sized at
// alignment at most 4): no padding, every bit pattern a valid value.
unsafe impl<C: bough_x::expr::Words> bough_x::expr::Words for HSel<C> {}
/// Where a colour expression keeps a selection key, if it is a selection by a constant key: the block word
/// and the key's index among the expression's words.
pub trait Keyed {
    const KEY: Option<(usize, usize)>;
}
impl<C> Keyed for C {
    default const KEY: Option<(usize, usize)> = None;
}
impl<C: Expr<Out = XColor> + bough_x::expr::Words> Keyed for HSel<C> {
    // certain only when the base does not itself read the hover word (it reads no block word that the
    // hover word's change could be confused with: the fast path is taken for a change of that word alone)
    const KEY: Option<(usize, usize)> = if C::READS >> HOVER & 1 == 0 {
        Some((HOVER, 0))
    } else {
        None
    };
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
    fn expr(&self) -> Self::E;
    /// What the operand is, as two words that are equal exactly when two descriptions' operands evaluate
    /// alike for every block: a constant's value, a bound signal's block word. `None`: not known to be the
    /// same as anything (a projection whose function captures values).
    fn ident(&self) -> Option<[u32; 2]>;
}
impl<O> TColor<O> for Color {
    type E = XColor;
    #[inline(always)]
    fn expr(&self) -> XColor {
        XColor(self.0)
    }
    #[inline(always)]
    fn ident(&self) -> Option<[u32; 2]> {
        Some([0, self.0])
    }
}
/// The identity of a bound signal as an operand: its block word, or (not a block word) its value now.
fn dy_ident(slot: u32, val: u32) -> [u32; 2] {
    if slot == NONE { [1, val] } else { [2, slot] }
}
impl<'a, O: 'static> TColor<O> for Wired<'a, Color, O> {
    type E = Dy<Color, fn(&Color) -> Color>;
    fn expr(&self) -> Self::E {
        let (slot, val) = slot_of(self);
        Dy {
            slot,
            val,
            f: |c| *c,
            _t: PhantomData,
        }
    }
    fn ident(&self) -> Option<[u32; 2]> {
        let (slot, val) = slot_of(self);
        Some(dy_ident(slot, val))
    }
}
impl<'a, T: Wordy, O: 'static, F: Fn(&T) -> Color + Copy + 'static> TColor<O>
    for WMap<'a, T, O, F>
{
    type E = Dy<T, F>;
    fn expr(&self) -> Self::E {
        let (slot, val) = slot_of(&self.w);
        Dy {
            slot,
            val,
            f: self.f,
            _t: PhantomData,
        }
    }
    fn ident(&self) -> Option<[u32; 2]> {
        // a function that captures nothing is its type, and the type is the column's
        (size_of::<F>() == 0).then(|| {
            let (slot, val) = slot_of(&self.w);
            dy_ident(slot, val)
        })
    }
}

// ---- archetypes ---------------------------------------------------------------------------------------------------

/// A box's record: its colour expression and its constants.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct PBox<C> {
    c: C,
    /// 1/256 device pixels.
    radius: u32,
    border: u32,
    border_color: u32,
    alpha: u32,
}
// SAFETY: `repr(C)`: a `Words` operand first, then four `u32`s: no padding, every bit pattern valid.
unsafe impl<C: bough_x::expr::Words> bough_x::expr::Words for PBox<C> {}
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
    // the colour is the record's first field, so its key's index is the record's
    const KEYED: Option<(usize, usize)> = <C as Keyed>::KEY;
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
// SAFETY (`Row`'s contract): one leaf, the line's record, visited, written and compared as that record.
unsafe impl<W, C: Expr<Out = XColor> + Copy + 'static> Row<W> for TLine<C> {
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
// SAFETY (`Row`'s contract): no leaf; nothing is visited, written or compared.
unsafe impl<W> Row<W> for Nope {
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
        enter(&self.sh, (&raw const self.v).cast(), || {
            self.sh.row.set((0, 0, 0, 0));
            self.sh.ord.set(0);
            self.sh.ti.set(0);
            self.sh.li.set(0);
            if S::FIXED && !self.sh.plain.get() {
                self.sh.describes.set(self.sh.describes.get() + 1);
                self.sh.text.borrow_mut().begin_pass();
            }
            mark(ph::ENTER);
            describe::<V, _, S>(&self.sh, self.v.view(), s)
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
    /// Whether the element can be read where it is ([`Typed::up`], [`Typed::lists`]); if not, a description
    /// of a view that holds it is always the conversion.
    const BY_REF: bool;
    /// The update walk: this element's operands against what the last description said of them ([`Up`]).
    fn up<Sk: Sink<Ty<V>>>(&self, u: &mut Up<'_, V, Sk>);
    /// Reach the element's lists (and nothing else of it): `k` counts the records passed, `rects` are the
    /// fixed part's (or `None` for a sink that keeps what it was told).
    fn lists<Sk: Sink<Ty<V>>>(&self, rects: Option<&[Rect]>, k: &mut usize, s: &mut Sk);
}
/// The children of a box.
pub trait TKids<V: View, R: Reach<V>>: Sized {
    type K: Part<Ty<V>> + bough_lay::shape::ShapeKids;
    fn kids(self) -> Self::K;
    const BY_REF: bool;
    fn up_kids<Sk: Sink<Ty<V>>>(&self, u: &mut Up<'_, V, Sk>);
    fn lists_kids<Sk: Sink<Ty<V>>>(&self, rects: Option<&[Rect]>, k: &mut usize, s: &mut Sk);
}
impl<V: View, R: Reach<V>> TKids<V, R> for () {
    type K = ();
    fn kids(self) {}
    const BY_REF: bool = true;
    fn up_kids<Sk: Sink<Ty<V>>>(&self, _: &mut Up<'_, V, Sk>) {}
    fn lists_kids<Sk: Sink<Ty<V>>>(&self, _: Option<&[Rect]>, _: &mut usize, _: &mut Sk) {}
}

/// The update walk over the fixed part of a view (Integration Card 5).
///
/// The element value a view returns is read where it is, operand by operand in description order, against
/// what the last description said (the tapes in [`Shared`]): layout specs as the author gave them, paint
/// operands as words, texts by their bytes. An operand that differs is stored on the tape; if it is a paint
/// operand its one record is brought up to date through the sink ([`Sink::leaf`]); if it is a layout word (a
/// spec, a text's width) or something the walk cannot patch, the walk is a `miss` and the description falls
/// back to the conversion (which reads the same tapes). A walk with no miss has described the fixed part:
/// nothing was converted, laid out or compared record by record.
pub struct Up<'s, V: View, S> {
    sh: &'s Shared,
    specs: std::cell::RefMut<'s, Vec<crate::layout::Spec>>,
    ops: std::cell::RefMut<'s, Vec<u32>>,
    si: usize,
    oi: usize,
    /// The record the walk is at (records are in description order: a box, then its children).
    k: usize,
    miss: bool,
    /// The sink, if it keeps the fixed row (else the walk only writes the tapes).
    sink: Option<&'s mut S>,
    _v: PhantomData<fn(&V)>,
}
impl<'s, V: View, S: Sink<Ty<V>>> Up<'s, V, S> {
    fn new(sh: &'s Shared, sink: Option<&'s mut S>) -> Self {
        Up {
            sh,
            specs: sh.specs.borrow_mut(),
            ops: sh.ops.borrow_mut(),
            si: 0,
            oi: 0,
            k: 0,
            miss: false,
            sink,
            _v: PhantomData,
        }
    }
    /// A layout spec: one that differs is a miss.
    #[inline(always)]
    fn spec(&mut self, spec: &crate::layout::Spec) {
        match self.specs.get_mut(self.si) {
            Some(kept) if spec_same(kept, spec) => {}
            Some(kept) => {
                *kept = *spec;
                self.miss = true;
            }
            None => {
                self.specs.push(*spec);
                self.miss = true;
            }
        }
        self.si += 1;
    }
    /// A paint operand: whether it is the word the tape held.
    #[inline(always)]
    fn word(&mut self, w: u32) -> bool {
        let same = match self.ops.get_mut(self.oi) {
            Some(kept) if *kept == w => true,
            Some(kept) => {
                *kept = w;
                false
            }
            None => {
                self.ops.push(w);
                false
            }
        };
        self.oi += 1;
        same
    }
    /// Several paint operands at once: whether they are the words the tape held.
    #[inline(always)]
    fn words<const N: usize>(&mut self, w: [u32; N]) -> bool {
        let at = self.oi;
        self.oi += N;
        match self.ops.get_mut(at..at + N) {
            Some(kept) if *kept == w => true,
            Some(kept) => {
                kept.copy_from_slice(&w);
                false
            }
            None => {
                self.ops.truncate(at);
                self.ops.extend_from_slice(&w);
                false
            }
        }
    }
    /// An operand whose change the walk cannot patch.
    #[inline(always)]
    fn must(&mut self, w: u32) {
        if !self.word(w) {
            self.miss = true;
        }
    }
    /// A colour operand as tape words, and whether its identity is known at all (an operand of unknown
    /// identity is never the one the tape held).
    #[inline(always)]
    fn color<O, C: TColor<O>>(c: &C) -> ([u32; 2], bool) {
        match c.ident() {
            Some(w) => (w, true),
            None => ([u32::MAX, 0], false),
        }
    }
    /// The record the walk is at is `a` now (called when one of its operands differed).
    #[inline(always)]
    fn leaf<A: Arch>(&mut self, a: &A) {
        if !self.miss
            && let Some(s) = self.sink.as_deref_mut()
        {
            s.leaf(self.k, a)
        }
    }
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
            const BY_REF: bool = true $(&& $t::BY_REF)+;
            #[allow(non_snake_case)]
            #[inline(always)]
            fn up_kids<Sk: Sink<Ty<V>>>(&self, u: &mut Up<'_, V, Sk>) {
                let ($($t,)+) = self;
                $($t.up_kids(u);)+
            }
            #[allow(non_snake_case)]
            #[inline(always)]
            fn lists_kids<Sk: Sink<Ty<V>>>(&self, rects: Option<&[Rect]>, k: &mut usize, s: &mut Sk) {
                let ($($t,)+) = self;
                $($t.lists_kids(rects, k, s);)+
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
            const BY_REF: bool = <$t as Typed<V, R>>::BY_REF;
            #[inline(always)]
            fn up_kids<Sk: Sink<Ty<V>>>(&self, u: &mut Up<'_, V, Sk>) {
                self.up(u)
            }
            #[inline(always)]
            fn lists_kids<Sk: Sink<Ty<V>>>(&self, rects: Option<&[Rect]>, k: &mut usize, s: &mut Sk) {
                self.lists(rects, k, s)
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
    const BY_REF: bool = Ch::BY_REF;
    #[inline(always)]
    fn up<Sk: Sink<Ty<V>>>(&self, u: &mut Up<'_, V, Sk>) {
        u.spec(&self.spec);
        let ([c0, c1], known) = Up::<V, Sk>::color(&self.bg);
        let [d, r, bw, bc] = box_words(self.draw, self.radius, self.border);
        if !(u.words([c0, c1, d, r, bw, bc]) & known) {
            u.leaf(&pbox(self.bg.expr(), self.draw, self.radius, self.border).0);
        }
        u.k += 1;
        self.ch.up_kids(u);
    }
    #[inline(always)]
    fn lists<Sk: Sink<Ty<V>>>(&self, rects: Option<&[Rect]>, k: &mut usize, s: &mut Sk) {
        *k += 1;
        self.ch.lists_kids(rects, k, s);
    }
}
/// Whether two specs are the same bytes (a spec's bytes are its identity: `layout::Spec::bytes`), compared as
/// words in line: 21 of them, no call.
#[inline(always)]
fn spec_same(a: &crate::layout::Spec, b: &crate::layout::Spec) -> bool {
    const N: usize = crate::layout::SPEC_BYTES / 4;
    const { assert!(crate::layout::SPEC_BYTES % 4 == 0 && align_of::<crate::layout::Spec>() == 4) };
    // SAFETY: a `Spec` is `SPEC_BYTES` initialised bytes with no padding at alignment 4 (the constants beside
    // the struct check it at compile time, and `Spec::bytes` relies on the same), so it is `N` valid `u32`s.
    let (a, b) = unsafe {
        (
            &*(a as *const crate::layout::Spec as *const [u32; N]),
            &*(b as *const crate::layout::Spec as *const [u32; N]),
        )
    };
    let mut d = 0u32;
    for i in 0..N {
        d |= a[i] ^ b[i];
    }
    d == 0
}
/// A box's constant paint operands as tape words.
#[inline(always)]
fn box_words(draw: bool, radius: f32, border: Option<(f32, Color)>) -> [u32; 4] {
    let (bw, bc) = border.map_or((u32::MAX, 0), |(w, c)| (w.to_bits(), c.0));
    [draw as u32, radius.to_bits(), bw, bc]
}

impl<V: View, R: Reach<V>, S: TextSrc<R::O>, C: TColor<R::O>> Typed<V, R> for Para<S, C> {
    type El = TLine<C::E>;
    fn typed(self) -> Self::El {
        let scale = with(|sh| sh.scale.get());
        if self.wrap {
            refuse("para (wrapped text)");
        }
        let (size, lh) = (self.size * scale, self.lh * scale);
        let src = self.s;
        let Line { run, w, base, .. } =
            with(|sh| line_step(sh, size, lh, |g| src.with_str(|t| g(t))));
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
    const BY_REF: bool = true;
    #[inline(always)]
    fn up<Sk: Sink<Ty<V>>>(&self, u: &mut Up<'_, V, Sk>) {
        u.spec(&self.spec);
        let ([c0, c1], known) = Up::<V, Sk>::color(&self.color);
        // (a line that wraps is refused at mount: a description that asks for it later is not a patch)
        if !u.word(self.wrap as u32) {
            u.miss = true;
        }
        let same_c = u.words([c0, c1]) & known;
        if !<S as TPeek<R::O>>::REF {
            // read only by value: the conversion describes it (and keeps its tape entry)
            u.sh.ti.set(u.sh.ti.get() + 1);
            u.miss = true;
        } else {
            let scale = u.sh.scale.get();
            let (size, lh) = (self.size * scale, self.lh * scale);
            let l = line_step(u.sh, size, lh, |g| self.s.peek(g));
            if !l.same_w {
                // the text's width is a layout word
                u.miss = true;
            }
            if !(l.same & same_c) {
                u.leaf(&PLine {
                    run: l.run,
                    base: l.base,
                    c: self.color.expr(),
                });
            }
        }
        u.k += 1;
    }
    #[inline(always)]
    fn lists<Sk: Sink<Ty<V>>>(&self, _: Option<&[Rect]>, k: &mut usize, _: &mut Sk) {
        *k += 1;
    }
}

/// A text source read where it is (the update walk reads the element by reference). A source with no such
/// reading (`REF` false) is described the long way every time.
pub trait TPeek<O>: TextSrc<O> {
    const REF: bool;
    fn peek(&self, f: &mut dyn FnMut(&str));
}
impl<O, S: TextSrc<O>> TPeek<O> for S {
    default const REF: bool = false;
    default fn peek(&self, _: &mut dyn FnMut(&str)) {
        unreachable!("a text source that is only read by value")
    }
}
impl<O> TPeek<O> for &str {
    const REF: bool = true;
    #[inline(always)]
    fn peek(&self, f: &mut dyn FnMut(&str)) {
        f(self)
    }
}
impl<O> TPeek<O> for String {
    const REF: bool = true;
    #[inline(always)]
    fn peek(&self, f: &mut dyn FnMut(&str)) {
        f(self)
    }
}
impl<O> TPeek<O> for &Signal<String> {
    const REF: bool = true;
    #[inline(always)]
    fn peek(&self, f: &mut dyn FnMut(&str)) {
        f(self)
    }
}
impl<'a, O> TPeek<O> for Wired<'a, String, O> {
    const REF: bool = true;
    #[inline(always)]
    fn peek(&self, f: &mut dyn FnMut(&str)) {
        f(self.get())
    }
}
impl<'a, T: 'static, O, F: Fn(&T) -> String + Copy + 'static> TPeek<O> for WMap<'a, T, O, F> {
    const REF: bool = true;
    #[inline(always)]
    fn peek(&self, f: &mut dyn FnMut(&str)) {
        f(&(self.f)(self.w.get()))
    }
}
macro_rules! int_peek {
    ($($t:ty),*) => {$(
        impl<O> TPeek<O> for $t {
            const REF: bool = true;
            #[inline(always)]
            fn peek(&self, f: &mut dyn FnMut(&str)) {
                f(crate::elem::decimal(*self as i128, &mut [0u8; 40]))
            }
        }
    )*};
}
int_peek!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

/// What a text is now: its run, its width, where its baseline is; whether the tape held exactly that (`same`)
/// and whether its width and line height are what the tape held (`same_w`: its layout words).
struct Line {
    run: (u32, u32),
    w: f32,
    base: i32,
    same: bool,
    same_w: bool,
}

/// The run of a line of text, its width and where its baseline is.
///
/// In the fixed part of a view the k-th text is the same call site in every description, so what the last
/// description said of it is kept (the tape). A description that produces the same bytes at the same size
/// keeps the run without hashing or shaping. A projection can read external state even if it has no
/// captures, so the author projection runs on each description, as in the legacy façade.
/// A list row, a changed text and every oracle pass look the text up by its bytes. `read` lends the text.
fn line_step(sh: &Shared, size: f32, lh: f32, read: impl FnOnce(&mut dyn FnMut(&str))) -> Line {
    let fresh = |t: &str| {
        let mut ts = sh.text.borrow_mut();
        mark(ph::VIEW);
        let run = ts.run(t, size);
        mark(ph::SHAPE);
        let mut p = sh.pass.get();
        p.lookups += 1;
        sh.pass.set(p);
        (
            run,
            ts.get(run.0, run.1).map_or(0.0, |r| r.w).ceil(),
            ts.baseline(size, lh),
        )
    };
    if sh.plain.get() || sh.row.get().3 != 0 {
        let mut out = None;
        read(&mut |t| out = Some(fresh(t)));
        let (run, w, base) = out.expect("a text source lends its text");
        return Line {
            run,
            w,
            base,
            same: false,
            same_w: false,
        };
    }
    let k = sh.ti.get();
    sh.ti.set(k + 1);
    let (sb, lb) = (size.to_bits(), lh.to_bits());
    let hit = |e: &TapeEnt| {
        sh.text.borrow_mut().touch(e.run.0);
        let mut p = sh.pass.get();
        p.taped += 1;
        sh.pass.set(p);
        Line {
            run: e.run,
            w: e.w,
            base: e.base,
            same: true,
            same_w: true,
        }
    };
    let mut out = None;
    read(&mut |t| {
        let mut tape = sh.tape.borrow_mut();
        if let Some(e) = tape.get(k).filter(|e| {
            (e.size, e.lh) == (sb, lb) && (cfg!(feature = "mutant_typed_tape") || *e.text == *t)
        }) {
            out = Some(hit(e));
            return;
        }
        let (run, w, base) = fresh(t);
        let same_w = tape
            .get(k)
            .is_some_and(|e| (e.lh, e.w.to_bits()) == (lb, w.to_bits()));
        let ent = TapeEnt {
            text: t.into(),
            size: sb,
            lh: lb,
            run,
            w,
            base,
        };
        assert!(
            k <= tape.len(),
            "the fixed part's texts are described in one order"
        );
        if k == tape.len() {
            tape.push(ent);
        } else {
            tape[k] = ent;
        }
        out = Some(Line {
            run,
            w,
            base,
            same: false,
            same_w,
        });
    });
    out.expect("a text source lends its text")
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
    // a handler that captures values is stored with its record: only the conversion writes it
    const BY_REF: bool = E::BY_REF && size_of::<F>() == 0;
    #[inline(always)]
    fn up<Sk: Sink<Ty<V>>>(&self, u: &mut Up<'_, V, Sk>) {
        self.0.up(u)
    }
    #[inline(always)]
    fn lists<Sk: Sink<Ty<V>>>(&self, rects: Option<&[Rect]>, k: &mut usize, s: &mut Sk) {
        self.0.lists(rects, k, s)
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
            const BY_REF: bool = E::BY_REF && size_of::<F>() == 0;
            #[inline(always)]
            fn up<Sk: Sink<Ty<V>>>(&self, u: &mut Up<'_, V, Sk>) {
                self.0.up(u)
            }
            #[inline(always)]
            fn lists<Sk: Sink<Ty<V>>>(&self, rects: Option<&[Rect]>, k: &mut usize, s: &mut Sk) {
                self.0.lists(rects, k, s)
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
    const BY_REF: bool = E::BY_REF;
    #[inline(always)]
    fn up<Sk: Sink<Ty<V>>>(&self, u: &mut Up<'_, V, Sk>) {
        self.0.up(u)
    }
    #[inline(always)]
    fn lists<Sk: Sink<Ty<V>>>(&self, rects: Option<&[Rect]>, k: &mut usize, s: &mut Sk) {
        self.0.lists(rects, k, s)
    }
}
impl<V: View, R: Reach<V>, Bg: TColor<R::O>, Ch: TKids<V, R>> Typed<V, R>
    for On<LBox<Bg, Ch, ()>, HoverBg>
{
    type El = Hd<TBox<HSel<Bg::E>, Ch::K>, HSet>;
    fn typed(self) -> Self::El {
        let b = self.0;
        let c = HSel {
            id: hover_id(),
            hot: self.1.0.0,
            base: b.bg.expr(),
        };
        Hd(
            tbox(&b.spec, pbox(c, true, b.radius, b.border), b.ch.kids()),
            HSet,
        )
    }
    const BY_REF: bool = Ch::BY_REF;
    #[inline(always)]
    fn up<Sk: Sink<Ty<V>>>(&self, u: &mut Up<'_, V, Sk>) {
        let b = &self.0;
        u.spec(&b.spec);
        let id = hover_id();
        let ([c0, c1], known) = Up::<V, Sk>::color(&b.bg);
        let [d, r, bw, bc] = box_words(true, b.radius, b.border);
        if !(u.words([c0, c1, self.1.0.0, d, r, bw, bc]) & known) {
            let c = HSel {
                id,
                hot: self.1.0.0,
                base: b.bg.expr(),
            };
            u.leaf(&pbox(c, true, b.radius, b.border).0);
        }
        u.k += 1;
        b.ch.up_kids(u);
    }
    #[inline(always)]
    fn lists<Sk: Sink<Ty<V>>>(&self, rects: Option<&[Rect]>, k: &mut usize, s: &mut Sk) {
        *k += 1;
        self.0.ch.lists_kids(rects, k, s);
    }
}
/// Name a hover paint by list occurrence, stable item slot, and leaf order.
/// The fixed part and first list keep the bottom half plus its endpoint. Other list sites use the
/// upper range, with five bits for site, eighteen for slot and eight for the nonzero leaf ordinal.
fn hover_id() -> u32 {
    with(|sh| {
        let n = sh.ord.get() + 1;
        sh.ord.set(n);
        let r = sh.row.get();
        let root = r.3 == 0;
        let site = if root || cfg!(feature = "mutant_hover_scope") {
            0
        } else {
            sh.row_site.get()
        };
        if site <= 1 {
            let item = if root { 0 } else { r.1 + 1 };
            if item >= 1 << 21 || n >= 1 << 10 {
                refuse("more than 2^21 list items or 2^10 hover paints in one row");
                return 0;
            }
            return ((item << 10) | n) + 1;
        }
        let extra_site = site - 2;
        if extra_site >= 32 || r.1 >= 1 << 18 || n >= 1 << 8 {
            refuse(
                "additional hover lists need a site below 34, a slot below 2^18 and fewer than 256 hover paints per row",
            );
            return 0;
        }
        0x8000_0000 | (extra_site << 26) | (r.1 << 8) | n
    })
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
        Scroll::new(
            crate::lower::lay_spec(&p.spec, s),
            Rec1(pbox(p.bg.expr(), p.draw, p.radius, p.border)),
            tbox(&c.spec, pbox(XColor(0), false, 0.0, None), c.ch.kids()),
            slot as u8,
        )
        .wheel_line(self.line)
    }
    // its content is a row of a child space, which the walk does not reach: always the conversion
    const BY_REF: bool = false;
    fn up<Sk: Sink<Ty<V>>>(&self, _: &mut Up<'_, V, Sk>) {
        unreachable!("a scrolled box is described by its conversion")
    }
    fn lists<Sk: Sink<Ty<V>>>(&self, _: Option<&[Rect]>, _: &mut usize, _: &mut Sk) {
        unreachable!("a scrolled box is described by its conversion")
    }
}

/// The rows of a `list(..)`: row `r` is item `r`'s view, converted for an owner reached through the list.
pub struct TRows<'a, V, T> {
    list: &'a List<T>,
    off: u32,
    site: u32,
    /// The list's journal (`Shared::lists`), null when the list was refused.
    journal: *const Journal,
    all: bool,
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
        let (list, off, site) = (self.list, self.off, self.site);
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
        // SAFETY: the journal is boxed in `Shared::lists`, which only grows and outlives every pass (the
        // world's stores are dropped with the `App` that holds the `Shared`); nothing writes it while a
        // pass runs (`App::after` feeds it before the pass).
        let Some(edits) = (unsafe { self.journal.as_ref() }) else {
            return;
        };
        // A row is its item's view, and its handlers reach the item by its slot: nothing in it is the index.
        let how = bough_x::sink::RowsHow {
            all: self.all,
            by_item: true,
        };
        f.y += s.rows_j(n as u32, geo, edits, how, |r| {
            assert!(r < n);
            let slot = list.slot(r).expect("a row under the list's length");
            with(|sh| {
                sh.row
                    .set((off, slot.index(), slot.generation(), r as u32 + 1));
                sh.row_site.set(site);
                sh.ord.set(0);
                let mut p = sh.pass.get();
                p.rows += 1;
                sh.pass.set(p);
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
        list_el::<V, R, T>(&self)
    }
    const BY_REF: bool = true;
    #[inline(always)]
    fn up<Sk: Sink<Ty<V>>>(&self, u: &mut Up<'_, V, Sk>) {
        u.spec(&self.spec);
        // what a typed list is admitted by, and where it reads its offset: a change of these is not a patch
        u.must(self.virt as u32);
        match &self.scroll {
            Some(w) => {
                let (slot, _) = slot_of(w);
                u.must(slot)
            }
            None => u.must(u32::MAX - 1),
        }
        let (c, draw) = (self.bg.map_or(0, |c| c.0), self.bg.is_some());
        let [d, r, bw, bc] = box_words(draw, self.radius, None);
        if !u.words([c, d, r, bw, bc]) {
            u.leaf(&pbox(XColor(c), draw, self.radius, None).0);
        }
        // the box's record and the hole its rows are under
        u.k += 2;
    }
    #[inline(always)]
    fn lists<Sk: Sink<Ty<V>>>(&self, rects: Option<&[Rect]>, k: &mut usize, s: &mut Sk) {
        list_el::<V, R, T>(self).holes(rects, k, s)
    }
}
/// A `list(..)` as a scrolled space of rows (read where it is: every operand is a copy).
fn list_el<'a, V: View, R: Reach<V>, T: View>(
    this: &LList<'a, ListRows<'a, T, R::O>, R::O>,
) -> Scroll<Rec1<BoxP<XColor>>, TRows<'a, V, T>> {
    let this = this;
    {
        let self_ = this;
        if !<T as RowView<V>>::OK {
            refuse("a list whose row view has no typed conversion");
        }
        if !self_.virt {
            refuse("a list that is not a viewport (every row in flow)");
        }
        if self_.spec.gap != 0.0 || self_.spec.pad != [0.0; 4] {
            refuse("a list with a gap or padding");
        }
        if self_.animate.is_some() {
            refuse("list move animation");
        }
        let s = with(|sh| sh.scale.get());
        let word = match &self_.scroll {
            Some(w) => {
                let (slot, _) = slot_of(w);
                if slot == NONE {
                    refuse("a list whose offset is not a signal of the root view");
                }
                slot as u8
            }
            None => NO_WORD,
        };
        let mut spec = self_.spec;
        spec.container = false;
        Scroll::new(
            crate::lower::lay_spec(&spec, s),
            Rec1(pbox(
                XColor(self_.bg.map_or(0, |c| c.0)),
                self_.bg.is_some(),
                self_.radius,
                None,
            )),
            {
                let (list, off) = (self_.src.list.list, self_.src.list.off);
                let (journal, all, site) = with(|sh| {
                    if sh.row.get().3 != 0
                        || TypeId::of::<R::O>() != sh.root
                        || sh.owner.get().wrapping_add(off as usize)
                            != std::ptr::from_ref(list).cast()
                    {
                        refuse("a typed list must be projected from the mounted root");
                        return (std::ptr::null(), false, 0);
                    }
                    let k = sh.li.get();
                    sh.li.set(k + 1);
                    let site = (k + 1).try_into().expect("too many list sites");
                    if sh.plain.get() {
                        return (&raw const bough_x::sink::FIXED, true, site);
                    }
                    let mut lists = sh.lists.borrow_mut();
                    assert!(
                        k <= lists.len(),
                        "the view's lists are described in one order"
                    );
                    if k == lists.len() || lists[k].off != off {
                        // Every occurrence of one root field uses the same tag. A newly displayed
                        // field gets a fresh tag even when it takes over an existing site.
                        let tag = lists.iter().find(|e| e.off == off).map_or_else(
                            || {
                                let tag = sh.next_list_tag.get();
                                assert!(tag < crate::signal::QUEUED, "list tags exhausted");
                                sh.next_list_tag.set(tag + 1);
                                tag
                            },
                            |e| e.tag,
                        );
                        list.head().set(tag);
                        let entry = ListEnt {
                            off,
                            journal: Box::new(Journal::by_item()),
                            ops: list_ops::<T>,
                            end: list_end::<T>,
                            pubs: list_pubs::<T>,
                            shown: 0,
                            tag,
                        };
                        if k == lists.len() {
                            lists.push(entry);
                        } else {
                            let old = std::mem::replace(&mut lists[k], entry);
                            sh.retired_lists.borrow_mut().push(old);
                        }
                    }
                    (&*lists[k].journal as *const Journal, sh.full.get(), site)
                });
                TRows {
                    list,
                    off,
                    site,
                    journal,
                    all,
                    _v: PhantomData,
                }
            },
            word,
        )
    }
}

/// Whether an element type converts, and its conversion: every type answers (the default is no).
pub trait MaybeTyped<V: View, R: Reach<V>>: Sized {
    const OK: bool;
    const BY_REF: bool;
    type El: Part<Ty<V>> + Shape;
    fn maybe(self) -> Self::El;
    fn maybe_up<Sk: Sink<Ty<V>>>(&self, u: &mut Up<'_, V, Sk>);
    fn maybe_lists<Sk: Sink<Ty<V>>>(&self, rects: Option<&[Rect]>, k: &mut usize, s: &mut Sk);
}
impl<V: View, R: Reach<V>, E> MaybeTyped<V, R> for E {
    default const OK: bool = false;
    default const BY_REF: bool = false;
    default type El = Nope;
    default fn maybe(self) -> Self::El {
        unreachable!("an element type with no typed conversion is driven through `bough::lower`")
    }
    default fn maybe_up<Sk: Sink<Ty<V>>>(&self, _: &mut Up<'_, V, Sk>) {
        unreachable!("an element type with no typed conversion is driven through `bough::lower`")
    }
    default fn maybe_lists<Sk: Sink<Ty<V>>>(&self, _: Option<&[Rect]>, _: &mut usize, _: &mut Sk) {
        unreachable!("an element type with no typed conversion is driven through `bough::lower`")
    }
}
impl<V: View, R: Reach<V>, E: Typed<V, R>> MaybeTyped<V, R> for E {
    const OK: bool = true;
    const BY_REF: bool = E::BY_REF;
    type El = E::El;
    fn maybe(self) -> E::El {
        self.typed()
    }
    #[inline(always)]
    fn maybe_up<Sk: Sink<Ty<V>>>(&self, u: &mut Up<'_, V, Sk>) {
        self.up(u)
    }
    #[inline(always)]
    fn maybe_lists<Sk: Sink<Ty<V>>>(&self, rects: Option<&[Rect]>, k: &mut usize, s: &mut Sk) {
        self.lists(rects, k, s)
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

/// The rects of the fixed part. Its layout words are compared, as they are emitted, with the words last
/// solved: the same words at the same viewport and scale are the kept rects (layout is a function of exactly
/// those; no word of the block is read by a spec on this path). Anything else is solved by the heap tree and
/// kept. An oracle's pass always solves.
fn fixed_rects<E: Shape>(e: &E, scale: f32, u: &U, size: [f32; 2]) -> LayMemo {
    struct Cmp<'k> {
        kept: &'k [bough_lay::shape::Nd],
        k: usize,
        same: bool,
    }
    impl<'a> bough_lay::shape::Sink<'a> for Cmp<'_> {
        #[inline(always)]
        fn node(&mut self, nd: bough_lay::shape::Nd) {
            self.same &= self.kept.get(self.k) == Some(&nd);
            self.k += 1;
        }
    }
    struct Keep<'k>(&'k mut Vec<bough_lay::shape::Nd>);
    impl<'a> bough_lay::shape::Sink<'a> for Keep<'_> {
        fn node(&mut self, nd: bough_lay::shape::Nd) {
            self.0.push(nd);
        }
    }
    let env = (scale.to_bits(), [size[0].to_bits(), size[1].to_bits()]);
    // The memo is taken for the length of the pass (an oracle's pass takes nothing and so always solves): a
    // pass that unwinds leaves none, never a half-written one.
    let mut m = with(|sh| {
        if sh.plain.get() {
            LayMemo::default()
        } else {
            std::mem::take(&mut *sh.lay.borrow_mut())
        }
    });
    let mut c = Cmp {
        kept: &m.words,
        k: 0,
        same: m.env == env && !m.words.is_empty(),
    };
    e.emit(&mut c);
    if !(c.same && c.k == m.words.len()) {
        with(|sh| {
            let mut p = sh.pass.get();
            p.layouts += 1;
            sh.pass.set(p);
        });
        // Kept rects as the solver's output lanes (`[i32; 4]` per element), then as rects.
        struct Out<'r>(&'r mut Vec<Rect>);
        impl bough_lay::Out for Out<'_> {
            #[inline(always)]
            fn put(&mut self, k: usize, r: [i32; 4]) {
                self.0[k] = Rect {
                    x0: r[0],
                    y0: r[1],
                    x1: r[2],
                    y1: r[3],
                };
            }
        }
        m.rects.clear();
        m.rects.resize(
            E::LEAVES,
            Rect {
                x0: 0,
                y0: 0,
                x1: 0,
                y1: 0,
            },
        );
        if E::LEAVES <= bough_lay::shape::MAXN {
            // the layout words through the one layout function, on the stack (as a list row is laid out)
            bough_lay::place_root(
                &bough_lay::shape::Flat::of(e).root(),
                scale,
                u,
                size,
                &mut Out(&mut m.rects),
            );
        } else {
            // past the flat form's element limit: the heap tree (no element or child limit)
            let tree = bough_lay::heap::Tree::of_shape(e);
            tree.place_root(0, scale, u, size, &mut Out(&mut m.rects));
        }
        m.words.clear();
        e.emit(&mut Keep(&mut m.words));
        m.env = env;
    }
    m
}

/// One pass over the root view's element `el`.
///
/// A sink that keeps the fixed row (a re-description) gets the update walk ([`Up`]): if it finds no layout
/// word changed, the fixed part is described and only the lists are reached. A pass after a turn (a sink
/// that takes no fixed rows) reaches the lists and converts nothing. Anything else (mount, an oracle, a
/// walk that missed, an element that cannot be read where it is) is the conversion, driven as an element.
fn describe<V: View, E: MaybeTyped<V, Root>, S: Sink<Ty<V>>>(sh: &Shared, el: E, s: &mut S) {
    if E::BY_REF && !sh.plain.get() {
        if !S::FIXED {
            return el.maybe_lists(None, &mut 0, s);
        }
        let env = s.env();
        let envk = (
            env.scale.to_bits(),
            [env.size[0].to_bits(), env.size[1].to_bits()],
        );
        let keeps = s.keeps() && {
            let m = sh.lay.borrow();
            m.env == envk && m.rects.len() == <E::El as Shape>::LEAVES
        };
        let mut u = Up::new(sh, keeps.then_some(&mut *s));
        el.maybe_up(&mut u);
        let miss = u.miss || !keeps;
        drop(u);
        mark(ph::VIEW);
        if !miss {
            // the fixed part is as it was laid out: its rects are the kept ones, taken for the pass as the
            // conversion's are (`fixed_rects`)
            let memo = std::mem::take(&mut *sh.lay.borrow_mut());
            sh.li.set(0);
            sh.ord.set(0);
            el.maybe_lists(Some(&memo.rects[..]), &mut 0, s);
            *sh.lay.borrow_mut() = memo;
            return;
        }
        // the conversion reads the same tapes, from their start
        sh.ti.set(0);
        sh.li.set(0);
        sh.ord.set(0);
    }
    let e = el.maybe();
    mark(ph::VIEW);
    drive(e, s)
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
    let memo = fixed_rects(&e, env.scale, env.u, env.size);
    mark(ph::LAYOUT);
    s.row(|| (&memo.rects[..], bough_x::el::ByRef(&e)));
    mark(ph::ROW);
    e.holes(Some(&memo.rects[..]), &mut 0, s);
    with(|sh| {
        if !sh.plain.get() {
            *sh.lay.borrow_mut() = memo;
        }
    });
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
    /// The run store's pass at the last description that asked for every exposed row's runs.
    full_at: u32,
    /// A description started but unwound. Its journals and tapes cannot certify that every write landed.
    retry_description: bool,
    /// The turn's publications for `View::changed` (kept: its buffers are reused).
    pubs: crate::changed::Pubs,
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
            owner: Cell::new(std::ptr::null()),
            row: Cell::new((0, 0, 0, 0)),
            row_site: Cell::new(0),
            ord: Cell::new(0),
            describes: Cell::new(0),
            tape: RefCell::new(Vec::new()),
            ti: Cell::new(0),
            specs: RefCell::new(Vec::new()),
            ops: RefCell::new(Vec::new()),
            lay: RefCell::new(LayMemo::default()),
            lists: RefCell::new(Vec::new()),
            retired_lists: RefCell::new(Vec::new()),
            next_list_tag: Cell::new(1),
            li: Cell::new(0),
            plain: Cell::new(false),
            full: Cell::new(false),
            pass: Cell::new(PassStats::default()),
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
            full_at: 0,
            retry_description: false,
            pubs: crate::changed::Pubs::default(),
            ring: Vec::new(),
            next: 0,
            shown: 0,
            stats: Stats::default(),
        };
        app.surfaces(if cfg.ring == 0 { 4 } else { cfg.ring });
        app.world.mount(&app.ty);
        count(|c| c.world_mount += 1);
        app.lists_end(true);
        let refused = app.ty.sh.refused.borrow().clone();
        if !refused.is_empty() {
            // the view goes back as it came: its lists not displayed, no log kept
            app.lists_end(false);
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
    /// The end of a turn for the view's lists: `on`, they stay displayed (their logs start empty).
    fn lists_end(&mut self, on: bool) {
        let base = &mut self.ty.v as *mut V as *mut u8;
        let lists = self.ty.sh.lists.borrow();
        for (k, e) in lists.iter().enumerate() {
            if lists[..k].iter().all(|o| o.off != e.off) {
                // SAFETY: `off` came from an `LWired<'_, T, V>` (only `sig!` makes one) and `end` was
                // instantiated for that `T`; `self.ty.v` is that `V`, exclusively borrowed.
                unsafe { (e.end)(base.add(e.off as usize), if on { e.tag } else { 0 }) }
            }
        }
    }
    /// What the turn did to the view's lists, into their journals (each list's own log, read once). A list
    /// whose head is not the tag it was given is another `List` value in the field's place: it kept no log,
    /// and every row of it is another row.
    fn lists_sync(&mut self) {
        self.effects();
        {
            let base = &self.ty.v as *const V as *const u8;
            let mut lists = self.ty.sh.lists.borrow_mut();
            for e in lists.iter_mut() {
                if cfg!(feature = "mutant_typed_log") {
                    continue;
                }
                // SAFETY: as `lists_end`: `off` is a `List<_>` field of `V`, whose first field is its head
                // (`Headed`: `repr(C)`, a `Cell<u32>` at offset 0); the view is not mutably borrowed here.
                let head = unsafe { (*(base.add(e.off as usize) as *const Cell<u32>)).get() };
                if head & !crate::signal::QUEUED != e.tag {
                    e.journal.push(Edit::Shift(0));
                    continue;
                }
                // SAFETY: as above; `ops` was instantiated for the field's item type.
                unsafe { (e.ops)(base.add(e.off as usize), &mut e.journal) }
            }
        }
        self.lists_end(true);
    }
    /// `View::changed`, the effect site: while a list of the view has log entries the owner has not been
    /// shown, it is shown them and may write state in return (at most 8 waves, as on the legacy runtime).
    /// A turn that edited no list does not call it.
    fn effects(&mut self) {
        for _ in 0..8 {
            let mut pubs = std::mem::take(&mut self.pubs);
            pubs.clear();
            {
                let base = &self.ty.v as *const V as *const u8;
                let mut lists = self.ty.sh.lists.borrow_mut();
                for k in 0..lists.len() {
                    if lists[..k].iter().any(|o| o.off == lists[k].off) {
                        continue;
                    }
                    let e = &mut lists[k];
                    // SAFETY: as `lists_end`: `off` is a `List<T>` field of `V` and `pubs` was instantiated
                    // for that `T`; the view is not mutably borrowed here.
                    e.shown = unsafe { (e.pubs)(base.add(e.off as usize), e.shown, &mut pubs) };
                }
            }
            if pubs.is_empty() {
                self.pubs = pubs;
                for e in self.ty.sh.lists.borrow_mut().iter_mut() {
                    e.shown = 0;
                }
                return;
            }
            self.ty.v.changed(crate::changed::Changed {
                pubs: &pubs,
                readers: &[],
                _v: PhantomData,
            });
            self.pubs = pubs;
        }
        panic!(
            "View::changed did not settle in 8 waves (a cycle between changed and its own boundaries)"
        );
    }
    /// What the last description did (count bands).
    pub fn pass_stats(&self) -> PassStats {
        self.ty.sh.pass.get()
    }
    fn retire_lists(&mut self) {
        let mut retired = self.ty.sh.retired_lists.borrow_mut();
        if retired.is_empty() {
            return;
        }
        let active = self.ty.sh.lists.borrow();
        let base = (&raw mut self.ty.v).cast::<u8>();
        for entry in retired.drain(..) {
            if active.iter().all(|list| list.off != entry.off) {
                // SAFETY: admission proved that offset and callback against this root type. No
                // description or other borrow of the view is active during retirement.
                unsafe { (entry.end)(base.add(entry.off as usize), 0) };
            }
        }
    }

    /// Refresh the runs of kept rows before the text store can evict them. Every fixed description,
    /// including resize and hover-handler descriptions, advances the text store's clock.
    fn update_description(&mut self) {
        let pass = self.text.borrow().pass();
        let full = self.retry_description || pass.wrapping_sub(self.full_at) >= 128;
        if self.retry_description {
            // Force the fixed conversion and replay all exposed rows once after an incomplete write.
            *self.ty.sh.lay.borrow_mut() = LayMemo::default();
        }
        self.retry_description = true;
        self.ty.sh.full.set(full);
        self.world.redescribe(&self.ty);
        self.retire_lists();
        self.retry_description = false;
        self.ty.sh.full.set(false);
        if full {
            self.full_at = pass;
        }
    }

    /// After author code ran: the façade's contract is that the owner is described again (the legacy runtime
    /// does so after every handler and every `turn`; plain fields are state, and only a description reads
    /// them). The description is an update: the fixed part's texts against the tape, its layout words against
    /// the words last solved, its records against their columns, and of each list the rows its log names.
    /// Under `Config::mount_once` only the block is compared, as on the legacy runtime.
    fn after(&mut self, ran: bool) {
        let motion = self.world.motion();
        self.after_since(ran, motion)
    }
    /// As [`App::after`], where `motion` is the world's motion count from before the event.
    fn after_since(&mut self, ran: bool, motion: u32) {
        self.ty.ran.set(false);
        self.lists_sync();
        // (the mutant is Card 4's skip: a turn that changed a bound word is settled and not described)
        let skip = cfg!(feature = "mutant_typed_skip")
            && self.world.changed_words(&self.ty) & !(1 << HOVER) != 0;
        if ran && !self.once && !skip {
            self.ty.sh.pass.set(PassStats::default());
            self.update_description();
            {
                let refused = self.ty.sh.refused.borrow();
                assert!(
                    refused.is_empty(),
                    "a view admitted to the typed path described what it does not hold: {refused:?}"
                );
            }
            // The turn may have moved content under a still pointer: the pick again, as the legacy runtime
            // does after every turn. A row that left the pointer hears its leave, the one now under it its
            // enter; if one of those is the author's handler the view is described once more.
            self.rehover(motion);
        } else {
            self.world.settle(&self.ty);
            self.rehover(motion);
        }
    }
    /// If something shown moved since `motion` (a row entered, a list scrolled, a rect changed), the pick
    /// under the pointer is taken again and hover brought up to date; what its handlers changed is settled,
    /// or described if one of them was the author's.
    fn rehover(&mut self, motion: u32) {
        if self.world.motion() == motion {
            return;
        }
        // content that got shorter than where it is scrolled to: the offset comes back within it
        if self.world.clamp_scroll(&mut self.ty) {
            self.ty.sync();
            self.world.settle(&self.ty);
        }
        self.world.repick(&mut self.ty);
        if self.ty.ran.replace(false) && !self.once {
            self.lists_sync();
            self.update_description();
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
        let motion = self.world.motion();
        self.world.wheel(&mut self.ty, px, py, dx, dy);
        self.ty.sync();
        // the offset is written back to its signal, and the pass that moves the space follows: what is
        // under the pointer afterwards is taken again (`rehover`), from the motion count before the wheel
        self.after_since(self.ty.ran.get(), motion.wrapping_sub(1));
    }
    /// The window is `w` x `h` points now: the world is kept and the view described at the new size.
    pub fn resize(&mut self, w_pt: f32, h_pt: f32) {
        self.size_pt = (w_pt, h_pt);
        let (w, h) = self.px();
        self.world.resize(w, h);
        let n = self.ring.len();
        self.surfaces(n);
        self.lists_sync();
        self.ty.sh.pass.set(PassStats::default());
        let motion = self.world.motion().wrapping_sub(1);
        self.update_description();
        self.rehover(motion);
    }
    /// Where a typed description's instructions go, measured apart (thread instructions, bracket subtracted):
    /// (running the view and converting its fixed part with no sink; one whole re-description; the same with
    /// every list row's conversion and compare skipped because nothing is exposed is not measurable from here,
    /// so the third number is a re-description of an unchanged view, for comparison with a changed one).
    /// Thread instructions of 1,000 reads of the description context, and of 1,000 conversions of a layout
    /// spec (two of the conversion's unit costs).
    pub fn profile_units(&mut self) -> (u64, u64) {
        let ot = bough_sys::perf::tinstr_overhead();
        let t0 = bough_sys::perf::thread_instructions();
        let mut acc = 0f32;
        enter(&self.ty.sh, (&raw const self.ty.v).cast(), || {
            for _ in 0..1000 {
                acc += std::hint::black_box(with(|sh| sh.scale.get()));
            }
        });
        let t1 = bough_sys::perf::thread_instructions();
        let spec = std::hint::black_box(crate::layout::Spec::new(false, false));
        let mut n = 0u32;
        for _ in 0..1000 {
            let l = crate::lower::lay_spec(std::hint::black_box(&spec), std::hint::black_box(2.0));
            let nd =
                bough_lay::shape::Nd::pack(&l, bough_lay::shape::K_BOX, true, 1, 0.0, 0.0, [0; 2]);
            n = n.wrapping_add(std::hint::black_box(nd).0[3]);
        }
        let t2 = bough_sys::perf::thread_instructions();
        std::hint::black_box((acc, n));
        ((t1 - t0).saturating_sub(ot), (t2 - t1).saturating_sub(ot))
    }
    pub fn profile_view(&mut self) -> u64 {
        let ot = bough_sys::perf::tinstr_overhead();
        let t0 = bough_sys::perf::thread_instructions();
        let e = self.ty.v.view();
        std::hint::black_box(&e);
        let t1 = bough_sys::perf::thread_instructions();
        (t1 - t0).saturating_sub(ot)
    }
    pub fn profile(&mut self) -> (u64, u64) {
        let ot = bough_sys::perf::tinstr_overhead();
        let t0 = bough_sys::perf::thread_instructions();
        enter(&self.ty.sh, (&raw const self.ty.v).cast(), || {
            self.ty.sh.row.set((0, 0, 0, 0));
            self.ty.sh.ord.set(0);
            self.ty.sh.ti.set(0);
            self.ty.sh.li.set(0);
            let e = root_el(&self.ty.v);
            std::hint::black_box(&e);
        });
        let t1 = bough_sys::perf::thread_instructions();
        self.update_description();
        let t2 = bough_sys::perf::thread_instructions();
        ((t1 - t0).saturating_sub(ot), (t2 - t1).saturating_sub(ot))
    }
    /// Thread instructions of one re-description of the view as it is, by phase (`bough_x::probe::ph`), each
    /// net of the counter's own cost. Feature `phase_probe`; the marks cost nothing without it.
    #[cfg(feature = "phase_probe")]
    pub fn phases(&mut self) -> [u64; ph::N] {
        self.phases_of(|_| {})
    }
    /// Thread instructions of one pointer move to (x, y) points, by phase.
    #[cfg(feature = "phase_probe")]
    pub fn phases_pointer(&mut self, x: f32, y: f32) -> [u64; ph::N] {
        let over = bough_sys::perf::tinstr_overhead();
        bough_x::probe::HOOK.with(|h| h.set(Some(phase_hook)));
        PHASES.with(|c| c.set(([0; ph::N], bough_sys::perf::thread_instructions(), over)));
        self.pointer_move(x, y);
        mark(ph::SETTLE);
        bough_x::probe::HOOK.with(|h| h.set(None));
        PHASES.with(|c| c.get().0)
    }
    /// As [`App::phases`], after `f` changed the state.
    #[cfg(feature = "phase_probe")]
    pub fn phases_of(&mut self, f: impl FnOnce(&mut V)) -> [u64; ph::N] {
        f(&mut self.ty.v);
        self.lists_sync();
        let over = bough_sys::perf::tinstr_overhead();
        bough_x::probe::HOOK.with(|h| h.set(Some(phase_hook)));
        PHASES.with(|c| c.set(([0; ph::N], bough_sys::perf::thread_instructions(), over)));
        self.update_description();
        bough_x::probe::HOOK.with(|h| h.set(None));
        PHASES.with(|c| c.get().0)
    }
    pub fn lifecycle(&self) -> (u32, u32) {
        self.world.lifecycle()
    }
    /// What the world's bounded deltas did so far, and what its last delta visited (count bands).
    pub fn delta_stats(&self) -> (bough_x::world::FastStats, bough_x::world::DeltaStats) {
        (self.world.fast_stats, self.world.delta_stats)
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
        // (an oracle's pass is not the description the count bands are about)
        let stats = self.ty.sh.pass.get();
        self.ty.sh.plain.set(true);
        bough_x::sink::View::view(
            &self.ty,
            &mut bough_x::soft::Immediate::on(&mut c, &self.world.shown, w, h),
        );
        self.ty.sh.plain.set(false);
        self.ty.sh.pass.set(stats);
        c.bytes()
    }
    /// The same state mounted into a new world.
    pub fn fresh_pixels(&self) -> Vec<u8> {
        let (w, h) = self.px();
        let mut world = World::<Ty<V>, Canvas>::new(w, h, APP_WORDS);
        world.set_scale(self.scale);
        world.tick(self.world.now().max(1));
        let stats = self.ty.sh.pass.get();
        self.ty.sh.plain.set(true);
        world.mount(&self.ty);
        self.ty.sh.plain.set(false);
        self.ty.sh.pass.set(stats);
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

#[cfg(test)]
mod card5_safety {
    use super::*;

    #[test]
    fn spec_same_respects_all_value_bytes() {
        use crate::layout::{Align, Dim, Justify, Spec};
        let mut a = Spec::new(true, false);
        a.w = Dim::Pt(f32::from_bits(0x7fc0_0001));
        a.h = Dim::Pct(0.5);
        a.items = Some(Align::Baseline);
        a.justify = Some(Justify::SpaceEvenly);
        let mut b = a;
        assert!(spec_same(&a, &b));
        b.w = Dim::Pt(f32::from_bits(0x7fc0_0002));
        assert!(!spec_same(&a, &b));
        b = a;
        b.justify = None;
        assert!(!spec_same(&a, &b));
        b = a;
        b.pad[3] = -0.0;
        assert!(!spec_same(&a, &b));
        assert_eq!(spec_same(&a, &b), a.bytes() == b.bytes());
    }

    #[test]
    fn list_callbacks_use_the_current_owner_and_field_type() {
        #[repr(C)]
        struct Owner {
            untouched: List<u8>,
            items: List<u32>,
        }
        let mut owner = Owner {
            untouched: List::new(),
            items: List::new(),
        };
        owner.items.push(11);
        let offset = crate::sig!(owner: Owner => items).off as usize;
        assert_ne!(offset, 0);
        // SAFETY: sig! established that the offset names Owner::items. This is its exclusive owner.
        unsafe { list_end::<u32>(std::ptr::from_mut(&mut owner).cast::<u8>().add(offset), 7) };
        owner.items.insert(0, 22);
        *owner.items.edit(1) = 33;
        let moved = owner;
        let mut journal = Journal::by_item();
        journal.win.store(2, core::sync::atomic::Ordering::Relaxed);
        let mut pubs = crate::changed::Pubs::default();
        // SAFETY: use the current shared owner and the same field offset and type. The callbacks borrow it
        // only during each call, after the move.
        let len = unsafe {
            let field = std::ptr::from_ref(&moved).cast::<u8>().add(offset);
            list_ops::<u32>(field, &mut journal);
            list_pubs::<u32>(field, 1, &mut pubs)
        };
        assert_eq!(len, 2);
        assert_eq!(&journal.log[..2], &[Edit::Insert(0), Edit::Update(1)]);
        let changed = crate::changed::Changed {
            pubs: &pubs,
            readers: &[],
            _v: PhantomData,
        };
        assert!(matches!(
            changed.list(crate::sig!(moved: Owner => items)),
            Some([crate::list::Op::Edit { .. }])
        ));
        assert!(
            changed
                .list(crate::sig!(moved: Owner => untouched))
                .is_none()
        );
    }
}
