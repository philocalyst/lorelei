//! Round 3 independent review of motion (springs, settle, retarget) and media (video slot leases).
//!
//! Written without reading `tests/fx.rs`'s assertions beyond its scene vocabulary. Every test states the
//! property it pins; a test that currently FAILS says so in its message and in
//! `runtime/round3/motion-media-review.md`. CPU only (no Metal needed).
//!
//! Units: ticks are ms; spring values are Q8 px; velocity is Q8 px per tick; the ODE reference is
//! integrated in px and seconds (semi-implicit Euler, 1e-5 s steps).
use bough_kernel::cpu::Pool as Workers;
use bough_paint::expr::*;
use bough_paint::media::{Pool as Media, Synth};
use bough_paint::motion::{Response, Spring, SpringX};
use bough_paint::paint::*;
use bough_paint::scene::Scene;

type Sp = SpringX<852, 21_889>;
type Pan = Off<Lane, I<Uw<0>>, I<Uw<1>>>;
type Any = Pm<Rec>;
type Sprung = Off<Pan, Sp, Sp>;
type ADot = RRect<Sprung, I<Rec>, Any>;
type AVideo = Video<Pan, I<Rec>, Rec, Rec, Rec>;

const KS: Response = Response::SNAPPY;

fn q(v: f32) -> i32 {
    (v * 256.0).round() as i32
}
fn ri(v: i32) -> I<Rec> {
    I { a: Rec(v as u32) }
}
fn pan() -> Pan {
    Off { b: Lane, dx: I { a: Uw }, dy: I { a: Uw } }
}
fn sprung(x: &Spring, y: &Spring) -> Sprung {
    Off { b: pan(), dx: x.operand(), dy: y.operand() }
}

// ---------------------------------------------------------------------------------------------------
// The reference: the differential equation, in floats.
// ---------------------------------------------------------------------------------------------------

/// State (x px, v px/s) of x'' = st (to - x) - dm x', advanced by `ms` milliseconds.
fn ode(st: f64, dm: f64, mut x: f64, mut v: f64, to: f64, ms: u32) -> (f64, f64) {
    for _ in 0..ms * 100 {
        let a = st * (to - x) - dm * v;
        v += a * 1e-5;
        x += v * 1e-5;
    }
    (x, v)
}
/// Q8 per tick to px per second.
fn pps(v: f64) -> f64 {
    v * 1000.0 / 256.0
}

const RESPONSES: [(f64, f64); 7] = [
    (170.0, 26.0),                        // firm, barely overshoots
    (120.0, 8.0),                         // bouncy
    (300.0, 40.0),                        // quick
    (90.0, 30.0),                         // over-damped (zeta 1.58)
    (170.0, 26.07680_f64),                // critical (2 sqrt 170)
    (1500.0, 60.0),                       // stiff
    (20000.0, 200.0),                     // very stiff micro-interaction (w0 = 141 rad/s)
];

// ---------------------------------------------------------------------------------------------------
// Motion: velocity (peak and everywhere)
// ---------------------------------------------------------------------------------------------------

/// `Spring::velocity` is what `retarget` carries into the next motion. It must be the true velocity at
/// every tick of the motion and in particular at its peak.
#[test]
fn spring_velocity_is_right_at_every_tick_and_at_the_peak() {
    let mut fails = Vec::new();
    for (st, dm) in RESPONSES {
        let k = Response::new(st, dm);
        let mut s = Spring::rest(0);
        s.retarget(k, 0, 300 * 256);
        let (mut x, mut v) = (0.0f64, 0.0f64);
        let (mut true_peak, mut peak_at) = (0.0f64, 0u32);
        let mut table = Vec::new();
        for t in 0..=s.dur {
            table.push(v);
            if v.abs() > true_peak {
                (true_peak, peak_at) = (v.abs(), t);
            }
            (x, v) = ode(st, dm, x, v, 300.0, 1);
        }
        let _ = x;
        let (mut worst, mut worst_at) = (0.0f64, 0u32);
        let mut model_peak = 0.0f64;
        for t in 1..s.dur {
            let got = pps(s.velocity(k, t));
            model_peak = model_peak.max(got.abs());
            let err = (got - table[t as usize]).abs();
            if err > worst {
                (worst, worst_at) = (err, t);
            }
        }
        eprintln!("velocity ({st}, {dm}): dur {} ms, true peak {true_peak:.0} px/s at {peak_at} ms, model peak {model_peak:.0}, worst error {worst:.1} px/s ({:.2} % of peak) at {worst_at} ms", s.dur, 100.0 * worst / true_peak);
        // 4 % of the peak, plus a floor for the Q8 quantisation of a one-tick difference
        if worst > 0.04 * true_peak + 40.0 {
            fails.push(format!("({st}, {dm}): worst {worst:.1} px/s of peak {true_peak:.0} at {worst_at} ms"));
        }
    }
    assert!(fails.is_empty(), "velocity wrong: {fails:?}");
}

