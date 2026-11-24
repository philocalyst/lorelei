//! What a completion costs through `Reader` plugs and a boundary: El (`bough::prelude`) against the kernel's typed
//! spelling (`sig!(self.r).map(..)`, `sig!(self.r).pending().map(..)`, `boundary(sig!(self.r))`), in one binary,
//! interleaved. Card: `docs/lanes/integration/2026-11-22-el-surface.md`, "Reader plugs and `boundary()` in El".
//!
//! A cycle: `turn(q += 1)` (the producer requests), `run_jobs(1)` (the manual executor runs the job: not
//! counted), `poll()` (admission, the cohort's commit, the ports' patches). Instructions of the request turn and
//! of the completion turn by slope over two repeat counts; allocator calls per whole cycle. Shapes: `port` (a
//! text port and an `is_pending` opacity port on the reader) and `boundary` (the same ports inside a boundary
//! with a loading arm), each under `Config::once()` and under re-description. `first` is the turn that commits
//! the first value of a fresh app (for `boundary`, its loading -> child switch): median of 8 apps. The allocation
//! columns of `request` and `completion` are both the whole cycle's (job and result boxes, the payload string,
//! the ports' strings).
//!
//!     cargo run --release -p bough --example reader_cost
use bough::raw::{App, Caps, Config, Request};
use bough_sys::perf::tinstr;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct Counting;
static CALLS: AtomicUsize = AtomicUsize::new(0);
// SAFETY: forwards to `System` unchanged; the counter is not part of the allocation contract.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        CALLS.store(CALLS.load(Relaxed) + 1, Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        CALLS.store(CALLS.load(Relaxed) + 1, Relaxed);
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        CALLS.store(CALLS.load(Relaxed) + 1, Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static ALLOC: Counting = Counting;

fn req(q: &u32) -> Request<String> {
    let q = *q;
    Request::new(move || format!("story {q}"))
}

mod typed {
    use super::req;
    use bough::builders::*;
    use bough::raw::produce;
    pub struct Port<const B: bool> {
        pub q: Signal<u32>,
        pub r: Reader<String>,
    }
    impl View for Port<false> {
        fn view(&self) -> impl Element<Self> + '_ {
            column((produce(sig!(self.q), sig!(self.r), req), text(sig!(self.r).map(|s: Option<&String>| s.cloned().unwrap_or_default())), row(()).w(40.0).h(16.0).bg(rgb(0x204080)).opacity(sig!(self.r).pending().map(|p| if p { 0.4 } else { 1.0 }))))
                .bg(rgb(0xffffff))
        }
    }
    impl View for Port<true> {
        fn view(&self) -> impl Element<Self> + '_ {
            column((
                produce(sig!(self.q), sig!(self.r), req),
                boundary(sig!(self.r))
                    .loading(text("loading"))
                    .child(column((text(sig!(self.r).map(|s: Option<&String>| s.cloned().unwrap_or_default())), row(()).w(40.0).h(16.0).bg(rgb(0x204080)).opacity(sig!(self.r).pending().map(|p| if p { 0.4 } else { 1.0 }))))),
            ))
            .bg(rgb(0xffffff))
        }
    }
}

/// The control of `first`: the same tree with a `Signal<String>` in the reader's place, its first write.
mod control {
    pub mod typed {
        use bough::builders::*;
        pub struct Sig {
            pub s: Signal<String>,
            pub p: Signal<f32>,
        }
        impl View for Sig {
            fn view(&self) -> impl Element<Self> + '_ {
                column((text(sig!(self.s)), row(()).w(40.0).h(16.0).bg(rgb(0x204080)).opacity(sig!(self.p)))).bg(rgb(0xffffff))
            }
        }
    }
    pub mod el {
        use bough::prelude::*;
        pub struct Sig {
            pub s: Signal<String>,
            pub p: Signal<f32>,
        }
        impl View for Sig {
            fn view(&self) -> El<'_> {
                column((text(&self.s), div().size(40, 16).bg(rgb(0x204080)).opacity(&self.p))).bg(rgb(0xffffff))
            }
        }
    }
}

/// `control`'s first write (the text from empty to "story 0", the opacity from 0.4 to 1): median of 8 fresh apps.
fn control_first<T: bough::app::View>(make: fn() -> T, write: fn(&mut T)) -> (u64, usize) {
    let mut v: Vec<(u64, usize)> = (0..8)
        .map(|_| {
            let mut a = App::new(make(), config(true), false);
            let (t0, c0) = (tinstr(), CALLS.load(Relaxed));
            a.turn(write);
            let r = (tinstr() - t0, CALLS.load(Relaxed) - c0);
            std::mem::forget(a);
            r
        })
        .collect();
    v.sort();
    v[4]
}

mod el {
    use super::req;
    use bough::prelude::*;
    pub struct Port<const B: bool> {
        pub q: Signal<u32>,
        pub r: Reader<String>,
    }
    impl<const B: bool> Port<B> {
        fn ports(&self) -> (El<'_>, El<'_>) {
            (text(self.r.map(|s| s.cloned().unwrap_or_default())), div().size(40, 16).bg(rgb(0x204080)).opacity(is_pending(&self.r).map(|p| if p { 0.4 } else { 1.0 })))
        }
    }
    impl View for Port<false> {
        fn view(&self) -> El<'_> {
            let (t, b) = self.ports();
            column((produce(&self.q, &self.r, req), t, b)).bg(rgb(0xffffff))
        }
    }
    impl View for Port<true> {
        fn view(&self) -> El<'_> {
            column((produce(&self.q, &self.r, req), boundary().loading(text("loading")).child(column(self.ports())))).bg(rgb(0xffffff))
        }
    }
}

