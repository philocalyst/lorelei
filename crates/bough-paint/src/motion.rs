//! Motion: springs as expressions of the clock, and the frame clock that paces them by the output.
//!
//! A spring here is not integrated frame by frame. It is the closed form of a damped oscillator
//! (`ops::x_spring`, integers only, the same words on every executor), so its value at any instant is a
//! pure function of (clock, six record words): the raster evaluates it for the frame it is drawing, at
//! whatever instant the output asks for, and a frame that was never drawn costs nothing and changes
//! nothing. The turn-side work is one call when the target changes ([`Spring::retarget`]): it reads the
//! value and velocity the spring has now and writes the six words of the motion that continues from them.
//! The motion ends: `dur` is the tick from which the leaf returns the target exactly, the expression's
//! `until` reports it, and the scene drops the record from its active set there (`Scene::animating`).
use crate::expr::{I, Ki, Rec, Spr, Uw};
use crate::ops::ops as x;

/// A spring's response: the envelope's decay per tick (Q16 nepers) and its angular rate (Q24 turns per
/// tick; 0: critically damped). Ticks are milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Response {
    pub decay: u32,
    pub omega: u32,
}
impl Response {
    /// From stiffness and damping per unit mass, in seconds (170, 26 is a firm spring that barely
    /// overshoots). Within 5 % of critical damping the response is taken as critical (the sine form's
    /// coefficient grows without bound there); beyond, it is the sum of its two decays.
    pub fn new(stiffness: f64, damping: f64) -> Response {
        let w0 = stiffness.max(1e-6).sqrt();
        let zeta = damping / (2.0 * w0);
        let q16 = |per_s: f64| ((per_s / 1000.0) * 65536.0).round().clamp(1.0, 65535.0) as u32;
        if zeta < 0.95 {
            let wd = w0 * (1.0 - zeta * zeta).sqrt();
            Response { decay: q16(zeta * w0), omega: ((wd / (2000.0 * std::f64::consts::PI)) * 16_777_216.0).round().clamp(1.0, 4_000_000.0) as u32 }
        } else if zeta <= 1.05 {
            Response { decay: q16(w0), omega: 0 }
        } else {
            let beta = w0 * (zeta * zeta - 1.0).sqrt();
            Response { decay: q16(zeta * w0 - beta), omega: 0x8000_0000 | q16(zeta * w0 + beta) }
        }
    }
    /// Stiffness 236, damping 26: quick, one small overshoot.
    pub const SNAPPY: Response = Response { decay: 852, omega: 21_889 };
}

/// A spring's six words: where it is going, when it left, when it rests, and its two starting coefficients.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Spring {
    pub t0: u32,
    pub dur: u32,
    /// The target, Q8.
    pub to: i32,
    pub a0: i32,
    pub b0: i32,
}
/// A spring as a paint operand: the clock is block word 63, the five words are the record's.
pub type SpringX<const DECAY: u32, const OMEGA: u32> = Spr<Uw<63>, Rec, Rec, I<Rec>, I<Rec>, I<Rec>, crate::expr::K<DECAY>, crate::expr::K<OMEGA>>;

impl Spring {
    /// At rest at `v` (Q8).
    pub fn rest(v: i32) -> Spring {
        Spring { t0: 0, dur: 0, to: v, a0: 0, b0: 0 }
    }
    /// The value (Q8) at `now`: the leaf the kernels run.
    pub fn value(&self, k: Response, now: u32) -> i32 {
        if self.dur == 0 {
            return self.to;
        }
        x::x_spring(now, self.t0, self.dur, self.to, self.a0, self.b0, k.decay, k.omega)
    }
    /// The velocity at `now`, Q8 per tick, as a central difference of the leaf (0 at rest). The difference
    /// spans four ticks either side where the motion has run that long (the leaf's trigonometry is good to
    /// about 1e-4 of the amplitude, which a two-tick difference would turn into velocity noise).
    pub fn velocity(&self, k: Response, now: u32) -> f64 {
        let e = now.wrapping_sub(self.t0);
        if self.dur == 0 || (e as i32) <= 0 || e >= self.dur {
            return 0.0;
        }
        let h = if e >= 4 && e + 4 < self.dur { 4 } else { 1 };
        (self.value(k, now.wrapping_add(h)) - self.value(k, now.wrapping_sub(h))) as f64 / (2 * h) as f64
    }
    pub fn resting(&self, now: u32) -> bool {
        self.dur == 0 || ((now.wrapping_sub(self.t0) as i32) >= 0 && now.wrapping_sub(self.t0) >= self.dur)
    }
    /// Once at rest, forget when the motion was: the six words become `Spring::rest(to)`. The leaf reads a
    /// wrapping tick, so a motion that ended more than 2^31 ticks ago (24.8 days) would look as if it had
    /// yet to start; a settled spring has no start to mistake. Returns whether the words changed (the
    /// owner rewrites its record then; the pixels are the same).
    pub fn settle(&mut self, now: u32) -> bool {
        if self.dur != 0 && self.resting(now) {
            *self = Spring::rest(self.to);
            return true;
        }
        false
    }
    /// Head for `to` (Q8) from `at` on, from the value and velocity the spring has at `at`: the motion in
    /// flight is not stopped, it bends. `at` may be later than now (a staggered start: until then the
    /// spring stays where it is, which must be at rest).
    pub fn retarget(&mut self, k: Response, at: u32, to: i32) {
        let (x0, v0) = (self.value(k, at), self.velocity(k, at));
        *self = Spring::launch(k, at, x0, v0, to);
    }
    /// A spring that leaves `from` (Q8) at `at` with velocity `v` (Q8 per tick) for `to`.
    pub fn launch(k: Response, at: u32, from: i32, v: f64, to: i32) -> Spring {
        let a = (from - to).clamp(-2_097_151, 2_097_151) as f64;
        let decay = k.decay as f64 / 65536.0;
        let rest = 32.0; // an eighth of a pixel: the step left at the settle tick
        let (b0, amp, a0) = if k.omega >= 0x8000_0000 {
            let r2 = (k.omega & 65535) as f64 / 65536.0;
            let c1 = (v + r2 * a) / (r2 - decay);
            (a - c1, c1.abs() + (a - c1).abs(), c1)
        } else if k.omega != 0 {
            let w = k.omega as f64 / 16_777_216.0 * 2.0 * std::f64::consts::PI;
            let b = (v + decay * a) / w;
            (b, a.abs() + b.abs(), a)
        } else {
            let c = v + decay * a;
            // (|a| + |c| t) e^(-decay t) peaks no higher than |a| + |c| / (e decay)
            (c * 1024.0, a.abs() + c.abs() / (std::f64::consts::E * decay), a)
        };
        if amp <= rest {
            return Spring { t0: at, dur: 0, to, a0: 0, b0: 0 };
        }
        let mut dur = ((amp / rest).ln() / decay).ceil();
        if k.omega == 0 {
            // the linear term keeps the envelope up a little longer: step until it is under the rest size
            let c = (b0 / 1024.0).abs();
            while dur < 60_000.0 && (a.abs() + c * dur) * (-decay * dur).exp() > rest {
                dur += 8.0;
            }
        }
        Spring { t0: at, dur: dur.clamp(1.0, 60_000.0) as u32, to, a0: a0.round().clamp(-2_097_151.0, 2_097_151.0) as i32, b0: b0.round().clamp(-2_097_151.0, 2_097_151.0) as i32 }
    }
    /// The spring as an operand with response `(DECAY, OMEGA)` (a `Response`'s fields as constants).
    pub fn operand<const DECAY: u32, const OMEGA: u32>(&self) -> SpringX<DECAY, OMEGA> {
        Spr { now: Uw, t0: Rec(self.t0), dur: Rec(self.dur), to: I { a: Rec(self.to as u32) }, a0: I { a: Rec(self.a0 as u32) }, b0: I { a: Rec(self.b0 as u32) }, decay: crate::expr::K, omega: crate::expr::K }
    }
}
/// A constant offset operand (for the axis a spring does not move).
pub type Still = Ki<0>;

