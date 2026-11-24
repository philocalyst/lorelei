//! Motion as a plug (`designs/reactivity/final/03-ports.md` §4; card in
//! `docs/lanes/integration/2026-11-22-el-surface.md`, "Motion as a plug, map recipes, hover colours"), and two
//! re-description fixes: a map whose captures changed, a hover colour changed while hovered.
//!
//! Negative controls (each must fail a test here):
//! * `--features mutant_motion_from_target`: a retarget starts from the old target, not from what is presented
//!   (`a_retarget_mid_flight_is_continuous`).
//! * `--features mutant_motion_snap`: a re-description applies a motion port's target over what it presents
//!   (`a_motion_port_reruns_no_view`, its re-described half).
//! * `--features mutant_motion_keep_track`: an unmounted motion keeps its track (`a_branch_flip_drops_the_track`).
//! * `--features mutant_el_recipe`: a re-described map's wire keeps the captures it was mounted with
//!   (`a_map_whose_captures_changed_is_right_at_once`).
//! * `--features mutant_el_hover`: a hover colour changed while hovered waits for the next hover
//!   (`a_hover_colour_changed_while_hovered_shows_at_once`).
use bough::prelude::*;
use bough::raw::{App, Config};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

/// Allocator calls made by this thread (tests run on several threads: each counts its own).
struct Counting;
thread_local! {
    static CALLS: Cell<usize> = const { Cell::new(0) };
}
// SAFETY: forwards to `System` unchanged; the counter is not part of the allocation contract.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let _ = CALLS.try_with(|c| c.set(c.get() + 1));
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let _ = CALLS.try_with(|c| c.set(c.get() + 1));
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let _ = CALLS.try_with(|c| c.set(c.get() + 1));
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static ALLOC: Counting = Counting;
fn calls() -> usize {
    // (the prelude's `With::with` is implemented for every type, so the `LocalKey` method is named in full)
    std::thread::LocalKey::with(&CALLS, Cell::get)
}

fn font() -> Vec<u8> {
    std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap()
}
fn cfg() -> Config {
    Config::new(200.0, 160.0, 1.0, font())
}
fn shot<V: bough::app::View>(app: &mut App<V>) -> Vec<u8> {
    app.frame();
    let p = app.presented_pixels();
    assert_eq!(p, app.reference_pixels(), "the repaired frame equals a full redraw");
    p
}

/// One frame of `dt` seconds.
fn tick<V: bough::app::View>(app: &mut App<V>, dt: f32) {
    app.tick(dt);
    app.frame();
}

/// Ticks until nothing moves (at most `limit` seconds of frames).
fn settle<V: bough::app::View>(app: &mut App<V>, dt: f32, limit: f32) {
    let mut t = 0.0;
    while app.animating() {
        assert!(t < limit, "a motion still runs after {limit} s");
        tick(app, dt);
        t += dt;
    }
}

// ------------------------------------------------------------------------------------------- the scene

#[derive(Clone, Copy, PartialEq, Debug)]
enum Timing {
    Spring,
    Smooth,
    Eased,
    Delayed,
}

struct Pop {
    reveal: Signal<f32>,
    width: Signal<f32>,
    tint: Signal<Color>,
    timing: Timing,
    views: Cell<u32>,
}
fn pop(reveal: f32, width: f32, tint: Color, timing: Timing) -> Pop {
    Pop { reveal: Signal::new(reveal), width: Signal::new(width), tint: Signal::new(tint), timing, views: Cell::new(0) }
}

impl View for Pop {
    fn view(&self) -> El<'_> {
        self.views.set(self.views.get() + 1);
        let base = animate(&self.reveal);
        let reveal = match self.timing {
            Timing::Spring => base.spring(Spring::snappy()),
            Timing::Smooth => base.curve(Curve::smooth(0.3)),
            Timing::Eased => base.over(300),
            Timing::Delayed => base.spring(Spring::bouncy()).delay(100),
        };
        column((
            div().size(80, 40).bg(rgb(0x2050a0)).opacity(reveal).translate_y(reveal.reverse().scale(8)),
            div().h(20).w(animate(&self.width).over(200)).bg(animate(&self.tint).spring(Spring::gentle())),
            text("ink").fg(animate(&self.tint).ease(Ease::InOutCubic)),
        ))
        .gap(4)
        .bg(rgb(0xffffff))
        .w_pct(1.0)
        .h_pct(1.0)
    }
}

