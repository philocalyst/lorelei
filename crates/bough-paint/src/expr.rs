//! Expressions that reach the GPU. The vocabulary (`U`, lenses, `Words`, `Code`, the stage types, `Expr` and
//! the leaves `Uw`, `Un`, `K`, `Ki`, `Rec`) is `bough-expr`, shared with the expr lane (`docs/r8/SEAM.md`).
//! What lives here is what is named after a leaf function of `ops.rs`: the combinators, and `Lane`.
//!
//! An expression value is its own per-record constants: reads of the block and constants are zero-sized,
//! `Rec` holds one word, a combinator is a `repr(C)` struct of its operands. So a record of an archetype is a
//! `[u32; N]` whose word `k` is the `k`-th `Rec` met walking the type depth first, which is how the generated
//! GPU function names it.
//!
//! A combinator has no body of its own. `op!` takes a leaf of `ops.rs` and emits both faces from that one
//! identifier: `eval` calls the leaf on the operands' values; `CODE` is a static tree naming the leaf and the
//! operands' trees. The CPU kernel is `eval` monomorphised and inlined; the GPU function is `CODE` rendered
//! (`emit.rs`). Neither can say something the other does not.
use crate::ops::ops as x;
pub use bough_expr::{Code, Cx, Expr, Frame, JoinAll, K, Ki, Lens, Mount, Rec, Stage, StageId, U, U_PICK, U_PTR_X, U_PTR_Y, U_TIME, Un, Uw, Words, lens, un};
use wgsl_rs::std::{Vec4, Vec4i, Vec4u};

/// The type of an expression's value, for generated code.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ty {
    U32,
    I32,
    V4u,
    V4i,
}
pub trait Val: Copy + PartialEq {
    const TY: Ty;
}
impl Val for u32 {
    const TY: Ty = Ty::U32;
}
impl Val for i32 {
    const TY: Ty = Ty::I32;
}
impl Val for Vec4u {
    const TY: Ty = Ty::V4u;
}
impl Val for Vec4i {
    const TY: Ty = Ty::V4i;
}

/// The record's rect lanes (Q8) as a value.
#[derive(Clone, Copy)]
pub struct Lane;
// SAFETY: zero-sized.
unsafe impl Words for Lane {}
impl Expr for Lane {
    type Out = Vec4i;
    type Stage = Mount;
    const READS: u64 = 0;
    const CODE: &'static Code = &Code::Lane;
    #[inline(always)]
    fn eval(&self, cx: &Cx) -> Vec4i {
        Vec4 { x: cx.rect[0], y: cx.rect[1], z: cx.rect[2], w: cx.rect[3] }
    }
}

