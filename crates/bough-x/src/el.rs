//! The element layer: the façade's way of writing a view, on this core.
//!
//! The author writes what the façade has: `fn view(&self) -> impl Element<Self> + '_`, `div()`, `row((..))`,
//! `.bg(sig!(self.look).map(|l| ..))`, `text(..)`, `.on_hover(|v, e| ..)`, `each(iter.map(row))`, plain state in
//! [`Signal`] fields that handlers set. Nothing here is a second runtime: an element is a value whose type is
//! both its records ([`Row`]) and its layout ([`bough_lay::Lay`]), and [`Element::drive`] hands it to whatever
//! sink is asking (mount, re-description, a turn's list pass, the immediate-mode oracle).
//!
//! What each construct becomes:
//!
//! * `sig!(self.field)` is a lens whose word of the block is found from the field's byte offset in the view
//!   (`offset_of!`, a constant of the type). The block is gathered from the view's own fields before a turn is
//!   settled, so a handler's write is a plain store and is noticed by the compare. No field is numbered.
//! * `.map(closure)` keeps captures in typed CPU slots. Padding is never exposed as words;
//!   evaluated outputs determine whether opaque records changed.
//! * `.transition(curve)` keeps where the value started from, and when, in the record. A turn that moves the
//!   target restarts it from what the record shows; frames in between evaluate nothing on the CPU.
//! * `each(iter)` under a scrolled box is a list by exposure: the iterator is indexed, never walked
//!   (`TrustedRandomAccess`), so a hundred thousand rows cost what is on screen.
//! * a handler is a component of the box's record; its captures are a lane beside the column.
//!
//! What cannot be carried from the façade unchanged is listed in the expr journal, with the reason for each.
use crate::event::{Click, Drag, Handler, Hover, On, OnFocus, OnKey, OnText, Press, Target, Wheel};
use crate::expr::{
    APP_WORDS, Code, Color, Cx as ECx, Expr, Lens, Slot, Turn, U, U_SCALE, U_TIME, Un, Word, Words,
};
use crate::kinds::{Fill, LabelArch, Motion, NumArch, OutlineArch, Txt};
#[cfg(feature = "inspect-metadata")]
use crate::reflect::Val;
use crate::sink::{Cx, Geo, Leaf, Mk, Row, Sink};
#[cfg(feature = "inspect-metadata")]
use crate::store::Op;
use crate::store::{Arch, Backend, ColRef, POINTS, Rect, Site, Space};
use bough_lay::flex::{Dim, Spec};
use bough_lay::shape::{Flat, K_BOX, K_LINE, Nd, Sink as LSink};
use bough_lay::{Chain, Hk, LBox, Lay, Rec1, Shape};
use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::iter::TrustedRandomAccessNoCoerce;

pub use bough_lay::{Align, Flex, Justify, block, column, div, row};

// ---- state ---------------------------------------------------------------------------------------------------

// The generation and revision share one atomic word. Checking generation in a
// separate load before incrementing a revision would let a stale setter race
// slot release/reuse and increment the new world's counter.
const INPUT_GEN_BITS: u32 = 20;
const INPUT_GEN_MAX: u32 = (1 << INPUT_GEN_BITS) - 1;
const INPUT_REV_BITS: u32 = 43;
const INPUT_REV_MASK: u64 = (1 << INPUT_REV_BITS) - 1;
const INPUT_SHARED: u64 = 1 << INPUT_REV_BITS;
const INPUT_GEN_SHIFT: u32 = INPUT_REV_BITS + 1;
const MULTI_WORLD: u32 = u32::MAX;

struct WorldInputSlot {
    state: AtomicU64,
    busy: AtomicBool,
}
impl WorldInputSlot {
    const fn new() -> Self {
        Self {
            state: AtomicU64::new(1 << INPUT_GEN_SHIFT),
            busy: AtomicBool::new(false),
        }
    }
    fn claim(&self, index: u32) -> Option<u32> {
        self.busy
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()?;
        let generation = (self.state.load(Ordering::Relaxed) >> INPUT_GEN_SHIFT) as u32;
        debug_assert!(generation > 0 && generation < INPUT_GEN_MAX);
        self.state
            .store((generation as u64) << INPUT_GEN_SHIFT, Ordering::Release);
        Some((index << INPUT_GEN_BITS) | generation)
    }
    fn live(&self, token: u32) -> bool {
        self.busy.load(Ordering::Acquire)
            && self.state.load(Ordering::Acquire) >> INPUT_GEN_SHIFT
                == (token & INPUT_GEN_MAX) as u64
    }
    fn update(&self, token: u32, shared: bool) {
        let generation = (token & INPUT_GEN_MAX) as u64;
        let mut state = self.state.load(Ordering::Relaxed);
        loop {
            if state >> INPUT_GEN_SHIFT != generation {
                return;
            }
            let next = if shared {
                state | INPUT_SHARED
            } else {
                assert!(
                    state & INPUT_REV_MASK < INPUT_REV_MASK,
                    "input revision exhausted"
                );
                state + 1
            };
            match self.state.compare_exchange_weak(
                state,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(current) => state = current,
            }
        }
    }
    fn release(&self, token: u32) {
        let generation = token & INPUT_GEN_MAX;
        // Generation replacement invalidates in-flight CAS operations from the
        // old owner before another allocator can claim this slot.
        self.state.store(
            ((generation + 1) as u64) << INPUT_GEN_SHIFT,
            Ordering::Release,
        );
        if generation + 1 < INPUT_GEN_MAX {
            self.busy.store(false, Ordering::Release);
        }
        // The terminal generation stays busy forever. It is never issued, so
        // neither stale tokens nor MULTI_WORLD can alias a future allocation.
    }
}
static WORLD_INPUTS: [WorldInputSlot; 4096] = [const { WorldInputSlot::new() }; 4096];
static MULTI_WORLD_REV: AtomicU64 = AtomicU64::new(0);

/// Owning slab reservation: automatically released when its World is dropped,
/// including unwinding during world construction.
pub(crate) struct InputToken(u32);
impl InputToken {
    pub(crate) fn new() -> Self {
        for (i, slot) in WORLD_INPUTS.iter().enumerate() {
            if let Some(token) = slot.claim(i as u32) {
                return Self(token);
            }
        }
        panic!("all 4096 world input slots are occupied or retired");
    }
    pub(crate) fn revision(&self) -> u64 {
        let state = input_slot(self.0).state.load(Ordering::Relaxed);
        let shared = if state & INPUT_SHARED != 0 {
            MULTI_WORLD_REV.load(Ordering::Relaxed)
        } else {
            0
        };
        (state & INPUT_REV_MASK)
            .checked_add(shared)
            .expect("combined input revision exhausted")
    }
}
impl Drop for InputToken {
    fn drop(&mut self) {
        input_slot(self.0).release(self.0);
    }
}
fn input_slot(token: u32) -> &'static WorldInputSlot {
    &WORLD_INPUTS[(token >> INPUT_GEN_BITS) as usize]
}
thread_local! {
    static INPUT_TOKEN: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
}
pub(crate) fn track_inputs<R>(owner: &InputToken, f: impl FnOnce() -> R) -> R {
    struct Pop(u32);
    impl Drop for Pop {
        fn drop(&mut self) {
            INPUT_TOKEN.with(|s| s.set(self.0));
        }
    }
    let _pop = Pop(INPUT_TOKEN.with(|s| s.replace(owner.0)));
    f()
}

/// State with a packed, generation-checked world input token.
/// Moving preserves identity; cloning creates independent, unobserved state.
/// There is no signal destructor or per-signal allocation.
#[derive(Debug)]
pub struct Signal<T>(T, AtomicU32);
impl<T: Clone> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self::new(self.0.clone())
    }
}
impl<T: Default> Default for Signal<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}
impl<T> Signal<T> {
    pub const fn new(v: T) -> Self {
        Self(v, AtomicU32::new(0))
    }
    pub fn into_inner(self) -> T {
        self.0
    }
    fn observe(&self) {
        let token = INPUT_TOKEN.with(core::cell::Cell::get);
        if token == 0 {
            return;
        }
        let mut current = self.1.load(Ordering::Relaxed);
        loop {
            if current == token {
                return;
            }
            if current == MULTI_WORLD {
                input_slot(token).update(token, true);
                return;
            }
            let shared = current != 0 && input_slot(current).live(current);
            let next = if shared { MULTI_WORLD } else { token };
            match self
                .1
                .compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => {
                    if shared {
                        input_slot(current).update(current, true);
                        input_slot(token).update(token, true);
                    }
                    return;
                }
                Err(actual) => current = actual,
            }
        }
    }
    pub fn set_silent(&mut self, v: T) {
        self.0 = v;
    }
}
impl<T: PartialEq> Signal<T> {
    pub fn set(&mut self, v: T) -> bool {
        if self.0 == v {
            return false;
        }
        self.0 = v;
        let token = *self.1.get_mut();
        if token == MULTI_WORLD {
            MULTI_WORLD_REV
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
                .expect("shared input revision exhausted");
        } else if token != 0 {
            input_slot(token).update(token, false);
        }
        true
    }
}
impl<T> core::ops::Deref for Signal<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.observe();
        &self.0
    }
}