/// A spring that has just been launched with velocity `v` has velocity `v` at its first tick. (`velocity`
/// reports 0 for `now <= t0`: a second retarget in the same tick therefore drops all momentum.)
#[test]
fn a_second_retarget_in_the_same_tick_keeps_the_momentum() {
    for (st, dm) in RESPONSES {
        let k = Response::new(st, dm);
        let mut s = Spring::rest(0);
        s.retarget(k, 0, 300 * 256);
        // the tick of peak speed
        let (mut x, mut v, mut best, mut at) = (0.0f64, 0.0f64, 0.0f64, 1u32);
        for t in 0..s.dur {
            if v.abs() > best {
                (best, at) = (v.abs(), t);
            }
            (x, v) = ode(st, dm, x, v, 300.0, 1);
        }
        let _ = x;
        let at = at.max(2);
        // (a) one retarget at the peak
        let mut one = s;
        one.retarget(k, at, -200 * 256);
        // (b) two in the same tick: the first changes the target a little, the second is the real one
        let mut two = s;
        two.retarget(k, at, 100 * 256);
        let v_mid = two.velocity(k, at + 1);
        two.retarget(k, at, -200 * 256);
        let (v1, v2) = (one.velocity(k, at + 1), two.velocity(k, at + 1));
        eprintln!("same-tick retarget ({st}, {dm}) at peak tick {at}: first-retarget velocity {v_mid:.2}, then single {v1:.2} vs double {v2:.2} Q8/ms (peak {:.2})", pps(best) * 0.256 / 1000.0 * 1000.0 / 1000.0);
        let (x1, x2) = (one.value(k, at + 12), two.value(k, at + 12));
        assert!(
            (x1 - x2).abs() <= 2 * 256 / 2 && (v1 - v2).abs() <= 0.1 * v1.abs() + 1.0,
            "({st}, {dm}): a second retarget at tick {at} lost the momentum: velocity {v2:.2} instead of {v1:.2} Q8/tick, value 12 ticks on {} vs {} (Q8)",
            x2,
            x1
        );
    }
}

/// Retarget at the peak of the speed (and at 40 other instants): the continuing motion follows the
/// differential equation started from the true (position, velocity), within a pixel, tick by tick.
#[test]
fn retarget_follows_the_ode_from_the_true_state_every_tick() {
    let mut fails = Vec::new();
    for (st, dm) in RESPONSES {
        let k = Response::new(st, dm);
        let mut first = Spring::rest(0);
        first.retarget(k, 0, 300 * 256);
        let mut instants: Vec<u32> = (1..first.dur).step_by((first.dur as usize / 40).max(1)).collect();
        // include the peak
        let (mut x, mut v, mut best, mut pk) = (0.0f64, 0.0f64, 0.0f64, 1u32);
        for t in 0..first.dur {
            if v.abs() > best {
                (best, pk) = (v.abs(), t);
            }
            (x, v) = ode(st, dm, x, v, 300.0, 1);
        }
        let _ = x;
        instants.push(pk.max(1));
        for tr in instants {
            let (x0, v0) = ode(st, dm, 0.0, 0.0, 300.0, tr);
            let mut s = first;
            s.retarget(k, tr, -150 * 256);
            if s.dur == 0 {
                continue;
            }
            let (mut xr, mut vr, mut worst) = (x0, v0, 0.0f64);
            for i in 0..=s.dur.min(3000) {
                worst = worst.max((s.value(k, tr + i) as f64 / 256.0 - xr).abs());
                (xr, vr) = ode(st, dm, xr, vr, -150.0, 1);
            }
            if worst > 1.0 {
                fails.push(format!("({st}, {dm}) retarget at {tr} ms (speed {:.0} px/s): {worst:.2} px from the ODE", v0.abs()));
            }
        }
    }
    assert!(fails.is_empty(), "{} retargets left the ODE: {:#?}", fails.len(), &fails[..fails.len().min(12)]);
}