const ALL: [Timing; 4] = [Timing::Spring, Timing::Smooth, Timing::Eased, Timing::Delayed];
const DT: f32 = 1.0 / 64.0;

#[test]
fn the_settled_frame_equals_a_fresh_mount_at_the_target() {
    for timing in ALL {
        for once in [true, false] {
            let c = if once { cfg().once() } else { cfg() };
            let mut app = App::new(pop(0.0, 40.0, rgb(0x00ff00), timing), c, true);
            let p0 = shot(&mut app);
            app.turn(|p| {
                p.reveal.set(1.0);
                p.width.set(120.0);
                p.tint.set(rgb(0xff0000));
            });
            assert!(app.animating(), "{timing:?}: a retarget starts the motions");
            tick(&mut app, DT);
            tick(&mut app, 4.0 * DT);
            tick(&mut app, 4.0 * DT);
            let mid = shot(&mut app);
            settle(&mut app, DT, 5.0);
            let end = shot(&mut app);
            let fresh = shot(&mut App::new(pop(1.0, 120.0, rgb(0xff0000), timing), cfg(), true));
            assert_eq!(end, fresh, "{timing:?} once={once}: settled = a fresh mount at the target");
            assert_ne!(mid, end, "{timing:?}: the motion was visible in flight");
            assert_ne!(mid, p0, "{timing:?}: and had left its start");
        }
    }
}

#[test]
fn a_motion_port_reruns_no_view() {
    let mut app = App::new(pop(0.0, 40.0, rgb(0x00ff00), Timing::Spring), cfg().once(), true);
    let p0 = shot(&mut app);
    let views = app.owner.views.get();
    app.turn(|p| {
        p.reveal.set(1.0);
    });
    for _ in 0..8 {
        tick(&mut app, DT);
    }
    assert_ne!(p0, shot(&mut app), "the port moved");
    assert_eq!(app.owner.views.get(), views, "publishing and every frame of the motion reran no view");
    // A re-describing owner: the kernel's frame clock describes it again after every frame of input animation
    // (`App::tick` settles as a turn does, for typed transitions too: open in the plan doc). Those descriptions
    // must apply nothing over a motion: every frame equals the wire-only twin's.
    for timing in ALL {
        let mut once = App::new(pop(0.0, 40.0, rgb(0x00ff00), timing), cfg().once(), true);
        let mut re = App::new(pop(0.0, 40.0, rgb(0x00ff00), timing), cfg(), true);
        for (k, target) in [1.0, 0.3].into_iter().enumerate() {
            for a in [&mut once, &mut re] {
                a.turn(|p| {
                    p.reveal.set(target);
                    p.width.set(60.0 + 40.0 * target);
                    p.tint.set(Color(0xff00_0000 | (k as u32 * 0x7f) << 8));
                });
            }
            for _ in 0..12 {
                tick(&mut once, DT);
                tick(&mut re, DT);
                assert_eq!(shot(&mut once), shot(&mut re), "{timing:?}: a re-description in flight moves nothing");
            }
        }
    }
}

#[test]
fn idle_after_settle_steps_nothing_and_allocates_nothing() {
    for timing in ALL {
        // headless: a GPU presenter's own calls are not this test's (objc2 checks message signatures in debug
        // builds, allocating); frames end at the tile list
        let mut app = App::new(pop(0.0, 40.0, rgb(0x00ff00), timing), cfg().once(), false);
        app.frame();
        // warm: one full motion, then a second (the running list has grown)
        for target in [1.0, 0.0] {
            app.turn(|p| {
                p.reveal.set(target);
                p.tint.set(Color(0xff00_0000 | (target as u32) * 0xff));
            });
            settle(&mut app, DT, 5.0);
        }
        app.turn(|p| {
            p.reveal.set(0.5);
        });
        let (mut in_tick, mut in_frame) = (0, 0);
        for _ in 0..6 {
            let a0 = calls();
            app.tick(DT);
            let a1 = calls();
            app.frame();
            (in_tick, in_frame) = (in_tick + a1 - a0, in_frame + calls() - a1);
        }
        assert_eq!((in_tick, in_frame), (0, 0), "{timing:?}: a frame of a running motion allocates nothing (tick, frame)");
        let a0 = calls();
        app.turn(|p| {
            p.reveal.set(0.25);
        });
        assert_eq!(calls() - a0, 0, "{timing:?}: a retarget allocates nothing");
        settle(&mut app, DT, 5.0);
        let (steps, retargets) = (app.rt.transitions.steps, app.rt.transitions.retargets);
        for _ in 0..10 {
            assert!(!app.tick(DT), "{timing:?}: a settled motion asks for no frame");
            assert!(!app.frame(), "{timing:?}: and its frame has nothing to draw");
        }
        // publishing the value a motion already heads for is a hint (L2): no segment, no frame
        app.turn(|p| {
            p.reveal.set(0.25);
        });
        assert!(!app.animating());
        assert_eq!((app.rt.transitions.steps, app.rt.transitions.retargets), (steps, retargets), "{timing:?}: nothing stepped");
    }
}