trait Make: bough::app::View {
    fn make() -> Self;
    fn q(&mut self) -> &mut bough::raw::Signal<u32>;
}
macro_rules! make {
    ($($t:ty),*) => {$(
        impl Make for $t {
            fn make() -> Self {
                Self { q: bough::raw::Signal::new(0), r: bough::raw::Reader::new() }
            }
            fn q(&mut self) -> &mut bough::raw::Signal<u32> {
                &mut self.q
            }
        }
    )*};
}
make!(typed::Port<false>, typed::Port<true>, el::Port<false>, el::Port<true>);

fn config(once: bool) -> Config {
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    let mut cfg = Config::new(640.0, 400.0, 1.0, font);
    cfg.caps = Caps { instances: 1 << 14, pages: 1 << 10, glyphs: 1 << 16, runs: 1 << 12, spaces: 1 << 10, clips: 1 << 10, strikes: 16 };
    if once { cfg.once() } else { cfg }
}

/// (request turn, completion turn) instructions and allocator calls per cycle, by slope.
fn cycle<T: Make>(once: bool) -> (u64, u64, f64) {
    let mut a = App::new(T::make(), config(once), false);
    a.run_jobs(8);
    a.poll();
    let mut run = |reps: u32| {
        let (mut req, mut done) = (0u64, 0u64);
        let c0 = CALLS.load(Relaxed);
        for _ in 0..reps {
            let t0 = tinstr();
            a.turn(|v| **v.q() += 1);
            let t1 = tinstr();
            assert_eq!(a.run_jobs(1), 1);
            let t2 = tinstr();
            a.poll();
            let t3 = tinstr();
            req += t1 - t0;
            done += t3 - t2;
        }
        (req, done, CALLS.load(Relaxed) - c0)
    };
    run(8);
    let (lo, hi) = (run(16), run(48));
    ((hi.0 - lo.0) / 32, (hi.1 - lo.1) / 32, (hi.2 as f64 - lo.2 as f64) / 32.0)
}

/// A turn with nothing to admit and nothing written: instructions and allocator calls, by slope.
fn idle<T: Make>(once: bool) -> (u64, f64) {
    let mut a = App::new(T::make(), config(once), false);
    a.run_jobs(8);
    a.poll();
    let mut run = |reps: u32| {
        let (t0, c0) = (tinstr(), CALLS.load(Relaxed));
        for _ in 0..reps {
            a.poll();
        }
        (tinstr() - t0, CALLS.load(Relaxed) - c0)
    };
    run(8);
    let (lo, hi) = (run(16), run(48));
    ((hi.0 - lo.0) / 32, (hi.1 as f64 - lo.1 as f64) / 32.0)
}

/// The turn committing a fresh app's first value (loading -> child): median over 8 apps, and its allocator calls.
fn first<T: Make>(once: bool) -> (u64, usize) {
    let mut v: Vec<(u64, usize)> = (0..8)
        .map(|_| {
            let mut a = App::new(T::make(), config(once), false);
            a.run_jobs(1);
            let (t0, c0) = (tinstr(), CALLS.load(Relaxed));
            a.poll();
            let r = (tinstr() - t0, CALLS.load(Relaxed) - c0);
            std::mem::forget(a);
            r
        })
        .collect();
    v.sort();
    v[4]
}

fn main() {
    println!("shape,mode,phase,typed_instr,el_instr,el_over_typed,typed_allocs,el_allocs");
    let row = |shape: &str, mode: &str, phase: &str, t: u64, e: u64, ta: f64, ea: f64| println!("{shape},{mode},{phase},{t},{e},{:.3},{ta:.2},{ea:.2}", e as f64 / t.max(1) as f64);
    {
        use bough::raw::Signal;
        let t = control_first(
            || control::typed::Sig { s: Signal::new(String::new()), p: Signal::new(0.4) },
            |v| {
                v.s.set("story 0".into());
                v.p.set(1.0);
            },
        );
        let e = control_first(
            || control::el::Sig { s: Signal::new(String::new()), p: Signal::new(0.4) },
            |v| {
                v.s.set("story 0".into());
                v.p.set(1.0);
            },
        );
        row("signal control", "once", "first", t.0, e.0, t.1 as f64, e.1 as f64);
    }
    for once in [true, false] {
        let mode = if once { "once" } else { "redescribe" };
        let (t, e) = (cycle::<typed::Port<false>>(once), cycle::<el::Port<false>>(once));
        row("port", mode, "request", t.0, e.0, t.2, e.2);
        row("port", mode, "completion", t.1, e.1, t.2, e.2);
        let (t, e) = (first::<typed::Port<false>>(once), first::<el::Port<false>>(once));
        row("port", mode, "first", t.0, e.0, t.1 as f64, e.1 as f64);
        let (t, e) = (idle::<typed::Port<false>>(once), idle::<el::Port<false>>(once));
        row("port", mode, "idle", t.0, e.0, t.1, e.1);
        let (t, e) = (cycle::<typed::Port<true>>(once), cycle::<el::Port<true>>(once));
        row("boundary", mode, "request", t.0, e.0, t.2, e.2);
        row("boundary", mode, "completion", t.1, e.1, t.2, e.2);
        let (t, e) = (idle::<typed::Port<true>>(once), idle::<el::Port<true>>(once));
        row("boundary", mode, "idle", t.0, e.0, t.1, e.1);
        let (t, e) = (first::<typed::Port<true>>(once), first::<el::Port<true>>(once));
        row("boundary", mode, "first", t.0, e.0, t.1 as f64, e.1 as f64);
    }
}
