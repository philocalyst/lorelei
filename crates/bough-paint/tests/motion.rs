//! The motion gates: the clock, the Bézier solver, springs against an integrator, velocity across a
//! retarget, keyframe timelines, parking, and the tick's allocations.
use bough_paint::motion::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
}
struct Counting;
// SAFETY: defers to the system allocator; the count is a thread-local cell.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
        // SAFETY: the caller's contract.
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: the caller's contract.
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
        // SAFETY: the caller's contract.
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static A: Counting = Counting;
fn allocs() -> u64 {
    ALLOCS.with(|c| c.get())
}

fn ms(v: u64) -> ClockNs {
    ClockNs::from_ms(v)
}
struct Lcg(u64);
impl Lcg {
    fn f(&mut self, a: f32, b: f32) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        a + (b - a) * ((self.0 >> 40) as f32 / (1u64 << 24) as f32)
    }
}

// ---- the clock ----

#[test]
fn the_clock_does_not_wrap_and_its_tick_is_compared_modularly() {
    // the projection wraps; the clock does not
    let before = ms(u32::MAX as u64 - 150);
    let after = before.after_ms(400);
    assert!(after > before && after.tick() < before.tick(), "the tick wrapped: {} then {}", before.tick(), after.tick());
    assert_eq!(after.since(before), 400_000_000);
    assert_eq!(tick_elapsed(after.tick(), before.tick()), Some(400));
    assert_eq!(tick_elapsed(before.tick(), after.tick()), None, "a start in the future has not elapsed");
    assert!(tick_reached(after.tick(), before.tick()) && !tick_reached(before.tick(), after.tick()));
    assert!(tick_reached(7, 7) && tick_reached(5, u32::MAX - 5) && !tick_reached(u32::MAX - 5, 5));

    // a spring is a function of elapsed time: across the tick's wrap it is what it is anywhere else
    let run = |start: ClockNs| {
        let mut b = AnimBank::new();
        let t = b.track(0, 0.0);
        b.spring_to(t, SpringSpec::BOUNCY, 100.0, start);
        b.spring_to(t, SpringSpec::BOUNCY, -40.0, start.after_ms(90));
        let mut out = Vec::new();
        for k in 0..2000u64 {
            b.tick(start.after_ms(k), |_, v| out.push((k, v)));
        }
        (out, b.get(t), b.active())
    };
    let (a, ra, na) = run(ms(1000));
    let (w, rw, nw) = run(before);
    assert_eq!(a, w, "a spring across the tick's wrap differs from the same spring elsewhere");
    assert_eq!((ra, rw, na, nw), (-40.0, -40.0, 0, 0), "it settles exactly on its target and retires");
    assert!(a.len() > 300 && a.windows(2).all(|p| (p[1].1 - p[0].1).abs() < 3.0), "no snap between consecutive milliseconds");

    // the frame clock: it used to hold the maximum of a wrapping tick, and stopped at the wrap
    let mut c = FrameClock::starting_at(u32::MAX as u64 - 100);
    let (mut last, mut wrapped) = (c.clock(), false);
    let mut prev_tick = c.now();
    for f in 0..40u64 {
        let tick = c.present(5_000_000_000 + f * 16_666_667);
        assert!(c.clock() >= last);
        if f > 0 {
            assert!(matches!(tick_elapsed(tick, prev_tick), Some(16 | 17)), "frame {f}: the tick went from {prev_tick} to {tick}");
        }
        wrapped |= tick < prev_tick;
        (last, prev_tick) = (c.clock(), tick);
    }
    assert!(wrapped, "the test did not cross the wrap");
    assert_eq!(c.clock().ms(), u32::MAX as u64 - 100 + 39 * 16_666_667 / 1_000_000);
    // an output timestamp that goes back holds the clock
    let t = c.present(1);
    assert_eq!(t, prev_tick);

    // the raster's own spring (a wrapping tick in a record's words) across the wrap, and what settling is for
    let k = Response::SNAPPY;
    let t0 = u32::MAX - 60;
    let mut s = Spring::rest(0);
    s.retarget(k, t0, 25_600);
    let mut prev = s.value(k, t0);
    for e in 1..s.dur + 10 {
        let v = s.value(k, t0.wrapping_add(e));
        assert!((v - prev).abs() < 1200, "tick {e} past the wrap: {prev} -> {v}");
        prev = v;
    }
    assert_eq!(prev, 25_600);
    assert!(s.resting(t0.wrapping_add(s.dur)));
    // 2^31 ticks on, a modular clock cannot tell \"long over\" from \"yet to start\": the unsettled words read
    // as the start again; settled, there is no start to mistake
    let far = t0.wrapping_add(s.dur).wrapping_add(1 << 31);
    assert_eq!(s.value(k, far), 0, "(the hazard `Spring::settle` removes)");
    let mut settled = s;
    assert!(settled.settle(t0.wrapping_add(s.dur)) && !settled.settle(far));
    for at in [t0.wrapping_add(s.dur), far, far.wrapping_add(1 << 30), t0.wrapping_sub(5)] {
        assert_eq!(settled.value(k, at), 25_600);
    }
}