struct Still {
    fade: Signal<f32>,
}
impl View for Still {
    fn view(&self) -> El<'_> {
        column((div().size(40, 40).bg(rgb(0x334455)).opacity(&self.fade), text("still"))).bg(rgb(0xffffff))
    }
}

#[test]
fn a_view_with_no_motion_makes_no_motion_state() {
    let mut app = App::new(Still { fade: Signal::new(0.5) }, cfg(), true);
    shot(&mut app);
    app.turn(|s| {
        s.fade.set(0.25);
    });
    shot(&mut app);
    for _ in 0..4 {
        assert!(!app.tick(DT));
    }
    let t = &app.rt.transitions;
    assert!(t.is_empty() && t.live() == 0 && !t.running() && t.steps == 0, "no lane, no clock subscriber");
}

// ------------------------------------------------------------------------------------------- the timeline

/// The opacity motion's presented value (motion 0: the first port mounted).
fn opacity(app: &mut App<Pop>) -> f32 {
    app.rt.transitions.motion_shown::<f32>(0).expect("the opacity motion")
}

#[test]
fn one_timestamp_gives_one_value_at_any_cadence() {
    // dyadic steps, so every cadence reaches the same timestamps exactly
    let cadences: [&[f32]; 3] = [&[1.0 / 64.0], &[1.0 / 128.0], &[1.0 / 256.0, 3.0 / 256.0, 1.0 / 64.0]];
    for timing in ALL {
        let mut runs = Vec::new();
        for steps in cadences {
            let mut app = App::new(pop(0.0, 40.0, rgb(0x00ff00), timing), cfg().once(), false);
            app.turn(|p| {
                p.reveal.set(1.0);
                p.tint.set(rgb(0x0000ff));
            });
            let (mut t, mut k, mut out) = (0.0f64, 0, Vec::new());
            let mut next = 1.0 / 32.0;
            while t < 1.5 {
                let dt = steps[k % steps.len()];
                k += 1;
                app.tick(dt);
                t += dt as f64;
                if t == 0.5 {
                    // interrupted at one common instant
                    app.turn(|p| {
                        p.reveal.set(0.25);
                    });
                }
                if t >= next {
                    assert_eq!(t, next, "the cadences meet at every 1/32 s");
                    let n = app.rt.transitions.len() as u32;
                    out.push(
                        (0..n)
                            .map(|i| (app.rt.transitions.motion_shown::<f32>(i).map(f32::to_bits), app.rt.transitions.motion_shown::<Color>(i).map(|c| c.0)))
                            .collect::<Vec<_>>(),
                    );
                    next += 1.0 / 32.0;
                }
            }
            runs.push(out);
        }
        assert_eq!(runs[0], runs[1], "{timing:?}: 64 Hz and 128 Hz");
        assert_eq!(runs[0], runs[2], "{timing:?}: 64 Hz and an irregular cadence");
    }
}

#[test]
fn a_retarget_mid_flight_is_continuous() {
    const H: f32 = 1.0 / 4096.0;
    for timing in [Timing::Spring, Timing::Smooth, Timing::Eased] {
        let mut app = App::new(pop(0.0, 40.0, rgb(0x00ff00), timing), cfg().once(), false);
        app.turn(|p| {
            p.reveal.set(1.0);
        });
        let mut xs = Vec::new();
        for _ in 0..400 {
            app.tick(H);
            xs.push(opacity(&mut app));
        }
        // interrupted in flight, heading back
        let at = xs.len();
        app.turn(|p| {
            p.reveal.set(0.0);
        });
        for _ in 0..8 {
            app.tick(H);
            xs.push(opacity(&mut app));
        }
        let x = xs[at - 1];
        assert!(x > 0.05 && x < 0.95, "{timing:?}: interrupted in flight ({x})");
        let d = |i: usize| xs[i] - xs[i - 1];
        let step = d(at - 1).abs().max(d(at - 2).abs());
        // C0: the first frame after the retarget moves no further than a frame did before it
        assert!(d(at).abs() <= 2.0 * step + 1e-6, "{timing:?}: C0 at the retarget: {} after {} per frame", d(at), step);
        if timing != Timing::Eased {
            // C1: the velocity does not jump (the change of the per-frame step stays the size it had)
            let curv = (d(at - 1) - d(at - 2)).abs().max((d(at - 2) - d(at - 3)).abs());
            assert!((d(at) - d(at - 1)).abs() <= 4.0 * curv + 1e-6, "{timing:?}: C1 at the retarget: step {} then {} (curvature {})", d(at - 1), d(at), curv);
        }
    }
}

