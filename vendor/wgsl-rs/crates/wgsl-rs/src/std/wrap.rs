//! WGSL's integer arithmetic for the CPU expansion of `#[wgsl]` modules.
//!
//! WGSL integer `+`, `-`, `*` and unary `-` wrap (two's complement), and a
//! shift's amount is taken modulo the bit width. Rust's operators on `i32` /
//! `u32` panic on overflow when overflow checks are on (debug builds), so the
//! same source would mean one thing on the GPU and another in a debug test.
//!
//! The `#[wgsl]` macro therefore rewrites, in the Rust (CPU) tokens of the
//! module only, every binary `+ - * << >>`, every compound `+= -= *= <<= >>=`
//! and every unary `-` whose operand is not a literal into calls of the
//! functions here (`a + b` -> `wrap::add(a, b)`). The WGSL it emits is built
//! from the source before the rewrite and is unchanged.
//!
//! * `i32`, `u32`: `wrapping_add` / `wrapping_sub` / `wrapping_mul` /
//!   `wrapping_neg` / `wrapping_shl` / `wrapping_shr` (the shift amount masked
//!   to its low five bits, which is WGSL's modulo). These compile to the same
//!   instructions as the unchecked operators of a release build.
//! * `f32`: the plain operator.
//! * vectors and matrices (and the scalar-vector / scalar-matrix forms): the
//!   type's own operator, which for integer vectors already wraps.
//!
//! Operations whose operands are both literals are left as Rust operators
//! (they are constants; an overflow there is a compile error), as are
//! `const` / `static` initialisers, array lengths, types and patterns, which
//! must stay constant expressions. Division and remainder are not rewritten:
//! WGSL defines `x / 0` and `i32::MIN / -1` (they yield `x` and `i32::MIN`)
//! where Rust panics in every profile; those cases stay Rust's.

use super::{
    Mat2x2f, Mat2x3f, Mat2x4f, Mat3x2f, Mat3x3f, Mat3x4f, Mat4x2f, Mat4x3f, Mat4x4f, Vec2, Vec3,
    Vec4,
};

/// `+` with WGSL semantics.
pub trait WrapAdd<Rhs = Self> {
    type Output;
    fn wrap_add(self, rhs: Rhs) -> Self::Output;
}
/// `-` with WGSL semantics.
pub trait WrapSub<Rhs = Self> {
    type Output;
    fn wrap_sub(self, rhs: Rhs) -> Self::Output;
}
/// `*` with WGSL semantics.
pub trait WrapMul<Rhs = Self> {
    type Output;
    fn wrap_mul(self, rhs: Rhs) -> Self::Output;
}
/// Unary `-` with WGSL semantics.
pub trait WrapNeg {
    type Output;
    fn wrap_neg(self) -> Self::Output;
}
/// `<<` with WGSL semantics (the amount modulo the bit width). The result has
/// the left operand's type, as for Rust's integer shifts.
pub trait WrapShl<Rhs> {
    fn wrap_shl(self, rhs: Rhs) -> Self;
}
/// `>>` with WGSL semantics (arithmetic for `i32`, logical for `u32`; the
/// amount modulo the bit width).
pub trait WrapShr<Rhs> {
    fn wrap_shr(self, rhs: Rhs) -> Self;
}

#[inline(always)]
pub fn add<A: WrapAdd<B>, B>(a: A, b: B) -> A::Output {
    a.wrap_add(b)
}
#[inline(always)]
pub fn sub<A: WrapSub<B>, B>(a: A, b: B) -> A::Output {
    a.wrap_sub(b)
}
#[inline(always)]
pub fn mul<A: WrapMul<B>, B>(a: A, b: B) -> A::Output {
    a.wrap_mul(b)
}
#[inline(always)]
pub fn neg<A: WrapNeg>(a: A) -> A::Output {
    a.wrap_neg()
}
#[inline(always)]
pub fn shl<A: WrapShl<B>, B>(a: A, b: B) -> A {
    a.wrap_shl(b)
}
#[inline(always)]
pub fn shr<A: WrapShr<B>, B>(a: A, b: B) -> A {
    a.wrap_shr(b)
}
#[inline(always)]
pub fn add_assign<A: WrapAdd<B, Output = A> + Copy, B>(a: &mut A, b: B) {
    *a = a.wrap_add(b);
}
#[inline(always)]
pub fn sub_assign<A: WrapSub<B, Output = A> + Copy, B>(a: &mut A, b: B) {
    *a = a.wrap_sub(b);
}
#[inline(always)]
pub fn mul_assign<A: WrapMul<B, Output = A> + Copy, B>(a: &mut A, b: B) {
    *a = a.wrap_mul(b);
}
#[inline(always)]
pub fn shl_assign<A: WrapShl<B> + Copy, B>(a: &mut A, b: B) {
    *a = a.wrap_shl(b);
}
#[inline(always)]
pub fn shr_assign<A: WrapShr<B> + Copy, B>(a: &mut A, b: B) {
    *a = a.wrap_shr(b);
}