// ---- cubic Bézier ----

/// The curve at `x` by plain bisection in f64 (an independent, slow solver).
fn bezier_ref(b: CubicBezier, x: f64) -> f64 {
    let c = |p1: f64, p2: f64, t: f64| 3.0 * (1.0 - t) * (1.0 - t) * t * p1 + 3.0 * (1.0 - t) * t * t * p2 + t * t * t;
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..200 {
        let m = 0.5 * (lo + hi);
        if c(b.x1 as f64, b.x2 as f64, m) < x { lo = m } else { hi = m }
    }
    c(b.y1 as f64, b.y2 as f64, 0.5 * (lo + hi))
}

#[test]
fn cubic_bezier_is_accurate() {
    let presets = [CubicBezier::LINEAR, CubicBezier::EASE, CubicBezier::EASE_IN, CubicBezier::EASE_OUT, CubicBezier::EASE_IN_OUT, CubicBezier::EMPHASIZED, CubicBezier::DECELERATE, CubicBezier::ACCELERATE, CubicBezier::BACK_OUT];
    let mut g = Lcg(9);
    let mut curves: Vec<CubicBezier> = presets.to_vec();
    for _ in 0..300 {
        curves.push(CubicBezier::new(g.f(0.0, 1.0), g.f(-1.0, 2.0), g.f(0.0, 1.0), g.f(-1.0, 2.0)));
    }
    // the hard ones: vertical tangents at the ends, a flat middle
    curves.extend([CubicBezier::new(0.0, 1.0, 1.0, 0.0), CubicBezier::new(1.0, 0.0, 0.0, 1.0), CubicBezier::new(0.0, 0.0, 0.0, 1.0), CubicBezier::new(1.0, 0.0, 1.0, 1.0), CubicBezier::new(0.5, 0.0, 0.5, 1.0)]);
    let mut worst = 0f64;
    for b in &curves {
        assert_eq!((b.at(0.0), b.at(1.0), b.at(-3.0), b.at(7.0)), (0.0, 1.0, 0.0, 1.0));
        let mono = (0.0..=1.0).contains(&b.y1) && (0.0..=1.0).contains(&b.y2);
        let mut prev = 0.0f32;
        for i in 1..2000 {
            let x = i as f32 / 2000.0;
            let y = b.at(x);
            worst = worst.max((y as f64 - bezier_ref(*b, x as f64)).abs());
            if mono {
                assert!(y >= prev - 1e-6, "{b:?} is not monotone at {x}");
            }
            prev = y;
        }
        let (lo, hi) = b.y_range();
        assert!((0..=1000).all(|i| (lo - 1e-6..=hi + 1e-6).contains(&b.at(i as f32 / 1000.0))));
    }
    eprintln!("cubic bezier: {} curves x 2000 abscissae, worst error {worst:.2e}", curves.len());
    assert!(worst < 2e-6, "worst error {worst}");
    assert!((0..=100).all(|i| CubicBezier::LINEAR.at(i as f32 / 100.0) == i as f32 / 100.0));
    // the values every browser gives
    assert!((CubicBezier::EASE.at(0.5) - 0.802_403).abs() < 1e-5);
    assert!((CubicBezier::EASE_IN_OUT.at(0.5) - 0.5).abs() < 1e-6);
    assert!((CubicBezier::EASE_IN.at(0.5) + CubicBezier::EASE_OUT.at(0.5) - 1.0).abs() < 1e-6);
    assert!((CubicBezier::EASE_IN.at(0.5) - 0.315_357).abs() < 1e-5);
    // an overshooting curve overshoots
    assert!((0..100).any(|i| CubicBezier::BACK_OUT.at(i as f32 / 100.0) > 1.05));
}