/// Monotone time: nanoseconds since an origin the host chose. 64 bits do not wrap (584 years), so every
/// deadline and every elapsed time of the animation engine is an ordinary comparison or subtraction of
/// these. The raster's `Time` word is a **projection** ([`ClockNs::tick`]: milliseconds modulo 2^32) and is
/// only ever compared modularly ([`tick_elapsed`], [`tick_reached`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClockNs(pub u64);
impl ClockNs {
    pub const fn from_ms(ms: u64) -> ClockNs {
        ClockNs(ms * 1_000_000)
    }
    pub fn from_secs(s: f64) -> ClockNs {
        ClockNs((s.max(0.0) * 1e9).round() as u64)
    }
    pub const fn ms(self) -> u64 {
        self.0 / 1_000_000
    }
    /// The raster's tick: milliseconds modulo 2^32. Never compare two ticks with `<`: use
    /// [`tick_elapsed`] / [`tick_reached`].
    pub const fn tick(self) -> u32 {
        (self.0 / 1_000_000) as u32
    }
    pub const fn after_ms(self, ms: u64) -> ClockNs {
        ClockNs(self.0.saturating_add(ms.saturating_mul(1_000_000)))
    }
    pub const fn after_ns(self, ns: u64) -> ClockNs {
        ClockNs(self.0.saturating_add(ns))
    }
    /// Nanoseconds from `earlier` to this instant (0 when `earlier` is later).
    pub const fn since(self, earlier: ClockNs) -> u64 {
        self.0.saturating_sub(earlier.0)
    }
    /// Seconds from `earlier` to this instant, negative when `earlier` is later.
    pub fn secs_since(self, earlier: ClockNs) -> f64 {
        if self.0 >= earlier.0 { (self.0 - earlier.0) as f64 * 1e-9 } else { -((earlier.0 - self.0) as f64 * 1e-9) }
    }
}
/// Ticks from `t0` to `now` on the wrapping tick clock: `None` when `t0` is ahead of `now` (within 2^31
/// ticks either way, which is all a modular clock can say).
#[inline]
pub fn tick_elapsed(now: u32, t0: u32) -> Option<u32> {
    let e = now.wrapping_sub(t0);
    (e < 1 << 31).then_some(e)
}
/// `now` is at or past `deadline` on the wrapping tick clock.
#[inline]
pub fn tick_reached(now: u32, deadline: u32) -> bool {
    tick_elapsed(now, deadline).is_some()
}

/// The frame clock: the scene's `Time` word follows the output, not a timer.
///
/// The host calls [`FrameClock::present`] with the instant the output will show the frame being drawn
/// (a frame callback's timestamp, a display link's target, a predicted presentation time). The clock
/// returns the tick for the block's `Time` word. Between frames nothing advances: an animation therefore
/// runs at exactly the output's refresh, whatever it is, and when the scene reports that nothing is in
/// motion (`Scene::animating() == false`) the host requests no further callback and the clock stands
/// still, so idle costs no frames at all. Reduced motion pushes the clock 2^30 ticks ahead, which puts
/// every one-shot at its end (`ops::x_prog`, `ops::x_spring`).
///
/// The clock itself is 64 bits of milliseconds and monotone; the tick it hands the raster is its low 32
/// bits. (It used to keep the tick alone and hold its maximum, which stopped it for good the first time
/// the tick wrapped, 49.7 days in.)
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameClock {
    /// Output time (nanoseconds) of the first frame presented.
    origin: Option<u64>,
    /// Milliseconds the clock shows: `base` plus the output time since the origin, never going back.
    last: u64,
    base: u64,
    pub reduced_motion: bool,
    /// Frames presented, and frames asked for while nothing was in motion (should stay 0).
    pub frames: u64,
}
impl FrameClock {
    /// A clock whose first frame is at `ms` milliseconds (a restored session; a test at the tick's wrap).
    pub fn starting_at(ms: u64) -> FrameClock {
        FrameClock { base: ms, last: ms, ..Default::default() }
    }
    /// The tick for a frame the output shows at `output_ns`. Monotone: an output timestamp that goes back
    /// (a compositor restart) holds the clock rather than rewinding an animation.
    pub fn present(&mut self, output_ns: u64) -> u32 {
        let o = *self.origin.get_or_insert(output_ns);
        let t = self.base + output_ns.saturating_sub(o) / 1_000_000;
        self.last = self.last.max(t);
        self.frames += 1;
        self.now()
    }
    /// The tick of the last frame presented (what a turn stamps a transition's start with).
    pub fn now(&self) -> u32 {
        (self.last as u32).wrapping_add(if self.reduced_motion { 1 << 30 } else { 0 })
    }
    /// The clock as monotone time (what `AnimBank` is ticked with).
    pub fn clock(&self) -> ClockNs {
        ClockNs::from_ms(self.last)
    }
}

