//! Ports (`designs/reactivity/final/03-ports.md`): what a property accepts.
//!
//! A port takes a plug: a constant, a borrowed `Signal` field, or a signal mapped through a closure
//! (`self.count.map(|n| ..)`). A closure never enters a port by itself: `map` over a source is the spelling,
//! so what the closure depends on is in its type. The family is closed (the traits here cannot be
//! implemented outside this crate), and every plug costs the same at every port: a constant is a store; a
//! signal or a map is a store plus a wire request the walk installs when the signal is a field of the view
//! that describes it. Publishing a wired signal then patches the element without rerunning `view`.
use super::arena::{self, F_INT, F_NEG, WireReq};
use super::arena::{W_GUARD, W_LATEST, W_PHEAD, tagged};
use crate::color::Color;
use crate::elem::{Apply, patch_map, patch_sig};
use crate::reader::{Reader, patch_rmap, patch_rpmap};
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
pub trait Scalar: PortValue<Wire = Self, Held = Self> + Copy {
    /// The value as one word (what a scalar port's eval returns, `arena::ScalarEval`).
    #[doc(hidden)]
    fn bits(self) -> u32;
    /// What a port with a wire request holds in its node: a fixed placeholder (its value is evaluated).
    #[doc(hidden)]
    const PLACEHOLDER: Self;
}

impl sealed::Sealed for f32 {}
impl PortValue for f32 {
    type Wire = f32;
    type Held = f32;
}
impl Scalar for f32 {
    #[inline(always)]
    fn bits(self) -> u32 {
        self.to_bits()
    }
    const PLACEHOLDER: f32 = 0.0;
}
impl sealed::Sealed for Color {}
impl PortValue for Color {
    type Wire = Color;
    type Held = Color;
}
impl Scalar for Color {
    #[inline(always)]
    fn bits(self) -> u32 {
        self.0
    }
    const PLACEHOLDER: Color = Color(0);
}

// ------------------------------------------------------------------------------- lazy values
//
// A plug with a wire request does not run its closure (or read its cell) when the description is built: the
// request carries an eval, monomorphised here with the patch, that the walk calls where a value is needed
// (`walk.rs`, "Lazy port values"). These are those evals, one per source kind and value kind.

/// A plug with a wire request: the node holds the placeholder, the request the eval.
#[inline(always)]
fn ported<T: PortValue>(v: T::Held, head: *const std::cell::Cell<u32>, ty: std::any::TypeId, patch: crate::rt::PatchFn, recipe: crate::rt::Recipe, eval: *const ()) -> Plug<T> {
    Plug { v, w: arena::put(WireReq { head, ty, patch, recipe, eval, state: arena::P_EVAL }) }
}
/// The node's text for a port with a request: empty (its value is evaluated).
const NO_TEXT: TextVal = TextVal { ptr: ptr::NonNull::<u8>::dangling().as_ptr(), len: 0, flags: 0 };

/// # Safety
/// `ScalarEval`'s contract, for a `Signal<T>`.
unsafe fn eval_sig<T: Scalar>(cell: usize, _: &crate::rt::Recipe) -> u32 {
    // SAFETY: the contract.
    unsafe { **(cell as *const Signal<T>) }.bits()
}
/// # Safety
/// `ScalarEval`'s contract, for a `Signal<S>`; the recipe holds an `F`.
unsafe fn eval_map<S: 'static, T: Scalar, F: Fn(&S) -> T + Copy>(cell: usize, r: &crate::rt::Recipe) -> u32 {
    // SAFETY: the contract.
    let (s, f) = unsafe { (&*(cell as *const Signal<S>), r.read_at::<F>(0)) };
    f(s).bits()
}
/// # Safety
/// `ScalarEval`'s contract, for a `Reader<T>`; the recipe holds an `F`.
unsafe fn eval_rmap<T: 'static, P: Scalar, F: Fn(Option<&T>) -> P + Copy>(cell: usize, r: &crate::rt::Recipe) -> u32 {
    // SAFETY: the contract.
    let (s, f) = unsafe { (&*(cell as *const Reader<T>), r.read_at::<F>(0)) };
    f(s.latest()).bits()
}
/// # Safety
/// `ScalarEval`'s contract, for a `Reader<T>`; the recipe holds an `F`.
unsafe fn eval_rpmap<T: 'static, P: Scalar, F: Fn(bool) -> P + Copy>(cell: usize, r: &crate::rt::Recipe) -> u32 {
    // SAFETY: the contract.
    let (s, f) = unsafe { (&*(cell as *const Reader<T>), r.read_at::<F>(0)) };
    f(s.is_pending()).bits()
}
/// # Safety
/// `TextEval`'s contract, for a `Signal<String>`.
unsafe fn eval_sig_text(cell: usize, _: &crate::rt::Recipe, k: &mut dyn FnMut(&str)) {
    // SAFETY: the contract.
    k(unsafe { &*(cell as *const Signal<String>) })
}
/// # Safety
/// `TextEval`'s contract, for a `Signal<S>`; the recipe holds an `F`.
unsafe fn eval_map_text<S: 'static, F: Fn(&S) -> String + Copy>(cell: usize, r: &crate::rt::Recipe, k: &mut dyn FnMut(&str)) {
    // SAFETY: the contract.
    let (s, f) = unsafe { (&*(cell as *const Signal<S>), r.read_at::<F>(0)) };
    k(&f(s))
}
/// # Safety
/// `TextEval`'s contract, for a `Reader<T>`; the recipe holds an `F`.
unsafe fn eval_rmap_text<T: 'static, F: Fn(Option<&T>) -> String + Copy>(cell: usize, r: &crate::rt::Recipe, k: &mut dyn FnMut(&str)) {
    // SAFETY: the contract.
    let (s, f) = unsafe { (&*(cell as *const Reader<T>), r.read_at::<F>(0)) };
    k(&f(s.latest()))
}
/// # Safety
/// `TextEval`'s contract, for a `Reader<T>`; the recipe holds an `F`.
unsafe fn eval_rpmap_text<T: 'static, F: Fn(bool) -> String + Copy>(cell: usize, r: &crate::rt::Recipe, k: &mut dyn FnMut(&str)) {
    // SAFETY: the contract.
    let (s, f) = unsafe { (&*(cell as *const Reader<T>), r.read_at::<F>(0)) };
    k(&f(s.is_pending()))
}

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
        ported(T::PLACEHOLDER, self.head(), cell_id::<Signal<T>>(), patch_sig::<K, T>, crate::rt::Recipe::NONE, eval_sig::<T> as *const ())
    }
}

