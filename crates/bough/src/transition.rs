//! Built-in transitions (round 4, input agent): a transition is a typed operand, a value plus a curve.
//!
//! `sig!(v.width).transition(Curve::out(0.24))` (or `.map(f).transition(..)`) lowers like any wired operand
//! for any property applier, so every animatable property (colour, width, span, angle, opacity, a
//! transform's matrix...) transitions with no code of its own: the wire's patch does not apply the new
//! value, it *retargets* the transition (from = the value shown now, to = the new value, start = now); the
//! frame clock (`App::tick`) evaluates every active transition and applies its value through the same
//! applier (a paint write, a layout restyle, ...), and the host publishes those frames paced
//! (`frame_paced`). A layout property relayouts per frame because its applier does.
//!
//! Interrupting is continuous by construction: a retarget starts from the value displayed at that moment
//! (value continuity, C0), and with `Curve::smooth` from the displayed velocity too (C1: a cubic Hermite
//! segment from (value, velocity) to (target, 0)).
use crate::color::Color;
use crate::elem::{Apply, IntoProp, Mounter};
use crate::rt::{Rt, Wire};
use crate::signal::{WMap, Wired};

/// Values a transition can interpolate.
pub trait Lerp: Copy + PartialEq + 'static {
    /// `a + (b - a) t`.
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self::add_scaled(a, b, a, t)
    }
    /// `a + (b - c) k` per component (the Hermite form's tangent term).
    fn add_scaled(a: Self, b: Self, c: Self, k: f32) -> Self;
    /// The value as up to four components (a motion's spring runs on each: [`Anim`]).
    #[doc(hidden)]
    const LANES: usize;
    /// How close to its target a spring is at rest, in component units: half the smallest step that shows.
    #[doc(hidden)]
    const REST: f32;
    #[doc(hidden)]
    fn parts(self) -> [f32; 4];
    #[doc(hidden)]
    fn from_parts(p: [f32; 4]) -> Self;
}
impl Lerp for f32 {
    #[inline]
    fn add_scaled(a: f32, b: f32, c: f32, k: f32) -> f32 {
        a + (b - c) * k
    }
    const LANES: usize = 1;
    const REST: f32 = 1.0 / 2048.0;
    #[inline(always)]
    fn parts(self) -> [f32; 4] {
        [self, 0.0, 0.0, 0.0]
    }
    #[inline(always)]
    fn from_parts(p: [f32; 4]) -> f32 {
        p[0]
    }
}
impl Lerp for (f32, f32) {
    #[inline]
    fn add_scaled(a: Self, b: Self, c: Self, k: f32) -> Self {
        (a.0 + (b.0 - c.0) * k, a.1 + (b.1 - c.1) * k)
    }
    const LANES: usize = 2;
    const REST: f32 = 1.0 / 2048.0;
    #[inline(always)]
    fn parts(self) -> [f32; 4] {
        [self.0, self.1, 0.0, 0.0]
    }
    #[inline(always)]
    fn from_parts(p: [f32; 4]) -> Self {
        (p[0], p[1])
    }
}
impl Lerp for [f32; 4] {
    #[inline]
    fn add_scaled(a: Self, b: Self, c: Self, k: f32) -> Self {
        [0, 1, 2, 3].map(|i| a[i] + (b[i] - c[i]) * k)
    }
    const LANES: usize = 4;
    const REST: f32 = 1.0 / 2048.0;
    #[inline(always)]
    fn parts(self) -> [f32; 4] {
        self
    }
    #[inline(always)]
    fn from_parts(p: [f32; 4]) -> Self {
        p
    }
}
impl Lerp for Color {
    /// Per channel (straight RGBA8), rounded and clamped.
    #[inline]
    fn add_scaled(a: Color, b: Color, c: Color, k: f32) -> Color {
        let ch = |v: u32, i: u32| ((v >> (8 * i)) & 255) as f32;
        let v = |i: u32| (ch(a.0, i) + (ch(b.0, i) - ch(c.0, i)) * k).round().clamp(0.0, 255.0) as u32;
        Color(v(0) | v(1) << 8 | v(2) << 16 | v(3) << 24)
    }
    const LANES: usize = 4;
    /// A quarter of one channel step.
    const REST: f32 = 0.25;
    /// The four channels (straight RGBA8) as `0.0..=255.0`.
    #[inline(always)]
    fn parts(self) -> [f32; 4] {
        [0, 1, 2, 3].map(|i| ((self.0 >> (8 * i)) & 255) as f32)
    }
    /// Rounded and clamped per channel (as `add_scaled`).
    #[inline(always)]
    fn from_parts(p: [f32; 4]) -> Color {
        let v = |i: usize| p[i].round().clamp(0.0, 255.0) as u32;
        Color(v(0) | v(1) << 8 | v(2) << 16 | v(3) << 24)
    }
}