/// A field of the view named by its path: `sig!(self.look)`. The offset is a constant of the type.
pub struct Sig<'a, V, T, const OFF: usize> {
    r: &'a T,
    projection: fn(&V) -> &T,
    cache: Option<&'static std::sync::OnceLock<Slot>>,
    _v: PhantomData<fn(&V)>,
}
/// A safe typed reader. OFF selects a nominal block slot and is never dereferenced.
#[inline(always)]
pub fn prove<'a, V, T, const OFF: usize>(r: &'a T, _p: fn(&V) -> &T) -> Sig<'a, V, T, OFF> {
    Sig {
        r,
        projection: _p,
        cache: None,
        _v: PhantomData,
    }
}
impl<'a, V, T, const OFF: usize> Sig<'a, V, T, OFF> {
    #[doc(hidden)]
    pub fn cached(mut self, cache: &'static std::sync::OnceLock<Slot>) -> Self {
        self.cache = Some(cache);
        self
    }
}
fn field_registry() -> &'static std::sync::Mutex<std::collections::HashMap<core::any::TypeId, Slot>>
{
    static FIELDS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<core::any::TypeId, Slot>>,
    > = std::sync::OnceLock::new();
    FIELDS.get_or_init(Default::default)
}
fn admitted_field<L: 'static>() -> Slot {
    let slot = field_registry()
        .lock()
        .unwrap()
        .get(&core::any::TypeId::of::<L>())
        .copied();
    slot.expect("field projection has not been admitted by sig!/prove")
}
/// `sig!(self.field)`: the field as an operand. In a view whose type has generic parameters the compiler
/// cannot name `Self` in a constant: write `sig!(self: TheType => field)`.
#[macro_export]
macro_rules! sig {
    ($o:ident : $t:ty => $($f:ident).+) => {{
        static SLOT: ::std::sync::OnceLock<$crate::expr::Slot> = ::std::sync::OnceLock::new();
        let r = &$o.$($f).+;
        let p: fn(&$t) -> &_ = |o: &$t| &o.$($f).+;
        $crate::el::prove::<$t, _, { ::core::mem::offset_of!($t, $($f).+) }>(r, p).cached(&SLOT)
    }};
    ($s:tt . $($f:ident).+) => {{
        static SLOT: ::std::sync::OnceLock<$crate::expr::Slot> = ::std::sync::OnceLock::new();
        let r = &$s.$($f).+;
        let p: fn(&Self) -> &_ = |o: &Self| &o.$($f).+;
        $crate::el::prove::<Self, _, { ::core::mem::offset_of!(Self, $($f).+) }>(r, p).cached(&SLOT)
    }};
}

/// The lens of a field at byte offset `OFF` of the view `V`: its word of the block is the offset's word,
/// folded into the application's words.
pub struct At<V, X, const OFF: usize>(PhantomData<fn(&V) -> X>);
impl<V, X, const OFF: usize> Clone for At<V, X, OFF> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<V, X, const OFF: usize> Copy for At<V, X, OFF> {}
impl<V: 'static, X: Word, const OFF: usize> Lens for At<V, X, OFF> {
    type T = X;
    const SLOT: usize = (OFF / 4) % APP_WORDS;
    fn field() -> Option<Slot> {
        Some(admitted_field::<Self>())
    }
}

impl<V: 'static, X: Word, const OFF: usize> crate::expr::FieldProj<V> for At<V, X, OFF> {
    type T = X;
    const NOMINAL_OFF: usize = OFF;
    fn read(v: &V) -> X {
        X::from_word(admitted_field::<Self>().read(v))
    }
}
/// An operand whose block words (if any) are fields of the view `V`. A field path proved against one view
/// cannot end up in another view's records.
pub trait Rooted<V> {}
impl<V> Rooted<V> for Color {}

impl<'a, V: 'static, X: Word, const OFF: usize> Sig<'a, V, Signal<X>, OFF> {
    fn admit(&self) {
        self.r.observe();
        let create = || {
            let mut fields = field_registry().lock().unwrap();
            *fields
                .entry(core::any::TypeId::of::<At<V, X, OFF>>())
                .or_insert_with(|| {
                    let p = self.projection;
                    Slot::projected::<V, Signal<X>, At<V, X, OFF>>(
                        <At<V, X, OFF> as Lens>::SLOT,
                        OFF,
                        p,
                        |s| s.0.to_word(),
                    )
                })
        };
        let slot = if let Some(cache) = self.cache {
            let cached = *cache.get_or_init(create);
            // A static inside a generic function is shared across instantiations.
            if cached.owner() == core::any::TypeId::of::<V>() && cached.offset() == OFF {
                cached
            } else {
                create()
            }
        } else {
            create()
        };
        assert_eq!(
            slot.owner(),
            core::any::TypeId::of::<V>(),
            "field cache owner mismatch"
        );
        assert_eq!(slot.offset(), OFF, "field cache projection mismatch");
    }
    /// The field through a function. The closure may capture: its captures are the record's own words.
    #[inline(always)]
    pub fn map<O: Copy + PartialEq, F: Fn(&X) -> O + Copy + 'static>(
        self,
        f: F,
    ) -> MapRef<F, At<V, X, OFF>> {
        self.admit();
        MapRef(f, PhantomData)
    }
    /// The field itself as an expression.
    #[inline(always)]
    pub fn get(self) -> Un<At<V, X, OFF>> {
        self.admit();
        crate::expr::un()
    }
    /// The field as a number to show (read now: a record's own value).
    #[inline(always)]
    pub fn num<F: Fn(&X) -> f32>(self, f: F, fmt: NumFmt) -> NumText {
        self.r.observe();
        fmt.of(f(&self.r.0))
    }
    /// The word of the block the field is.
    #[inline(always)]
    pub fn word(&self) -> u8 {
        self.admit();
        <At<V, X, OFF> as Lens>::SLOT as u8
    }
}
impl<'a, V: 'static, X: Word, const N: usize, const OFF: usize> Sig<'a, V, [Signal<X>; N], OFF> {
    /// One element of an array field.
    #[inline(always)]
    pub fn at(self, i: usize) -> SigAt<'a, X> {
        self.r[i].observe();
        SigAt(&self.r[i].0)
    }
}
/// One element of an array field of signals: its value now.
pub struct SigAt<'a, X>(&'a X);
impl<X: Word> SigAt<'_, X> {
    #[inline(always)]
    pub fn num<F: Fn(&X) -> f32>(self, f: F, fmt: NumFmt) -> NumText {
        fmt.of(f(self.0))
    }
}

/// A field through an opaque CPU closure. Captures are never exposed as raw words.
#[repr(transparent)]
pub struct MapRef<F, L>(F, PhantomData<L>);
impl<F: Copy, L> Clone for MapRef<F, L> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<F: Copy, L> Copy for MapRef<F, L> {}
impl<F, L, O> Expr for MapRef<F, L>
where
    L: Lens,
    O: Copy + PartialEq,
    F: Fn(&L::T) -> O + Copy + 'static,
{
    type Out = O;
    type Stage = Turn;
    const READS: u64 = 1 << L::SLOT;
    const CODE: &'static Code = &Code::Cpu(&[&Code::U(L::SLOT)]);
    #[inline(always)]
    fn eval(&self, cx: &ECx) -> O {
        (self.0)(&cx.u.get::<L>())
    }
    #[inline]
    fn slots(f: &mut dyn FnMut(Slot)) {
        <Un<L> as Expr>::slots(f)
    }
}
impl<V: 'static, X: Word, F, const OFF: usize> Rooted<V> for MapRef<F, At<V, X, OFF>> {}
impl<V: 'static, X: Word, const OFF: usize> Rooted<V> for Un<At<V, X, OFF>> {}

// ---- transitions ---------------------------------------------------------------------------------------------

/// How a transition runs: its length and its easing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Curve {
    pub ms: u32,
    pub ease_out: bool,
}
impl Curve {
    /// Ease out over `secs` seconds.
    pub const fn out(secs: f32) -> Curve {
        Curve {
            ms: (secs * 1000.0) as u32,
            ease_out: true,
        }
    }
    pub const fn linear(secs: f32) -> Curve {
        Curve {
            ms: (secs * 1000.0) as u32,
            ease_out: false,
        }
    }
}

