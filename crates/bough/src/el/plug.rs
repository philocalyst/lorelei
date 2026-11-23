//! Ports (`designs/reactivity/final/03-ports.md`): what a property accepts.
//!
//! A port takes a plug: a constant, a borrowed `Signal` field, or a signal mapped through a closure
//! (`self.count.map(|n| ..)`). A closure never enters a port by itself: `map` over a source is the spelling,
//! so what the closure depends on is in its type. The family is closed (the traits here cannot be
//! implemented outside this crate), and every plug costs the same at every port: a constant is a store; a
//! signal or a map is a store plus a wire request the walk installs when the signal is a field of the view
//! that describes it. Publishing a wired signal then patches the element without rerunning `view`.
use super::arena::{self, WireReq, F_INT, F_NEG};
use crate::color::Color;
use crate::elem::{patch_map, patch_sig, Apply};
use crate::reflect::cell_id;
use crate::signal::Signal;
use std::ptr;

mod sealed {
    pub trait Sealed {}
    /// Passed to every `plug`: a method outside this crate cannot be called (or written) without it.
    pub struct Seal;
}
pub(crate) use sealed::Seal;

/// A property type a port can carry: `f32`, [`Color`], [`Text`].
pub trait PortValue: sealed::Sealed + 'static {
    /// What a wire writes.
    #[doc(hidden)]
    type Wire: 'static;
    /// What a description holds for the port's current value.
    #[doc(hidden)]
    type Held: Copy;
}

/// Property types whose description value is the value itself.
pub trait Scalar: PortValue<Wire = Self, Held = Self> + Copy {}

impl sealed::Sealed for f32 {}
impl PortValue for f32 {
    type Wire = f32;
    type Held = f32;
}
impl Scalar for f32 {}
impl sealed::Sealed for Color {}
impl PortValue for Color {
    type Wire = Color;
    type Held = Color;
}
impl Scalar for Color {}

/// The content of a text element: any string, an integer (formatted without allocating), a
/// `Signal<String>` field, or a signal mapped to a `String`.
pub struct Text(());
impl sealed::Sealed for Text {}
impl PortValue for Text {
    type Wire = String;
    type Held = TextVal;
}

/// Text as a description holds it: bytes borrowed for the description's lifetime or copied into its arena,
/// or an integer.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct TextVal {
    pub(crate) ptr: *const u8,
    /// Byte length, or the integer.
    pub(crate) len: u64,
    /// `F_INT`, `F_NEG`.
    pub(crate) flags: u8,
}

/// A plug as its port receives it.
#[doc(hidden)]
pub struct Plug<T: PortValue> {
    pub(crate) v: T::Held,
    /// The wire to ask for, or null (a constant).
    pub(crate) w: *const WireReq,
}

/// What a port of property type `T` accepts, for a description that lives `'f`.
pub trait IntoProp<'f, T: PortValue>: Sized {
    /// `K`: the property the port writes (the wire's patch is monomorphised for it here, where both the
    /// plug's type and the port's are known).
    #[doc(hidden)]
    fn plug<K: Apply<P = T::Wire>>(self, _: Seal) -> Plug<T>;
}

impl<'f> IntoProp<'f, Color> for Color {
    #[inline(always)]
    fn plug<K: Apply<P = Color>>(self, _: Seal) -> Plug<Color> {
        Plug { v: self, w: ptr::null() }
    }
}

macro_rules! number_plugs {
    ($($t:ty),*) => {$(
        impl<'f> IntoProp<'f, f32> for $t {
            #[inline(always)]
            fn plug<K: Apply<P = f32>>(self, _: Seal) -> Plug<f32> {
                Plug { v: self as f32, w: ptr::null() }
            }
        }
    )*};
}
number_plugs!(f32, i32, u32);

/// A signal field: the port follows it.
impl<'f, T: Scalar> IntoProp<'f, T> for &'f Signal<T> {
    #[inline]
    fn plug<K: Apply<P = T>>(self, _: Seal) -> Plug<T> {
        Plug { v: **self, w: arena::put(WireReq { head: self.head(), ty: cell_id::<Signal<T>>(), patch: patch_sig::<K, T>, recipe: crate::rt::Recipe::NONE, exact: true }) }
    }
}

/// A source: what `map` is closed over. Implemented for a borrowed `Signal`.
pub trait Source<'f>: sealed::Sealed + Sized {
    type Value: 'static;
    /// The source seen through `f`. `f` may capture at most 16 bytes, by copy: what else it depends on
    /// belongs in a signal.
    fn map<T, F: Fn(&Self::Value) -> T + Copy + 'static>(self, f: F) -> Map<'f, Self::Value, F>;
}
impl<S> sealed::Sealed for &Signal<S> {}
impl<'f, S: 'static> Source<'f> for &'f Signal<S> {
    type Value = S;
    #[inline(always)]
    fn map<T, F: Fn(&S) -> T + Copy + 'static>(self, f: F) -> Map<'f, S, F> {
        Map { sig: self, f }
    }
}