/// Easing of a transition's progress.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ease {
    Linear,
    InCubic,
    OutCubic,
    InOutCubic,
    /// C1 retargets: a cubic Hermite segment from the displayed (value, velocity) to (target, 0).
    Smooth,
}

impl Ease {
    #[inline]
    pub fn at(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Ease::Linear => t,
            Ease::InCubic => t * t * t,
            Ease::OutCubic => 1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t),
            Ease::InOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) * 0.5
                }
            }
            Ease::Smooth => t * t * (3.0 - 2.0 * t),
        }
    }
}

/// A transition's timing: duration (seconds) and easing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Curve {
    pub dur: f32,
    pub ease: Ease,
}

impl Curve {
    pub const fn new(dur: f32, ease: Ease) -> Curve {
        Curve { dur, ease }
    }
    pub const fn linear(dur: f32) -> Curve {
        Curve { dur, ease: Ease::Linear }
    }
    /// Ease-out cubic (zgui's tween).
    pub const fn out(dur: f32) -> Curve {
        Curve { dur, ease: Ease::OutCubic }
    }
    pub const fn in_out(dur: f32) -> Curve {
        Curve { dur, ease: Ease::InOutCubic }
    }
    /// Velocity-continuous retargets (Hermite).
    pub const fn smooth(dur: f32) -> Curve {
        Curve { dur, ease: Ease::Smooth }
    }
}

/// One running (or resting) transition of a property of node `node`.
struct State<T: Lerp> {
    node: u32,
    apply: Box<dyn Fn(&mut Rt, u32, &T)>,
    curve: Curve,
    from: T,
    to: T,
    /// Hermite: `from + velocity * dur` (the start tangent as a point).
    vel_to: T,
    t0: f64,
    shown: T,
    active: bool,
}

impl<T: Lerp> State<T> {
    /// The value at time `now`.
    fn value(&self, now: f64) -> T {
        let p = if self.curve.dur <= 0.0 { 1.0 } else { ((now - self.t0) / self.curve.dur as f64) as f32 };
        if p >= 1.0 {
            return self.to;
        }
        let p = p.max(0.0);
        if self.curve.ease == Ease::Smooth {
            // cubic Hermite: from h00 + (dur v) h10 + to h01 = lerp(from, to, h01) + (vel_to - from) h10
            let h01 = -2.0 * p * p * p + 3.0 * p * p;
            let h10 = p * p * p - 2.0 * p * p + p;
            return T::add_scaled(T::lerp(self.from, self.to, h01), self.vel_to, self.from, h10);
        }
        T::lerp(self.from, self.to, self.curve.ease.at(p))
    }
}

/// The type-erased half the frame clock drives.
trait Running {
    fn step(&mut self, now: f64) -> Option<(u32, *const u8)>;
    fn apply_shown(&self, rt: &mut Rt);
    fn is_active(&self) -> bool;
    fn as_any(&mut self) -> &mut dyn std::any::Any;
    /// Stop for good (its element was unmounted): whether it was running.
    fn retire(&mut self) -> bool {
        false
    }
}