// ---- springs ----

/// The spring's equation integrated by RK4 at 20 microsecond steps: (position, velocity) at `t` seconds.
fn integrate(spec: SpringSpec, x0: f64, v0: f64, to: f64, t: f64) -> (f64, f64) {
    let (k, c, m) = (spec.stiffness as f64, spec.damping as f64, spec.mass as f64);
    let f = |x: f64, v: f64| (v, (-k * (x - to) - c * v) / m);
    let n = (t / 2e-5).ceil().max(1.0) as usize;
    let h = t / n as f64;
    let (mut x, mut v) = (x0, v0);
    for _ in 0..n {
        let (a1, b1) = f(x, v);
        let (a2, b2) = f(x + 0.5 * h * a1, v + 0.5 * h * b1);
        let (a3, b3) = f(x + 0.5 * h * a2, v + 0.5 * h * b2);
        let (a4, b4) = f(x + h * a3, v + h * b3);
        x += h / 6.0 * (a1 + 2.0 * a2 + 2.0 * a3 + a4);
        v += h / 6.0 * (b1 + 2.0 * b2 + 2.0 * b3 + b4);
    }
    (x, v)
}

const SPECS: [(&str, SpringSpec); 8] = [
    ("snappy", SpringSpec::SNAPPY),
    ("bouncy", SpringSpec::BOUNCY),
    ("gentle", SpringSpec::GENTLE),
    ("stiff", SpringSpec::STIFF),
    ("underdamped, heavy", SpringSpec::new(120.0, 6.0, 2.5)),
    ("critical", SpringSpec::new(400.0, 40.0, 1.0)),
    ("overdamped", SpringSpec::new(90.0, 60.0, 1.0)),
    ("barely overdamped", SpringSpec::new(100.0, 20.5, 1.0)),
];

