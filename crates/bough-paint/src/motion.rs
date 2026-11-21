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

/// The frame clock: the scene's `Time` word follows the output, not a timer.
///
/// The host calls [`FrameClock::present`] with the instant the output will show the frame being drawn
/// (a frame callback's timestamp, a display link's target, a predicted presentation time). The clock
/// returns the tick for the block's `Time` word. Between frames nothing advances: an animation therefore
/// runs at exactly the output's refresh, whatever it is, and when the scene reports that nothing is in
/// motion (`Scene::animating() == false`) the host requests no further callback and the clock stands
/// still, so idle costs no frames at all. Reduced motion pushes the clock 2^30 ticks ahead, which puts
/// every one-shot at its end (`ops::x_prog`, `ops::x_spring`).
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameClock {
    /// Output time (nanoseconds) of tick 0, once the first frame has been presented.
    origin: Option<u64>,
    last: u32,
    pub reduced_motion: bool,
    /// Frames presented, and frames asked for while nothing was in motion (should stay 0).
    pub frames: u64,
}
impl FrameClock {
    /// The tick for a frame the output shows at `output_ns`. Monotone: an output timestamp that goes back
    /// (a compositor restart) holds the clock rather than rewinding an animation.
    pub fn present(&mut self, output_ns: u64) -> u32 {
        let o = *self.origin.get_or_insert(output_ns);
        let t = (output_ns.saturating_sub(o) / 1_000_000) as u32;
        self.last = self.last.max(t);
        self.frames += 1;
        self.now()
    }
    /// The tick of the last frame presented (what a turn stamps a transition's start with).
    pub fn now(&self) -> u32 {
        self.last.wrapping_add(if self.reduced_motion { 1 << 30 } else { 0 })
    }
}