impl<T: Lerp> Running for State<T> {
    fn step(&mut self, now: f64) -> Option<(u32, *const u8)> {
        if !self.active {
            return None;
        }
        let v = self.value(now);
        let p = if self.curve.dur <= 0.0 { 1.0 } else { (now - self.t0) / self.curve.dur as f64 };
        if p >= 1.0 {
            self.active = false;
        }
        if v == self.shown {
            return None;
        }
        self.shown = v;
        Some((self.node, &self.shown as *const T as *const u8))
    }
    fn apply_shown(&self, rt: &mut Rt) {
        (self.apply)(rt, self.node, &self.shown);
    }
    fn is_active(&self) -> bool {
        self.active
    }
    fn as_any(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Every transition of a runtime and the frame clock's time.
#[derive(Default)]
pub struct Transitions {
    items: Vec<Box<dyn Running>>,
    // A resting operand is absent. In mount order this is also the order in which its
    // writers previously ran when the clock traversed the entire operand table.
    running_ids: Vec<u32>,
    /// Seconds since the runtime started, advanced by `App::tick`.
    pub now: f64,
    pub active: u32,
    /// Retargets and steps so far (for the benchmarks).
    pub retargets: u64,
    pub steps: u64,
    /// Retired motions' ids, reused by the next motion mounted (`add_anim`).
    free: Vec<u32>,
}

impl Transitions {
    pub(crate) fn add<T: Lerp>(&mut self, node: u32, apply: Box<dyn Fn(&mut Rt, u32, &T)>, init: T, curve: Curve) -> u32 {
        self.items.push(Box::new(State { node, apply, curve, from: init, to: init, vel_to: init, t0: 0.0, shown: init, active: false }));
        (self.items.len() - 1) as u32
    }

    /// The transition `id` (of value type `T`) retargets to `to` at the current time, from the value it
    /// shows now (and, smooth, the velocity it has now).
    pub(crate) fn retarget<T: Lerp>(&mut self, id: u32, to: T) {
        let now = self.now;
        let Some(s) = self.items[id as usize].as_any().downcast_mut::<State<T>>() else { return };
        if s.to == to && s.active {
            return;
        }
        let cur = s.value(now);
        if s.curve.ease == Ease::Smooth && s.active {
            // the velocity now (a 1 ms difference), as the point `cur + velocity * dur`
            let h = 1e-3f64;
            let ahead = s.value(now + h);
            s.vel_to = T::add_scaled(cur, ahead, cur, (s.curve.dur as f64 / h) as f32);
        } else {
            s.vel_to = cur;
        }
        s.from = cur;
        s.to = to;
        s.t0 = now;
        if !s.active {
            s.active = true;
            self.running_ids.push(id);
            self.active += 1;
        }
        self.retargets += 1;
    }

    /// Any transition running?
    pub fn running(&self) -> bool {
        self.active > 0
    }

    /// The value transition `id` (mount order: the n-th transition operand lowered) shows now.
    pub fn shown<T: Lerp>(&mut self, id: u32) -> Option<T> {
        self.items.get_mut(id as usize)?.as_any().downcast_mut::<State<T>>().map(|s| s.shown)
    }

    /// Transitions created so far.
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl Rt {
    /// The frame clock: advance by `dt` seconds and apply every running transition's value (through its
    /// property's applier: paint writes, restyles). Returns whether any ran.
    pub fn step_transitions(&mut self, dt: f32) -> bool {
        self.transitions.now += dt as f64;
        if self.transitions.active == 0 {
            return false;
        }
        let now = self.transitions.now;
        let mut items = std::mem::take(&mut self.transitions.items);
        let mut active = std::mem::take(&mut self.transitions.running_ids);
        if !active.is_sorted() {
            active.sort_unstable();
        }
        active.retain(|&id| {
            let it = &mut items[id as usize];
            if !cfg!(feature = "mutant_skip_transition") && it.step(now).is_some() {
                it.apply_shown(self);
            }
            self.transitions.steps += 1;
            it.is_active()
        });
        // The appliers currently write paint and layout. Preserve the existing path
        // for appending any operands added while applying those writes.
        debug_assert!(self.transitions.running_ids.is_empty());
        items.append(&mut self.transitions.items);
        self.transitions.items = items;
        self.transitions.active = active.len() as u32;
        self.transitions.running_ids = active;
        true
    }
}

/// A transition operand: a wired signal (or a projection of one) whose changes are eased.
pub struct Transit<W> {
    pub(crate) w: W,
    pub(crate) curve: Curve,
}

impl<'a, T, V> Wired<'a, T, V> {
    /// This signal's changes transition along `curve` (any property the operand drives).
    pub fn transition(self, curve: Curve) -> Transit<Wired<'a, T, V>> {
        Transit { w: self, curve }
    }
}

impl<'a, T, V, F> WMap<'a, T, V, F> {
    /// The projection's changes transition along `curve`.
    pub fn transition(self, curve: Curve) -> Transit<WMap<'a, T, V, F>> {
        Transit { w: self, curve }
    }
}

/// # Safety
/// As every patch: `sig` points at a `Signal<T>`; the recipe holds the transition id.
pub(crate) unsafe fn patch_transit<T: Lerp>(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8) {
    // SAFETY: see the function contract.
    let s = unsafe { &*(sig as *const crate::signal::Signal<T>) };
    let id = unsafe { w.recipe.read_at::<u32>(0) };
    rt.transitions.retarget::<T>(id, **s);
}

/// # Safety
/// As `patch_transit`; the recipe holds the id then an `F` (at most 12 bytes).
unsafe fn patch_transit_map<T: 'static, P: Lerp, F: Fn(&T) -> P + Copy>(rt: &mut Rt, w: &Wire, sig: *const u8, _owner: *const u8) {
    // SAFETY: see the function contract.
    let s = unsafe { &*(sig as *const crate::signal::Signal<T>) };
    let id = unsafe { w.recipe.read_at::<u32>(0) };
    let f: F = unsafe { std::ptr::read_unaligned(w.recipe.as_ptr().add(4) as *const F) };
    rt.transitions.retarget::<P>(id, f(s));
}