/// A source (03 §1): what `map` is closed over. A borrowed `Signal` (the closure sees `&T`), a borrowed
/// `Reader` (it sees `Option<&T>`: `None` before first readiness), `is_pending(&reader)` (a `bool`) and
/// `latest(&signal)` (`&T`, tracking current intent while a cohort holds the signal).
pub trait Source<'f>: sealed::Sealed + Sized {
    /// What `map`'s closure is given.
    type Ready<'a>;
    /// The source seen through `f`. `f` may capture at most 16 bytes, by copy: what else it depends on
    /// belongs in a signal.
    #[inline(always)]
    fn map<T, F: for<'a> Fn(Self::Ready<'a>) -> T + Copy + 'static>(self, f: F) -> Map<Self, F> {
        Map { src: self, f }
    }
}
impl<S> sealed::Sealed for &Signal<S> {}
impl<'f, S: 'static> Source<'f> for &'f Signal<S> {
    type Ready<'a> = &'a S;
}

/// A source mapped through a closure (`Source::map`).
#[must_use = "a mapped source does nothing until it is plugged into a port"]
pub struct Map<S, F> {
    src: S,
    f: F,
}

fn recipe<F: Copy>(f: F) -> crate::rt::Recipe {
    const { assert!(size_of::<F>() <= 16, "a map closure captures at most 16 bytes; put what it depends on in a signal") };
    crate::rt::Recipe::of(f)
}

impl<'f, S: 'static, T: Scalar, F: Fn(&S) -> T + Copy + 'static> IntoProp<'f, T> for Map<&'f Signal<S>, F> {
    #[inline]
    fn plug<K: Apply<P = T>>(self, _: Seal) -> Plug<T> {
        ported(T::PLACEHOLDER, self.src.head(), cell_id::<Signal<S>>(), patch_map::<K, S, F>, recipe(self.f), eval_map::<S, T, F> as *const ())
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
        ported(NO_TEXT, self.head(), cell_id::<Signal<String>>(), patch_sig::<K, String>, crate::rt::Recipe::NONE, eval_sig_text as *const ())
    }
}

/// A signal mapped to its text (`self.count.map(|n| format!("Count: {n}"))`): the text follows the signal. The
/// closure runs where its text is needed (mount, or a description of a signal that is not a field), not each
/// time the view is described.
impl<'f, S: 'static, F: Fn(&S) -> String + Copy + 'static> IntoProp<'f, Text> for Map<&'f Signal<S>, F> {
    #[inline]
    fn plug<K: Apply<P = String>>(self, _: Seal) -> Plug<Text> {
        ported(NO_TEXT, self.src.head(), cell_id::<Signal<S>>(), patch_map::<K, S, F>, recipe(self.f), eval_map_text::<S, F> as *const ())
    }
}

// ------------------------------------------------------------------------------- readers (05 §3, §5)

/// A `Reader` field: `self.story.map(|s: Option<&Story>| ..)` is a direct port. A completion the cohort
/// commits patches the element without rerunning `view`; inside a `boundary`, the boundary guards the reader.
/// A reader that is not a field of the view (behind a `Vec`, in another object) is a constant of the
/// description, as a signal is.
impl<T> sealed::Sealed for &Reader<T> {}
impl<'f, T: 'static> Source<'f> for &'f Reader<T> {
    type Ready<'a> = Option<&'a T>;
}