#[test]
fn springs_follow_their_equation_and_presets_mean_what_they_say() {
    for (name, r, z, s) in [("snappy", 0.3, 0.86, SpringSpec::SNAPPY), ("bouncy", 0.5, 0.55, SpringSpec::BOUNCY), ("gentle", 0.6, 1.0, SpringSpec::GENTLE), ("stiff", 0.2, 1.0, SpringSpec::STIFF)] {
        let f = SpringSpec::response(r, z);
        assert!((f.stiffness - s.stiffness).abs() < 0.02 && (f.damping - s.damping).abs() < 0.02 && s.mass == 1.0, "{name}: {f:?} vs {s:?}");
        let (_, zeta) = s.omega_zeta();
        assert!((zeta - z as f64).abs() < 1e-3, "{name}: damping ratio {zeta}");
    }
    let mut worst = 0f64;
    for (name, spec) in SPECS {
        for (x0, v0, to) in [(0.0, 0.0, 100.0), (50.0, -900.0, -20.0), (-3.0, 2500.0, 4.0), (1.0, 0.0, 0.0)] {
            let mut b = AnimBank::new();
            let t = b.track(0, x0 as f32);
            b.spring_with_velocity(t, spec, to as f32, v0, ms(500));
            let end = b.end(t).unwrap();
            let mut overshoot = false;
            for k in (0..3000u64).step_by(7) {
                let at = ms(500 + k);
                if at >= end {
                    assert_eq!(b.sample(t, at), to as f32, "{name}: from the settle time on the value is the target exactly");
                    continue;
                }
                let (x, v) = integrate(spec, x0, v0, to, k as f64 * 1e-3);
                let (gx, gv) = (b.sample(t, at) as f64, b.velocity(t, at));
                let scale = (x0 - to).abs() + v0.abs() * 0.1 + 1.0;
                worst = worst.max((gx - x).abs() / scale).max((gv - v).abs() / (scale * 20.0));
                overshoot |= (x0 - to) * (gx - to) < -1e-3 * scale;
            }
            if v0 == 0.0 {
                let (_, zeta) = spec.omega_zeta();
                assert_eq!(overshoot, zeta < 0.999, "{name}: a spring from rest overshoots exactly when it is underdamped");
            }
            // within the rest threshold from the settle time on, by the integrator too
            let (x, _) = integrate(spec, x0, v0, to, end.secs_since(ms(500)));
            assert!((x - to).abs() <= 1e-3, "{name}: {x} at its settle time, target {to}");
        }
    }
    eprintln!("springs: {} responses x 4 launches against RK4, worst relative error {worst:.2e}", SPECS.len());
    assert!(worst < 2e-5, "worst {worst}");
}

/// The jump in value and in velocity (per second) of track `t` across the instant `at`, where `change` is
/// applied: what a retarget must keep at zero.
fn jump(b: &mut AnimBank, t: TrackId, at: ClockNs, change: impl FnOnce(&mut AnimBank)) -> (f64, f64) {
    let (x, v) = (b.sample(t, at) as f64, b.velocity(t, at));
    change(b);
    ((b.sample(t, at) as f64 - x).abs(), (b.velocity(t, at) - v).abs())
}

#[test]
fn a_retarget_keeps_position_and_velocity() {
    let mut g = Lcg(4);
    let (mut worst_x, mut worst_v, mut fastest) = (0f64, 0f64, 0f64);
    for (name, spec) in SPECS {
        for _ in 0..40 {
            let mut b = AnimBank::new();
            let t = b.track(0, g.f(-50.0, 50.0));
            let mut now = ms(10_000);
            b.spring_to(t, spec, g.f(-300.0, 300.0), now);
            // interrupted again and again, mid-flight, onto other springs
            for _ in 0..6 {
                now = now.after_ns((g.f(0.004, 0.25) as f64 * 1e9) as u64);
                let (to, next) = (g.f(-300.0, 300.0), SPECS[(g.f(0.0, 7.99)) as usize].1);
                let v_before = b.velocity(t, now);
                let (dx, dv) = jump(&mut b, t, now, |b| b.spring_to(t, next, to, now));
                fastest = fastest.max(v_before.abs());
                (worst_x, worst_v) = (worst_x.max(dx), worst_v.max(dv / (1.0 + v_before.abs())));
                assert!(dx < 1e-4, "{name}: the value jumped by {dx} at a retarget");
                assert!(dv <= 1e-6 * (1.0 + v_before.abs()), "{name}: the velocity jumped by {dv} (it was {v_before}) at a retarget");
                // and the motion that follows is the equation's from that state
                let (x0, to64) = (b.sample(t, now) as f64, to as f64);
                let (x, _) = integrate(next, x0, v_before, to64, 0.05);
                assert!((b.sample(t, now.after_ms(50)) as f64 - x).abs() < 2e-3 * (1.0 + (x0 - to64).abs()), "{name}: after a retarget the spring left its equation");
            }
        }
    }
    eprintln!("spring retargets: {} mid-flight interruptions, worst value jump {worst_x:.1e}, worst relative velocity jump {worst_v:.1e}, fastest {fastest:.0} units/s", SPECS.len() * 40 * 6);
    assert!(fastest > 500.0, "the retargets were not mid-flight");

    // a tween in flight bends into a spring with the tween's velocity
    let mut b = AnimBank::new();
    let t = b.track(0, 0.0);
    b.tween_to(t, 200.0, Ease::Linear, Timing::once(400), ms(0));
    let (dx, dv) = jump(&mut b, t, ms(150), |b| b.spring_to(t, SpringSpec::SNAPPY, 0.0, ms(150)));
    assert!(dx < 1e-4 && dv < 0.5, "tween into spring: value jump {dx}, velocity jump {dv} (the tween moved at 500 units/s)");
    assert!((b.velocity(t, ms(150)) - 500.0).abs() < 0.5);

    // the negative control: a retarget that forgets the velocity (what `mutant_spring_velocity` does) is
    // seen by the same measure
    let mut b = AnimBank::new();
    let t = b.track(0, 0.0);
    b.spring_to(t, SpringSpec::BOUNCY, 100.0, ms(0));
    let v = b.velocity(t, ms(80));
    let (dx, dv) = jump(&mut b, t, ms(80), |b| b.spring_with_velocity(t, SpringSpec::BOUNCY, -100.0, 0.0, ms(80)));
    assert!(dx < 1e-4 && dv > 100.0 && (dv - v.abs()).abs() < 1e-6, "a velocity-forgetting retarget went unseen: jump {dv}, velocity {v}");
}