#[test]
fn a_re_described_timing_takes_over_in_flight() {
    for (from, to) in [(Timing::Spring, Timing::Eased), (Timing::Eased, Timing::Smooth), (Timing::Smooth, Timing::Delayed), (Timing::Delayed, Timing::Spring)] {
        let mut app = App::new(pop(0.0, 40.0, rgb(0x00ff00), from), cfg(), true);
        app.turn(|p| {
            p.reveal.set(1.0);
        });
        for _ in 0..11 {
            tick(&mut app, DT);
        }
        let earlier = opacity(&mut app);
        tick(&mut app, DT);
        let before = opacity(&mut app);
        app.turn(|p| p.timing = to);
        assert_eq!(opacity(&mut app), before, "{from:?} -> {to:?}: the new timing starts from what is presented");
        tick(&mut app, DT);
        let step = (opacity(&mut app) - before).abs();
        assert!(step <= 2.0 * (before - earlier).abs() + 0.02, "{from:?} -> {to:?}: and moves on from it ({step} after {})", before - earlier);
        settle(&mut app, DT, 5.0);
        assert_eq!(shot(&mut app), shot(&mut App::new(pop(1.0, 40.0, rgb(0x00ff00), to), cfg(), true)), "{from:?} -> {to:?}: settled");
    }
}

// ------------------------------------------------------------------------------------------- against the typed spelling

struct ElFade {
    fade: Signal<f32>,
}
impl View for ElFade {
    fn view(&self) -> El<'_> {
        column(div().size(80, 40).bg(rgb(0x2050a0)).opacity(animate(&self.fade).curve(Curve::out(0.3)))).bg(rgb(0xffffff)).w_pct(1.0).h_pct(1.0)
    }
}
mod typed {
    use bough::builders::*;
    pub struct Fade {
        pub fade: Signal<f32>,
    }
    impl View for Fade {
        fn view(&self) -> impl Element<Self> + '_ {
            column((div().w(80.0).h(40.0).bg(rgb(0x2050a0)).opacity(sig!(self.fade).transition(Curve::out(0.3))),)).bg(rgb(0xffffff)).w_pct(1.0).h_pct(1.0)
        }
    }
}

#[test]
fn an_eased_motion_presents_what_the_typed_transition_does() {
    let mut el = App::new(ElFade { fade: Signal::new(1.0) }, cfg().once(), true);
    let mut ty = App::new(typed::Fade { fade: Signal::new(1.0) }, cfg().once(), true);
    assert_eq!(shot(&mut el), shot(&mut ty), "mounted");
    for (k, target) in [0.0, 0.6, 0.1].into_iter().enumerate() {
        el.turn(|v| {
            v.fade.set(target);
        });
        ty.turn(|v| {
            v.fade.set(target);
        });
        for _ in 0..(6 + 4 * k) {
            tick(&mut el, DT);
            tick(&mut ty, DT);
            assert_eq!(shot(&mut el), shot(&mut ty), "the same frame at the same time (target {target})");
        }
    }
}

// ------------------------------------------------------------------------------------------- identity (02 §6)

struct Flip {
    open: bool,
    reveal: Signal<f32>,
}
impl View for Flip {
    fn view(&self) -> El<'_> {
        let at = if self.open { div().size(60, 30).bg(rgb(0xa02050)).opacity(animate(&self.reveal).over(400)) } else { text("closed") };
        column((at, div().size(60, 10).bg(rgb(0x20a050)))).bg(rgb(0xffffff)).w_pct(1.0).h_pct(1.0)
    }
}