/// `is_pending(&self.results)` (05 §5): does the next visible answer of this reader still wait on a request?
/// A source of `bool`: `.opacity(is_pending(&self.results).map(|p| if p { 0.65 } else { 1.0 }))`.
pub fn is_pending<T>(r: &Reader<T>) -> Pending<'_, T> {
    Pending(r)
}
/// `is_pending(..)`: see [`is_pending`].
#[derive(Clone, Copy)]
pub struct Pending<'f, T>(&'f Reader<T>);
impl<T> sealed::Sealed for Pending<'_, T> {}
impl<'f, T: 'static> Source<'f> for Pending<'f, T> {
    type Ready<'a> = bool;
}

/// `latest(&self.selected_id)` (05 §5): bind a port to current model intent, not to its cohort's retained
/// operand. A port of a signal a pending producer holds keeps its pre-write value until the cohort commits;
/// through `latest` it follows every write at once. Plugs as the signal does (`.bg(latest(&self.tint))`,
/// `latest(&self.id).map(f)`).
pub fn latest<S>(s: &Signal<S>) -> Latest<'_, S> {
    Latest(s)
}
/// `latest(..)`: see [`latest`].
#[derive(Clone, Copy)]
pub struct Latest<'f, S>(&'f Signal<S>);
impl<S> sealed::Sealed for Latest<'_, S> {}
impl<'f, S: 'static> Source<'f> for Latest<'f, S> {
    type Ready<'a> = &'a S;
}

impl<'f, T: Scalar> IntoProp<'f, T> for Latest<'f, T> {
    #[inline]
    fn plug<K: Apply<P = T>>(self, _: Seal) -> Plug<T> {
        ported(T::PLACEHOLDER, tagged(self.0.head(), W_LATEST), cell_id::<Signal<T>>(), patch_sig::<K, T>, crate::rt::Recipe::NONE, eval_sig::<T> as *const ())
    }
}
impl<'f> IntoProp<'f, Text> for Latest<'f, String> {
    #[inline]
    fn plug<K: Apply<P = String>>(self, _: Seal) -> Plug<Text> {
        ported(NO_TEXT, tagged(self.0.head(), W_LATEST), cell_id::<Signal<String>>(), patch_sig::<K, String>, crate::rt::Recipe::NONE, eval_sig_text as *const ())
    }
}

impl<'f, S: 'static, P: Scalar, F: Fn(&S) -> P + Copy + 'static> IntoProp<'f, P> for Map<Latest<'f, S>, F> {
    #[inline]
    fn plug<K: Apply<P = P>>(self, _: Seal) -> Plug<P> {
        ported(P::PLACEHOLDER, tagged(self.src.0.head(), W_LATEST), cell_id::<Signal<S>>(), patch_map::<K, S, F>, recipe(self.f), eval_map::<S, P, F> as *const ())
    }
}
impl<'f, S: 'static, F: Fn(&S) -> String + Copy + 'static> IntoProp<'f, Text> for Map<Latest<'f, S>, F> {
    #[inline]
    fn plug<K: Apply<P = String>>(self, _: Seal) -> Plug<Text> {
        ported(NO_TEXT, tagged(self.src.0.head(), W_LATEST), cell_id::<Signal<S>>(), patch_map::<K, S, F>, recipe(self.f), eval_map_text::<S, F> as *const ())
    }
}
impl<'f, T: 'static, P: Scalar, F: Fn(Option<&T>) -> P + Copy + 'static> IntoProp<'f, P> for Map<&'f Reader<T>, F> {
    #[inline]
    fn plug<K: Apply<P = P>>(self, _: Seal) -> Plug<P> {
        ported(P::PLACEHOLDER, tagged(self.src.head_cell(), W_GUARD), cell_id::<Reader<T>>(), patch_rmap::<K, T, F>, recipe(self.f), eval_rmap::<T, P, F> as *const ())
    }
}
impl<'f, T: 'static, F: Fn(Option<&T>) -> String + Copy + 'static> IntoProp<'f, Text> for Map<&'f Reader<T>, F> {
    #[inline]
    fn plug<K: Apply<P = String>>(self, _: Seal) -> Plug<Text> {
        ported(NO_TEXT, tagged(self.src.head_cell(), W_GUARD), cell_id::<Reader<T>>(), patch_rmap::<K, T, F>, recipe(self.f), eval_rmap_text::<T, F> as *const ())
    }
}
impl<'f, T: 'static, P: Scalar, F: Fn(bool) -> P + Copy + 'static> IntoProp<'f, P> for Map<Pending<'f, T>, F> {
    #[inline]
    fn plug<K: Apply<P = P>>(self, _: Seal) -> Plug<P> {
        ported(P::PLACEHOLDER, tagged(self.src.0.phead_cell(), W_PHEAD), cell_id::<Reader<T>>(), patch_rpmap::<K, T, F>, recipe(self.f), eval_rpmap::<T, P, F> as *const ())
    }
}
impl<'f, T: 'static, F: Fn(bool) -> String + Copy + 'static> IntoProp<'f, Text> for Map<Pending<'f, T>, F> {
    #[inline]
    fn plug<K: Apply<P = String>>(self, _: Seal) -> Plug<Text> {
        ported(NO_TEXT, tagged(self.src.0.phead_cell(), W_PHEAD), cell_id::<Reader<T>>(), patch_rpmap::<K, T, F>, recipe(self.f), eval_rpmap_text::<T, F> as *const ())
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