// ---- keyframes and timelines ----

#[test]
fn keyframes_repeat_alternate_pause_and_scrub() {
    let keys = [Key::new(0.0, 10.0, Ease::Linear), Key::new(0.25, 50.0, Ease::Hold), Key::new(0.5, 20.0, Ease::IN_OUT), Key::new(1.0, 100.0, Ease::Linear)];
    let mut b = AnimBank::new();
    let t = b.track(7, 0.0);
    let t0 = ms(1_000);
    b.keyframes(t, &keys, Timing::once(400).delay(100), t0);
    let at = |b: &AnimBank, m: f64| b.sample(t, ClockNs((t0.0 as f64 + (100.0 + m) * 1e6) as u64));
    // before the delay is over it holds its first value; then each stop at its time, each segment its ease
    assert_eq!((b.sample(t, t0), at(&b, -1.0), at(&b, 0.0)), (10.0, 10.0, 10.0));
    assert!((at(&b, 50.0) - 30.0).abs() < 1e-4 && (at(&b, 100.0) - 50.0).abs() < 1e-4);
    assert_eq!((at(&b, 150.0), at(&b, 199.0)), (50.0, 50.0), "a held segment keeps its first value");
    assert_eq!(at(&b, 200.0), 20.0);
    assert!((at(&b, 300.0) - 60.0).abs() < 1e-3 && at(&b, 250.0) < 40.0 - 5.0 && at(&b, 350.0) > 80.0 + 5.0, "ease-in-out is slow at both ends");
    assert_eq!((at(&b, 400.0), at(&b, 9_000.0)), (100.0, 100.0));
    assert_eq!(b.end(t), Some(t0.after_ms(500)));
    // a tick drives it and retires it on its last value
    let mut seen = Vec::new();
    for k in 0..=520u64 {
        b.tick(t0.after_ms(k), |tag, v| seen.push((tag, k, v)));
    }
    assert_eq!(seen.last(), Some(&(7, 500, 100.0)));
    assert_eq!((b.active(), b.get(t), b.animating()), (0, 100.0, false));

    // three iterations, there and back
    b.keyframes(t, &keys, Timing::once(400).repeat(Repeat::Count(3)).direction(Direction::Alternate), t0);
    let at = |b: &AnimBank, m: f64| b.sample(t, ClockNs((t0.0 as f64 + m * 1e6) as u64));
    assert!((at(&b, 50.0) - 30.0).abs() < 1e-4);
    assert!((at(&b, 400.0 + 350.0) - 30.0).abs() < 1e-3, "the second iteration runs backwards: {}", at(&b, 750.0));
    assert!((at(&b, 800.0 + 50.0) - 30.0).abs() < 1e-3, "the third forwards again");
    assert_eq!((at(&b, 1200.0), b.end(t)), (100.0, Some(t0.after_ms(1200))));
    b.keyframes(t, &keys, Timing::once(400).repeat(Repeat::Count(2)).direction(Direction::Alternate), t0);
    assert_eq!(at(&b, 800.0), 10.0, "an even count there and back ends where it began");
    b.keyframes(t, &keys, Timing::once(400).direction(Direction::Reverse), t0);
    assert_eq!((at(&b, 0.0), at(&b, 400.0)), (100.0, 10.0));
    assert!((at(&b, 350.0) - 30.0).abs() < 1e-3);
    // endless
    b.keyframes(t, &keys, Timing::once(400).repeat(Repeat::Infinite), t0);
    assert_eq!(b.end(t), None);
    assert!((at(&b, 400.0 * 1_000_000.0 + 50.0) - 30.0).abs() < 1e-2 && b.animating());

    // pause and resume: time stands still for the track, then goes on from there
    b.keyframes(t, &keys, Timing::once(400), t0);
    b.tick(t0.after_ms(50), |_, _| {});
    b.pause(t, t0.after_ms(50));
    let mut calls = 0;
    for k in 51..300 {
        b.tick(t0.after_ms(k), |_, _| calls += 1);
    }
    assert_eq!((calls, b.active(), b.get(t)), (0, 0, at(&b, 50.0)), "a paused track is evaluated once, then not at all");
    assert!((b.get(t) - 30.0).abs() < 1e-4 && b.paused(t));
    b.resume(t, t0.after_ms(300));
    b.tick(t0.after_ms(350), |_, _| {});
    assert!((b.get(t) - 50.0).abs() < 1e-4, "50 ms before the pause and 50 after are 100 ms of the track: {}", b.get(t));
    assert_eq!(b.end(t), Some(t0.after_ms(650)));

    // scrubbing is a function of the position alone, whatever the clock, in any order, even after the end
    let mut fresh = AnimBank::new();
    let f = fresh.track(0, 0.0);
    fresh.keyframes(f, &keys, Timing::once(400), ms(0));
    for (clock, pos) in [(9_999u64, 300.0f64), (10_000, 120.0), (10_001, 0.0), (20_000, 399.5), (5, 250.25), (6, 400.0), (7, 60.0)] {
        b.scrub(t, pos);
        b.tick(ms(clock), |_, _| {});
        assert_eq!(b.get(t), fresh.sample(f, ClockNs((pos * 1e6) as u64)), "scrubbed to {pos} ms at clock {clock}");
        assert_eq!(b.active(), 0);
    }
    // groups: a staggered timeline paused and scrubbed as one
    let mut b = AnimBank::new();
    let ids: Vec<TrackId> = (0..5).map(|i| b.track(i, 0.0)).collect();
    for (i, &id) in ids.iter().enumerate() {
        b.set_group(id, 3);
        b.tween_to(id, 1.0, Ease::Linear, Timing::once(200).delay(40 * i as u32), ms(0));
    }
    let lone = b.track(99, 0.0);
    b.tween_to(lone, 1.0, Ease::Linear, Timing::once(200), ms(0));
    b.scrub_group(3, 100.0);
    b.tick(ms(100), |_, _| {});
    assert!(ids.iter().all(|&id| b.get(id) == 0.5), "every track of the group at 100 ms of its own time");
    b.pause_group(3, ms(100));
    b.tick(ms(1000), |_, _| {});
    assert_eq!((b.get(ids[0]), b.get(lone)), (0.5, 1.0), "the group is held; the track outside it ran");
    b.resume_group(3, ms(1000));
    b.tick(ms(1050), |_, _| {});
    assert!(ids.iter().all(|&id| (b.get(id) - 0.75).abs() < 1e-5));
    // a stagger by start time: each spring holds until its own start
    let mut b = AnimBank::new();
    let ids: Vec<TrackId> = (0..4).map(|i| b.track(i, 0.0)).collect();
    for (i, &id) in ids.iter().enumerate() {
        b.spring_to_at(id, SpringSpec::SNAPPY, 10.0, ms(0), ms(30 * i as u64));
    }
    b.tick(ms(45), |_, _| {});
    assert!(b.get(ids[0]) > b.get(ids[1]) && b.get(ids[1]) > 0.0 && b.get(ids[2]) == 0.0 && b.get(ids[3]) == 0.0);
}