/// A value that can be part way between two values. `p` is progress in 1/256ths.
pub trait Lerp: Copy + PartialEq {
    type W: Words;
    fn to_w(self) -> Self::W;
    fn from_w(w: Self::W) -> Self;
    fn lerp(a: Self, b: Self, p: u32) -> Self;
}
impl Lerp for Color {
    type W = u32;
    #[inline(always)]
    fn to_w(self) -> u32 {
        self.0
    }
    #[inline(always)]
    fn from_w(w: u32) -> Color {
        Color(w)
    }
    #[inline(always)]
    fn lerp(a: Color, b: Color, p: u32) -> Color {
        let ch = |s: u32| {
            let (x, y) = ((a.0 >> s & 0xff) as i32, (b.0 >> s & 0xff) as i32);
            ((x + ((y - x) * p as i32 >> 8)) as u32 & 0xff) << s
        };
        Color(ch(0) | ch(8) | ch(16) | ch(24))
    }
}
impl Lerp for (f32, f32) {
    type W = [f32; 2];
    #[inline(always)]
    fn to_w(self) -> [f32; 2] {
        [self.0, self.1]
    }
    #[inline(always)]
    fn from_w(w: [f32; 2]) -> (f32, f32) {
        (w[0], w[1])
    }
    #[inline(always)]
    fn lerp(a: (f32, f32), b: (f32, f32), p: u32) -> (f32, f32) {
        let t = p as f32 / 256.0;
        (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
    }
}

/// `e.transition(curve)`: the value of `e`, reached from wherever it was over the curve's length. Where it
/// started from and when are words of the record, written by the core in the turn that moves the target
/// ([`Expr::carry`]); `t0 == 0` is "at rest".
#[repr(C)]
pub struct Tr<E: Expr>
where
    E::Out: Lerp,
{
    e: E,
    from: <E::Out as Lerp>::W,
    t0: u32,
    /// Milliseconds; the top bit is ease-out.
    dur: u32,
}
impl<E: Expr> Clone for Tr<E>
where
    E::Out: Lerp,
{
    fn clone(&self) -> Self {
        *self
    }
}
impl<E: Expr> Copy for Tr<E> where E::Out: Lerp {}
// SAFETY: `repr(C)` over `Words` fields of alignment at most 4: no padding.
unsafe impl<E: Expr + Words> Words for Tr<E> where E::Out: Lerp {}
impl<E: Expr> Tr<E>
where
    E::Out: Lerp,
{
    #[inline(always)]
    fn progress(&self, now: u32) -> Option<u32> {
        let dur = self.dur & 0x7fff_ffff;
        let dt = now.wrapping_sub(self.t0);
        if self.t0 == 0 || dt >= dur {
            return None;
        }
        let p = (dt as u64 * 256 / dur as u64) as u32;
        Some(if self.dur >> 31 != 0 {
            // ease out, cubic: 1 - (1 - p)^3
            let q = 256 - p;
            256 - (q * q * q >> 16)
        } else {
            p
        })
    }
}
impl<E: Expr> Expr for Tr<E>
where
    E::Out: Lerp,
{
    type Out = E::Out;
    type Stage = Turn;
    const READS: u64 = E::READS | 1 << U_TIME;
    const RESTS: bool = E::RESTS;
    const STATE: bool = true;
    const CODE: &'static Code = &Code::Cpu(&[E::CODE, &Code::U(U_TIME)]);
    #[inline(always)]
    fn eval(&self, cx: &ECx) -> E::Out {
        let to = self.e.eval(cx);
        match self.progress(cx.u.w[U_TIME]) {
            None => to,
            Some(p) => Lerp::lerp(<E::Out as Lerp>::from_w(self.from), to, p),
        }
    }
    #[inline(always)]
    fn until(&self, cx: &ECx) -> u32 {
        let now = cx.u.w[U_TIME];
        let mine = if self.t0 == 0 {
            0
        } else {
            crate::expr::ramp_end(self.t0, self.dur & 0x7fff_ffff, now)
        };
        crate::expr::latest(mine, self.e.until(cx), now)
    }
    #[inline]
    fn slots(f: &mut dyn FnMut(Slot)) {
        E::slots(f)
    }
    #[inline(always)]
    fn eq_ops(&self, o: &Self) -> bool {
        (self.dur == o.dur)
            & if E::STATE {
                self.e.eq_ops(&o.e)
            } else {
                self.e.same_record(&o.e)
            }
    }
    #[inline(always)]
    fn carry(&mut self, was: &Self, old: &ECx, new: &ECx) {
        if E::STATE {
            self.e.carry(&was.e, old, new);
        }
        if self.e.eval(new) != was.e.eval(old) {
            // another target: go there from what is on screen now
            self.from = was.eval(old).to_w();
            self.t0 = new.u.w[U_TIME].max(1);
        } else {
            (self.from, self.t0) = (was.from, was.t0);
        }
    }
}
impl<V, E: Expr + Rooted<V>> Rooted<V> for Tr<E> where E::Out: Lerp {}

/// `.transition(curve)` on any operand whose value can be part way.
pub trait Transition: Expr + Sized
where
    Self::Out: Lerp,
{
    #[inline(always)]
    fn transition(self, c: Curve) -> Tr<Self> {
        // SAFETY: `W: Words`: zero bytes are a valid value.
        Tr {
            e: self,
            from: unsafe { core::mem::zeroed() },
            t0: 0,
            dur: c.ms.max(1) | (c.ease_out as u32) << 31,
        }
    }
}
impl<E: Expr> Transition for E where E::Out: Lerp {}

/// Points to device pixels: an offset in points as the pixels a record moves by.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Px<E>(pub E);
// SAFETY: transparent over `E: Words`.
unsafe impl<E: Words> Words for Px<E> {}
impl<E: Expr<Out = (f32, f32)>> Expr for Px<E> {
    type Out = (i32, i32);
    type Stage = Turn;
    const READS: u64 = E::READS | 1 << U_SCALE;
    const RESTS: bool = E::RESTS;
    const STATE: bool = E::STATE;
    const CODE: &'static Code = &Code::Cpu(&[E::CODE, &Code::U(U_SCALE)]);
    #[inline(always)]
    fn eval(&self, cx: &ECx) -> (i32, i32) {
        let (x, y) = self.0.eval(cx);
        let s = f32::from_bits(cx.u.w[U_SCALE]);
        (bough_lay::val::round(x * s), bough_lay::val::round(y * s))
    }
    #[inline(always)]
    fn until(&self, cx: &ECx) -> u32 {
        self.0.until(cx)
    }
    #[inline]
    fn slots(f: &mut dyn FnMut(Slot)) {
        E::slots(f)
    }
    #[inline(always)]
    fn eq_ops(&self, o: &Self) -> bool {
        self.0.eq_ops(&o.0)
    }
    #[inline(always)]
    fn carry(&mut self, was: &Self, old: &ECx, new: &ECx) {
        self.0.carry(&was.0, old, new)
    }
}
impl<E: Expr<Out = (f32, f32)>> Motion for Px<E> {}
impl<V, E: Rooted<V>> Rooted<V> for Px<E> {}

/// A record that does not move.
#[derive(Clone, Copy)]
pub struct Still;
// SAFETY: zero-sized.
unsafe impl Words for Still {}
impl Expr for Still {
    type Out = (i32, i32);
    type Stage = crate::expr::Const;
    const READS: u64 = 0;
    const CODE: &'static Code = &Code::Lit(0);
    #[inline(always)]
    fn eval(&self, _: &ECx) -> (i32, i32) {
        (0, 0)
    }
}
impl Motion for Still {}
impl<V> Rooted<V> for Still {}

// ---- paints --------------------------------------------------------------------------------------------------

/// `rgb(0xRRGGBB)`.
#[inline(always)]
pub const fn rgb(v: u32) -> Color {
    Color(v)
}

/// What `.bg(..)` was given: the record it makes.
pub trait Paints<V> {
    type A: Arch;
    fn arch(&self) -> Self::A;
}
impl<V, E: Expr<Out = Color> + Rooted<V>> Paints<V> for E {
    type A = Fill<E>;
    #[inline(always)]
    fn arch(&self) -> Fill<E> {
        Fill(*self)
    }
}

/// A border of `w` points in colour `c`, moved by `t` (pixels).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Bord<C, T> {
    pub w: f32,
    pub c: C,
    pub t: T,
}
// SAFETY: `repr(C)` over `Words` fields.
unsafe impl<C: Words, T: Words> Words for Bord<C, T> {}
impl<C: Expr<Out = Color>, T: Motion> Arch for Bord<C, T> {
    const READS: u64 = C::READS | T::READS;
    const MOVES: bool = T::READS != 0;
    const TIMED: bool = (T::READS | C::READS) >> U_TIME & 1 != 0;
    const ENDLESS: bool = !(T::RESTS & C::RESTS);
    const NAME: &'static str = "outline";
    const STAGE: crate::expr::StageId = crate::expr::later(&[T::STAGE, C::STAGE]);
    const STATE: bool = C::STATE | T::STATE;
    #[inline(always)]
    fn eq_ops(&self, o: &Self) -> bool {
        (self.w == o.w) & self.c.eq_ops(&o.c) & self.t.eq_ops(&o.t)
    }
    #[inline(always)]
    fn carry(&mut self, was: &Self, old: &U, new: &U) {
        self.c.carry(&was.c, &ECx::of(old), &ECx::of(new));
        self.t.carry(&was.t, &ECx::of(old), &ECx::of(new));
    }
    #[inline]
    fn slots(f: &mut dyn FnMut(Slot)) {
        C::slots(f);
        T::slots(f)
    }
    #[inline(always)]
    fn until(&self, u: &U) -> u32 {
        crate::expr::latest(
            self.t.until(&ECx::of(u)),
            self.c.until(&ECx::of(u)),
            u.w[U_TIME],
        )
    }
    #[inline(always)]
    fn sweep(&self, r: Rect, u: &U) -> Rect {
        let (lo, hi) = self.t.bounds(u);
        r.shift(lo).or(r.shift(hi))
    }
    #[inline(always)]
    fn changed(&self, old: &U, new: &U) -> bool {
        self.c.eval(&ECx::of(old)) != self.c.eval(&ECx::of(new))
    }
    #[inline(always)]
    fn offset(&self, u: &U) -> (i32, i32) {
        self.t.eval(&ECx::of(u))
    }
    #[inline(always)]
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
        b.outline(col, u)
    }
}
impl<C: Expr<Out = Color>, T: Motion> OutlineArch for Bord<C, T> {
    const WIDTH: i32 = 0;
    #[inline(always)]
    fn width(&self, u: &U) -> i32 {
        bough_lay::val::round(self.w * f32::from_bits(u.w[U_SCALE])).max(1)
    }
    #[inline(always)]
    fn color(&self, u: &U) -> Color {
        self.c.eval(&ECx::of(u))
    }
}
impl<V, C: Expr<Out = Color> + Rooted<V>, T: Motion + Rooted<V>> Paints<V> for Bord<C, T> {
    type A = Bord<C, T>;
    #[inline(always)]
    fn arch(&self) -> Self {
        *self
    }
}