/// Retargeting a spring that is *in flight* with a start in the future (a stagger) replaces it from `at`
/// on; before `at` the new spring "rests where it started", which is the value it will have at `at`.
/// So the displayed value jumps now. The doc says the spring "must be at rest". This test pins the
/// documented precondition from the other side: at rest, a staggered retarget holds the old value until
/// `at`, for a big move AND for a move under 1/8 px (`launch` returns `dur == 0` there, and `value` of a
/// zero-duration spring is `to` at every instant, including before `at`).
#[test]
fn a_staggered_retarget_at_rest_holds_the_old_value_until_it_starts() {
    let k = KS;
    for (name, from, to) in [("big move", 100 * 256, -40 * 256), ("sub-eighth-pixel move", 100 * 256, 100 * 256 + 20)] {
        let mut s = Spring::rest(from);
        s.retarget(k, 1000, from); // a first, no-op retarget
        s.retarget(k, 2000, to); // staggered start 2000, issued at t = 1500
        let before = s.value(k, 1500);
        assert_eq!(before, from, "{name}: a spring that has not started moved before its start: {before} (was {from})");
        assert_eq!(s.value(k, 1999), from, "{name}: moved at 1999, a tick before its start");
        assert_ne!(s.value(k, 2020), from, "{name}: never moved");
    }
}

/// Retarget at rest, at the tick a spring settles and on either side: the value and velocity are exactly
/// the rest ones, and the new motion equals the one launched from a spring that was always at rest.
#[test]
fn retarget_at_rest_is_exact_around_the_settle_tick() {
    for (st, dm) in RESPONSES {
        let k = Response::new(st, dm);
        let mut s = Spring::rest(0);
        s.retarget(k, 500, 120 * 256);
        let end = s.t0 + s.dur;
        for at in [end - 1, end, end + 1, end + 5000] {
            let mut r = s;
            let (xv, vv) = (r.value(k, at), r.velocity(k, at));
            r.retarget(k, at, 120 * 256 + 300 * 256);
            let mut fresh = Spring::rest(120 * 256);
            fresh.retarget(k, at, 120 * 256 + 300 * 256);
            if at >= end {
                assert_eq!((xv, vv), (120 * 256, 0.0), "({st}, {dm}) at {at}: not at rest {xv} {vv}");
                assert_eq!(r, fresh, "({st}, {dm}) retarget at {at} (rest from {end}): not the motion of a spring that was always at rest");
            } else {
                // one tick before the end: within a quarter pixel of the target, and the new motion starts from there
                assert!((r.value(k, at) - xv).abs() <= 2, "({st}, {dm}) value jumped at {at}");
            }
            // a retarget to the very same target at rest does nothing visible and does not wake the scene
            let mut same = s;
            same.retarget(k, at, 120 * 256);
            if at >= end {
                assert_eq!(same.dur, 0, "({st}, {dm}): a same-target retarget at rest set a duration of {}", same.dur);
                assert_eq!(same.value(k, at), 120 * 256);
                assert_eq!(same.value(k, at + 77), 120 * 256);
            }
        }
    }
}