fn apply_fn<K: Apply>(rt: &mut Rt, node: u32, v: &K::P) {
    K::apply(rt, node, v)
}

impl<'a, T: Lerp, V> IntoProp<T, V> for Transit<Wired<'a, T, V>> {
    fn lower<K: Apply<P = T>>(self, m: &mut Mounter<'_, V>, node: u32) -> T {
        let init = *self.w.get();
        let id = m.rt.transitions.add::<T>(node, Box::new(apply_fn::<K> as fn(&mut Rt, u32, &T)), init, self.curve);
        m.wire(&self.w, patch_transit::<T>, node, crate::rt::Recipe::of(id));
        init
    }
    /// At a re-description the wire keeps driving the transition; the value is its target.
    fn value(self) -> T {
        *self.w.get()
    }
}

impl<'a, T: 'static, V, F, P: Lerp> IntoProp<P, V> for Transit<WMap<'a, T, V, F>>
where
    F: Fn(&T) -> P + Copy + 'static,
{
    fn lower<K: Apply<P = P>>(self, m: &mut Mounter<'_, V>, node: u32) -> P {
        assert!(size_of::<F>() <= 12, "a transition's projection must capture at most 12 bytes");
        let init = (self.w.f)(self.w.w.get());
        let id = m.rt.transitions.add::<P>(node, Box::new(apply_fn::<K> as fn(&mut Rt, u32, &P)), init, self.curve);
        // F is at most 12 bytes (asserted): it fits after the id
        m.wire(&self.w.w, patch_transit_map::<T, P, F>, node, crate::rt::Recipe::of(id).with_at(4, self.w.f));
        init
    }
    fn value(self) -> P {
        (self.w.f)(self.w.w.get())
    }
}