/// The façade's calls on a box that are paint, not layout.
pub trait Deco<P, C, H>: Sized {
    /// A border of `w` points. (First cut: a bordered box has no fill.)
    #[cfg_attr(feature = "inspect-metadata", track_caller)]
    fn border<Cl>(self, w: f32, c: Cl) -> LBox<Rec1<Bord<Cl, Still>>, C, H>;
    /// Positioned by its parent's layout as usual. (The façade's marker for an `abs` child's container.)
    fn relative(self) -> Self {
        self
    }
}
impl<P, C, H> Deco<P, C, H> for LBox<P, C, H> {
    #[cfg_attr(feature = "inspect-metadata", track_caller)]
    #[inline(always)]
    fn border<Cl>(self, w: f32, c: Cl) -> LBox<Rec1<Bord<Cl, Still>>, C, H> {
        #[cfg(feature = "inspect-metadata")]
        {
            return self.bg_at(Bord { w, c, t: Still }, core::panic::Location::caller());
        }
        #[cfg(not(feature = "inspect-metadata"))]
        {
            self.bg(Bord { w, c, t: Still })
        }
    }
}
/// `.translate(offset in points)` on a bordered box: it moves without layout.
pub trait Translate<Cl, C, H> {
    #[cfg_attr(feature = "inspect-metadata", track_caller)]
    fn translate<E: Expr<Out = (f32, f32)>>(self, e: E) -> LBox<Rec1<Bord<Cl, Px<E>>>, C, H>;
}
impl<Cl: Copy, C, H> Translate<Cl, C, H> for LBox<Rec1<Bord<Cl, Still>>, C, H> {
    #[cfg_attr(feature = "inspect-metadata", track_caller)]
    #[inline(always)]
    fn translate<E: Expr<Out = (f32, f32)>>(self, e: E) -> LBox<Rec1<Bord<Cl, Px<E>>>, C, H> {
        let b = self.paint.0;
        let painted = Bord {
            w: b.w,
            c: b.c,
            t: Px(e),
        };
        #[cfg(feature = "inspect-metadata")]
        {
            return self.bg_at(painted, core::panic::Location::caller());
        }
        #[cfg(not(feature = "inspect-metadata"))]
        {
            self.bg(painted)
        }
    }
}

// ---- text ----------------------------------------------------------------------------------------------------

/// How a number is shown. (The façade's: a prefix, decimals, a suffix. Prefix and suffix are not drawn yet.)
#[derive(Clone, Copy, Debug)]
pub struct NumFmt {
    pub decimals: u8,
}
impl NumFmt {
    pub const fn new(_prefix: &'static str, decimals: Option<u8>, _suffix: &'static str) -> NumFmt {
        NumFmt {
            decimals: match decimals {
                Some(d) => d,
                None => 0,
            },
        }
    }
    #[inline(always)]
    pub fn of(self, v: f32) -> NumText {
        let mut k = 1.0f64;
        let mut i = 0;
        while i < self.decimals {
            k *= 10.0;
            i += 1;
        }
        NumText {
            value: (v as f64 * k).round() as i32,
            decimals: self.decimals as u32,
        }
    }
}
/// A number to show: an integer scaled by `10^decimals`, drawn with tabular digits while shading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumText {
    pub value: i32,
    pub decimals: u32,
}

/// What a text shows.
pub trait Content {
    type A<C: Expr<Out = Color>>: Arch;
    fn arch<C: Expr<Out = Color>>(&self, c: C) -> Self::A<C>;
    /// Characters, for the stand-in measure (the text lane's `Measure` replaces it).
    fn chars(&self) -> usize;
}

/// A number record: the value and its decimals are the record's.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NumRec<C> {
    v: i32,
    d: u32,
    c: C,
}
// SAFETY: `repr(C)` over `Words` fields.
unsafe impl<C: Words> Words for NumRec<C> {}
/// A text record: interned bytes (address and length) and a colour.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct StrRec<C> {
    at: [u32; 2],
    len: u32,
    c: C,
}
// SAFETY: `repr(C)` over `Words` fields.
unsafe impl<C: Words> Words for StrRec<C> {}

#[cfg(not(feature = "inspect-metadata"))]
macro_rules! color_arch {
    ($t:ident, $name:literal, $method:ident, |$s:ident, $o:ident| $eq:expr) => {
        impl<C: Expr<Out = Color>> Arch for $t<C> {
            const READS: u64 = C::READS;
            const NAME: &'static str = $name;
            const STAGE: crate::expr::StageId = C::STAGE;
            const TIMED: bool = C::READS >> U_TIME & 1 != 0;
            const ENDLESS: bool = !C::RESTS;
            const STATE: bool = C::STATE;
            #[inline(always)]
            fn eq_ops(&self, o: &Self) -> bool {
                let ($s, $o) = (self, o);
                $eq & self.c.eq_ops(&o.c)
            }
            #[inline(always)]
            fn carry(&mut self, was: &Self, old: &U, new: &U) {
                self.c.carry(&was.c, &ECx::of(old), &ECx::of(new));
            }
            #[inline]
            fn slots(f: &mut dyn FnMut(Slot)) {
                C::slots(f)
            }
            #[inline(always)]
            fn until(&self, u: &U) -> u32 {
                self.c.until(&ECx::of(u))
            }
            #[inline(always)]
            fn changed(&self, old: &U, new: &U) -> bool {
                self.c.eval(&ECx::of(old)) != self.c.eval(&ECx::of(new))
            }
            #[inline(always)]
            fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
                b.$method(col, u)
            }
        }
    };
}
#[cfg(not(feature = "inspect-metadata"))]
color_arch!(NumRec, "num", num, |s, o| (s.v == o.v) & (s.d == o.d));
#[cfg(not(feature = "inspect-metadata"))]
color_arch!(StrRec, "label", label, |s, o| (s.at == o.at)
    & (s.len == o.len));
#[cfg(feature = "inspect-metadata")]
impl<C: Expr<Out = Color>> Arch for NumRec<C> {
    const OPS: &'static [Op] = &[
        Op {
            name: "value",
            code: &Code::Rec,
            ty: core::any::type_name::<i32>,
            stage: crate::expr::StageId::Mount,
            reads: 0,
            word: 0,
            words: 1,
        },
        Op {
            name: "decimals",
            code: &Code::Rec,
            ty: core::any::type_name::<u32>,
            stage: crate::expr::StageId::Mount,
            reads: 0,
            word: 1,
            words: 1,
        },
        Op {
            name: "color",
            code: C::CODE,
            ty: core::any::type_name::<C>,
            stage: C::STAGE,
            reads: C::READS,
            word: (core::mem::offset_of!(NumRec<C>, c) / 4) as u16,
            words: (core::mem::size_of::<C>() / 4) as u16,
        },
    ];
    const READS: u64 = C::READS;
    const NAME: &'static str = "num";
    const STAGE: crate::expr::StageId = C::STAGE;
    const TIMED: bool = C::READS >> U_TIME & 1 != 0;
    const ENDLESS: bool = !C::RESTS;
    const STATE: bool = C::STATE;
    #[inline(always)]
    fn eq_ops(&self, o: &Self) -> bool {
        (self.v == o.v) & (self.d == o.d) & self.c.eq_ops(&o.c)
    }
    #[inline(always)]
    fn carry(&mut self, was: &Self, old: &U, new: &U) {
        self.c.carry(&was.c, &ECx::of(old), &ECx::of(new));
    }
    #[inline]
    fn slots(f: &mut dyn FnMut(Slot)) {
        C::slots(f)
    }
    #[inline(always)]
    fn until(&self, u: &U) -> u32 {
        self.c.until(&ECx::of(u))
    }
    #[inline(always)]
    fn changed(&self, old: &U, new: &U) -> bool {
        self.c.eval(&ECx::of(old)) != self.c.eval(&ECx::of(new))
    }
    #[inline(always)]
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
        b.num(col, u)
    }
    #[inline]
    fn val(&self, op: usize, u: &U) -> Val {
        match op {
            0 => Val::Num(self.v, self.d as u8),
            1 => Val::Num(self.d as i32, 0),
            2 => Val::Color(self.c.eval(&ECx::of(u)).0),
            _ => Val::None,
        }
    }
}
#[cfg(feature = "inspect-metadata")]
impl<C: Expr<Out = Color>> Arch for StrRec<C> {
    const OPS: &'static [Op] = &[
        Op {
            name: "text",
            code: &Code::Rec,
            ty: core::any::type_name::<Txt>,
            stage: crate::expr::StageId::Mount,
            reads: 0,
            word: 0,
            words: 0,
        },
        Op {
            name: "color",
            code: C::CODE,
            ty: core::any::type_name::<C>,
            stage: C::STAGE,
            reads: C::READS,
            word: (core::mem::offset_of!(StrRec<C>, c) / 4) as u16,
            words: (core::mem::size_of::<C>() / 4) as u16,
        },
    ];
    const READS: u64 = C::READS;
    const NAME: &'static str = "label";
    const STAGE: crate::expr::StageId = C::STAGE;
    const TIMED: bool = C::READS >> U_TIME & 1 != 0;
    const ENDLESS: bool = !C::RESTS;
    const STATE: bool = C::STATE;
    #[inline(always)]
    fn eq_ops(&self, o: &Self) -> bool {
        (self.at == o.at) & (self.len == o.len) & self.c.eq_ops(&o.c)
    }
    #[inline(always)]
    fn carry(&mut self, was: &Self, old: &U, new: &U) {
        self.c.carry(&was.c, &ECx::of(old), &ECx::of(new));
    }
    #[inline]
    fn slots(f: &mut dyn FnMut(Slot)) {
        C::slots(f)
    }
    #[inline(always)]
    fn until(&self, u: &U) -> u32 {
        self.c.until(&ECx::of(u))
    }
    #[inline(always)]
    fn changed(&self, old: &U, new: &U) -> bool {
        self.c.eval(&ECx::of(old)) != self.c.eval(&ECx::of(new))
    }
    #[inline(always)]
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
        b.label(col, u)
    }
    #[inline]
    fn val(&self, op: usize, u: &U) -> Val {
        match op {
            0 => Val::Text(Txt::from_bytes(self.str())),
            1 => Val::Color(self.c.eval(&ECx::of(u)).0),
            _ => Val::None,
        }
    }
}
impl<C: Expr<Out = Color>> NumArch for NumRec<C> {
    const DECIMALS: u8 = 0;
    #[inline(always)]
    fn decimals(&self) -> u8 {
        self.d as u8
    }
    #[inline(always)]
    fn value(&self, _: &U) -> i32 {
        self.v
    }
    #[inline(always)]
    fn color(&self, u: &U) -> Color {
        self.c.eval(&ECx::of(u))
    }
}
impl<C: Expr<Out = Color>> LabelArch for StrRec<C> {
    fn text(&self) -> Txt {
        let b = self.str();
        let mut t = [0u32; 3];
        for (i, &c) in b.iter().take(12).enumerate() {
            t[i / 4] |= (c as u32) << (8 * (i % 4));
        }
        Txt(t)
    }
    #[inline(always)]
    fn str(&self) -> &[u8] {
        // SAFETY: `at` and `len` were taken from an interned string, which lives for the process.
        unsafe {
            core::slice::from_raw_parts(
                (self.at[0] as usize | (self.at[1] as usize) << 32) as *const u8,
                self.len as usize,
            )
        }
    }
    #[inline(always)]
    fn color(&self, u: &U) -> Color {
        self.c.eval(&ECx::of(u))
    }
}
impl Content for NumText {
    type A<C: Expr<Out = Color>> = NumRec<C>;
    #[inline(always)]
    fn arch<C: Expr<Out = Color>>(&self, c: C) -> NumRec<C> {
        NumRec {
            v: self.value,
            d: self.decimals,
            c,
        }
    }
    fn chars(&self) -> usize {
        12
    }
}
/// A string kept for the life of the process: equal strings are one address, so a text record is three words
/// and comparing two is comparing those. (First cut: nothing is ever released; the text lane's shaped-run
/// store is what a record will hold.)
pub fn intern(s: &str) -> &'static str {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static SET: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    let mut g = SET.lock().unwrap();
    let set = g.get_or_insert_with(HashSet::new);
    if let Some(&k) = set.get(s) {
        return k;
    }
    let k: &'static str = Box::leak(s.to_owned().into_boxed_str());
    set.insert(k);
    k
}
/// Interned text.
#[derive(Clone, Copy)]
pub struct Str(&'static str);
impl Content for Str {
    type A<C: Expr<Out = Color>> = StrRec<C>;
    #[inline(always)]
    fn arch<C: Expr<Out = Color>>(&self, c: C) -> StrRec<C> {
        let p = self.0.as_ptr() as usize;
        StrRec {
            at: [p as u32, (p >> 32) as u32],
            len: self.0.len() as u32,
            c,
        }
    }
    fn chars(&self) -> usize {
        self.0.chars().count()
    }
}
/// What `text(..)` accepts.
pub trait IntoContent {
    type K: Content;
    fn content(self) -> Self::K;
}
impl IntoContent for &'static str {
    type K = Str;
    #[inline(always)]
    fn content(self) -> Str {
        // a literal is already where it will stay
        Str(self)
    }
}
impl IntoContent for String {
    type K = Str;
    fn content(self) -> Str {
        Str(intern(&self))
    }
}
impl IntoContent for NumText {
    type K = NumText;
    #[inline(always)]
    fn content(self) -> NumText {
        self
    }
}