/// A combinator from a leaf: `op!(Name = leaf(field: Type: OperandOut, ..) -> Out; opaque = <const expr>)`.
macro_rules! op {
    ($(#[$m:meta])* $name:ident = $leaf:ident ( $($f:ident : $T:ident : $ty:ty),* ) -> $out:ty ; opaque = $opq:expr) => {
        $(#[$m])*
        #[repr(C)]
        #[derive(Clone, Copy)]
        pub struct $name<$($T),*> { $(pub $f: $T),* }
        // SAFETY: `repr(C)` over operands that are `Words` (whole words, alignment <= 4): no padding arises.
        unsafe impl<$($T: Words),*> Words for $name<$($T),*> {}
        impl<$($T: Expr<Out = $ty>),*> Expr for $name<$($T),*>
        where
            ($($T::Stage,)*): JoinAll,
        {
            type Out = $out;
            type Stage = <($($T::Stage,)*) as JoinAll>::Out;
            const READS: u64 = 0 $(| $T::READS)*;
            const OPAQUE: bool = $opq;
            const CODE: &'static Code = &Code::Call(stringify!($leaf), &[$($T::CODE),*]);
            #[inline(always)]
            fn eval(&self, cx: &Cx) -> $out {
                x::$leaf($(self.$f.eval(cx)),*)
            }
            /// At rest when every operand is: a combinator adds no time of its own.
            #[inline(always)]
            fn until(&self, cx: &Cx) -> u32 {
                let (now, t) = (cx.u.w[U_TIME], 0u32);
                $(let t = bough_expr::latest(t, self.$f.until(cx), now);)*
                let _ = now;
                t
            }
        }
    };
}

op!(/// 1 when two words are equal.
    Eq = x_eq(a: A: u32, b: B: u32) -> u32; opaque = false);
op!(Sel = x_sel(c: C: u32, a: A: u32, b: B: u32) -> u32; opaque = A::OPAQUE && B::OPAQUE);
op!(SelI = x_seli(c: C: u32, a: A: i32, b: B: i32) -> i32; opaque = false);
op!(Inv = x_inv(t: T: u32) -> u32; opaque = false);
op!(SelV = x_selv(c: C: u32, a: A: Vec4u, b: B: Vec4u) -> Vec4u; opaque = A::OPAQUE && B::OPAQUE);
op!(/// A packed RGBA8 word as a premultiplied Q15 colour.
    Pm = x_pm(c: C: u32) -> Vec4u; opaque = C::OPAQUE);
op!(/// The same with alpha forced full: opaque whatever the word holds.
    Pmo = x_pmo(c: C: u32) -> Vec4u; opaque = true);
op!(MixV = x_mixv(a: A: Vec4u, b: B: Vec4u, t: T: u32) -> Vec4u; opaque = A::OPAQUE && B::OPAQUE);
op!(Fade = x_fade(a: A: Vec4u, k: T: u32) -> Vec4u; opaque = false);
op!(/// A looping there-and-back progress of (now + phase) over `period` ticks, Q15.
    Tri = x_tri(now: N: u32, phase: P: u32, period: D: u32) -> u32; opaque = false);
op!(Smooth = x_smooth(t: T: u32) -> u32; opaque = false);

/// When a one-shot that starts at `t0` and lasts `dur` ticks stops changing, seen from `now`: 0 once it is
/// over, its end while it runs or has yet to start (a staggered start is in the future).
#[inline(always)]
pub fn anim_end(t0: u32, dur: u32, now: u32) -> u32 {
    let e = now.wrapping_sub(t0);
    if (e as i32) >= 0 && e >= dur.clamp(1, 65535) {
        return 0;
    }
    match t0.wrapping_add(dur.clamp(1, 65535)) {
        0 | u32::MAX => 1,
        t => t,
    }
}
/// One-shot progress (Q15) of a transition started at `t0` lasting `dur` ticks. It says when it ends
/// (`until`), so a record that only reads the clock through it leaves the scene's active set there.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Prog<N, A, D> {
    pub now: N,
    pub t0: A,
    pub dur: D,
}
// SAFETY: `repr(C)` over `Words` operands.
unsafe impl<N: Words, A: Words, D: Words> Words for Prog<N, A, D> {}
impl<N: Expr<Out = u32>, A: Expr<Out = u32>, D: Expr<Out = u32>> Expr for Prog<N, A, D>
where
    (N::Stage, A::Stage, D::Stage): JoinAll,
{
    type Out = u32;
    type Stage = <(N::Stage, A::Stage, D::Stage) as JoinAll>::Out;
    const READS: u64 = N::READS | A::READS | D::READS;
    const CODE: &'static Code = &Code::Call("x_prog", &[N::CODE, A::CODE, D::CODE]);
    #[inline(always)]
    fn eval(&self, cx: &Cx) -> u32 {
        x::x_prog(self.now.eval(cx), self.t0.eval(cx), self.dur.eval(cx))
    }
    #[inline(always)]
    fn until(&self, cx: &Cx) -> u32 {
        let now = cx.u.w[U_TIME];
        let own = anim_end(self.t0.eval(cx), self.dur.eval(cx), self.now.eval(cx));
        bough_expr::latest(bough_expr::latest(own, self.t0.until(cx), now), self.dur.until(cx), now)
    }
}

/// A spring's value (Q8) as an expression of the clock: `ops::x_spring`. Its operands are what
/// `motion::Spring::retarget` computes; it rests from `t0 + dur` on, exactly at `to`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Spr<N, A, D, T, P, Q, E, W> {
    pub now: N,
    pub t0: A,
    pub dur: D,
    pub to: T,
    pub a0: P,
    pub b0: Q,
    pub decay: E,
    pub omega: W,
}
// SAFETY: `repr(C)` over `Words` operands.
unsafe impl<N: Words, A: Words, D: Words, T: Words, P: Words, Q: Words, E: Words, W: Words> Words for Spr<N, A, D, T, P, Q, E, W> {}
impl<N: Expr<Out = u32>, A: Expr<Out = u32>, D: Expr<Out = u32>, T: Expr<Out = i32>, P: Expr<Out = i32>, Q: Expr<Out = i32>, E: Expr<Out = u32>, W: Expr<Out = u32>> Expr for Spr<N, A, D, T, P, Q, E, W>
where
    (N::Stage, A::Stage, D::Stage, T::Stage, P::Stage, Q::Stage, E::Stage, W::Stage): JoinAll,
{
    type Out = i32;
    type Stage = <(N::Stage, A::Stage, D::Stage, T::Stage, P::Stage, Q::Stage, E::Stage, W::Stage) as JoinAll>::Out;
    const READS: u64 = N::READS | A::READS | D::READS | T::READS | P::READS | Q::READS | E::READS | W::READS;
    const CODE: &'static Code = &Code::Call("x_spring", &[N::CODE, A::CODE, D::CODE, T::CODE, P::CODE, Q::CODE, E::CODE, W::CODE]);
    #[inline(always)]
    fn eval(&self, cx: &Cx) -> i32 {
        x::x_spring(self.now.eval(cx), self.t0.eval(cx), self.dur.eval(cx), self.to.eval(cx), self.a0.eval(cx), self.b0.eval(cx), self.decay.eval(cx), self.omega.eval(cx))
    }
    #[inline(always)]
    fn until(&self, cx: &Cx) -> u32 {
        let now = cx.u.w[U_TIME];
        // (a spring of no duration is at rest wherever the clock is)
        let dur = self.dur.eval(cx);
        let mut t = if dur == 0 { 0 } else { anim_end(self.t0.eval(cx), dur, self.now.eval(cx)) };
        for o in [self.t0.until(cx), self.dur.until(cx), self.to.until(cx), self.a0.until(cx), self.b0.until(cx), self.decay.until(cx), self.omega.until(cx)] {
            t = bough_expr::latest(t, o, now);
        }
        t
    }
}
op!(EIn = x_ein(t: T: u32) -> u32; opaque = false);
op!(EOut = x_eout(t: T: u32) -> u32; opaque = false);
op!(EInOut = x_einout(t: T: u32) -> u32; opaque = false);
op!(/// A rect from four words (Q8 x0, y0, x1, y1).
    RectW = x_rect(a: A: u32, b: B: u32, c: C: u32, d: D: u32) -> Vec4i; opaque = false);