// ======================================================================================================
// The animation bank: springs, cubic Béziers and keyframe timelines as data, evaluated on the turn side.
//
// The typed path above puts a motion in a record's own words and lets the raster evaluate it (no CPU per
// frame, but the response is part of the archetype's type). The bank is the other half: a motion is a
// row, any property of any flat node can follow any of them, and a frame's work is one closed-form
// evaluation per track in motion. Every track is a pure function of monotone time (`ClockNs`), so a
// frame that was never drawn changes nothing, a retarget continues from the exact value and velocity,
// and any instant can be sampled again (`scrub`, an inspector's time travel).
// ======================================================================================================

/// A CSS `cubic-bezier(x1, y1, x2, y2)` easing: the curve from (0, 0) to (1, 1) with those control points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CubicBezier {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}
impl CubicBezier {
    /// `x1` and `x2` are clamped to [0, 1] (the curve must be a function of x).
    pub const fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> CubicBezier {
        CubicBezier { x1: x1.clamp(0.0, 1.0), y1, x2: x2.clamp(0.0, 1.0), y2 }
    }
    pub const LINEAR: CubicBezier = CubicBezier::new(0.0, 0.0, 1.0, 1.0);
    /// CSS `ease` (also UIKit's default curve).
    pub const EASE: CubicBezier = CubicBezier::new(0.25, 0.1, 0.25, 1.0);
    pub const EASE_IN: CubicBezier = CubicBezier::new(0.42, 0.0, 1.0, 1.0);
    pub const EASE_OUT: CubicBezier = CubicBezier::new(0.0, 0.0, 0.58, 1.0);
    pub const EASE_IN_OUT: CubicBezier = CubicBezier::new(0.42, 0.0, 0.58, 1.0);
    /// Material's emphasized curve (a quick start into a long settle).
    pub const EMPHASIZED: CubicBezier = CubicBezier::new(0.2, 0.0, 0.0, 1.0);
    /// A decelerating entrance and an accelerating exit.
    pub const DECELERATE: CubicBezier = CubicBezier::new(0.0, 0.0, 0.2, 1.0);
    pub const ACCELERATE: CubicBezier = CubicBezier::new(0.4, 0.0, 1.0, 1.0);
    /// An overshoot at the end (`y2 > 1`).
    pub const BACK_OUT: CubicBezier = CubicBezier::new(0.34, 1.56, 0.64, 1.0);

    #[inline(always)]
    fn poly(a1: f64, a2: f64, t: f64) -> f64 {
        (((1.0 - 3.0 * a2 + 3.0 * a1) * t + (3.0 * a2 - 6.0 * a1)) * t + 3.0 * a1) * t
    }
    #[inline(always)]
    fn slope(a1: f64, a2: f64, t: f64) -> f64 {
        (3.0 * (1.0 - 3.0 * a2 + 3.0 * a1) * t + 2.0 * (3.0 * a2 - 6.0 * a1)) * t + 3.0 * a1
    }
    /// The curve's parameter at abscissa `x` in [0, 1]: Newton's method from `x` (quadratic where the
    /// slope is healthy, a handful of steps to the last bit), and bisection to the last bit when a step
    /// leaves [0, 1], the slope vanishes or it has not converged.
    pub fn solve(&self, x: f64) -> f64 {
        let (x1, x2) = (self.x1 as f64, self.x2 as f64);
        let mut t = x;
        for _ in 0..12 {
            let err = Self::poly(x1, x2, t) - x;
            let d = Self::slope(x1, x2, t);
            if d.abs() < 1e-6 {
                break;
            }
            let next = t - err / d;
            if !(0.0..=1.0).contains(&next) {
                break;
            }
            if (next - t).abs() < 1e-14 {
                return next;
            }
            t = next;
        }
        // x(t) is monotone on [0, 1] for control abscissae in [0, 1]
        let (mut lo, mut hi) = (0.0f64, 1.0f64);
        for _ in 0..60 {
            t = 0.5 * (lo + hi);
            if Self::poly(x1, x2, t) < x { lo = t } else { hi = t }
        }
        0.5 * (lo + hi)
    }
    /// The eased progress at `x`: exactly 0 at 0 and before, exactly 1 at 1 and after.
    pub fn at(&self, x: f32) -> f32 {
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        if self.x1 == self.y1 && self.x2 == self.y2 {
            return x;
        }
        Self::poly(self.y1 as f64, self.y2 as f64, self.solve(x as f64)) as f32
    }
    /// Bounds of the eased progress over [0, 1] (the control polygon's: a curve with `y` beyond [0, 1]
    /// overshoots).
    pub fn y_range(&self) -> (f32, f32) {
        (0f32.min(self.y1).min(self.y2), 1f32.max(self.y1).max(self.y2))
    }
}