/// Text. One record.
#[derive(Clone, Copy)]
pub struct Text<K, C> {
    pub spec: Spec,
    k: K,
    c: C,
    /// Line height in points (0: the content's).
    line: f32,
    /// Where the `text(..)` call was written: cold, and only for the inspector.
    #[cfg(feature = "inspect-metadata")]
    loc: &'static core::panic::Location<'static>,
}
/// `text("label")`, `text(format!(..))`, `text(sig!(self.cells).at(i).num(|v| *v, FMT))`.
#[cfg_attr(feature = "inspect-metadata", track_caller)]
#[inline(always)]
pub fn text<T: IntoContent>(t: T) -> Text<T::K, Color> {
    Text {
        spec: Spec::new(false),
        k: t.content(),
        c: Color(0),
        line: 0.0,
        #[cfg(feature = "inspect-metadata")]
        loc: core::panic::Location::caller(),
    }
}
impl<K, C> Text<K, C> {
    /// Size and line height, in points. (The stand-in raster has one size; the line height is laid out.)
    #[inline(always)]
    pub fn font(mut self, _size: f32, line: f32) -> Self {
        self.line = line;
        self
    }
    #[inline(always)]
    pub fn color<C2>(self, c: C2) -> Text<K, C2> {
        Text {
            spec: self.spec,
            k: self.k,
            c,
            line: self.line,
            #[cfg(feature = "inspect-metadata")]
            loc: self.loc,
        }
    }
}
/// Text as layout words: one line. The stand-in measure: the reference raster's glyphs are 12 device pixels
/// apart and 15 tall, after 12 of inset; a line height given in points (`.font(_, line)`) is the leaf's height.
/// The text lane's measure takes this one's place.
impl<K: Content, C> Shape for Text<K, C> {
    const LEAVES: usize = 1;
    const WRAPS: bool = false;
    const VARIES: bool = false;
    const HK: Hk = Hk::Fixed;
    #[inline(always)]
    fn emit<'a, S: LSink<'a>>(&'a self, s: &mut S) {
        let mut spec = self.spec;
        if self.line > 0.0 && matches!(spec.h, Dim::Auto) {
            spec.h = Dim::Pt(self.line);
        }
        s.node(Nd::pack(
            &spec,
            K_LINE,
            true,
            0,
            24.0 + 12.0 * self.k.chars() as f32,
            15.0,
            [0; 2],
        ));
    }
}
// SAFETY (`Row`'s contract): one leaf: the content's record, written as that record.
unsafe impl<V, K: Content, C: Expr<Out = Color> + Rooted<V>> Row<V> for Text<K, C> {
    const LEAVES: u32 = 1;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, l: &mut L) {
        #[cfg(feature = "inspect-metadata")]
        l.at(self.loc);
        <K::A<C> as Row<V>>::visit(&self.k.arch(self.c), l)
    }
    #[inline(always)]
    unsafe fn write(&self, sites: *const Site, slot: u32) {
        // SAFETY: the caller's contract, for this one record.
        unsafe { <K::A<C> as Row<V>>::write(&self.k.arch(self.c), sites, slot) }
    }
    #[inline(always)]
    unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
        // SAFETY: as `write`.
        unsafe { <K::A<C> as Row<V>>::patch(&self.k.arch(self.c), sites, slot, mk, cx) }
    }
}

// ---- boxes as rows -------------------------------------------------------------------------------------------

// SAFETY (`Row`'s contract): no leaves, nothing written.
unsafe impl<V> Row<V> for () {
    const LEAVES: u32 = 0;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, _: &mut L) {}
    #[inline(always)]
    unsafe fn write(&self, _: *const Site, _: u32) {}
    #[inline(always)]
    unsafe fn patch(&self, _: *const Site, _: u32, _: &Mk, _: &mut Cx<'_>) {}
}
// SAFETY (`Row`'s contract): the first part's `C::LEAVES` sites, then the second's, in order.
unsafe impl<V, C: Row<V>, E: Row<V>> Row<V> for Chain<C, E> {
    const LEAVES: u32 = C::LEAVES + E::LEAVES;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, l: &mut L) {
        self.0.visit(l);
        self.1.visit(l);
    }
    #[inline(always)]
    unsafe fn write(&self, sites: *const Site, slot: u32) {
        // SAFETY: the first part owns the first `C::LEAVES` sites of the caller's range, the second the rest.
        unsafe {
            self.0.write(sites, slot);
            self.1.write(sites.add(C::LEAVES as usize), slot);
        }
    }
    #[inline(always)]
    unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
        // SAFETY: as `write`.
        unsafe {
            self.0.patch(sites, slot, mk, cx);
            self.1.patch(sites.add(C::LEAVES as usize), slot, mk, cx);
        }
    }
}
// SAFETY (`Row`'s contract): an unpainted box is its children.
unsafe impl<V, C: Row<V>, H> Row<V> for LBox<(), C, H> {
    const LEAVES: u32 = C::LEAVES;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, l: &mut L) {
        self.kids.visit(l)
    }
    #[inline(always)]
    unsafe fn write(&self, sites: *const Site, slot: u32) {
        // SAFETY: the caller's contract; the box has no record of its own.
        unsafe { self.kids.write(sites, slot) }
    }
    #[inline(always)]
    unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
        // SAFETY: as `write`.
        unsafe { self.kids.patch(sites, slot, mk, cx) }
    }
}
// SAFETY (`Row`'s contract): the box's own record (one leaf, written as `P::A`), then its children's `C::LEAVES`.
unsafe impl<V, P: Paints<V>, C: Row<V>, H> Row<V> for LBox<Rec1<P>, C, H> {
    const LEAVES: u32 = 1 + C::LEAVES;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, l: &mut L) {
        #[cfg(feature = "inspect-metadata")]
        l.at(self.loc);
        <P::A as Row<V>>::visit(&self.paint.0.arch(), l);
        self.kids.visit(l)
    }
    #[inline(always)]
    unsafe fn write(&self, sites: *const Site, slot: u32) {
        // SAFETY: the box's record is the first site of the caller's range, its children's the rest.
        unsafe {
            <P::A as Row<V>>::write(&self.paint.0.arch(), sites, slot);
            self.kids.write(sites.add(1), slot);
        }
    }
    #[inline(always)]
    unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
        // SAFETY: as `write`.
        unsafe {
            <P::A as Row<V>>::patch(&self.paint.0.arch(), sites, slot, mk, cx);
            self.kids.patch(sites.add(1), slot, mk, cx);
        }
    }
}

// ---- handlers ------------------------------------------------------------------------------------------------

/// A painted box with one more handler on its record.
#[derive(Clone, Copy)]
pub struct Hd<X, H>(pub X, pub H);