/// Launching from rest must start at rest. A critically damped response stores `b0 = c * 1024`, clamped to
/// +-2^21 in `launch` and again in the leaf: `c` (Q8 per tick) is capped at 2048, so any move over about
/// 600 px with stiffness 170 starts with a velocity the caller did not ask for.
#[test]
fn a_long_critically_damped_move_starts_at_rest_and_follows_the_ode() {
    let (st, dm) = (170.0, 2.0 * 170.0f64.sqrt());
    let k = Response::new(st, dm);
    assert_eq!(k.omega, 0, "not the critical form");
    let mut fails = Vec::new();
    for dist in [100.0f64, 400.0, 700.0, 1500.0] {
        let mut s = Spring::rest(0);
        s.retarget(k, 0, q(dist as f32));
        let first = (s.value(k, 1) - s.value(k, 0)) as f64 / 256.0; // px in the first ms; ODE: ~ w0^2 dist / 2 e-6
        let want = st * dist / 2.0 * 1e-6;
        let (mut xr, mut vr, mut worst) = (0.0f64, 0.0f64, 0.0f64);
        for t in 0..s.dur.min(4000) {
            worst = worst.max((s.value(k, t) as f64 / 256.0 - xr).abs());
            (xr, vr) = ode(st, dm, xr, vr, dist, 1);
        }
        eprintln!("critical {dist} px: first-ms travel {first:.3} px (ODE {want:.3}), worst error vs ODE {worst:.2} px, b0 {} a0 {}", s.b0, s.a0);
        if first.abs() > 4.0 * want + 0.05 || worst > 1.0 {
            fails.push(format!("{dist} px: first-ms travel {first:.2} px (ODE {want:.3}), worst error {worst:.2} px, b0 {}", s.b0));
        }
    }
    assert!(fails.is_empty(), "{fails:#?}");
}

// ---------------------------------------------------------------------------------------------------
// Motion in a scene: settle, active set, every frame
// ---------------------------------------------------------------------------------------------------

const FW: u32 = 128;
const FH: u32 = 96;

fn dot(sx: &Spring, sy: &Spring) -> ADot {
    RRect { b: sprung(sx, sy), r: ri(q(4.0)), c: Pm { c: Rec(0xff30_90e0) } }
}
const DOT_RECT: [i32; 4] = [20 * 256, 20 * 256, 36 * 256, 34 * 256];

fn fresh_frame(sx: &Spring, sy: &Spring, u: &U) -> Vec<u32> {
    let mut s = Scene::new(FW, FH, 1 << 16);
    let c = s.column::<ADot>(4);
    s.push(c, DOT_RECT, dot(sx, sy));
    let mut px = vec![0u32; (FW * FH) as usize];
    s.frame(u, &mut px, FW as usize, &Workers::new(1), 0);
    px
}

