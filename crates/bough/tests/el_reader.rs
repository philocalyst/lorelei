//! `Reader<T>` plugs and `boundary()` in El (`designs/reactivity/final/05-async.md` §2–§5; card in
//! `docs/lanes/integration/2026-11-22-el-surface.md`, "Reader plugs and `boundary()` in El").
//!
//! The oracle of every incremental step: the retained frame equals a full redraw, and equals a fresh mount of
//! the same state (the same signals, the readers' states copied by `Reader::snapshot`, no producer: a producer
//! mounts nothing).
//!
//! Negative controls (each makes a test here fail):
//! * `--features mutant_reader_stamp`: admission passes every completion: `a_stale_completion_is_rejected`.
//! * `--features mutant_el_boundary_ready`: the boundary ignores first readiness (shows loading while any
//!   guard is pending): `loading_then_content_then_revalidation_keeps_content`. (The fresh-mount oracle does
//!   not see it: a fresh mount computes the same wrong verdict; the lifecycle test asserts the policy.)
//! * `--features mutant_el_bound_wire`: a guard's transition does not reach its boundary: the fresh-mount
//!   oracle fails (`random_scripts_match_a_fresh_mount_at_every_step`, under `once`, and the lifecycle test), and
//!   a boundary stepped over in a list never switches (`a_listed_boundary_is_stepped_over_and_switched_by_its_wires`).
use bough::prelude::*;
use bough::raw::{App, Config};
use std::cell::Cell;

const FONT: &[u8] = zgx_workload::FONT_BYTES;

fn cfg(once: bool) -> Config {
    let c = Config::new(240.0, 160.0, 1.0, FONT.to_vec());
    if once { c.once() } else { c }
}

fn shot<V: bough::app::View>(app: &mut App<V>) -> Vec<u8> {
    app.frame();
    let p = app.presented_pixels();
    assert_eq!(p, app.reference_pixels(), "the repaired frame equals a full redraw");
    p
}

/// A request for `q`: `q % 5 == 4` fails.
fn req(q: &u32) -> Request<String> {
    let q = *q;
    Request::fallible(move || if q % 5 == 4 { Err(format!("bad {q}")) } else { Ok(format!("story {q}")) })
}

fn label(s: Option<&String>) -> String {
    s.cloned().unwrap_or_else(|| "none".into())
}

// ------------------------------------------------------------------------------------------- ports

struct Ports {
    q: Signal<u32>,
    r: Reader<String>,
    far: Vec<Reader<String>>,
    oracle: bool,
    describes: Cell<u32>,
}
impl View for Ports {
    fn view(&self) -> El<'_> {
        self.describes.set(self.describes.get() + 1);
        column((
            (!self.oracle).then(|| produce(&self.q, &self.r, req)),
            text(self.r.map(label)),
            div().size(40, 16).bg(rgb(0x204080)).opacity(is_pending(&self.r).map(|p| if p { 0.4 } else { 1.0 })),
            text(is_pending(&self.r).map(|p| if p { "pending".to_string() } else { "idle".to_string() })),
            div().size(40, 16).bg(self.r.map(|s| if s.is_some() { rgb(0x10a030) } else { rgb(0xa01030) })),
            text(latest(&self.q).map(|q| format!("now {q}"))),
            text(self.far[0].map(label)),
        ))
        .bg(rgb(0xffffff))
        .w_pct(1.0)
        .h_pct(1.0)
    }
}
impl Ports {
    fn new(q: u32) -> Ports {
        Ports { q: Signal::new(q), r: Reader::new(), far: vec![Reader::new()], oracle: false, describes: Cell::new(0) }
    }
    fn fresh(&self, once: bool) -> App<Ports> {
        let v = Ports { q: Signal::new(*self.q), r: self.r.snapshot(), far: vec![self.far[0].snapshot()], oracle: true, describes: Cell::new(0) };
        App::new(v, cfg(once), true)
    }
}

fn texts<V: bough::app::View>(a: &App<V>) -> Vec<String> {
    a.rt.texts.iter().map(|t| String::from_utf8_lossy(t.bytes()).into_owned()).collect()
}