/// A box whose own record can carry handlers.
pub trait Boxy<V> {
    type Tgt: Target<V>;
    type Kids: Row<V>;
    fn tgt(&self) -> Self::Tgt;
    fn kids(&self) -> &Self::Kids;
}
impl<V, P: Paints<V>, C: Row<V>, H> Boxy<V> for LBox<Rec1<P>, C, H> {
    type Tgt = P::A;
    type Kids = C;
    #[inline(always)]
    fn tgt(&self) -> P::A {
        self.paint.0.arch()
    }
    #[inline(always)]
    fn kids(&self) -> &C {
        &self.kids
    }
}
impl<V, X: Boxy<V>, H: Handler<V, <X::Tgt as Target<V>>::Rec>> Boxy<V> for Hd<X, H> {
    type Tgt = On<X::Tgt, H>;
    type Kids = X::Kids;
    #[inline(always)]
    fn tgt(&self) -> On<X::Tgt, H> {
        On(self.0.tgt(), self.1)
    }
    #[inline(always)]
    fn kids(&self) -> &X::Kids {
        self.0.kids()
    }
}
/// The one position a scrolled box's content takes in its row: a hole.
#[derive(Clone, Copy)]
pub struct HoleRow;
// SAFETY (`Row`'s contract): one position, a hole: it writes and compares nothing.
unsafe impl<V> Row<V> for HoleRow {
    const LEAVES: u32 = 1;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, l: &mut L) {
        l.hole()
    }
    #[inline(always)]
    unsafe fn write(&self, _: *const Site, _: u32) {}
    #[inline(always)]
    unsafe fn patch(&self, _: *const Site, _: u32, _: &Mk, _: &mut Cx<'_>) {}
}
/// A painted scrolled box carries handlers on its own record (a list that hears keys, a port that hears the
/// wheel): its record, then the hole.
impl<V, P: Paints<V>, C> Boxy<V> for Scroll<Rec1<P>, C> {
    type Tgt = P::A;
    type Kids = HoleRow;
    #[inline(always)]
    fn tgt(&self) -> P::A {
        self.paint.0.arch()
    }
    #[inline(always)]
    fn kids(&self) -> &HoleRow {
        &HoleRow
    }
}
impl<P, C> Interactive for Scroll<Rec1<P>, C> {}
// SAFETY (`Row`'s contract): the box's record with its handlers (one leaf, `On<X::Tgt, H>`), then the children's leaves.
unsafe impl<V: 'static, X: Boxy<V>, H: Handler<V, <X::Tgt as Target<V>>::Rec>> Row<V> for Hd<X, H> {
    const LEAVES: u32 = 1 + <X::Kids as Row<V>>::LEAVES;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, l: &mut L) {
        <On<X::Tgt, H> as Row<V>>::visit(&self.tgt(), l);
        self.kids().visit(l)
    }
    #[inline(always)]
    unsafe fn write(&self, sites: *const Site, slot: u32) {
        // SAFETY: the box's record (with its handlers' captures) is the first site, its children's the rest.
        unsafe {
            <On<X::Tgt, H> as Row<V>>::write(&self.tgt(), sites, slot);
            self.kids().write(sites.add(1), slot);
        }
    }
    #[inline(always)]
    unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
        // SAFETY: as `write`.
        unsafe {
            <On<X::Tgt, H> as Row<V>>::patch(&self.tgt(), sites, slot, mk, cx);
            self.kids().patch(sites.add(1), slot, mk, cx);
        }
    }
}
impl<X: Shape, H> Shape for Hd<X, H> {
    const LEAVES: usize = X::LEAVES;
    const WRAPS: bool = X::WRAPS;
    const VARIES: bool = X::VARIES;
    const HK: Hk = X::HK;
    #[inline(always)]
    fn emit<'a, S: LSink<'a>>(&'a self, s: &mut S) {
        self.0.emit(s)
    }
}

/// The façade's interaction calls, on a painted box.
pub trait Interactive: Sized {
    /// Activation (a primary press and release on the box).
    fn on_click<F>(self, f: F) -> Hd<Self, Click<F>> {
        Hd(self, Click(f))
    }
    /// The press itself.
    fn on_press<F>(self, f: F) -> Hd<Self, Press<F>> {
        Hd(self, Press(f))
    }
    /// The pointer enters (`ev.on`) or leaves.
    fn on_hover<F>(self, f: F) -> Hd<Self, Hover<F>> {
        Hd(self, Hover(f))
    }
    /// Keys while focused; a tab stop.
    fn on_key<F>(self, f: F) -> Hd<Self, OnKey<F>> {
        Hd(self, OnKey(f))
    }
    /// Pointer travel while pressed.
    fn on_drag<F>(self, f: F) -> Hd<Self, Drag<F>> {
        Hd(self, Drag(f))
    }
    /// Text typed while focused; a tab stop.
    fn on_text<F>(self, f: F) -> Hd<Self, OnText<F>> {
        Hd(self, OnText(f))
    }
    /// The wheel over the box; return `true` to consume it.
    fn on_wheel<F>(self, f: F) -> Hd<Self, Wheel<F>> {
        Hd(self, Wheel(f))
    }
    /// Focus gained or lost.
    fn on_focus<F>(self, f: F) -> Hd<Self, OnFocus<F>> {
        Hd(self, OnFocus(f))
    }
}
impl<P, C, H> Interactive for LBox<Rec1<P>, C, H> {}
impl<X, H> Interactive for Hd<X, H> {}

// ---- scrolled boxes and lists ----------------------------------------------------------------------------------

/// `each(iter)`: one child per item. Under a scrolled box it is a list by exposure: the iterator is indexed
/// (`TrustedRandomAccess`: ranges, slices, `map`, `enumerate`, `zip`, `copied`), so a row that is not exposed
/// is never asked for.
pub struct Each<I>(pub I);
#[inline(always)]
pub fn each<I: IntoIterator>(i: I) -> Each<I::IntoIter> {
    Each(i.into_iter())
}

/// A box whose content scrolls: its own record (if painted), then a hole where the content's space is.
pub struct Scroll<P, C> {
    pub spec: Spec,
    pub paint: P,
    pub content: C,
    /// The block word of the scroll offset (an `f32` of points).
    pub word: u8,
    /// Where `.scroll(..)` was written: cold, and only for the inspector.
    #[cfg(feature = "inspect-metadata")]
    loc: &'static core::panic::Location<'static>,
}
impl<P, C> Scroll<P, C> {
    /// Assemble a scroll element from its layout, records and offset word.
    #[cfg_attr(feature = "inspect-metadata", track_caller)]
    #[inline(always)]
    pub fn from_parts(spec: Spec, paint: P, content: C, word: u8) -> Self {
        Self {
            spec,
            paint,
            content,
            word,
            #[cfg(feature = "inspect-metadata")]
            loc: core::panic::Location::caller(),
        }
    }
}
/// `.scroll(sig!(self.y))` on a box: its children scroll by the field's value (points).
pub trait Scrolls<P, C>: Sized {
    #[cfg_attr(feature = "inspect-metadata", track_caller)]
    fn scroll<V: 'static, const OFF: usize>(self, y: Sig<'_, V, Signal<f32>, OFF>) -> Scroll<P, C>;
}
impl<P, C, H> Scrolls<P, C> for LBox<P, C, H> {
    #[cfg_attr(feature = "inspect-metadata", track_caller)]
    #[inline(always)]
    fn scroll<V: 'static, const OFF: usize>(self, y: Sig<'_, V, Signal<f32>, OFF>) -> Scroll<P, C> {
        Scroll::from_parts(self.spec, self.paint, self.kids, y.word())
    }
}
/// The hole of a scrolled box, as layout: out of flow at the box's corner and as large as the box, so its rect
/// is the box's and it does not size the box (the content scrolls; it never sizes its box).
const HOLE: Spec = {
    let mut s = Spec::new(false);
    s.abs = Some((0.0, 0.0));
    s.w = Dim::Pct(1.0);
    s.h = Dim::Pct(1.0);
    s
};
impl<P: bough_lay::Paint, C> Shape for Scroll<P, C> {
    const LEAVES: usize = P::N + 1;
    const WRAPS: bool = false;
    const VARIES: bool = false;
    const HK: Hk = Hk::Fixed;
    #[inline(always)]
    fn emit<'a, S: LSink<'a>>(&'a self, s: &mut S) {
        s.node(Nd::pack(&self.spec, K_BOX, P::N == 1, 1, 0.0, 0.0, [0; 2]));
        s.node(Nd::pack(&HOLE, K_BOX, true, 0, 0.0, 0.0, [0; 2]));
    }
}
// SAFETY (`Row`'s contract): one hole, nothing written.
unsafe impl<V, C> Row<V> for Scroll<(), C> {
    const LEAVES: u32 = 1;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, l: &mut L) {
        l.hole()
    }
    #[inline(always)]
    unsafe fn write(&self, _: *const Site, _: u32) {}
    #[inline(always)]
    unsafe fn patch(&self, _: *const Site, _: u32, _: &Mk, _: &mut Cx<'_>) {}
}
// SAFETY (`Row`'s contract): the box's record (one leaf, written as `P::A`) and one hole.
unsafe impl<V, P: Paints<V>, C> Row<V> for Scroll<Rec1<P>, C> {
    const LEAVES: u32 = 2;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, l: &mut L) {
        #[cfg(feature = "inspect-metadata")]
        l.at(self.loc);
        <P::A as Row<V>>::visit(&self.paint.0.arch(), l);
        l.hole()
    }
    #[inline(always)]
    unsafe fn write(&self, sites: *const Site, slot: u32) {
        // SAFETY: the box's record is the first site of the caller's range.
        unsafe { <P::A as Row<V>>::write(&self.paint.0.arch(), sites, slot) }
    }
    #[inline(always)]
    unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
        // SAFETY: as `write`.
        unsafe { <P::A as Row<V>>::patch(&self.paint.0.arch(), sites, slot, mk, cx) }
    }
}

