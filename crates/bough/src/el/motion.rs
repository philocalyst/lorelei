//! Motion as a plug (`designs/reactivity/final/03-ports.md` §4): a property value sampled on a timeline.
//!
//! ```ignore
//! fn view(&self) -> El<'_> {
//!     let reveal = animate(&self.reveal).spring(Spring::snappy());
//!     div().bg(CARD).opacity(reveal).translate_y(reveal.reverse().scale(8))
//! }
//! ```
//!
//! `animate(&self.reveal)` observes the signal and presents a sampled value: a [`Motion`], a small `Copy` draft
//! value accepted by every `f32` and [`Color`] port (`opacity`, `translate_x`/`_y`, `w`, `h`, `bg`, `fg`) like a
//! constant or a signal. It lowers onto the kernel's wire transitions (`transition.rs`): at mount the port gets a
//! motion resting at the signal's value and a wire on the signal whose patch *retargets* the motion instead of
//! applying the value; the frame clock (`App::tick`) samples every running motion and applies its value through
//! the port's own applier. So publishing the signal reruns no `view`, a motion at rest is not stepped and asks
//! for no frame, and a view that never animates makes no motion state at all.
//!
//! Timings: a damped [`Spring`] (the default, [`Spring::snappy`]), an eased curve (`.over(ms)`, `.ease(..)`,
//! `.curve(Curve)`), and `.delay(ms)`. On `f32`, `.reverse()` (`1 - v`) and `.scale(k)` map the value, so one
//! signal drives several ports (each port is its own motion; a spring is linear, so the mapped motion is the
//! motion of the mapped value). A retarget starts from the value and velocity presented at that instant; a spring
//! is evaluated in closed form, so a timestamp gives one value at any frame cadence.
//!
//! What 03 §4 lists and the kernel does not have yet (open): keyframes, sequence / parallel / race, repeat,
//! gesture handoff and other timelines (scroll, media, demo time), `jump(target)`, reduced motion, completion as a
//! queued output, structural enter/exit, and motion on the lowered runtime (`lower` presents the target).
//! A delay holds the presented value, so a delayed motion retargeted in flight is continuous in value only.
use super::arena::{self, WireReq};
use super::plug::{IntoProp, Len, Plug, Scalar, Seal};
use crate::color::Color;
use crate::elem::{Apply, Mounter};
use crate::reflect::cell_id;
use crate::rt::{Recipe, Rt, Wire};
use crate::signal::Signal;
use crate::transition::{Curve, Ease, Lerp, Program, Transitions};

/// A spring's response: a damped oscillator of unit mass (03 §4). A retarget always keeps the velocity the
/// spring presents ([`preserve_velocity`](Self::preserve_velocity) states it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    w0: f32,
    zeta: f32,
}

impl Spring {
    fn of(s: crate::motion::SpringSpec) -> Spring {
        let (w0, zeta) = s.omega_zeta();
        Spring { w0: w0 as f32, zeta: zeta as f32 }
    }
    /// By stiffness and damping (per unit mass, seconds).
    pub fn new(stiffness: f32, damping: f32) -> Spring {
        Spring::of(crate::motion::SpringSpec::new(stiffness, damping, 1.0))
    }
    /// By its response (the undamped period, seconds: how quick it feels) and damping ratio (1: no overshoot).
    pub fn response(response: f32, damping_ratio: f32) -> Spring {
        Spring::of(crate::motion::SpringSpec::response(response, damping_ratio))
    }
    /// Quick, a trace of overshoot (response 0.3 s, ratio 0.86).
    pub fn snappy() -> Spring {
        Spring::of(crate::motion::SpringSpec::SNAPPY)
    }
    /// A visible bounce (response 0.5 s, ratio 0.55).
    pub fn bouncy() -> Spring {
        Spring::of(crate::motion::SpringSpec::BOUNCY)
    }
    /// Slow and soft, no overshoot (response 0.6 s).
    pub fn gentle() -> Spring {
        Spring::of(crate::motion::SpringSpec::GENTLE)
    }
    /// Fast, no overshoot (response 0.2 s).
    pub fn stiff() -> Spring {
        Spring::of(crate::motion::SpringSpec::STIFF)
    }
    /// A retarget continues from the presented value *and velocity*: what every spring here does.
    pub fn preserve_velocity(self) -> Spring {
        self
    }
}