/// Under `once` (no re-description), a completion reaches every reader port (text, colour, opacity, a text of
/// the pending flag) and `view` ran once; each step equals a fresh mount of the same state.
#[test]
fn a_reader_port_patches_without_rerunning_view() {
    let mut a = App::new(Ports::new(1), cfg(true), true);
    let p0 = shot(&mut a);
    assert!(a.owner.r.is_pending(), "the producer requested at mount");
    assert!(texts(&a).contains(&"pending".to_string()));
    assert_eq!(p0, shot(&mut a.owner.fresh(true)), "mount = fresh mount");
    assert_eq!(a.run_jobs(8), 1);
    a.poll();
    let p1 = shot(&mut a);
    assert_eq!(a.owner.r.latest().map(String::as_str), Some("story 1"));
    assert!(texts(&a).contains(&"story 1".to_string()) && texts(&a).contains(&"idle".to_string()), "{:?}", texts(&a));
    assert_ne!(p0, p1, "the completion changed the pixels");
    assert_eq!(p1, shot(&mut a.owner.fresh(true)), "completion = fresh mount");
    // a new request: the pending flag (opacity and text) moves, the value stays
    a.turn(|v| *v.q = 2);
    let p2 = shot(&mut a);
    assert!(texts(&a).contains(&"pending".to_string()) && texts(&a).contains(&"story 1".to_string()) && texts(&a).contains(&"now 2".to_string()));
    assert_ne!(p1, p2);
    assert_eq!(p2, shot(&mut a.owner.fresh(true)), "request = fresh mount");
    a.run_jobs(8);
    a.poll();
    assert!(texts(&a).contains(&"story 2".to_string()));
    assert_eq!(shot(&mut a), shot(&mut a.owner.fresh(true)), "second completion = fresh mount");
    assert_eq!(a.owner.describes.get(), 1, "no completion reran `view`");
}

/// A reader that is not a field of the view (behind a `Vec`) is a constant of the description: its port asked
/// for a wire and none was installed; the field's ports were.
#[test]
fn a_reader_behind_a_vec_is_a_constant() {
    let mut a = App::new(Ports::new(1), cfg(true), true);
    a.frame();
    let on = |a: &App<Ports>, head: usize| a.rt.live_wires_on(head);
    let (r, far) = (&a.owner.r as *const Reader<String> as usize, &a.owner.far[0] as *const Reader<String> as usize);
    assert_eq!(on(&a, r), 2, "the field's value head: its text and colour ports");
    assert_eq!(on(&a, r + 4), 2, "its pending head: the opacity and the pending text");
    assert_eq!(on(&a, far) + on(&a, far + 4), 0, "the reader in a Vec: none");
}

#[test]
#[should_panic(expected = "`produce` takes a `Signal<u32>` field and a `Reader<alloc::string::String>` field of the view")]
fn a_producer_of_a_reader_elsewhere_is_refused_by_name() {
    struct Far {
        q: Signal<u32>,
        rs: Vec<Reader<String>>,
    }
    impl View for Far {
        fn view(&self) -> El<'_> {
            column((produce(&self.q, &self.rs[0], req), text("x")))
        }
    }
    let _ = App::new(Far { q: Signal::new(0), rs: vec![Reader::new()] }, cfg(false), false);
}

// ------------------------------------------------------------------------------------------- stale

/// Requests for 1 and 2 (after the mount's for 0) are outstanding; 1's and 0's completions arrive first and
/// are discarded (superseded tickets), the reader stays pending without a value, and only 2's lands.
#[test]
fn a_stale_completion_is_rejected() {
    let mut a = App::new(Ports::new(0), cfg(false), true);
    a.frame();
    a.turn(|v| *v.q = 1);
    a.turn(|v| *v.q = 2);
    assert_eq!(a.rt.jobs_queued(), 3);
    let s0 = a.rt.asy.stats;
    assert!(a.rt.run_job_at(1), "q = 1 finishes first");
    a.poll();
    assert_eq!(a.owner.r.latest(), None, "an older request finishing after a newer one was requested lands nowhere");
    assert!(a.owner.r.is_pending());
    assert_eq!(shot(&mut a), shot(&mut a.owner.fresh(false)));
    assert!(a.rt.run_job_at(0), "then q = 0");
    a.poll();
    assert_eq!(a.owner.r.latest(), None);
    assert_eq!(a.rt.asy.stats.stale - s0.stale, 2, "two stale completions");
    a.run_jobs(8);
    a.poll();
    assert_eq!(a.owner.r.latest().map(String::as_str), Some("story 2"));
    assert!(!a.owner.r.is_pending());
    assert_eq!(a.rt.asy.stats.accepted - s0.accepted, 1);
    assert_eq!(shot(&mut a), shot(&mut a.owner.fresh(false)));
}

// ------------------------------------------------------------------------------------------- boundary