/// Drive a retained scene frame by frame at irregular cadences (1 ms to 60 ms, with a jump past the end),
/// retargeting at random frames including frames at rest. Every frame: equal to a fresh scene's frame;
/// `animating()` is true exactly while a spring is still before its end; the frame that reaches the end
/// lands exactly on the target; a frame after it visits no record and shades no tile.
#[test]
fn every_frame_of_a_retargeted_spring_equals_a_fresh_scene_and_settling_stops_the_scene() {
    let workers = Workers::new(1);
    for (case, (st, dm)) in [(170.0, 26.0), (120.0, 8.0), (90.0, 30.0)].into_iter().enumerate() {
        let k = Response::new(st, dm);
        // the scene is built with SNAPPY's operand constants (the response is part of the paint type), so
        // the spring words are launched with SNAPPY; the other responses are exercised in the unit tests above
        let _ = k;
        let k = KS;
        let mut g = bough_kernel::scenes::Rng(77 + case as u64);
        let (mut sx, mut sy) = (Spring::rest(0), Spring::rest(0));
        let mut s = Scene::new(FW, FH, 1 << 16);
        let c = s.column::<ADot>(4);
        s.push(c, DOT_RECT, dot(&sx, &sy));
        let mut px = vec![0u32; (FW * FH) as usize];
        let mut u = U::default();
        u.w[U_TIME] = 10_000;
        s.frame(&u, &mut px, FW as usize, &workers, 0);
        assert!(!s.animating(), "a scene at rest is animating");
        let mut now = 10_000u32;
        let (mut frames, mut moving_frames, mut landed) = (0u32, 0u32, 0u32);
        for step in 0..160 {
            // a turn: sometimes retarget on the frame about to be drawn (`at == now`), sometimes staggered
            let retarget = step % 7 == 3 || (step % 40 == 39);
            if retarget {
                let ax = (g.u(100) as i32 - 50) as f32;
                sx.retarget(k, now, q(ax));
                if g.u(3) == 0 {
                    sy.retarget(k, now + g.u(30), q((g.u(60) as i32 - 30) as f32));
                }
                s.set(c, 0, DOT_RECT, dot(&sx, &sy));
            }
            // the clock: usually a display frame, sometimes a stall, once a jump over every end
            let dt = match step % 11 {
                0 => 1,
                5 => 40 + g.u(20),
                10 if step > 100 => 5000,
                _ => 16 + (step % 2),
            };
            now += dt;
            u.w[U_TIME] = now;
            let fs = s.frame(&u, &mut px, FW as usize, &workers, 0);
            frames += 1;
            let want = fresh_frame(&sx, &sy, &u);
            let d = px.iter().zip(&want).filter(|(a, b)| a != b).count();
            assert_eq!(d, 0, "case {case} step {step} t={now}: {d} words differ from a fresh scene (x: t0 {} dur {} to {}; y: t0 {} dur {} to {})", sx.t0, sx.dur, sx.to, sy.t0, sy.dur, sy.to);
            let ahead = |sp: &Spring| sp.dur != 0 && (sp.t0.wrapping_add(sp.dur).wrapping_sub(now) as i32) > 0;
            let should = ahead(&sx) || ahead(&sy);
            assert_eq!(s.animating(), should, "case {case} step {step} t={now}: animating() {} but a spring {} (x ends {}, y ends {})", s.animating(), if should { "is still before its end" } else { "has reached its end" }, sx.t0 + sx.dur, sy.t0 + sy.dur);
            if should {
                moving_frames += 1;
            } else if sx.dur > 0 || sy.dur > 0 {
                // at rest: exactly on the targets
                assert_eq!((sx.value(k, now), sy.value(k, now)), (sx.to, sy.to));
                landed += 1;
            }
            if !should && !retarget {
                // one more time-only frame: nothing visited, nothing shaded
                u.w[U_TIME] = now + 7;
                let again = s.frame(&u, &mut px, FW as usize, &workers, 0);
                now += 7;
                assert_eq!((again.recs_visited, again.tiles), (0, 0), "case {case} step {step}: a frame after settling did work (visited {}, tiles {})", again.recs_visited, again.tiles);
                let _ = fs;
            }
        }
        eprintln!("case {case} ({st}, {dm}): {frames} frames, {moving_frames} while moving, {landed} landed at rest");
        assert!(moving_frames > 20 && landed > 5);
    }
}

// ---------------------------------------------------------------------------------------------------
// Media: slot lease lifetime
// ---------------------------------------------------------------------------------------------------

fn vid(slot: u32, w: u32, h: u32) -> AVideo {
    Video { b: pan(), r: ri(q(2.0)), slot: Rec(slot), dims: Rec(w | h << 16), op: Rec(32768) }
}
const VW: u32 = 64;
const VH: u32 = 48;
const VRECT: [i32; 4] = [8 * 256, 8 * 256, 72 * 256, 56 * 256];