// ---- motions: the prelude's `animate(..)` (`el/motion.rs`) ----
//
// A motion is a transition of the same table, stepped by the same frame clock (`step_transitions`, `App::tick`)
// and retargeted by its wire's patch, with two more timings than `State`: a damped spring and a delay. A spring
// is not integrated frame by frame: each segment is the closed form of the damped oscillator from the value and
// velocity it had when it was retargeted, so one timestamp gives one value at any cadence, and a segment knows
// when it is at rest (`rest`, from a bound of its envelope) instead of finding it out frame by frame.

/// A motion's timing as data: a spring or an eased curve, and a delay. Four plain words (a seal writes them).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Program {
    /// `PK_SPRING`, or `PK_CURVE | ease << 8`.
    pub(crate) kind: u32,
    /// Spring: the undamped angular rate (rad/s). Curve: the duration (s).
    pub(crate) a: f32,
    /// Spring: the damping ratio. Curve: 0.
    pub(crate) b: f32,
    /// Seconds a retarget holds the value before moving.
    pub(crate) delay: f32,
}
pub(crate) const PK_SPRING: u32 = 1;
pub(crate) const PK_CURVE: u32 = 2;

impl Program {
    pub(crate) fn spring(w0: f32, zeta: f32) -> Program {
        Program { kind: PK_SPRING, a: w0, b: zeta, delay: 0.0 }
    }
    pub(crate) fn curve(c: Curve) -> Program {
        let e = match c.ease {
            Ease::Linear => 0,
            Ease::InCubic => 1,
            Ease::OutCubic => 2,
            Ease::InOutCubic => 3,
            Ease::Smooth => 4,
        };
        Program { kind: PK_CURVE | e << 8, a: c.dur, b: 0.0, delay: 0.0 }
    }
    pub(crate) fn is_spring(&self) -> bool {
        self.kind == PK_SPRING
    }
    /// The curve of a curve program (a spring's: none).
    pub(crate) fn as_curve(&self) -> Option<Curve> {
        if self.kind & 0xff != PK_CURVE {
            return None;
        }
        let ease = match self.kind >> 8 {
            0 => Ease::Linear,
            1 => Ease::InCubic,
            2 => Ease::OutCubic,
            3 => Ease::InOutCubic,
            _ => Ease::Smooth,
        };
        Some(Curve { dur: self.a, ease })
    }
}

/// The spring's basis at local time `t` (seconds, `t >= 0`): `x(t) = to + A a + B v` and `x'(t) = A' a + B' v`
/// for a displacement `a` and a velocity `v` at `t = 0` (a unit-mass damped oscillator of rate `w0`, ratio `z`).
#[inline]
fn spring_basis(w0: f64, z: f64, t: f64) -> [f64; 4] {
    if z < 1.0 - 1e-4 {
        let s = z * w0;
        let wd = w0 * (1.0 - z * z).sqrt();
        let e = (-s * t).exp();
        let (sn, cs) = (wd * t).sin_cos();
        [e * (cs + s / wd * sn), e * sn / wd, -e * sn * w0 * w0 / wd, e * (cs - s / wd * sn)]
    } else if z <= 1.0 + 1e-4 {
        let e = (-w0 * t).exp();
        [e * (1.0 + w0 * t), t * e, -w0 * w0 * t * e, e * (1.0 - w0 * t)]
    } else {
        let root = w0 * (z * z - 1.0).sqrt();
        let (r1, r2) = (z * w0 - root, z * w0 + root);
        let (e1, e2) = ((-r1 * t).exp(), (-r2 * t).exp());
        let d = r1 - r2;
        [(r1 * e2 - r2 * e1) / d, (e2 - e1) / d, r1 * r2 * (e1 - e2) / d, (r1 * e1 - r2 * e2) / d]
    }
}