struct Page {
    q: Signal<u32>,
    r: Reader<String>,
    show: bool,
    oracle: bool,
}
fn skeleton<'f>() -> El<'f> {
    div().size(80, 12).bg(rgb(0xcccccc))
}
impl View for Page {
    fn view(&self) -> El<'_> {
        let content = || column((text(self.r.map(label)), div().size(60, 8).bg(rgb(0x3060c0))));
        column((
            text(is_pending(&self.r).map(|p| if p { "pending".to_string() } else { "idle".to_string() })),
            self.show.then(|| {
                column((
                    (!self.oracle).then(|| produce(&self.q, &self.r, req)),
                    // keep-stale (the default) and replace-with-error, side by side
                    boundary().loading(skeleton).failed(text("failed").fg(rgb(0xc00000))).child(content()),
                    boundary().loading(text("loading")).failed(text("failed!")).replace_on_failure().child(content()),
                ))
            }),
        ))
        .bg(rgb(0xffffff))
        .w_pct(1.0)
        .h_pct(1.0)
    }
}
impl Page {
    fn new(q: u32) -> Page {
        Page { q: Signal::new(q), r: Reader::new(), show: true, oracle: false }
    }
    fn fresh(&self, once: bool) -> App<Page> {
        App::new(Page { q: Signal::new(*self.q), r: self.r.snapshot(), show: self.show, oracle: true }, cfg(once), true)
    }
}

/// The arms each boundary of the page shows now (the last two boundaries mounted).
fn arms<V: bough::app::View>(a: &App<V>) -> [u8; 2] {
    let n = a.rt.boundaries();
    [a.rt.boundary_shown(n - 2), a.rt.boundary_shown(n - 1)]
}

#[test]
fn loading_then_content_then_revalidation_keeps_content() {
    for once in [true, false] {
        let mut a = App::new(Page::new(1), cfg(once), true);
        let check = |a: &mut App<Page>, ctx: &str| {
            let p = shot(a);
            assert_eq!(p, shot(&mut a.owner.fresh(once)), "once {once}, {ctx}: retained = fresh mount");
        };
        check(&mut a, "mount");
        assert_eq!(arms(&a), [0, 0], "loading before first readiness");
        a.run_jobs(8);
        a.poll();
        check(&mut a, "first readiness");
        assert_eq!(arms(&a), [2, 2], "content");
        a.turn(|v| *v.q = 2);
        check(&mut a, "revalidating");
        assert!(a.owner.r.is_pending());
        assert_eq!(arms(&a), [2, 2], "revalidation keeps the content mounted");
        a.run_jobs(8);
        a.poll();
        check(&mut a, "revalidated");
        assert!(texts(&a).contains(&"story 2".to_string()));
        a.turn(|v| *v.q = 4);
        a.run_jobs(8);
        a.poll();
        check(&mut a, "failed");
        assert_eq!(a.owner.r.fault(), Some("bad 4"));
        assert_eq!(arms(&a), [2, 1], "a failure after content: stale content (default), the failed arm (replace)");
        a.turn(|v| *v.q = 5);
        check(&mut a, "retrying");
        assert_eq!(arms(&a), [2, 1], "a retry keeps the failure until it succeeds");
        a.run_jobs(8);
        a.poll();
        check(&mut a, "healed");
        assert_eq!(arms(&a), [2, 2], "a success heals");
    }
}

#[test]
fn a_first_failure_shows_the_failed_arm() {
    let mut a = App::new(Page::new(4), cfg(false), true);
    a.run_jobs(8);
    a.poll();
    assert_eq!(arms(&a), [1, 1]);
    assert_eq!(shot(&mut a), shot(&mut a.owner.fresh(false)));
    a.turn(|v| *v.q = 6);
    a.run_jobs(8);
    a.poll();
    assert_eq!(arms(&a), [2, 2]);
    assert_eq!(shot(&mut a), shot(&mut a.owner.fresh(false)));
}

/// Unmounting the branch that declares the producer revokes it: the pending request is cancelled (the reader is
/// no longer pending), a late completion is discarded and changes nothing; mounted again, it requests anew.
#[test]
fn unmounting_a_branch_cancels_its_pending_work() {
    let mut a = App::new(Page::new(1), cfg(false), true);
    a.frame();
    assert!(a.owner.r.is_pending());
    a.turn(|v| v.show = false);
    assert!(!a.owner.r.is_pending(), "the revoked request is no longer outstanding");
    let p = shot(&mut a);
    assert_eq!(p, shot(&mut a.owner.fresh(false)));
    let stale = a.rt.asy.stats.stale;
    assert_eq!(a.run_jobs(8), 1, "the job itself was queued (logical cancellation)");
    a.poll();
    assert_eq!(a.rt.asy.stats.stale, stale + 1, "its completion was discarded");
    assert_eq!(a.owner.r.latest(), None, "and wrote nothing");
    assert_eq!(shot(&mut a), p, "a late completion is harmless");
    a.turn(|v| *v.q = 3);
    assert_eq!(a.rt.jobs_queued(), 0, "no producer, no request");
    a.turn(|v| v.show = true);
    assert!(a.owner.r.is_pending(), "remounted, the producer requests the current input");
    assert_eq!(shot(&mut a), shot(&mut a.owner.fresh(false)));
    a.run_jobs(8);
    a.poll();
    assert_eq!(a.owner.r.latest().map(String::as_str), Some("story 3"));
    assert_eq!(arms(&a), [2, 2]);
    assert_eq!(shot(&mut a), shot(&mut a.owner.fresh(false)));
}