/// Where the content of a scrolled box is being put: its width and how far down the next part starts, in
/// device pixels of the box's space. `None` when the sink keeps the layout it was told.
pub struct Flow {
    pub w: i32,
    pub y: i32,
    pub lay: bool,
}

/// One part of a scrolled box's content: a list, or an element.
pub trait Seg<V> {
    fn seg<S: Sink<V>>(&mut self, s: &mut S, f: &mut Flow);
}
///
/// The list pass asks a row more than once in one pass (row 0 or the first exposed row, to lay the list out
/// and make its columns, then again to write it), so the iterator must be `Clone`: std's
/// `TrustedRandomAccessNoCoerce` allows `__iterator_get_unchecked` to be called twice with one index only for
/// a `Clone` iterator (condition 2; soundness review S07). A `!Clone` one (`iter_mut()`) is refused:
///
/// ```compile_fail,E0277
/// use bough_x::el::prelude::*;
/// struct V { y: Signal<f32> }
/// impl View for V {
///     fn view(&self) -> impl Element<Self> + '_ {
///         let rows: &'static mut [u32] = Box::leak(Box::new([0u32; 4]));
///         column(each(rows.iter_mut().map(|_| text("row")))).h(100.0).scroll(sig!(self.y))
///     }
/// }
/// ```
impl<V, I> Seg<V> for Each<I>
where
    I: Iterator + TrustedRandomAccessNoCoerce + Clone,
    I::Item: Row<V> + Shape,
{
    #[inline(always)]
    fn seg<S: Sink<V>>(&mut self, s: &mut S, f: &mut Flow) {
        let n = self.0.size();
        let it = &mut self.0;
        let geo = if f.lay {
            Some(Geo {
                x: 0,
                y: f.y,
                w: f.w,
            })
        } else {
            None
        };
        // SAFETY: `r < size()` (asserted: the list pass asks only for rows under `n`); `I: Clone`, so asking
        // one index more than once is allowed (the pass does: a row is asked to lay the list out and again to
        // be written); nothing but `size` and `__iterator_get_unchecked` is called on the iterator. The
        // contract of `TrustedRandomAccessNoCoerce`, conditions 1, 2 and 4.
        f.y += s.rows(n as u32, geo, |r| {
            assert!(r < n);
            unsafe { it.__iterator_get_unchecked(r) }
        });
    }
}
/// An element in a scrolled box's content: laid out as a row of the content's width, where the flow is.
#[inline(always)]
pub fn seg_el<V, E: Part<V> + Shape, S: Sink<V>>(e: &mut E, s: &mut S, f: &mut Flow) {
    if !f.lay {
        return e.holes(None, &mut 0, s);
    }
    let env = s.env();
    let mut out = vec![[0i32; 4]; <E as Shape>::LEAVES];
    let (h, abs) = {
        let flat = Flat::of(&*e);
        (
            bough_lay::shape::place_flat(&flat, env.scale, env.u, f.w as f32, &mut out[..]).0,
            flat.root().spec().abs.is_some(),
        )
    };
    let rects: Vec<Rect> = out
        .iter()
        .map(|r| Rect {
            x0: r[0],
            y0: r[1] + f.y,
            x1: r[2],
            y1: r[3] + f.y,
        })
        .collect();
    s.row(|| (&rects[..], ByRef(&*e)));
    e.holes(Some(&rects), &mut 0, s);
    if !abs {
        f.y += h;
    }
}
macro_rules! seg_node {
    ([$($g:tt)*] $t:ty) => {
        impl<V, $($g)*> Seg<V> for $t
        where
            $t: Part<V> + Shape,
        {
            #[inline(always)]
            fn seg<S: Sink<V>>(&mut self, s: &mut S, f: &mut Flow) {
                seg_el(self, s, f)
            }
        }
    };
}
/// A box in a scrolled box's content: an element like any other, or (a box that only wraps `each(..)`, the
/// façade's `column(each(rows))`) the list itself.
pub trait SegKids<V, P, H>: Sized {
    fn seg_box<S: Sink<V>>(b: &mut LBox<P, Self, H>, s: &mut S, f: &mut Flow);
}
/// Children that are not (and do not hold) a bare `each(..)`: what tells a box of elements from a box that
/// is a list. An auto trait with one negative impl, so the two impls below are disjoint to the compiler.
pub auto trait NotEach {}
impl<I> !NotEach for Each<I> {}
// a scrolled box is an element whatever its content holds
impl<P, C> NotEach for Scroll<P, C> {}
impl<V, P, H, C: NotEach> SegKids<V, P, H> for C
where
    LBox<P, C, H>: Part<V> + Shape,
{
    #[inline(always)]
    fn seg_box<S: Sink<V>>(b: &mut LBox<P, C, H>, s: &mut S, f: &mut Flow) {
        seg_el(b, s, f)
    }
}
impl<V, H, I> SegKids<V, (), H> for Each<I>
where
    Each<I>: Seg<V>,
{
    #[inline(always)]
    fn seg_box<S: Sink<V>>(b: &mut LBox<(), Each<I>, H>, s: &mut S, f: &mut Flow) {
        b.kids.seg(s, f)
    }
}
impl<V, P, C: SegKids<V, P, H>, H> Seg<V> for LBox<P, C, H> {
    #[inline(always)]
    fn seg<S: Sink<V>>(&mut self, s: &mut S, f: &mut Flow) {
        C::seg_box(self, s, f)
    }
}
seg_node!([X, H] Hd<X, H>);
seg_node!([K, C] Text<K, C>);
seg_node!([P, C] Scroll<P, C>);
macro_rules! seg_tuple {
    ($($t:ident)+) => {
        impl<V, $($t: Seg<V>),+> Seg<V> for ($($t,)+) {
            #[inline(always)]
            #[allow(non_snake_case)]
            fn seg<S: Sink<V>>(&mut self, s: &mut S, f: &mut Flow) {
                let ($($t,)+) = self;
                $($t.seg(s, f);)+
            }
        }
    };
}
seg_tuple!(A);
seg_tuple!(A B);
seg_tuple!(A B C);
seg_tuple!(A B C D);
impl<V, C: Seg<V>, E: Seg<V>> Seg<V> for Chain<C, E> {
    #[inline(always)]
    fn seg<S: Sink<V>>(&mut self, s: &mut S, f: &mut Flow) {
        self.0.seg(s, f);
        self.1.seg(s, f);
    }
}
impl<V> Seg<V> for () {
    #[inline(always)]
    fn seg<S: Sink<V>>(&mut self, _: &mut S, _: &mut Flow) {}
}

// ---- driving an element ----------------------------------------------------------------------------------------

/// A row by reference: what an element hands a sink while keeping itself for its lists.
pub struct ByRef<'a, E>(pub &'a E);
// SAFETY (`Row`'s contract): exactly `E`'s leaves and writes.
unsafe impl<V, E: Row<V>> Row<V> for ByRef<'_, E> {
    const LEAVES: u32 = E::LEAVES;
    #[inline(always)]
    fn visit<L: Leaf<V>>(&self, l: &mut L) {
        self.0.visit(l)
    }
    #[inline(always)]
    unsafe fn write(&self, sites: *const Site, slot: u32) {
        // SAFETY: the caller's contract, for the row behind the reference.
        unsafe { self.0.write(sites, slot) }
    }
    #[inline(always)]
    unsafe fn patch(&self, sites: *const Site, slot: u32, mk: &Mk, cx: &mut Cx<'_>) {
        // SAFETY: as `write`.
        unsafe { self.0.patch(sites, slot, mk, cx) }
    }
}