/// The local time from which a spring segment stays within `rest` of its target on every lane: `|x - to| <=
/// K e^(-σ t)` with `K`, `σ` from the segment's form (an upper bound, so the segment never rests early).
fn spring_rest(w0: f64, z: f64, a: &[f32; 4], v: &[f32; 4], lanes: usize, rest: f64) -> f64 {
    let mut t = 0.0f64;
    for i in 0..lanes {
        let (a, v) = (a[i] as f64, v[i] as f64);
        let (k, sigma) = if z < 1.0 - 1e-4 {
            let s = z * w0;
            let b = (v + s * a) / (w0 * (1.0 - z * z).sqrt());
            ((a * a + b * b).sqrt(), s)
        } else if z <= 1.0 + 1e-4 {
            // e^(-s t) |a + b t| <= (|a| + 2 |b| / (s e)) e^(-s t / 2)
            let b = v + w0 * a;
            (a.abs() + 2.0 * b.abs() / (w0 * std::f64::consts::E), w0 / 2.0)
        } else {
            let root = w0 * (z * z - 1.0).sqrt();
            let (r1, r2) = (z * w0 - root, z * w0 + root);
            let c2 = (v + r1 * a) / (r1 - r2);
            ((a - c2).abs() + c2.abs(), r1)
        };
        if k > rest {
            t = t.max(if sigma > 0.0 { (k / rest).ln() / sigma } else { f64::INFINITY });
        }
    }
    t.min(60.0)
}

/// One motion of a property of node `node` (what `State` is for a typed transition). An eased curve is evaluated
/// as `State` evaluates it, operation for operation, so an El curve presents what the typed one does; a spring and
/// `Curve::smooth` run on the value's components (`Lerp::parts`), which also carry the velocity a retarget keeps.
struct Anim<T: Lerp> {
    node: u32,
    apply: fn(&mut Rt, u32, &T),
    prog: Program,
    // the program, decoded (`set_prog`)
    mode: u8,
    ease: Ease,
    dur: f32,
    delay: f64,
    /// The segment: where it left (as a value and as components), its velocity then (components per second),
    /// where it goes, when it left, and the local time (after the delay) from which it is at rest.
    from_t: T,
    from: [f32; 4],
    vel: [f32; 4],
    to: [f32; 4],
    target: T,
    /// When the segment starts moving: the retarget's time plus the delay.
    start: f64,
    rest: f64,
    shown: T,
    active: bool,
    retired: bool,
}

/// `Anim::mode`.
const M_EASED: u8 = 0;
const M_SMOOTH: u8 = 1;
const M_SPRING: u8 = 2;