/// The documented lifetime: a slot "stays immutable until the frame that stopped showing it has been
/// drawn". `publish(.., prev)` retires `prev` at publish time, but the record names `prev` until the
/// caller's `Scene::set`, and `reclaim()` frees every retired slot at the next draw boundary. A publish
/// that lands between two draws (a decoder thread; a storm), followed by any draw and its `reclaim`,
/// frees a slot a record still shows; the next lease overwrites it: the video tears.
#[test]
fn a_slot_a_record_still_names_is_never_reused_after_a_publish_storm() {
    let workers = Workers::new(1);
    let mut s = Scene::new(FW, FH, 1 << 16);
    s.media = Media::new(1 << 14);
    let c = s.column::<AVideo>(2);
    let mut src = Synth::new(VW, VH, 5);
    let a = src.next(&mut s.media).expect("pool");
    s.push(c, VRECT, vid(a, VW, VH));
    let mut u = U::default();
    let mut px = vec![0u32; (FW * FH) as usize];
    s.frame(&u, &mut px, FW as usize, &workers, 0);
    s.media.reclaim();
    let shown = px.clone();
    // storm: the source publishes frame after frame; the record is rewritten only when the turn runs
    let mut slots = vec![a];
    for _ in 0..3 {
        // (a draw boundary falls between publishes: e.g. a hover frame elsewhere)
        let nxt = src.next(&mut s.media).expect("pool");
        slots.push(nxt);
        u.w[U_TIME] += 16;
        s.frame(&u, &mut px, FW as usize, &workers, 0);
        s.media.reclaim();
    }
    // the record still names `a`; was it overwritten?
    s.invalidate();
    s.frame(&u, &mut px, FW as usize, &workers, 0);
    let torn = px.iter().zip(&shown).filter(|(x, y)| x != y).count();
    let reused = slots[1..].contains(&a);
    assert!(!reused && torn == 0, "slot {a} (still named by the record) was handed out again ({reused}); {torn} pixels of the video changed without a record write (slots handed out: {slots:?})");
}

/// A frame source that fails after `lease` (decoder error, cancelled seek) has no way to give the slot
/// back: `Lease` has no `Drop` and `Pool` no `abandon`. The slot stays `Leased` forever.
#[test]
fn an_abandoned_lease_is_not_lost_for_good() {
    let mut p = Media::new(1 << 12);
    let mut got = 0;
    for _ in 0..1000 {
        match p.lease(VW, VH) {
            Some(l) => {
                got += 1;
                drop(l); // decode failed: nothing published
                p.reclaim();
            }
            None => break,
        }
    }
    assert_eq!(got, 1000, "the pool ran dry after {got} abandoned leases with nothing shown ({:?} slots, {:?} words used)", p.stats().0, p.stats().1);
}

/// Adaptive streams change resolution. Free slots are reused only for exactly the same word count and are
/// never merged, so a pool that is entirely free can refuse a frame of another size.
#[test]
fn a_resolution_change_does_not_strand_a_pool_of_free_slots() {
    let mut p = Media::new(1 << 13);
    let mut shown: Option<u32> = None;
    let mut n = 0;
    // fill the pool with 64 x 48 frames, one shown at a time
    while let Some(l) = p.lease(VW, VH) {
        shown = Some(p.publish(l, n, shown));
        n += 1;
    }
    assert!(n >= 4, "pool too small to be a test ({n})");
    p.reclaim();
    // the last frame is shown; retire it too by publishing... there is no `retire`: free what we can.
    // Everything but one slot is Free. A larger frame must still fit: 96 x 54 needs at most 2 slots of 64 x 48 worth.
    let words = p.words().len();
    let larger = bough_paint::media::frame_words(96, 54);
    let old = bough_paint::media::frame_words(VW, VH);
    assert!(words - n as usize * old < larger, "unallocated tail would fit the larger frame");
    assert!(2 * old >= larger && n >= 3, "the first two adjacent retired slots must fit the larger frame");
    let l = p.lease(96, 54);
    assert!(l.is_some(), "no room for a 96 x 54 frame in a {words}-word pool holding one shown 64 x 48 frame and {} free slots ({:?} slots, {:?} words used)", n - 1, p.stats().0, p.stats().1);
}