/// How a segment's progress is shaped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ease {
    Linear,
    Bezier(CubicBezier),
    /// Keep the segment's first value until its end (CSS `step-end`).
    Hold,
}
impl Ease {
    pub const EASE: Ease = Ease::Bezier(CubicBezier::EASE);
    pub const IN: Ease = Ease::Bezier(CubicBezier::EASE_IN);
    pub const OUT: Ease = Ease::Bezier(CubicBezier::EASE_OUT);
    pub const IN_OUT: Ease = Ease::Bezier(CubicBezier::EASE_IN_OUT);
    pub const EMPHASIZED: Ease = Ease::Bezier(CubicBezier::EMPHASIZED);
    #[inline]
    pub fn at(self, p: f32) -> f32 {
        match self {
            Ease::Linear => p.clamp(0.0, 1.0),
            Ease::Bezier(b) => b.at(p),
            Ease::Hold => (p >= 1.0) as u32 as f32,
        }
    }
    fn y_range(self) -> (f32, f32) {
        match self {
            Ease::Bezier(b) => b.y_range(),
            _ => (0.0, 1.0),
        }
    }
}

/// A spring's physics: `x'' = -(stiffness / mass) (x - target) - (damping / mass) x'`, in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringSpec {
    pub stiffness: f32,
    pub damping: f32,
    pub mass: f32,
}
impl SpringSpec {
    pub const fn new(stiffness: f32, damping: f32, mass: f32) -> SpringSpec {
        SpringSpec { stiffness, damping, mass }
    }
    /// From a response (the period of the undamped motion, seconds: how quick it feels) and a damping
    /// ratio (1: critically damped, no overshoot; below: bouncy; above: sluggish). Mass 1.
    pub fn response(response: f32, damping_ratio: f32) -> SpringSpec {
        let w = std::f32::consts::TAU / response.max(1e-3);
        SpringSpec { stiffness: w * w, damping: 2.0 * damping_ratio.max(0.0) * w, mass: 1.0 }
    }
    /// Quick, a trace of overshoot (response 0.3 s, ratio 0.86).
    pub const SNAPPY: SpringSpec = SpringSpec::new(438.65, 36.02, 1.0);
    /// A visible bounce (response 0.5 s, ratio 0.55).
    pub const BOUNCY: SpringSpec = SpringSpec::new(157.91, 13.82, 1.0);
    /// Slow and soft, no overshoot (response 0.6 s, critically damped).
    pub const GENTLE: SpringSpec = SpringSpec::new(109.66, 20.94, 1.0);
    /// Fast and critically damped (response 0.2 s): a control snapping into place.
    pub const STIFF: SpringSpec = SpringSpec::new(986.96, 62.83, 1.0);
    /// The undamped angular rate and the damping ratio.
    pub fn omega_zeta(&self) -> (f64, f64) {
        let m = self.mass.max(1e-6) as f64;
        let w0 = (self.stiffness.max(1e-6) as f64 / m).sqrt();
        (w0, self.damping.max(0.0) as f64 / (2.0 * m * w0))
    }
}

/// A spring launched: the closed form's coefficients about its target. Local time in seconds.
#[derive(Clone, Copy, Debug)]
struct Sprung {
    to: f64,
    /// 0: underdamped `e^(-s t) (a cos w t + b sin w t)`; 1: critical `e^(-s t) (a + b t)`;
    /// 2: overdamped `a e^(-s t) + b e^(-w t)` (`s` the slow rate).
    form: u8,
    s: f64,
    w: f64,
    a: f64,
    b: f64,
}
impl Sprung {
    fn launch(spec: SpringSpec, from: f64, v: f64, to: f64) -> Sprung {
        let (w0, zeta) = spec.omega_zeta();
        let a = from - to;
        if zeta < 1.0 - 1e-4 {
            let wd = w0 * (1.0 - zeta * zeta).sqrt();
            let s = zeta * w0;
            Sprung { to, form: 0, s, w: wd, a, b: (v + s * a) / wd }
        } else if zeta <= 1.0 + 1e-4 {
            Sprung { to, form: 1, s: w0, w: 0.0, a, b: v + w0 * a }
        } else {
            let root = w0 * (zeta * zeta - 1.0).sqrt();
            let (r1, r2) = (zeta * w0 - root, zeta * w0 + root);
            // x = c1 e^(-r1 t) + c2 e^(-r2 t): c1 + c2 = a, -r1 c1 - r2 c2 = v
            let c2 = (v + r1 * a) / (r1 - r2);
            Sprung { to, form: 2, s: r1, w: r2, a: a - c2, b: c2 }
        }
    }
    /// (value, velocity per second) at local time `t` (before 0: where it started, at rest).
    #[inline]
    fn at(&self, t: f64) -> (f64, f64) {
        if t < 0.0 {
            return (self.to + if self.form == 2 { self.a + self.b } else { self.a }, 0.0);
        }
        match self.form {
            0 => {
                let (e, (sn, cs)) = ((-self.s * t).exp(), (self.w * t).sin_cos());
                (self.to + e * (self.a * cs + self.b * sn), e * ((self.w * self.b - self.s * self.a) * cs - (self.s * self.b + self.w * self.a) * sn))
            }
            1 => {
                let e = (-self.s * t).exp();
                (self.to + e * (self.a + self.b * t), e * (self.b - self.s * (self.a + self.b * t)))
            }
            _ => {
                let (e1, e2) = ((-self.s * t).exp(), (-self.w * t).exp());
                (self.to + self.a * e1 + self.b * e2, -self.s * self.a * e1 - self.w * self.b * e2)
            }
        }
    }
    /// A bound of `|x - to|` from local time `t` on.
    #[inline]
    fn envelope(&self, t: f64) -> f64 {
        let t = t.max(0.0);
        match self.form {
            0 => (self.a * self.a + self.b * self.b).sqrt() * (-self.s * t).exp(),
            1 => {
                // (|a| + |b| t) e^(-s t) falls from t* = 1 / s - |a| / |b| on
                let peak = if self.b != 0.0 { (1.0 / self.s - self.a.abs() / self.b.abs()).max(t) } else { t };
                (self.a.abs() + self.b.abs() * peak) * (-self.s * peak).exp()
            }
            _ => self.a.abs() * (-self.s * t).exp() + self.b.abs() * (-self.w * t).exp(),
        }
    }
    /// The least and the greatest value from local time `t` on: exact, from the motion's turning points
    /// (an underdamped spring's alternate sides and shrink, so the next two bound all later ones; the
    /// other two forms turn at most once).
    fn range(&self, t: f64) -> (f64, f64) {
        let t = t.max(0.0);
        let x = self.at(t).0;
        let (mut lo, mut hi) = (x.min(self.to), x.max(self.to));
        let mut turn = |te: f64| {
            if te.is_finite() && te > t {
                let v = self.at(te).0;
                (lo, hi) = (lo.min(v), hi.max(v));
            }
        };
        match self.form {
            0 => {
                // velocity = e^(-s t) (p cos w t + q sin w t)
                let (p, q) = (self.w * self.b - self.s * self.a, -(self.s * self.b + self.w * self.a));
                let th = (-p).atan2(q);
                let k = ((self.w * t - th) / std::f64::consts::PI).floor() + 1.0;
                turn((th + k * std::f64::consts::PI) / self.w);
                turn((th + (k + 1.0) * std::f64::consts::PI) / self.w);
            }
            1 => {
                if self.b != 0.0 {
                    turn(1.0 / self.s - self.a / self.b);
                }
            }
            _ => {
                let arg = -(self.w * self.b) / (self.s * self.a);
                if self.a != 0.0 && arg > 0.0 {
                    turn(arg.ln() / (self.w - self.s));
                }
            }
        }
        (lo, hi)
    }
    /// The local time from which the motion is within `eps` of its target for good (at most a minute).
    fn settle(&self, eps: f64) -> f64 {
        if self.envelope(0.0) <= eps {
            return 0.0;
        }
        let rate = self.s.max(1e-6);
        let mut t = (self.envelope(0.0) / eps).ln() / rate;
        while self.envelope(t) > eps && t < 60.0 {
            t += 0.25 / rate;
        }
        t.min(60.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    Count(u32),
    Infinite,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Normal,
    Reverse,
    /// There and back: odd iterations run backwards.
    Alternate,
}
/// When a tween or a keyframe track runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timing {
    /// Before the first iteration (a stagger): the track holds its first value.
    pub delay_ms: u32,
    /// One iteration.
    pub dur_ms: u32,
    pub repeat: Repeat,
    pub direction: Direction,
}
impl Timing {
    pub const fn once(dur_ms: u32) -> Timing {
        Timing { delay_ms: 0, dur_ms, repeat: Repeat::Count(1), direction: Direction::Normal }
    }
    pub const fn delay(mut self, ms: u32) -> Timing {
        self.delay_ms = ms;
        self
    }
    pub const fn repeat(mut self, r: Repeat) -> Timing {
        self.repeat = r;
        self
    }
    pub const fn direction(mut self, d: Direction) -> Timing {
        self.direction = d;
        self
    }
}
/// One stop of a keyframe track: at fraction `t` of the iteration the value is `value`; `ease` shapes the
/// way from this stop to the next.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key {
    pub t: f32,
    pub value: f32,
    pub ease: Ease,
}
impl Key {
    pub const fn new(t: f32, value: f32, ease: Ease) -> Key {
        Key { t, value, ease }
    }
}