impl<T: Lerp> Anim<T> {
    fn set_prog(&mut self, prog: Program) {
        self.prog = prog;
        let c = prog.as_curve().unwrap_or(Curve::linear(0.0));
        self.mode = if prog.is_spring() {
            M_SPRING
        } else if c.ease == Ease::Smooth {
            M_SMOOTH
        } else {
            M_EASED
        };
        (self.ease, self.dur, self.delay) = (c.ease, c.dur, prog.delay as f64);
    }
    /// A spring's or a smooth curve's components and their velocities at local time `0 < tau < rest`.
    #[inline]
    fn lanes_at(&self, tau: f64) -> ([f32; 4], [f32; 4]) {
        let (mut x, mut v) = (self.from, [0.0f32; 4]);
        if self.mode == M_SPRING {
            let [a_, b_, ad, bd] = spring_basis(self.prog.a as f64, self.prog.b as f64, tau);
            for i in 0..T::LANES {
                let (a, u) = ((self.from[i] - self.to[i]) as f64, self.vel[i] as f64);
                x[i] = (self.to[i] as f64 + a_ * a + b_ * u) as f32;
                v[i] = (ad * a + bd * u) as f32;
            }
        } else {
            // cubic Hermite from (from, vel) to (to, 0)
            let p = ((tau / self.dur as f64) as f32).min(1.0);
            let (h01, h10) = (-2.0 * p * p * p + 3.0 * p * p, p * p * p - 2.0 * p * p + p);
            let (d01, d10) = (-6.0 * p * p + 6.0 * p, 3.0 * p * p - 4.0 * p + 1.0);
            for i in 0..T::LANES {
                let (span, tan) = (self.to[i] - self.from[i], self.vel[i] * self.dur);
                x[i] = self.from[i] + span * h01 + tan * h10;
                v[i] = (span * d01 + tan * d10) / self.dur;
            }
        }
        (x, v)
    }
    /// The value at `now`.
    #[inline]
    fn value(&self, now: f64) -> T {
        let tau = now - self.start;
        if self.mode == M_EASED {
            // as `State::value` (before the start `p` is negative and clamps to 0: the value it left)
            let p = if self.dur <= 0.0 { 1.0 } else { (tau / self.dur as f64) as f32 };
            if p >= 1.0 {
                return self.target;
            }
            return T::lerp(self.from_t, self.target, self.ease.at(p.max(0.0)));
        }
        if tau <= 0.0 {
            self.from_t
        } else if tau >= self.rest {
            self.target
        } else {
            T::from_parts(self.lanes_at(tau).0)
        }
    }
    /// (value, components, velocity) presented at `now` (an eased curve's: the value alone).
    #[inline]
    fn sample(&self, now: f64) -> (T, [f32; 4], [f32; 4]) {
        if self.mode == M_EASED {
            // an eased curve keeps no velocity (as `State`): it restarts from the value shown
            return (if self.active { self.value(now) } else { self.shown }, [0.0; 4], [0.0; 4]);
        }
        let tau = now - self.start;
        if !self.active || tau <= 0.0 {
            let x = if self.active { self.from_t } else { self.shown };
            return (x, x.parts(), [0.0; 4]);
        }
        if tau < self.rest {
            let (x, v) = self.lanes_at(tau);
            return (T::from_parts(x), x, v);
        }
        (self.target, self.target.parts(), [0.0; 4])
    }
    /// A segment from `(x, v)` at `now` to the target (an eased curve's: from `xt` alone).
    #[inline]
    fn launch(&mut self, now: f64, (xt, x, v): (T, [f32; 4], [f32; 4])) {
        (self.from_t, self.start) = (xt, now + self.delay);
        if self.mode == M_EASED {
            self.rest = self.dur.max(0.0) as f64;
            return;
        }
        (self.from, self.vel, self.to) = (x, v, self.target.parts());
        self.rest = if self.mode == M_SPRING {
            let a: [f32; 4] = std::array::from_fn(|i| x[i] - self.to[i]);
            spring_rest(self.prog.a as f64, self.prog.b as f64, &a, &v, T::LANES, T::REST as f64)
        } else {
            self.dur.max(0.0) as f64
        };
    }
}

impl<T: Lerp> Running for Anim<T> {
    fn step(&mut self, now: f64) -> Option<(u32, *const u8)> {
        if !self.active {
            return None;
        }
        let v = self.value(now);
        if now - self.start >= self.rest {
            self.active = false;
        }
        if v == self.shown {
            return None;
        }
        self.shown = v;
        Some((self.node, &self.shown as *const T as *const u8))
    }
    fn apply_shown(&self, rt: &mut Rt) {
        (self.apply)(rt, self.node, &self.shown);
    }
    fn is_active(&self) -> bool {
        self.active
    }
    fn as_any(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn retire(&mut self) -> bool {
        let was = self.active;
        (self.active, self.retired) = (false, true);
        was
    }
}

impl Transitions {
    /// A motion of node `node`'s property (`apply`), resting at `init`; its id (a retired motion's, when one is
    /// free).
    pub(crate) fn add_anim<T: Lerp>(&mut self, node: u32, apply: fn(&mut Rt, u32, &T), init: T, prog: Program) -> u32 {
        let p = init.parts();
        let mut a = Box::new(Anim {
            node,
            apply,
            prog,
            mode: M_EASED,
            ease: Ease::Linear,
            dur: 0.0,
            delay: 0.0,
            from_t: init,
            from: p,
            vel: [0.0; 4],
            to: p,
            target: init,
            start: self.now,
            rest: 0.0,
            shown: init,
            active: false,
            retired: false,
        });
        a.set_prog(prog);
        if let Some(id) = self.free.pop() {
            self.items[id as usize] = a;
            return id;
        }
        self.items.push(a);
        (self.items.len() - 1) as u32
    }

