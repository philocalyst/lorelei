//! What an El motion port costs (`docs/lanes/integration/2026-11-22-el-surface.md`, "Motion as a plug"): N
//! animated boxes, each an opacity port on one signal, written three ways in one binary:
//!
//! * `typed`: the kernel's typed builder, `opacity(sig!(self.fade).transition(Curve::out(..)))`;
//! * `el_eased`: the prelude, `opacity(animate(&self.fade).curve(Curve::out(..)))` (same curve);
//! * `el_spring`: the prelude, `opacity(animate(&self.fade).spring(..))`.
//!
//! Cells, instructions on the calling thread by slope over two repeat counts: `tick` is one `App::tick` with
//! every box in flight (the curve and the spring are slow enough to stay in flight), `retarget` is one turn that
//! publishes the signal (`Config::once`: the wires alone), and `tick_re` is a tick of the same view mounted to
//! re-describe (the kernel's frame clock settles as a turn does: `App::tick` -> `settle_input`). Allocator calls per
//! tick and per retarget are counted in every mode (two loads and a store per allocator call); frames are drawn
//! between measured calls, outside the brackets.
//!
//!     cargo run --release -p bough --example el_motion_cost -- [boxes...]
use bough::raw::{App, Caps, Config};
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

/// Long enough that no box arrives while it is measured.
const DUR: f32 = 1000.0;

mod typed {
    use bough::builders::*;
    pub struct Boxes {
        pub fade: Signal<f32>,
        pub n: usize,
    }
    impl View for Boxes {
        fn view(&self) -> impl Element<Self> + '_ {
            let one = |i: usize| div().w(8.0).h(8.0).bg(rgb(0x2050a0 + i as u32)).opacity(sig!(self.fade).transition(Curve::out(super::DUR)));
            div().bg(rgb(0xffffff)).child(each((0..self.n).map(one)))
        }
    }
}

mod el {
    use bough::prelude::*;
    pub struct Eased {
        pub fade: Signal<f32>,
        pub n: usize,
    }
    impl View for Eased {
        fn view(&self) -> El<'_> {
            let fade = animate(&self.fade).curve(Curve::out(super::DUR));
            div().bg(rgb(0xffffff)).child((0..self.n).map(move |i| div().size(8, 8).bg(rgb(0x2050a0 + i as u32)).opacity(fade)))
        }
    }
    pub struct Sprung {
        pub fade: Signal<f32>,
        pub n: usize,
    }
    impl View for Sprung {
        fn view(&self) -> El<'_> {
            // a slow, underdamped spring: in flight for minutes
            let fade = animate(&self.fade).spring(Spring::response(400.0, 0.3));
            div().bg(rgb(0xffffff)).child((0..self.n).map(move |i| div().size(8, 8).bg(rgb(0x2050a0 + i as u32)).opacity(fade)))
        }
    }
}

trait Scene: bough::app::View {
    fn make(n: usize) -> Self;
    fn fade(&mut self) -> &mut bough::prelude::Signal<f32>;
}
impl Scene for typed::Boxes {
    fn make(n: usize) -> Self {
        typed::Boxes { fade: bough::prelude::Signal::new(0.0), n }
    }
    fn fade(&mut self) -> &mut bough::prelude::Signal<f32> {
        &mut self.fade
    }
}
impl Scene for el::Eased {
    fn make(n: usize) -> Self {
        el::Eased { fade: bough::prelude::Signal::new(0.0), n }
    }
    fn fade(&mut self) -> &mut bough::prelude::Signal<f32> {
        &mut self.fade
    }
}
impl Scene for el::Sprung {
    fn make(n: usize) -> Self {
        el::Sprung { fade: bough::prelude::Signal::new(0.0), n }
    }
    fn fade(&mut self) -> &mut bough::prelude::Signal<f32> {
        &mut self.fade
    }
}

fn app<S: Scene>(n: usize, once: bool) -> App<S> {
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    let mut cfg = Config::new(1280.0, 800.0, 1.0, font);
    cfg.caps = Caps { instances: 1 << 17, pages: 1 << 13, glyphs: 1 << 20, runs: 1 << 17, spaces: 1 << 12, clips: 1 << 12, strikes: 16 };
    if once {
        cfg = cfg.once();
    }
    let mut a = App::new(S::make(n), cfg, false);
    a.frame();
    a
}

/// (instructions, allocator calls) per call of `f`, by slope; `between` runs outside the brackets.
fn slope<A>(a: &mut A, f: &mut dyn FnMut(&mut A, u32), between: &mut dyn FnMut(&mut A)) -> (u64, f64) {
    let mut run = |a: &mut A, reps: u32| {
        let (mut ins, mut calls) = (0u64, 0usize);
        for k in 0..reps {
            let (t0, c0) = (tinstr(), CALLS.load(Relaxed));
            f(a, k);
            (ins, calls) = (ins + tinstr() - t0, calls + CALLS.load(Relaxed) - c0);
            between(a);
        }
        (ins, calls)
    };
    run(a, 8);
    let (lo, hi) = (run(a, 16), run(a, 48));
    ((hi.0 - lo.0) / 32, (hi.1 as f64 - lo.1 as f64) / 32.0)
}

/// Per tick with every box in flight, and per retarget turn.
fn cells<S: Scene>(n: usize) -> [(u64, f64); 3] {
    let mut a = app::<S>(n, true);
    a.turn(|v| {
        v.fade().set(1.0);
    });
    let tick = slope(&mut a, &mut |a, _| {
        a.tick(1.0 / 60.0);
    }, &mut |a| {
        a.frame();
    });
    assert!(a.animating(), "still in flight");
    let retarget = slope(&mut a, &mut |a, k| {
        a.turn(|v| {
            v.fade().set(0.25 + (k % 2) as f32 * 0.5);
        })
    }, &mut |a| {
        a.tick(1.0 / 60.0);
        a.frame();
    });
    // re-described: the frame clock's settle describes the view again after each tick
    let mut r = app::<S>(n, false);
    r.turn(|v| {
        v.fade().set(1.0);
    });
    let tick_re = slope(&mut r, &mut |a, _| {
        a.tick(1.0 / 60.0);
    }, &mut |a| {
        a.frame();
    });
    [tick, retarget, tick_re]
}

fn main() {
    let sizes: Vec<usize> = std::env::args().skip(1).filter_map(|s| s.parse().ok()).collect();
    let sizes = if sizes.is_empty() { vec![1, 1000] } else { sizes };
    println!("boxes,spelling,tick_instr,tick_allocs,retarget_instr,retarget_allocs,tick_redescribe_instr,tick_redescribe_allocs");
    for n in sizes {
        let rows = [("typed", cells::<typed::Boxes>(n)), ("el_eased", cells::<el::Eased>(n)), ("el_spring", cells::<el::Sprung>(n))];
        for (name, [t, r, re]) in rows {
            println!("{n},{name},{},{:.2},{},{:.2},{},{:.2}", t.0, t.1, r.0, r.1, re.0, re.1);
        }
    }
}