/// Values a motion carries: `f32` and [`Color`] (per channel). Sealed (`Scalar` is).
pub trait Animatable: Scalar + Lerp {
    #[doc(hidden)]
    fn affine(self, m: f32, c: f32) -> Self;
    #[doc(hidden)]
    fn bits(self) -> u32;
    #[doc(hidden)]
    fn of_bits(b: u32) -> Self;
}
impl Animatable for f32 {
    #[inline(always)]
    fn affine(self, m: f32, c: f32) -> f32 {
        self * m + c
    }
    #[inline(always)]
    fn bits(self) -> u32 {
        self.to_bits()
    }
    #[inline(always)]
    fn of_bits(b: u32) -> f32 {
        f32::from_bits(b)
    }
}
impl Animatable for Color {
    /// (A colour is not mapped: `reverse` and `scale` exist on `f32` motions only.)
    #[inline(always)]
    fn affine(self, _: f32, _: f32) -> Color {
        self
    }
    #[inline(always)]
    fn bits(self) -> u32 {
        self.0
    }
    #[inline(always)]
    fn of_bits(b: u32) -> Color {
        Color(b)
    }
}

/// `animate(&self.x)`: the signal field, presented through a timeline (a spring by default).
#[inline]
pub fn animate<'f, T: Animatable>(sig: &'f Signal<T>) -> Motion<'f, T> {
    let s = Spring::snappy();
    Motion { sig, prog: Program::spring(s.w0, s.zeta), m: 1.0, c: 0.0 }
}

/// A motion draft: a signal, a timing and (on `f32`) an affine map. `Copy`: one draft drives several ports.
#[derive(Clone, Copy)]
#[must_use = "a motion does nothing until it is plugged into a port"]
pub struct Motion<'f, T> {
    sig: &'f Signal<T>,
    prog: Program,
    m: f32,
    c: f32,
}

impl<'f, T: Animatable> Motion<'f, T> {
    /// A damped spring.
    pub fn spring(mut self, s: Spring) -> Self {
        let d = self.prog.delay;
        self.prog = Program::spring(s.w0, s.zeta);
        self.prog.delay = d;
        self
    }
    /// An eased curve (duration and easing; the kernel's [`Curve`]).
    pub fn curve(mut self, c: Curve) -> Self {
        let d = self.prog.delay;
        self.prog = Program::curve(c);
        self.prog.delay = d;
        self
    }
    /// An eased curve of `ms` milliseconds (the easing set so far, else ease-out cubic).
    pub fn over(self, ms: u32) -> Self {
        let ease = self.prog.as_curve().map_or(Ease::OutCubic, |c| c.ease);
        self.curve(Curve::new(ms as f32 / 1000.0, ease))
    }
    /// An eased curve with easing `e` (the duration set so far, else 250 ms).
    pub fn ease(self, e: Ease) -> Self {
        let dur = self.prog.as_curve().map_or(0.25, |c| c.dur);
        self.curve(Curve::new(dur, e))
    }
    /// Each retarget holds the presented value for `ms` milliseconds before it moves.
    pub fn delay(mut self, ms: u32) -> Self {
        self.prog.delay = ms as f32 / 1000.0;
        self
    }
}

impl<'f> Motion<'f, f32> {
    /// `1 - v`: the same motion run the other way (a reveal as a slide-out).
    pub fn reverse(self) -> Self {
        Motion { m: -self.m, c: 1.0 - self.c, ..self }
    }
    /// `v * k`.
    pub fn scale(self, k: impl Len) -> Self {
        let k = k.pt();
        Motion { m: self.m * k, c: self.c * k, ..self }
    }
}

/// What a motion wire's recipe holds: its transition and the affine map.
#[repr(C)]
#[derive(Clone, Copy)]
struct Steer {
    id: u32,
    m: f32,
    c: f32,
}

/// A motion port asked for (in the description's arena, beside its `WireReq`): the timing, and the functions
/// monomorphised where the port's applier and value type were known.
#[derive(Clone, Copy)]
pub(crate) struct MotionReq {
    pub prog: Program,
    /// A motion of `node` resting at the value (bits) with this program: its id.
    pub add: fn(&mut Transitions, u32, u32, Program) -> u32,
    /// A re-description: motion `id` takes the program and heads for the target (bits); the value it presents.
    pub sync: fn(&mut Transitions, u32, u32, Program) -> u32,
}

fn apply_k<K: Apply>(rt: &mut Rt, node: u32, v: &K::P) {
    K::apply(rt, node, v)
}