    /// Motion `id` heads for `to` from now, from the value and velocity it presents now. Publishing the target
    /// it already has does nothing (no segment, no frame).
    pub(crate) fn retarget_motion<T: Lerp>(&mut self, id: u32, to: T) {
        let now = self.now;
        let Some(s) = self.items[id as usize].as_any().downcast_mut::<Anim<T>>() else { return };
        if s.retired || s.target == to {
            return;
        }
        let from = if cfg!(feature = "mutant_motion_from_target") { (s.target, s.target.parts(), [0.0; 4]) } else { s.sample(now) };
        s.target = to;
        s.launch(now, from);
        if !s.active {
            s.active = true;
            self.running_ids.push(id);
            self.active += 1;
        }
        self.retargets += 1;
    }

    /// Motion `id` continues on `prog` from what it presents now (a re-description changed its timing).
    pub(crate) fn reprogram<T: Lerp>(&mut self, id: u32, prog: Program) {
        let now = self.now;
        let Some(s) = self.items[id as usize].as_any().downcast_mut::<Anim<T>>() else { return };
        if s.prog == prog || s.retired {
            return;
        }
        let (xt, _, v) = s.sample(now);
        s.set_prog(prog);
        if s.active {
            s.launch(now, (xt, xt.parts(), v));
        }
    }

    /// The value motion `id` presents.
    pub fn motion_shown<T: Lerp>(&mut self, id: u32) -> Option<T> {
        self.items.get_mut(id as usize)?.as_any().downcast_mut::<Anim<T>>().map(|s| s.shown)
    }

    /// Transition `id` stops for good (the element it drives was unmounted: 02 §6, a track drops with its
    /// position) and its id is free for the next motion.
    pub(crate) fn retire(&mut self, id: u32) {
        if cfg!(feature = "mutant_motion_keep_track") {
            return;
        }
        let Some(it) = self.items.get_mut(id as usize) else { return };
        if it.retire() {
            self.running_ids.retain(|&i| i != id);
            self.active -= 1;
        }
        self.free.push(id);
    }

    /// Motions mounted and not retired (lanes in use).
    pub fn live(&self) -> usize {
        self.items.len() - self.free.len()
    }
}

// ---- the same timings as values of the animation bank (`motion`) ----

impl Ease {
    /// This easing as a cubic Bézier. Exact for the three plain cubics and for `Smooth` as a curve (a
    /// cubic in `t` is a Bézier whose abscissae are thirds); the in-out cubic is two cubics, and gets the
    /// customary single curve.
    pub fn bezier(self) -> bough_paint::motion::CubicBezier {
        use bough_paint::motion::CubicBezier as B;
        const THIRD: f32 = 1.0 / 3.0;
        match self {
            Ease::Linear => B::LINEAR,
            Ease::InCubic => B::new(THIRD, 0.0, 2.0 * THIRD, 0.0),
            Ease::OutCubic => B::new(THIRD, 1.0, 2.0 * THIRD, 1.0),
            Ease::InOutCubic => B::new(0.65, 0.0, 0.35, 1.0),
            Ease::Smooth => B::new(THIRD, 0.0, 2.0 * THIRD, 1.0),
        }
    }
}
impl From<Curve> for crate::motion::AnimSpec {
    /// A curve's duration and easing as a motion of the animation bank. `Curve::smooth` promises that a
    /// retarget keeps the displayed velocity: that is a spring's property, so it becomes a critically
    /// damped spring that arrives in about the curve's duration.
    fn from(c: Curve) -> Self {
        match c.ease {
            Ease::Smooth => crate::motion::AnimSpec::response((c.dur * 0.6).max(0.01), 1.0),
            e => crate::motion::AnimSpec::ease((c.dur * 1000.0).round().max(0.0) as u32, bough_paint::motion::Ease::Bezier(e.bezier())),
        }
    }
}