#[derive(Clone, Copy, Debug)]
enum Motion {
    Rest,
    Spring(Sprung),
    /// `n == 0`: a tween from `from` to `to` under `ease`; else `n` keys of the arena from `first`.
    Keys { first: u32, n: u32, from: f32, to: f32, ease: Ease, dur_ns: u64, repeat: Repeat, dir: Direction },
}

/// A track's handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TrackId(pub u32);

const NOWHERE: u32 = u32::MAX;
#[derive(Clone, Copy, Debug)]
struct Track {
    tag: u64,
    value: f32,
    motion: Motion,
    /// When local time is 0 (after the delay).
    t0: ClockNs,
    /// When the motion is over (`None`: never, an endless repeat; meaningless at rest or while held).
    end: Option<ClockNs>,
    /// Paused or scrubbed: the local time (ns) it is held at.
    held: Option<i64>,
    /// The values at which the track's subject can be seen (`AnimBank::set_window`).
    window: Option<(f32, f32)>,
    /// Its index in the active list, or `NOWHERE`.
    at: u32,
    parked: bool,
    /// Its value was written (`AnimBank::set`) and the sink has not been told.
    fresh: bool,
    group: u32,
    /// The key range the track owns in the arena (reused by later keyframe motions that fit).
    keys: (u32, u32),
}

/// Springs, tweens and keyframe timelines as rows. See the section's description.
pub struct AnimBank {
    tracks: Vec<Track>,
    active: Vec<u32>,
    parked: Vec<u32>,
    /// The earliest end among the parked tracks.
    wake: Option<ClockNs>,
    arena: Vec<Key>,
    /// How close to its target a spring is declared at rest (in the track's own unit).
    pub rest_epsilon: f32,
    /// Track evaluations so far (a parked or resting track adds none).
    pub evals: u64,
}
impl Default for AnimBank {
    fn default() -> Self {
        AnimBank::new()
    }
}