// ---- hulls, parking, and the tick's work ----

#[test]
fn hulls_bound_the_future_and_unseen_or_settled_tracks_cost_nothing() {
    // the hull at any instant bounds every later value
    let mut g = Lcg(21);
    let mut checked = 0;
    for round in 0..120 {
        let mut b = AnimBank::new();
        let t = b.track(0, g.f(-100.0, 100.0));
        let now = ms(5_000);
        match round % 3 {
            0 => b.spring_with_velocity(t, SPECS[round % 8].1, g.f(-200.0, 200.0), g.f(-3000.0, 3000.0) as f64, now),
            1 => b.tween_to(t, g.f(-200.0, 200.0), Ease::Bezier(CubicBezier::new(g.f(0.0, 1.0), g.f(-0.8, 1.8), g.f(0.0, 1.0), g.f(-0.8, 1.8))), Timing::once(300).delay(50), now),
            _ => {
                let keys: Vec<Key> = (0..5).map(|i| Key::new(i as f32 / 4.0, g.f(-150.0, 150.0), if i % 2 == 0 { Ease::OUT } else { Ease::Linear })).collect();
                b.keyframes(t, &keys, Timing::once(250).repeat(Repeat::Count(2)).direction(Direction::Alternate), now);
            }
        }
        for from in [0u64, 20, 90, 200, 450] {
            let (lo, hi) = b.hull(t, now.after_ms(from));
            for later in (from..from + 3_000).step_by(3) {
                let v = b.sample(t, now.after_ms(later));
                assert!(v >= lo && v <= hi, "round {round}: {v} at +{later} ms is outside the hull [{lo}, {hi}] taken at +{from} ms");
                checked += 1;
            }
        }
    }
    // a page far off-screen springing to another place off-screen: seen through a window it never enters
    let mut b = AnimBank::new();
    let off = b.track(1, 4_000.0);
    let on = b.track(2, 0.0);
    b.set_window(off, Some((-1280.0, 1280.0)));
    b.set_window(on, Some((-1280.0, 1280.0)));
    b.spring_to(off, SpringSpec::BOUNCY, 2_500.0, ms(0));
    b.spring_to(on, SpringSpec::BOUNCY, 300.0, ms(0));
    let end = b.end(off).unwrap();
    let mut seen_off = Vec::new();
    b.tick(ms(1), |tag, v| if tag == 1 { seen_off.push(v) });
    assert_eq!((b.parked(), b.active()), (1, 1), "the unseen track is parked, the seen one runs");
    let evals = b.evals;
    let mut k = 2;
    while ms(k) < end {
        b.tick(ms(k), |tag, v| if tag == 1 { seen_off.push(v) });
        k += 1;
    }
    let on_evals = b.evals - evals;
    assert!(seen_off.len() <= 1, "a parked track was handed to the sink {} times", seen_off.len());
    // every evaluation since was the seen track's: the parked one did no work at all
    let mut solo = AnimBank::new();
    let s = solo.track(2, 0.0);
    solo.spring_to(s, SpringSpec::BOUNCY, 300.0, ms(0));
    solo.tick(ms(1), |_, _| {});
    let base = solo.evals;
    for j in 2..k {
        solo.tick(ms(j), |_, _| {});
    }
    assert_eq!(on_evals, solo.evals - base, "the parked track was evaluated");
    // when its motion ends it is brought to its target, once
    b.tick(end.after_ms(1), |tag, v| if tag == 1 { seen_off.push(v) });
    assert_eq!((seen_off.last().copied(), b.get(off), b.parked()), (Some(2_500.0), 2_500.0, 0));
    // retargeted into view, it runs again, from where the closed form says it is
    b.spring_to(off, SpringSpec::SNAPPY, 100.0, end.after_ms(2));
    b.tick(end.after_ms(40), |_, _| {});
    assert!(b.parked() == 0 && b.get(off) < 2_500.0 && b.get(off) > 100.0);
    // parked forever: an endless loop nobody can see
    let lp = b.track(3, 9_000.0);
    b.set_window(lp, Some((0.0, 1.0)));
    b.keyframes(lp, &[Key::new(0.0, 9_000.0, Ease::Linear), Key::new(1.0, 9_500.0, Ease::Linear)], Timing::once(100).repeat(Repeat::Infinite).direction(Direction::Alternate), end);
    for j in 0..2_000 {
        b.tick(end.after_ms(50 + j), |_, _| {});
    }
    let evals = b.evals;
    for j in 2_000..4_000 {
        b.tick(end.after_ms(50 + j), |_, _| {});
    }
    assert_eq!((b.evals, b.active(), b.parked()), (evals, 0, 1), "everything is settled or parked: a tick is no work");
    eprintln!("motion hulls: {checked} future samples inside their hulls; a parked track cost 0 of {on_evals} evaluations");
}