macro_rules! int_impls {
    ($($t:ty),*) => {$(
        impl WrapAdd for $t {
            type Output = $t;
            #[inline(always)]
            fn wrap_add(self, rhs: $t) -> $t { self.wrapping_add(rhs) }
        }
        impl WrapSub for $t {
            type Output = $t;
            #[inline(always)]
            fn wrap_sub(self, rhs: $t) -> $t { self.wrapping_sub(rhs) }
        }
        impl WrapMul for $t {
            type Output = $t;
            #[inline(always)]
            fn wrap_mul(self, rhs: $t) -> $t { self.wrapping_mul(rhs) }
        }
        impl WrapShl<u32> for $t {
            #[inline(always)]
            fn wrap_shl(self, rhs: u32) -> $t { self.wrapping_shl(rhs) }
        }
        impl WrapShl<i32> for $t {
            #[inline(always)]
            fn wrap_shl(self, rhs: i32) -> $t { self.wrapping_shl(rhs as u32) }
        }
        impl WrapShr<u32> for $t {
            #[inline(always)]
            fn wrap_shr(self, rhs: u32) -> $t { self.wrapping_shr(rhs) }
        }
        impl WrapShr<i32> for $t {
            #[inline(always)]
            fn wrap_shr(self, rhs: i32) -> $t { self.wrapping_shr(rhs as u32) }
        }
    )*};
}
int_impls!(i32, u32);

impl WrapNeg for i32 {
    type Output = i32;
    #[inline(always)]
    fn wrap_neg(self) -> i32 {
        self.wrapping_neg()
    }
}

impl WrapAdd for f32 {
    type Output = f32;
    #[inline(always)]
    fn wrap_add(self, rhs: f32) -> f32 {
        self + rhs
    }
}
impl WrapSub for f32 {
    type Output = f32;
    #[inline(always)]
    fn wrap_sub(self, rhs: f32) -> f32 {
        self - rhs
    }
}
impl WrapMul for f32 {
    type Output = f32;
    #[inline(always)]
    fn wrap_mul(self, rhs: f32) -> f32 {
        self * rhs
    }
}
impl WrapNeg for f32 {
    type Output = f32;
    #[inline(always)]
    fn wrap_neg(self) -> f32 {
        -self
    }
}

/// A type whose own operators already have WGSL's meaning (vectors and
/// matrices; integer vectors wrap): the wrapping op is the operator.
macro_rules! delegate {
    ([$($g:tt)*] $t:ty) => {
        impl<$($g)* __R> WrapAdd<__R> for $t where $t: ::core::ops::Add<__R> {
            type Output = <$t as ::core::ops::Add<__R>>::Output;
            #[inline(always)]
            fn wrap_add(self, rhs: __R) -> Self::Output { self + rhs }
        }
        impl<$($g)* __R> WrapSub<__R> for $t where $t: ::core::ops::Sub<__R> {
            type Output = <$t as ::core::ops::Sub<__R>>::Output;
            #[inline(always)]
            fn wrap_sub(self, rhs: __R) -> Self::Output { self - rhs }
        }
        impl<$($g)* __R> WrapMul<__R> for $t where $t: ::core::ops::Mul<__R> {
            type Output = <$t as ::core::ops::Mul<__R>>::Output;
            #[inline(always)]
            fn wrap_mul(self, rhs: __R) -> Self::Output { self * rhs }
        }
    };
}
macro_rules! delegate_neg {
    ($($t:ident),*) => {$(
        impl<T> WrapNeg for $t<T> where $t<T>: ::core::ops::Neg {
            type Output = <$t<T> as ::core::ops::Neg>::Output;
            #[inline(always)]
            fn wrap_neg(self) -> Self::Output { -self }
        }
    )*};
}
delegate_neg!(Vec2, Vec3, Vec4);
delegate!([T,] Vec2<T>);
delegate!([T,] Vec3<T>);
delegate!([T,] Vec4<T>);
delegate!([] Mat2x2f);
delegate!([] Mat2x3f);
delegate!([] Mat2x4f);
delegate!([] Mat3x2f);
delegate!([] Mat3x3f);
delegate!([] Mat3x4f);
delegate!([] Mat4x2f);
delegate!([] Mat4x3f);
delegate!([] Mat4x4f);

/// Scalar on the left of a vector (`2.0 * v`, `k + v`) or of a matrix (`2.0 * m`):
/// the operator the vector / matrix module defines for it.
macro_rules! scalar_left {
    ($s:ty; $([$($g:tt)*] $t:ty),*) => {$(
        impl<$($g)*> WrapAdd<$t> for $s where $s: ::core::ops::Add<$t> {
            type Output = <$s as ::core::ops::Add<$t>>::Output;
            #[inline(always)]
            fn wrap_add(self, rhs: $t) -> Self::Output { self + rhs }
        }
        impl<$($g)*> WrapSub<$t> for $s where $s: ::core::ops::Sub<$t> {
            type Output = <$s as ::core::ops::Sub<$t>>::Output;
            #[inline(always)]
            fn wrap_sub(self, rhs: $t) -> Self::Output { self - rhs }
        }
        impl<$($g)*> WrapMul<$t> for $s where $s: ::core::ops::Mul<$t> {
            type Output = <$s as ::core::ops::Mul<$t>>::Output;
            #[inline(always)]
            fn wrap_mul(self, rhs: $t) -> Self::Output { self * rhs }
        }
    )*};
}
scalar_left!(i32; [T] Vec2<T>, [T] Vec3<T>, [T] Vec4<T>);
scalar_left!(u32; [T] Vec2<T>, [T] Vec3<T>, [T] Vec4<T>);
scalar_left!(f32; [T] Vec2<T>, [T] Vec3<T>, [T] Vec4<T>);
macro_rules! scalar_mat_mul {
    ($($m:ty),*) => {$(
        impl WrapMul<$m> for f32 {
            type Output = $m;
            #[inline(always)]
            fn wrap_mul(self, rhs: $m) -> $m { self * rhs }
        }
    )*};
}
scalar_mat_mul!(
    Mat2x2f, Mat2x3f, Mat2x4f, Mat3x2f, Mat3x3f, Mat3x4f, Mat4x2f, Mat4x3f, Mat4x4f
);