fn add_motion<K: Apply<P = T>, T: Animatable>(tr: &mut Transitions, node: u32, init: u32, prog: Program) -> u32 {
    tr.add_anim::<T>(node, apply_k::<K>, T::of_bits(init), prog)
}

fn sync_motion<T: Animatable>(tr: &mut Transitions, id: u32, target: u32, prog: Program) -> u32 {
    tr.reprogram::<T>(id, prog);
    tr.retarget_motion::<T>(id, T::of_bits(target));
    tr.motion_shown::<T>(id).map_or(target, <T as Animatable>::bits)
}

/// # Safety
/// The wire contract (`PatchFn`): `sig` is a `Signal<T>`; the recipe holds a `Steer` (`install`).
unsafe fn patch_motion<T: Animatable>(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8) {
    // SAFETY: see the function contract.
    let (s, st) = unsafe { (&*(sig as *const Signal<T>), w.recipe.read_at::<Steer>(0)) };
    rt.transitions.retarget_motion::<T>(st.id, (**s).affine(st.m, st.c));
}

/// A motion port's value where the walk evaluates it (mount, a constant): its target, `affine(signal)`.
///
/// # Safety
/// `ScalarEval`'s contract (`arena.rs`), for a `Signal<T>`; the recipe holds a `Steer`.
unsafe fn eval_motion<T: Animatable>(cell: usize, r: &Recipe) -> u32 {
    // SAFETY: the contract.
    let (s, st) = unsafe { (&*(cell as *const Signal<T>), r.read_at::<Steer>(0)) };
    Animatable::bits((**s).affine(st.m, st.c))
}

impl<'f, T: Animatable> IntoProp<'f, T> for Motion<'f, T> {
    #[inline]
    fn plug<K: Apply<P = T>>(self, _: Seal) -> Plug<T> {
        let v = (**self.sig).affine(self.m, self.c);
        let motion = arena::put(MotionReq { prog: self.prog, add: add_motion::<K, T>, sync: sync_motion::<T> });
        let recipe = Recipe::of(Steer { id: 0, m: self.m, c: self.c });
        Plug { v, w: arena::put(WireReq { head: self.sig.head(), ty: cell_id::<Signal<T>>(), patch: patch_motion::<T>, recipe, motion, eval: eval_motion::<T> as *const (), state: arena::P_EVAL }) }
    }
}

/// Mount a motion port of `node` showing `init` (bits): a motion and the wire that retargets it, when the
/// signal is a field of the mounting view. Otherwise the port is a constant of this description (as any port
/// whose cell lives elsewhere), and nothing is made.
///
/// # Safety
/// `w` was written by `Motion::plug` this description: `head` is the head of a live `Signal<T>` (`ty`), and
/// `motion` its request.
pub(crate) unsafe fn install<V>(m: &mut Mounter<'_, V>, w: &WireReq, node: u32, init: u32) {
    // SAFETY: the function contract.
    let (head, mr) = unsafe { (&*w.wire_head(), &*w.motion) };
    let Some(off) = m.field_off(head, w.ty) else { return };
    let id = (mr.add)(&mut m.rt.transitions, node, init, mr.prog);
    // SAFETY: `off` is the field's proven offset (`field_off`), and `patch_motion::<T>` reads that cell type
    // with the `Steer` written here.
    let i = unsafe { m.wire_cell(head, off, w.patch, node, w.recipe.with_at(0, id)) };
    m.rt.wires[i as usize].motion = true;
}

/// A re-description of a motion port whose installed wire is `wi`: the wire takes the description's map (its
/// `Steer`, keeping the motion's id), and the motion its new target and timing. True: the motion presents the
/// port's value, so the update applies nothing over it (false under `mutant_motion_snap`: the target is applied).
pub(crate) fn steer<V>(m: &mut Mounter<'_, V>, wi: u32, req: &WireReq) -> bool {
    let w = &mut m.rt.wires[wi as usize];
    // SAFETY: a motion wire's recipe holds its `Steer` (`install`); `motion` is this description's request.
    let (id, mr) = unsafe { (w.recipe.read_at::<u32>(0), &*req.motion) };
    w.recipe = req.recipe.with_at(0, id);
    let target = req.scalar();
    (mr.sync)(&mut m.rt.transitions, id, target, mr.prog);
    !cfg!(feature = "mutant_motion_snap")
}