/// A signal mapped through a closure (`Source::map`).
#[must_use = "a mapped source does nothing until it is plugged into a port"]
pub struct Map<'f, S, F> {
    sig: &'f Signal<S>,
    f: F,
}

fn recipe<F: Copy>(f: F) -> crate::rt::Recipe {
    const { assert!(size_of::<F>() <= 16, "a map closure captures at most 16 bytes; put what it depends on in a signal") };
    crate::rt::Recipe::of(f)
}

impl<'f, S: 'static, T: Scalar, F: Fn(&S) -> T + Copy + 'static> IntoProp<'f, T> for Map<'f, S, F> {
    #[inline]
    fn plug<K: Apply<P = T>>(self, _: Seal) -> Plug<T> {
        Plug { v: (self.f)(self.sig), w: arena::put(WireReq { head: self.sig.head(), ty: cell_id::<Signal<S>>(), patch: patch_map::<K, S, F>, recipe: recipe(self.f), exact: crate::reflect::padding_free::<F>() }) }
    }
}

#[inline(always)]
fn borrowed(s: &str) -> TextVal {
    TextVal { ptr: s.as_ptr(), len: s.len() as u64, flags: 0 }
}

impl<'f> IntoProp<'f, Text> for &'f str {
    #[inline(always)]
    fn plug<K: Apply<P = String>>(self, _: Seal) -> Plug<Text> {
        Plug { v: borrowed(self), w: ptr::null() }
    }
}
impl<'f> IntoProp<'f, Text> for &'f String {
    #[inline(always)]
    fn plug<K: Apply<P = String>>(self, _: Seal) -> Plug<Text> {
        Plug { v: borrowed(self), w: ptr::null() }
    }
}
/// An owned string (`format!(..)`) is copied into the description's arena and dropped.
impl<'f> IntoProp<'f, Text> for String {
    #[inline]
    fn plug<K: Apply<P = String>>(self, _: Seal) -> Plug<Text> {
        Plug { v: TextVal { ptr: arena::bytes(self.as_bytes()), len: self.len() as u64, flags: 0 }, w: ptr::null() }
    }
}

macro_rules! int_text {
    ($($t:ty),*) => {$(
        /// An integer as its decimal text, formatted only when the text is needed (`text(row.id)` allocates
        /// nothing).
        impl<'f> IntoProp<'f, Text> for $t {
            #[inline(always)]
            #[allow(unused_comparisons)]
            fn plug<K: Apply<P = String>>(self, _: Seal) -> Plug<Text> {
                Plug { v: TextVal { ptr: ptr::null(), len: self as i128 as u64, flags: F_INT | if self < 0 { F_NEG } else { 0 } }, w: ptr::null() }
            }
        }
    )*};
}
int_text!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

/// A `Signal<String>` field: the text follows it.
impl<'f> IntoProp<'f, Text> for &'f Signal<String> {
    #[inline]
    fn plug<K: Apply<P = String>>(self, _: Seal) -> Plug<Text> {
        Plug { v: borrowed(self), w: arena::put(WireReq { head: self.head(), ty: cell_id::<Signal<String>>(), patch: patch_sig::<K, String>, recipe: crate::rt::Recipe::NONE, exact: true }) }
    }
}

/// A signal mapped to its text (`self.count.map(|n| format!("Count: {n}"))`): the text follows the signal.
impl<'f, S: 'static, F: Fn(&S) -> String + Copy + 'static> IntoProp<'f, Text> for Map<'f, S, F> {
    #[inline]
    fn plug<K: Apply<P = String>>(self, _: Seal) -> Plug<Text> {
        let s = (self.f)(self.sig);
        Plug { v: TextVal { ptr: arena::bytes(s.as_bytes()), len: s.len() as u64, flags: 0 }, w: arena::put(WireReq { head: self.sig.head(), ty: cell_id::<Signal<S>>(), patch: patch_map::<K, S, F>, recipe: recipe(self.f), exact: crate::reflect::padding_free::<F>() }) }
    }
}

/// A length in points: `f32`, or an integer (`.gap(8)`).
pub trait Len: Copy {
    fn pt(self) -> f32;
}
impl Len for f32 {
    #[inline(always)]
    fn pt(self) -> f32 {
        self
    }
}
impl Len for i32 {
    #[inline(always)]
    fn pt(self) -> f32 {
        self as f32
    }
}
impl Len for u32 {
    #[inline(always)]
    fn pt(self) -> f32 {
        self as f32
    }
}