#[test]
fn a_branch_flip_drops_the_track() {
    let mut app = App::new(Flip { open: true, reveal: Signal::new(0.0) }, cfg(), true);
    shot(&mut app);
    app.turn(|f| {
        f.reveal.set(1.0);
    });
    tick(&mut app, 0.1);
    assert!(app.animating());
    // the box leaves its position mid-flight: its motion leaves with it
    app.turn(|f| f.open = false);
    assert!(!app.rt.transitions.running() && app.rt.transitions.live() == 0, "the remounted position's track dropped");
    tick(&mut app, 0.1);
    assert_eq!(shot(&mut app), shot(&mut App::new(Flip { open: false, reveal: Signal::new(1.0) }, cfg(), true)), "closed = a fresh mount");
    // back, mid-way through another change: a new motion, at rest where the signal is (no enter animation)
    app.turn(|f| {
        f.reveal.set(0.5);
    });
    app.turn(|f| f.open = true);
    assert!(!app.animating(), "a fresh position starts at rest");
    assert_eq!(app.rt.transitions.live(), 1, "one track, its id reused");
    assert_eq!(shot(&mut app), shot(&mut App::new(Flip { open: true, reveal: Signal::new(0.5) }, cfg(), true)), "open again = a fresh mount");
    // and it moves
    app.turn(|f| {
        f.reveal.set(1.0);
    });
    assert!(app.animating());
    settle(&mut app, DT, 2.0);
    assert_eq!(shot(&mut app), shot(&mut App::new(Flip { open: true, reveal: Signal::new(1.0) }, cfg(), true)));
}

// ------------------------------------------------------------------------------------------- re-description fixes

struct Scaled {
    n: Signal<f32>,
    k: f32,
}
impl View for Scaled {
    fn view(&self) -> El<'_> {
        let k = self.k;
        column((
            // items of a sealed part: a description sealed alike is stepped over
            (0..2).map(move |i| div().size(60, 16).bg(rgb(0x203060 + i * 0x200000)).opacity(self.n.map(move |n| n * k))),
            Some(text(self.n.map(move |n| format!("{:.0}", n * k * 100.0)))),
            div().size(60, 16).bg(rgb(0x206030)).opacity(animate(&self.n).scale(k).over(100)),
        ))
        .bg(rgb(0xffffff))
        .w_pct(1.0)
        .h_pct(1.0)
    }
}

#[test]
fn a_map_whose_captures_changed_is_right_at_once() {
    let mut app = App::new(Scaled { n: Signal::new(0.5), k: 1.0 }, cfg(), true);
    shot(&mut app);
    // k = 0: every value the maps give is 0, and stays 0 when n moves
    for (n, k) in [(0.5, 0.0), (0.8, 0.0), (0.3, 0.0), (0.3, 0.5), (0.9, 0.5), (0.9, 1.0)] {
        app.turn(|s| {
            s.n.set(n);
            s.k = k;
        });
        settle(&mut app, DT, 2.0);
        assert_eq!(
            shot(&mut app),
            shot(&mut App::new(Scaled { n: Signal::new(n), k }, cfg(), true)),
            "n {n}, k {k}: = a fresh mount on the turn it is described"
        );
    }
}

struct Hovered {
    alt: bool,
}
impl View for Hovered {
    fn view(&self) -> El<'_> {
        let h = if self.alt { rgb(0xc03030) } else { rgb(0x3030c0) };
        column(div().size(100, 60).bg(rgb(0x808080)).opacity(0.75).hover(move |s| s.bg(h))).bg(rgb(0xffffff)).w_pct(1.0).h_pct(1.0)
    }
}

#[test]
fn a_hover_colour_changed_while_hovered_shows_at_once() {
    let hovered = |alt: bool| {
        let mut a = App::new(Hovered { alt }, cfg(), true);
        a.pointer_move(50.0, 30.0);
        shot(&mut a)
    };
    let mut app = App::new(Hovered { alt: false }, cfg(), true);
    app.pointer_move(50.0, 30.0);
    assert_eq!(shot(&mut app), hovered(false));
    for alt in [true, false, true] {
        app.turn(|h| h.alt = alt);
        assert_eq!(shot(&mut app), hovered(alt), "alt {alt}: the new hover colour shows while hovered");
    }
    app.pointer_leave();
    let left = shot(&mut app);
    let mut fresh = App::new(Hovered { alt: true }, cfg(), true);
    assert_eq!(left, shot(&mut fresh), "and the base colour returns on leave");
}