/// Boundaries as items of a list (a sealed part): described as before, a boundary is stepped over whole (its
/// arms are not entered), and its verdict is its wires' to keep. Under `mutant_el_bound_wire` (the wires do
/// nothing) its arm then never changes, re-described or not, which is what this test sees.
struct Listed {
    q: Signal<u32>,
    r: Reader<String>,
    oracle: bool,
}
impl View for Listed {
    fn view(&self) -> El<'_> {
        column((
            (!self.oracle).then(|| produce(&self.q, &self.r, req)),
            (0..2).map(move |i| {
                let b = boundary().loading(skeleton).failed(text("failed"));
                // the second replaces its content with the failed arm on a failure (it switches twice more)
                let b = if i == 1 { b.replace_on_failure() } else { b };
                b.child(column((text(self.r.map(label)), div().size(20 + i * 10, 8).bg(rgb(0x3060c0)))))
            }),
        ))
        .bg(rgb(0xffffff))
        .w_pct(1.0)
        .h_pct(1.0)
    }
}

#[test]
fn a_listed_boundary_is_stepped_over_and_switched_by_its_wires() {
    let fresh = |a: &App<Listed>| App::new(Listed { q: Signal::new(*a.owner.q), r: a.owner.r.snapshot(), oracle: true }, cfg(false), true);
    let mut a = App::new(Listed { q: Signal::new(1), r: Reader::new(), oracle: false }, cfg(false), true);
    assert_eq!(shot(&mut a), shot(&mut fresh(&a)), "mount");
    assert_eq!(arms(&a), [0, 0], "loading");
    // two re-descriptions with nothing to admit: the first seals the list's items, the second steps over them,
    // so every switch below is the boundaries' wires' alone
    a.poll();
    a.poll();
    let mut seen = Vec::new();
    for (step, q) in [(0, None), (1, Some(2)), (2, None), (3, Some(4)), (4, None), (5, Some(6)), (6, None)] {
        if let Some(q) = q {
            a.turn(|v| *v.q = q);
        } else {
            a.run_jobs(8);
            a.poll();
        }
        assert_eq!(shot(&mut a), shot(&mut fresh(&a)), "step {step}: retained = fresh mount");
        seen.push(arms(&a));
    }
    assert_eq!(seen[0], [2, 2], "first readiness: content");
    assert_eq!(seen[4], [2, 1], "a failure: stale content, and the replacing one's failed arm");
    assert_eq!(seen[6], [2, 2], "healed");
}

// ------------------------------------------------------------------------------------------- scripts

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Seeded scripts of requests, supersessions, failures, in-order and out-of-order completions and branch
/// flips (re-description only: under `once` a flip is not described): after every step the retained frame is
/// a full redraw and a fresh mount of the same state.
#[test]
fn random_scripts_match_a_fresh_mount_at_every_step() {
    for once in [false, true] {
        for seed in 1..13u64 {
            let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let mut a = App::new(Page::new(0), cfg(once), true);
            for step in 0..40 {
                let op = r.next() % 7;
                match op {
                    0 | 1 => {
                        let q = (r.next() % 10) as u32;
                        a.turn(|v| *v.q = q);
                    }
                    2 => {
                        let n = a.rt.jobs_queued();
                        if n > 0 {
                            a.rt.run_job_at((r.next() % n as u64) as usize);
                        }
                        a.poll();
                    }
                    3 => {
                        a.run_jobs(1);
                        a.poll();
                    }
                    4 if !once => a.turn(|v| v.show = !v.show),
                    _ => a.poll(),
                }
                let p = shot(&mut a);
                assert_eq!(p, shot(&mut a.owner.fresh(once)), "once {once} seed {seed} step {step} (op {op}): retained = fresh mount");
            }
        }
    }
}