#[test]
fn a_warm_tick_allocates_nothing() {
    let mut g = Lcg(2);
    let mut b = AnimBank::new();
    let keys = [Key::new(0.0, 0.0, Ease::EMPHASIZED), Key::new(0.4, 80.0, Ease::Hold), Key::new(1.0, -20.0, Ease::Linear)];
    let n = 3_000u64;
    let ids: Vec<TrackId> = (0..n).map(|i| b.track(i, g.f(-10.0, 10.0))).collect();
    for (i, &id) in ids.iter().enumerate() {
        match i % 5 {
            0 => b.spring_to(id, SPECS[i % 4].1, g.f(-500.0, 500.0), ms(0)),
            1 => b.tween_to(id, g.f(-5.0, 5.0), Ease::EASE, Timing::once(120 + (i as u32 % 400)).delay(i as u32 % 90), ms(0)),
            2 => b.keyframes(id, &keys, Timing::once(200).repeat(Repeat::Count(1 + i as u32 % 3)).direction(Direction::Alternate), ms(0)),
            3 => {
                // far outside its window: parks
                b.set(id, 50_000.0);
                b.set_window(id, Some((-100.0, 100.0)));
                b.spring_to(id, SpringSpec::GENTLE, 40_000.0 + i as f32, ms(0));
            }
            _ => b.keyframes(id, &keys, Timing::once(300).repeat(Repeat::Infinite), ms(0)),
        }
    }
    let mut sum = 0f64;
    let mut calls = 0u64;
    b.tick(ms(1), |_, v| sum += v as f64);
    let before = allocs();
    let mut k = 2;
    // every kind of event a tick has: values, ends, parking, wakes of parked tracks, held tracks
    while k < 4_000 {
        b.tick(ms(k), |_, v| {
            sum += v as f64;
            calls += 1;
        });
        k += 3;
    }
    let after = allocs();
    assert_eq!(after - before, 0, "{} allocations in {} warm ticks of {n} tracks", after - before, (4_000 - 2) / 3);
    assert!(calls > 100_000 && b.parked() == 0 && b.active() == n as usize / 5, "{calls} values; {} active, {} parked", b.active(), b.parked());
    // retargets and scrubs between ticks do not allocate either
    let before = allocs();
    for (i, &id) in ids.iter().enumerate().take(500) {
        b.spring_to(id, SPECS[i % 8].1, i as f32, ms(4_000));
        b.scrub(ids[i + 600], 33.0);
    }
    b.tick(ms(4_010), |_, v| sum += v as f64);
    assert_eq!(allocs() - before, 0, "a retarget or a scrub allocated");
    assert!(sum.is_finite());
}