/// A part of an element: its records are a row; it may hold scrolled boxes (holes), which are driven after
/// the row, each with the rect the layout gave it.
pub trait Part<V>: Row<V> {
    const HOLES: bool;
    /// Drive the scrolled boxes under this part. `rects` is the row's layout when the sink asked for one;
    /// `k` is this part's first position in it.
    fn holes<S: Sink<V>>(&mut self, rects: Option<&[Rect]>, k: &mut usize, s: &mut S);
}
macro_rules! no_holes {
    () => {
        const HOLES: bool = false;
        #[inline(always)]
        fn holes<S: Sink<V>>(&mut self, _: Option<&[Rect]>, k: &mut usize, _: &mut S) {
            *k += <Self as Row<V>>::LEAVES as usize;
        }
    };
}
impl<V> Part<V> for () {
    no_holes!();
}
impl<V, K: Content, C: Expr<Out = Color> + Rooted<V>> Part<V> for Text<K, C> {
    no_holes!();
}
impl<V, C: Part<V>, H> Part<V> for LBox<(), C, H> {
    const HOLES: bool = C::HOLES;
    #[inline(always)]
    fn holes<S: Sink<V>>(&mut self, rects: Option<&[Rect]>, k: &mut usize, s: &mut S) {
        self.kids.holes(rects, k, s)
    }
}
impl<V, P: Paints<V>, C: Part<V>, H> Part<V> for LBox<Rec1<P>, C, H> {
    const HOLES: bool = C::HOLES;
    #[inline(always)]
    fn holes<S: Sink<V>>(&mut self, rects: Option<&[Rect]>, k: &mut usize, s: &mut S) {
        *k += 1;
        self.kids.holes(rects, k, s)
    }
}
impl<V: 'static, X: Boxy<V> + Part<V>, H: Handler<V, <X::Tgt as Target<V>>::Rec>> Part<V>
    for Hd<X, H>
{
    const HOLES: bool = X::HOLES;
    #[inline(always)]
    fn holes<S: Sink<V>>(&mut self, rects: Option<&[Rect]>, k: &mut usize, s: &mut S) {
        self.0.holes(rects, k, s)
    }
}
impl<V, C: Part<V>, E: Part<V>> Part<V> for Chain<C, E> {
    const HOLES: bool = C::HOLES | E::HOLES;
    #[inline(always)]
    fn holes<S: Sink<V>>(&mut self, rects: Option<&[Rect]>, k: &mut usize, s: &mut S) {
        self.0.holes(rects, k, s);
        self.1.holes(rects, k, s);
    }
}
impl<V, X: Part<V>, const N: usize> Part<V> for [X; N] {
    const HOLES: bool = X::HOLES;
    #[inline(always)]
    fn holes<S: Sink<V>>(&mut self, rects: Option<&[Rect]>, k: &mut usize, s: &mut S) {
        if !X::HOLES {
            *k += <Self as Row<V>>::LEAVES as usize;
            return;
        }
        for x in self {
            x.holes(rects, k, s);
        }
    }
}
macro_rules! part_tuple {
    ($($t:ident)+) => {
        impl<V, $($t: Part<V>),+> Part<V> for ($($t,)+) {
            const HOLES: bool = false $(| $t::HOLES)+;
            #[inline(always)]
            #[allow(non_snake_case)]
            fn holes<S: Sink<V>>(&mut self, rects: Option<&[Rect]>, k: &mut usize, s: &mut S) {
                let ($($t,)+) = self;
                $($t.holes(rects, k, s);)+
            }
        }
    };
}
part_tuple!(A);
part_tuple!(A B);
part_tuple!(A B C);
part_tuple!(A B C D);
part_tuple!(A B C D E);
part_tuple!(A B C D E F);
part_tuple!(A B C D E F G);
part_tuple!(A B C D E F G H);
part_tuple!(A B C D E F G H I);
part_tuple!(A B C D E F G H I J);
part_tuple!(A B C D E F G H I J K);
part_tuple!(A B C D E F G H I J K L);

/// The scrolled box itself: the hole is where its content's space is.
macro_rules! scroll_part {
    ($p:ty, $own:expr, $($bound:tt)*) => {
        impl<V, C: Seg<V>, $($bound)*> Part<V> for Scroll<$p, C> {
            const HOLES: bool = true;
            #[inline(always)]
            fn holes<S: Sink<V>>(&mut self, rects: Option<&[Rect]>, k: &mut usize, s: &mut S) {
                let at = *k + $own;
                *k = at + 1;
                let (space, w) = match rects {
                    Some(r) => (Space { clip: r[at], base: (r[at].x0, r[at].y0), scroll: self.word | POINTS }, r[at].x1 - r[at].x0),
                    // a sink that keeps what it was told ignores the space
                    None => (Space::root(0, 0), 0),
                };
                let content = &mut self.content;
                s.scope(space, |s| content.seg(s, &mut Flow { w, y: 0, lay: rects.is_some() }));
            }
        }
    };
}
scroll_part!((), 0,);
scroll_part!(Rec1<P>, 1, P: Paints<V>);

/// What `view` returns.
pub trait Element<V> {
    fn drive<S: Sink<V>>(self, s: &mut S);
}
impl<V, E: Part<V> + Shape> Element<V> for E {
    #[inline]
    fn drive<S: Sink<V>>(mut self, s: &mut S) {
        const {
            assert!(
                <E as Row<V>>::LEAVES as usize == <E as Shape>::LEAVES,
                "an element's layout has one rect per record"
            )
        };
        if !S::FIXED {
            // a turn: the fixed records are expressions over the block and are not described; only lists are
            return self.holes(None, &mut 0, s);
        }
        let env = s.env();
        let mut out = vec![[0i32; 4]; <E as Shape>::LEAVES];
        // the element's layout words, then the one layout function (bough-lay's `shape`): no layout code is
        // generated per element type
        bough_lay::place_root(
            &Flat::of(&self).root(),
            env.scale,
            env.u,
            env.size,
            &mut out[..],
        );
        let rects: Vec<Rect> = out
            .iter()
            .map(|r| Rect {
                x0: r[0],
                y0: r[1],
                x1: r[2],
                y1: r[3],
            })
            .collect();
        s.row(|| (&rects[..], ByRef(&self)));
        self.holes(Some(&rects), &mut 0, s);
    }
}

/// A view, as the façade has it.
pub trait View: Sized + 'static {
    fn view(&self) -> impl Element<Self> + '_;
}
static NO_BLOCK: U = U { w: [0; 64] };
impl<V: View> crate::sink::View for V {
    fn block(&self) -> &U {
        &NO_BLOCK
    }
    /// The block is the view's own fields: each word the columns' lenses named is read from its field.
    #[inline]
    fn gathered<'a>(&'a self, slots: &[Slot], out: &'a mut U) -> &'a U {
        for slot in slots {
            out.w[slot.index()] = slot.read(self);
        }
        out
    }
    #[inline]
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        View::view(self).drive(s)
    }
}

// ---- the application --------------------------------------------------------------------------------------------

/// A view and its world: what the façade's `App` is to a scene. Handlers run in `turn` (or are dispatched by
/// the pointer and key calls); what they changed is found afterwards, by comparing.
pub struct App<V: View, B: Backend> {
    pub v: V,
    pub world: crate::world::World<V, B>,
    /// Mount-once: the view is described once and state reaches it through `sig!` operands and `Signal`s. If
    /// false, the exposed view is described again after every turn (the façade's default).
    pub once: bool,
    sets: u64,
}
impl<V: View, B: Backend> App<V, B> {
    /// A window of `w` x `h` device pixels at `scale` device pixels per point.
    pub fn new(v: V, w: i32, h: i32, scale: f32) -> App<V, B> {
        let mut world = crate::world::World::<V, B>::new(w, h, APP_WORDS);
        world.set_scale(scale);
        // Transitions keep the time they started at, and 0 means at rest: time starts after 0.
        world.tick(1);
        world.mount(&v);
        {
            let sets = world.input_revision();
            App {
                v,
                world,
                once: false,
                sets,
            }
        }
    }
    pub fn once(mut self) -> Self {
        self.once = true;
        self
    }
    /// Run `f` on the state, then find what it changed.
    #[inline]
    pub fn turn(&mut self, f: impl FnOnce(&mut V)) {
        f(&mut self.v);
        self.settle();
    }
    /// After handlers: the block's compare, and a re-description of the exposed view when the compare cannot
    /// account for every change.
    #[inline]
    pub fn settle(&mut self) {
        let now = self.world.input_revision();
        let sets = now.wrapping_sub(self.sets);
        self.sets = now;
        if self.once && sets == self.world.changed_words(&self.v).count_ones() as u64 {
            self.world.settle(&self.v);
        } else {
            self.world.redescribe(&self.v);
        }
    }
    /// A frame at time `now` (milliseconds, after 0).
    #[inline]
    pub fn frame(&mut self, now: u32) {
        self.world.tick(now.max(1));
    }
    pub fn pointer_move(&mut self, x: f32, y: f32) {
        self.world.pointer(&mut self.v, x as i32, y as i32);
        self.settle();
    }
    pub fn key(&mut self, key: crate::event::Key) {
        self.world
            .key_down(&mut self.v, key, crate::event::Mods::default());
        self.settle();
    }
}

/// What a scene imports.
pub mod prelude {
    pub use super::{
        Align, App, Curve, Deco, Element, Flex, Interactive, Justify, NumFmt, Scrolls, Signal,
        Transition, Translate, View, block, column, div, each, rgb, row, text,
    };
    pub use crate::event::{Ev, Key};
    pub use crate::expr::Color;
    pub use crate::sig;
}

#[cfg(test)]
mod input_slot_tests {
    use super::*;

    #[test]
    fn stale_generation_cannot_increment_or_mark_a_reused_slot() {
        let slot = WorldInputSlot::new();
        let old = slot.claim(0).unwrap();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                for _ in 0..10_000 {
                    slot.update(old, false);
                    slot.update(old, true);
                }
            });
            slot.release(old);
            let next = slot.claim(0).unwrap();
            assert_ne!(old, next);
        });
        let state = slot.state.load(Ordering::Relaxed);
        assert_eq!(state & (INPUT_REV_MASK | INPUT_SHARED), 0);
    }

    #[test]
    fn terminal_generation_retires_without_issuing_sentinel() {
        let slot = WorldInputSlot::new();
        slot.state.store(
            ((INPUT_GEN_MAX - 1) as u64) << INPUT_GEN_SHIFT,
            Ordering::Relaxed,
        );
        let last = slot.claim(4095).unwrap();
        assert_ne!(last, MULTI_WORLD);
        slot.release(last);
        let retired = slot.state.load(Ordering::Relaxed);
        slot.update(last, false);
        slot.update(last, true);
        assert_eq!(slot.state.load(Ordering::Relaxed), retired);
        assert!(slot.claim(4095).is_none());
    }

    #[test]
    fn input_scope_restores_nested_and_unwound_tokens() {
        let a = InputToken::new();
        let b = InputToken::new();
        assert_eq!(INPUT_TOKEN.with(core::cell::Cell::get), 0);
        track_inputs(&a, || {
            assert_eq!(INPUT_TOKEN.with(core::cell::Cell::get), a.0);
            let _ = std::panic::catch_unwind(|| {
                track_inputs(&b, || {
                    assert_eq!(INPUT_TOKEN.with(core::cell::Cell::get), b.0);
                    panic!("probe");
                })
            });
            assert_eq!(INPUT_TOKEN.with(core::cell::Cell::get), a.0);
        });
        assert_eq!(INPUT_TOKEN.with(core::cell::Cell::get), 0);
    }
}