op!(Off = x_off(b: B: Vec4i, dx: X: i32, dy: Y: i32) -> Vec4i; opaque = false);
op!(Grow = x_grow(b: B: Vec4i, d: D: i32) -> Vec4i; opaque = false);
op!(MixR = x_mixr(a: A: Vec4i, b: B: Vec4i, t: T: u32) -> Vec4i; opaque = false);
op!(MixI = x_mixi(a: A: i32, b: B: i32, t: T: u32) -> i32; opaque = false);
op!(Top = x_top(b: B: Vec4i, h: H: i32) -> Vec4i; opaque = false);
op!(And = x_and(a: A: Vec4i, b: B: Vec4i) -> Vec4i; opaque = false);
op!(/// Three words as a vector (a backdrop's filter factors).
    V3 = x_vecf(a: A: u32, b: B: u32, c: C: u32) -> Vec4u; opaque = false);
op!(/// A signed value, clamped at zero, as a word (an interpolated opacity).
    I2U = x_u(a: A: i32) -> u32; opaque = false);
op!(/// A word read as a signed value.
    I = x_i(a: A: u32) -> i32; opaque = false);

/// Whether a colour's alpha is full, as a property of the record: a function of record words and constants
/// only, never of the block, so it cannot change between frames (only a record write changes it, and that
/// reruns the window pass). This is what lets a colour the author gave as a plain run-time value
/// (`.bg(theme.panel)`: one record word) cover what is beneath it: no type has to assert it.
///
/// `MAY`: the type admits a full alpha at all (false: the cover is not even computed). The generated GPU
/// function derives the same predicate from the code tree (`emit::full`); `emit::archetype` asserts the two
/// agree on `MAY`.
pub trait Alpha: Expr<Out = Vec4u> {
    const MAY: bool;
    fn full(&self, cx: &Cx) -> bool;
}
impl<C: Expr<Out = u32>> Alpha for Pm<C> {
    const MAY: bool = C::READS == 0;
    #[inline(always)]
    fn full(&self, cx: &Cx) -> bool {
        C::READS == 0 && x::x_full(self.c.eval(cx))
    }
}
impl<C: Expr<Out = u32>> Alpha for Pmo<C> {
    const MAY: bool = true;
    #[inline(always)]
    fn full(&self, _: &Cx) -> bool {
        true
    }
}
impl<A: Alpha, B: Alpha, T: Expr<Out = u32>> Alpha for MixV<A, B, T>
where
    (A::Stage, B::Stage, T::Stage): JoinAll,
{
    const MAY: bool = A::MAY && B::MAY;
    #[inline(always)]
    fn full(&self, cx: &Cx) -> bool {
        self.a.full(cx) && self.b.full(cx)
    }
}
impl<C: Expr<Out = u32>, A: Alpha, B: Alpha> Alpha for SelV<C, A, B>
where
    (C::Stage, A::Stage, B::Stage): JoinAll,
{
    const MAY: bool = A::MAY && B::MAY;
    #[inline(always)]
    fn full(&self, cx: &Cx) -> bool {
        self.a.full(cx) && self.b.full(cx)
    }
}
impl<A: Expr<Out = Vec4u>, T: Expr<Out = u32>> Alpha for Fade<A, T>
where
    (A::Stage, T::Stage): JoinAll,
{
    const MAY: bool = false;
    #[inline(always)]
    fn full(&self, _: &Cx) -> bool {
        false
    }
}

/// A record's four words as a rect.
pub type RecRect = RectW<Rec, Rec, Rec, Rec>;
pub fn rec_rect(r: [i32; 4]) -> RecRect {
    RectW { a: Rec(r[0] as u32), b: Rec(r[1] as u32), c: Rec(r[2] as u32), d: Rec(r[3] as u32) }
}