impl AnimBank {
    pub fn new() -> AnimBank {
        AnimBank { tracks: Vec::new(), active: Vec::new(), parked: Vec::new(), wake: None, arena: Vec::new(), rest_epsilon: 5e-4, evals: 0 }
    }
    /// A track at rest at `v`. `tag` names what it drives (`flat_scene::Prop::tag`); the tick hands it back.
    pub fn track(&mut self, tag: u64, v: f32) -> TrackId {
        self.tracks.push(Track { tag, value: v, motion: Motion::Rest, t0: ClockNs(0), end: None, held: None, window: None, at: NOWHERE, parked: false, fresh: false, group: 0, keys: (0, 0) });
        // the lists a tick edits never grow inside it
        self.active.reserve(self.tracks.len() - self.active.len());
        self.parked.reserve(self.tracks.len() - self.parked.len());
        TrackId(self.tracks.len() as u32 - 1)
    }
    pub fn len(&self) -> usize {
        self.tracks.len()
    }
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }
    /// The value the last tick (or write) left.
    pub fn get(&self, id: TrackId) -> f32 {
        self.tracks[id.0 as usize].value
    }
    pub fn tag(&self, id: TrackId) -> u64 {
        self.tracks[id.0 as usize].tag
    }
    /// Where the track is heading (its value at rest).
    pub fn target(&self, id: TrackId) -> f32 {
        let t = &self.tracks[id.0 as usize];
        match t.motion {
            Motion::Rest => t.value,
            Motion::Spring(s) => s.to as f32,
            Motion::Keys { .. } => self.final_value(t),
        }
    }
    /// Put the track in group `g` (0: none): `pause_group`, `resume_group`, `scrub_group` act on a group.
    pub fn set_group(&mut self, id: TrackId, g: u32) {
        self.tracks[id.0 as usize].group = g;
    }

    #[inline]
    fn local(t: &Track, now: ClockNs) -> i64 {
        t.held.unwrap_or(now.0 as i64 - t.t0.0 as i64)
    }
    fn key_at(&self, first: u32, n: u32, p: f32) -> f32 {
        let keys = &self.arena[first as usize..(first + n) as usize];
        if p <= keys[0].t {
            return keys[0].value;
        }
        for w in keys.windows(2) {
            if p < w[1].t {
                let span = w[1].t - w[0].t;
                let q = if span > 0.0 { (p - w[0].t) / span } else { 1.0 };
                return w[0].value + (w[1].value - w[0].value) * w[0].ease.at(q);
            }
        }
        keys[n as usize - 1].value
    }
    /// A keyed motion at local time `ns`: (value, finished).
    #[inline]
    fn keyed(&self, m: &Motion, start: f32, ns: i64) -> (f32, bool) {
        let Motion::Keys { first, n, from, to, ease, dur_ns, repeat, dir } = *m else { unreachable!() };
        let shape = |p: f32| if n == 0 { from + (to - from) * ease.at(p) } else { self.key_at(first, n, p) };
        let flip = |i: u64, p: f32| match dir {
            Direction::Normal => p,
            Direction::Reverse => 1.0 - p,
            Direction::Alternate => if i & 1 == 1 { 1.0 - p } else { p },
        };
        if ns < 0 {
            let _ = start;
            return (shape(flip(0, 0.0)), false);
        }
        let ns = ns as u64;
        let (i, p) = if dur_ns == 0 { (u64::MAX, 0.0) } else { (ns / dur_ns, (ns % dur_ns) as f64 / dur_ns as f64) };
        match repeat {
            Repeat::Count(c) if i >= c as u64 => (shape(flip((c as u64).saturating_sub(1), 1.0)), true),
            _ => (shape(flip(i, p as f32)), false),
        }
    }
    fn final_value(&self, t: &Track) -> f32 {
        match t.motion {
            Motion::Rest => t.value,
            Motion::Spring(s) => s.to as f32,
            Motion::Keys { repeat, .. } => match repeat {
                Repeat::Count(_) => self.keyed(&t.motion, t.value, i64::MAX).0,
                Repeat::Infinite => t.value,
            },
        }
    }
    /// (value, velocity per second, finished) of a track at local time `ns`.
    #[inline]
    fn state(&self, t: &Track, ns: i64, end: bool) -> (f32, f64, bool) {
        match t.motion {
            Motion::Rest => (t.value, 0.0, true),
            Motion::Spring(s) => {
                if end {
                    return (s.to as f32, 0.0, true);
                }
                let (x, v) = s.at(ns as f64 * 1e-9);
                (x as f32, v, false)
            }
            Motion::Keys { .. } => {
                let (v, done) = self.keyed(&t.motion, t.value, ns);
                (v, 0.0, done)
            }
        }
    }
    /// The track's value at `at`: a pure function of time (nothing is changed). Sampling a past or a
    /// future instant is how a timeline is inspected.
    pub fn sample(&self, id: TrackId, at: ClockNs) -> f32 {
        let t = &self.tracks[id.0 as usize];
        let end = t.held.is_none() && t.end.is_some_and(|e| at >= e);
        self.state(t, Self::local(t, at), end).0
    }
    /// The track's velocity at `at`, per second: analytic for a spring, a one-millisecond central
    /// difference for a tween or a keyframe track, 0 at rest or while held.
    pub fn velocity(&self, id: TrackId, at: ClockNs) -> f64 {
        let t = &self.tracks[id.0 as usize];
        if t.held.is_some() || t.end.is_some_and(|e| at >= e) {
            return 0.0;
        }
        let ns = Self::local(t, at);
        match t.motion {
            Motion::Rest => 0.0,
            Motion::Spring(s) => s.at(ns as f64 * 1e-9).1,
            Motion::Keys { .. } => {
                let h = 500_000i64;
                (self.keyed(&t.motion, t.value, ns + h).0 as f64 - self.keyed(&t.motion, t.value, ns - h).0 as f64) / (2.0 * h as f64 * 1e-9)
            }
        }
    }

    #[inline]
    fn activate(&mut self, id: u32) {
        let t = &mut self.tracks[id as usize];
        if t.parked {
            t.parked = false;
            if let Some(k) = self.parked.iter().position(|&p| p == id) {
                self.parked.swap_remove(k);
            }
        }
        if t.at == NOWHERE {
            t.at = self.active.len() as u32;
            self.active.push(id);
        }
    }
    #[inline]
    fn deactivate(&mut self, id: u32) {
        let at = self.tracks[id as usize].at;
        if at != NOWHERE {
            self.active.swap_remove(at as usize);
            if let Some(&moved) = self.active.get(at as usize) {
                self.tracks[moved as usize].at = at;
            }
            self.tracks[id as usize].at = NOWHERE;
        }
    }
    fn start(&mut self, id: TrackId, motion: Motion, t0: ClockNs, end: Option<ClockNs>) {
        let t = &mut self.tracks[id.0 as usize];
        (t.motion, t.t0, t.end, t.held) = (motion, t0, end, None);
        self.activate(id.0);
    }

    /// Put the track at `v`, at rest, now.
    pub fn set(&mut self, id: TrackId, v: f32) {
        let t = &mut self.tracks[id.0 as usize];
        (t.value, t.motion, t.held, t.end, t.fresh) = (v, Motion::Rest, None, None, true);
        // one evaluation hands the value to the sink and retires the track
        self.activate(id.0);
    }
    /// Head for `to` on a spring from `now`, **from the value and the velocity the track has now**: a
    /// motion in flight (a spring, a tween, a keyframe track) is not stopped, it bends.
    pub fn spring_to(&mut self, id: TrackId, spec: SpringSpec, to: f32, now: ClockNs) {
        let v = if cfg!(feature = "mutant_spring_velocity") { 0.0 } else { self.velocity(id, now) };
        self.spring_from(id, spec, to, v, now, now);
    }
    /// The same with the velocity given (per second): a gesture's release.
    pub fn spring_with_velocity(&mut self, id: TrackId, spec: SpringSpec, to: f32, velocity: f64, now: ClockNs) {
        self.spring_from(id, spec, to, velocity, now, now);
    }
    /// The same starting at `start` (a stagger): until then the track holds the value it has now.
    pub fn spring_to_at(&mut self, id: TrackId, spec: SpringSpec, to: f32, now: ClockNs, start: ClockNs) {
        if start <= now {
            return self.spring_to(id, spec, to, now);
        }
        self.spring_from(id, spec, to, 0.0, now, start);
    }
    fn spring_from(&mut self, id: TrackId, spec: SpringSpec, to: f32, v: f64, now: ClockNs, start: ClockNs) {
        let from = self.sample(id, now) as f64;
        let s = Sprung::launch(spec, from, v, to as f64);
        let settle = s.settle(self.rest_epsilon.max(1e-9) as f64);
        self.tracks[id.0 as usize].value = from as f32;
        self.start(id, Motion::Spring(s), start, Some(start.after_ns((settle * 1e9).ceil() as u64)));
    }
    fn timed(t: Timing, now: ClockNs) -> (ClockNs, u64, Option<ClockNs>) {
        let (t0, dur) = (now.after_ms(t.delay_ms as u64), t.dur_ms as u64 * 1_000_000);
        let end = match t.repeat {
            Repeat::Count(c) => Some(t0.after_ns(dur.saturating_mul(c as u64))),
            Repeat::Infinite if dur == 0 => Some(t0),
            Repeat::Infinite => None,
        };
        (t0, dur, end)
    }
    /// Go from the value the track has now to `to` under `ease`.
    pub fn tween_to(&mut self, id: TrackId, to: f32, ease: Ease, timing: Timing, now: ClockNs) {
        let from = self.sample(id, now);
        let (t0, dur_ns, end) = Self::timed(timing, now);
        self.tracks[id.0 as usize].value = from;
        self.start(id, Motion::Keys { first: 0, n: 0, from, to, ease, dur_ns, repeat: timing.repeat, dir: timing.direction }, t0, end);
    }
    /// Run a keyframe track: `keys` by ascending `t` in [0, 1]. (The keys are copied into the bank's
    /// arena here; a tick allocates nothing.)
    pub fn keyframes(&mut self, id: TrackId, keys: &[Key], timing: Timing, now: ClockNs) {
        assert!(!keys.is_empty() && keys.windows(2).all(|w| w[0].t <= w[1].t), "keyframes are given in order");
        let own = self.tracks[id.0 as usize].keys;
        let first = if own.1 as usize >= keys.len() {
            own.0
        } else {
            self.tracks[id.0 as usize].keys = (self.arena.len() as u32, keys.len() as u32);
            self.arena.resize(self.arena.len() + keys.len(), Key::new(0.0, 0.0, Ease::Linear));
            self.tracks[id.0 as usize].keys.0
        };
        self.arena[first as usize..first as usize + keys.len()].copy_from_slice(keys);
        let (t0, dur_ns, end) = Self::timed(timing, now);
        self.start(id, Motion::Keys { first, n: keys.len() as u32, from: 0.0, to: 0.0, ease: Ease::Linear, dur_ns, repeat: timing.repeat, dir: timing.direction }, t0, end);
    }

    // ---- pause, resume, scrub ----

    /// Hold the track where it is at `now` (a motion that is over stays over).
    pub fn pause(&mut self, id: TrackId, now: ClockNs) {
        let t = &mut self.tracks[id.0 as usize];
        if t.held.is_none() && !matches!(t.motion, Motion::Rest) && !t.end.is_some_and(|e| now >= e) {
            t.held = Some(Self::local(t, now));
            self.activate(id.0);
        }
    }
    /// Let a held track run on from where it is held.
    pub fn resume(&mut self, id: TrackId, now: ClockNs) {
        let t = &mut self.tracks[id.0 as usize];
        let Some(h) = t.held.take() else { return };
        let old = t.t0;
        t.t0 = ClockNs((now.0 as i64 - h).max(0) as u64);
        if let Some(e) = t.end.as_mut() {
            *e = ClockNs((e.0 as i64 + (t.t0.0 as i64 - old.0 as i64)).max(0) as u64);
        }
        self.activate(id.0);
    }
    /// Hold the track at local time `ms` (after its delay), whatever the clock says: deterministic.
    pub fn scrub(&mut self, id: TrackId, ms: f64) {
        let t = &mut self.tracks[id.0 as usize];
        if !matches!(t.motion, Motion::Rest) {
            t.held = Some((ms * 1e6).round() as i64);
            self.activate(id.0);
        }
    }
    pub fn paused(&self, id: TrackId) -> bool {
        self.tracks[id.0 as usize].held.is_some()
    }
    fn each_in(&mut self, g: u32, mut f: impl FnMut(&mut AnimBank, TrackId)) {
        for i in 0..self.tracks.len() as u32 {
            if g == 0 || self.tracks[i as usize].group == g {
                f(self, TrackId(i));
            }
        }
    }
    /// The same for every track of group `g` (0: every track).
    pub fn pause_group(&mut self, g: u32, now: ClockNs) {
        self.each_in(g, |b, id| b.pause(id, now));
    }
    pub fn resume_group(&mut self, g: u32, now: ClockNs) {
        self.each_in(g, |b, id| b.resume(id, now));
    }
    pub fn scrub_group(&mut self, g: u32, ms: f64) {
        self.each_in(g, |b, id| b.scrub(id, ms));
    }

    // ---- hulls and parking ----

    /// Bounds of the track's value from `now` until it rests: its motion hull.
    pub fn hull(&self, id: TrackId, now: ClockNs) -> (f32, f32) {
        let t = &self.tracks[id.0 as usize];
        let v = self.sample(id, now);
        if t.held.is_some() || t.end.is_some_and(|e| now >= e) {
            return (v, v);
        }
        match t.motion {
            Motion::Rest => (v, v),
            Motion::Spring(s) => {
                if t.end.is_some_and(|e| now >= e) {
                    return (v, v);
                }
                let (lo, hi) = s.range(now.secs_since(t.t0));
                // (computed in f64, handed out in f32: a unit of slack either side)
                let slack = |x: f64| (x.abs() * 1e-6 + 1e-6) as f32;
                ((lo as f32 - slack(lo)).min(v), (hi as f32 + slack(hi)).max(v))
            }
            Motion::Keys { first, n, from, to, ease, .. } => {
                let (mut lo, mut hi) = (v, v);
                let mut span = |a: f32, b: f32, e: Ease| {
                    let (y0, y1) = e.y_range();
                    for y in [y0, y1] {
                        let p = a + (b - a) * y;
                        (lo, hi) = (lo.min(p), hi.max(p));
                    }
                };
                if n == 0 {
                    span(from, to, ease);
                } else {
                    for w in self.arena[first as usize..(first + n) as usize].windows(2) {
                        span(w[0].value, w[1].value, w[0].ease);
                    }
                    let k = self.arena[first as usize];
                    (lo, hi) = (lo.min(k.value), hi.max(k.value));
                }
                (lo, hi)
            }
        }
    }
    /// Declare the values at which the track's subject can be seen (a page's offset while any of it is in
    /// the viewport; an opacity above nothing). While the track's whole hull lies outside, it is
    /// **parked**: no tick evaluates it, and it is brought to its final value when its motion ends.
    /// `None`: always evaluated.
    pub fn set_window(&mut self, id: TrackId, window: Option<(f32, f32)>) {
        self.tracks[id.0 as usize].window = window;
        if self.tracks[id.0 as usize].parked {
            self.activate(id.0);
        }
    }
    pub fn parked(&self) -> usize {
        self.parked.len()
    }
    /// Tracks a tick evaluates.
    pub fn active(&self) -> usize {
        self.active.len()
    }
    /// Some track is in motion (or parked with an end to come): the host owes the bank a tick at the
    /// output's next refresh (or, with nothing active, at `next_wake`).
    pub fn animating(&self) -> bool {
        !self.active.is_empty() || self.wake.is_some()
    }
    /// When the first parked track ends, if nothing else is in motion.
    pub fn next_wake(&self) -> Option<ClockNs> {
        self.wake
    }
    /// When the track's motion is (or was) over (`None`: never started, held, or endless).
    pub fn end(&self, id: TrackId) -> Option<ClockNs> {
        let t = &self.tracks[id.0 as usize];
        if matches!(t.motion, Motion::Rest) || t.held.is_some() { None } else { t.end }
    }

    /// Bring every track in motion to `now`; `sink(tag, value)` receives each value that changed. A track
    /// at rest, held or parked costs nothing; nothing is allocated. Returns [`AnimBank::animating`].
    pub fn tick(&mut self, now: ClockNs, mut sink: impl FnMut(u64, f32)) -> bool {
        // parked tracks whose motion ended while nobody looked: their final values, once
        if self.wake.is_some_and(|w| now >= w) {
            let mut next = None;
            let mut k = 0;
            while k < self.parked.len() {
                let id = self.parked[k];
                let t = self.tracks[id as usize];
                match t.end {
                    Some(e) if now >= e => {
                        let v = self.final_value(&t);
                        let t = &mut self.tracks[id as usize];
                        t.parked = false;
                        if v != t.value {
                            t.value = v;
                            sink(t.tag, v);
                        }
                        self.parked.swap_remove(k);
                    }
                    e => {
                        next = match (next, e) {
                            (Some(a), Some(b)) => Some(if a < b { a } else { b }),
                            (a, b) => a.or(b),
                        };
                        k += 1;
                    }
                }
            }
            self.wake = next;
        }
        let mut i = 0;
        while i < self.active.len() {
            let id = self.active[i];
            let t = self.tracks[id as usize];
            let ended = t.held.is_none() && t.end.is_some_and(|e| now >= e);
            let (v, _, done) = self.state(&t, Self::local(&t, now), ended);
            self.evals += 1;
            let done = done || ended;
            let v = if done && t.held.is_none() { self.final_value(&t) } else { v };
            if v != t.value || t.fresh {
                (self.tracks[id as usize].value, self.tracks[id as usize].fresh) = (v, false);
                sink(t.tag, v);
            }
            if t.held.is_some() {
                // held: evaluated once where it is held, then no more until it is resumed or scrubbed
                self.deactivate(id);
                continue;
            }
            if done {
                // (the motion stays in the row: an ended track can still be sampled and scrubbed)
                self.deactivate(id);
                continue;
            }
            if let Some((lo, hi)) = t.window {
                let (a, b) = self.hull(TrackId(id), now);
                if b < lo || a > hi {
                    self.deactivate(id);
                    self.tracks[id as usize].parked = true;
                    self.parked.push(id);
                    if let Some(e) = t.end {
                        self.wake = Some(self.wake.map_or(e, |w| if e < w { e } else { w }));
                    }
                    continue;
                }
            }
            i += 1;
        }
        self.animating()
    }
}