/// A publish storm between draws needs a bounded number of slots: a frame that was published and
/// superseded before any draw was never shown, so nothing can be reading it.
#[test]
fn a_storm_between_draws_holds_a_bounded_number_of_slots() {
    let mut p = Media::new(1 << 16);
    let mut shown: Option<u32> = None;
    for n in 0..200u32 {
        let l = p.lease(VW, VH).expect("pool");
        shown = Some(p.publish(l, n, shown));
        if n % 50 == 49 {
            p.reclaim(); // one draw per 50 published frames
        }
    }
    let (slots, _) = p.stats();
    assert!(slots <= 4, "a 50-frame storm per draw grew the pool to {slots} slots (a shown frame, a pending one and one being written need 3)");
}

#[test]
fn backdrop_over_video_over_spring_matches_full_replay() {
    type Pane = Backdrop<Lane, Ki<3072>, Ki<1536>, Filt<K<256>, K<256>, K<256>>>;
    type Back = Solid<Lane, Pm<K<0xff14_2638>>>;
    const VIDEO_RECT: [i32; 4] = [33 * 256, 5 * 256, 97 * 256, 53 * 256];
    const GLASS_RECT: [i32; 4] = [21 * 256, 22 * 256, 109 * 256, 71 * 256];

    fn publish(s: &mut Scene, old: Option<u32>, seq: u32, y: u8) -> u32 {
        let lease = s.media.lease(VW, VH).expect("video lease");
        let (luma, chroma, _, _) = s.media.planes(&lease);
        luma.fill(y);
        chroma.fill(128);
        s.media.publish(lease, seq, old)
    }

    let mount = || {
        let mut s = Scene::new(FW, FH, 1 << 16);
        s.media = Media::new(1 << 14);
        let (b, d, v, g) = (s.column::<Back>(1), s.column::<ADot>(1), s.column::<AVideo>(1), s.column::<Pane>(1));
        s.push(b, [0, 0, FW as i32 * 256, FH as i32 * 256], Solid { b: Lane, c: Pm { c: K } });
        s.push(d, DOT_RECT, dot(&Spring::rest(0), &Spring::rest(0)));
        let slot = publish(&mut s, None, 0, 16);
        s.push(v, VIDEO_RECT, vid(slot, VW, VH));
        s.push(g, GLASS_RECT, Backdrop { b: Lane, r: Ki, blur: Ki, f: V3 { a: K, b: K, c: K } });
        (s, d, v, slot)
    };
    let mut worlds = [mount(), mount()];
    let words = (FW * FH) as usize;
    let mut surfaces = [vec![0; words], vec![0; words]];
    let pool = Workers::new(1);
    let (mut u, mut spring) = (U::default(), Spring::rest(0));
    let mut gray = 0;
    let mut prefix_jobs = 0;
    for tick in 0..96_u32 {
        let now = tick * 1000 / 120;
        u.w[U_TIME] = now;
        if tick % 24 == 1 || tick % 24 == 12 {
            let target = if tick % 24 == 1 { 23 * 256 } else { -8 * 256 };
            spring.retarget(KS, now, target);
        }
        for (i, (s, d, v, slot)) in worlds.iter_mut().enumerate() {
            if tick % 24 == 1 || tick % 24 == 12 {
                s.set(*d, 0, DOT_RECT, dot(&spring, &Spring::rest(0)));
            }
            if tick > 0 && tick % 4 == 0 {
                gray = if tick % 8 == 4 { 255 } else { 0 };
                let luma = if gray == 255 { 235 } else { 16 };
                *slot = publish(s, Some(*slot), tick / 4, luma);
                s.set(*v, 0, VIDEO_RECT, vid(*slot, VW, VH));
            }
            if i == 1 {
                s.invalidate();
            }
            let stats = s.frame(&u, &mut surfaces[i], FW as usize, &pool, 0);
            s.media.reclaim();
            if i == 0 && tick > 0 {
                prefix_jobs += stats.prefix_tiles;
            }
        }
        assert!(surfaces[0] == surfaces[1], "layered media differs at tick {tick}");
        // This pixel sees only the video. Neutral BT.709 black and white have exact byte values.
        assert_eq!(surfaces[0][10 * FW as usize + 82], 0xff00_0000 | gray * 0x0001_0101, "video witness at tick {tick}");
    }
    assert!(prefix_jobs > 0, "the motion and video must reach the backdrop");
}
