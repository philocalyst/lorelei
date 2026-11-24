//! What a keyed list costs as El children against the typed list region (card "Keyed collections as El
//! children, and routes on the owner chain (B3)" in `docs/lanes/integration/2026-11-22-el-surface.md`).
//!
//! The same table both ways: a column holding a title (a plain field of the root) and a `List` of N rows, each
//! a painted 960x32 row with an integer text and a `Signal<String>` label. Typed: `list(sig!(self.items))`
//! with rows in `bough::builders`; El: `column(&self.items)` with rows in `bough::prelude`. Default config
//! (the root re-describes every turn). Instructions per `App::turn` on the calling thread by slope over two
//! repeat counts, allocator calls per turn; El and typed interleaved in one run.
//!
//! Cells: `mount` (App::new + first turn, N rows minus 0 rows, the least of three each; the region mounts the rows its visible rect
//! meets, 25 of 32-point rows in the 800-point window, plus the overscan), `edit` (one visible row's label set
//! through `List::edit`, in place), `move` (one visible row moved there and back), `splice` (one row inserted among the
//! visible ones and removed, per pair), `parent` (the root's title alternates between two values; the list does not change). Also the rows
//! described per turn.
//!
//!     cargo run --release -p bough --example el_list_cost -- [rows...]
//!
//! `RAW=1` prints each cell's two runs (instructions, allocator calls, rows described) to stderr.
use bough::raw::{App, Caps, Config};
use bough_sys::perf::tinstr;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

/// Allocation calls (as `redescribe_cost`'s counter: a load and a store each).
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

mod typed {
    use bough::builders::*;
    pub struct Row {
        pub id: u32,
        pub label: Signal<String>,
    }
    impl View for Row {
        fn view(&self) -> impl Element<Self> + '_ {
            row((text(self.id).w(64.0), text(sig!(self.label)))).w(960.0).h(32.0).bg(rgb(0xf0f0f0))
        }
    }
    pub struct Table {
        pub items: List<Row>,
        pub title: u32,
    }
    impl View for Table {
        fn view(&self) -> impl Element<Self> + '_ {
            column((text(self.title), list(sig!(self.items)).w_pct(1.0))).bg(rgb(0xffffff))
        }
    }
}

mod el {
    use bough::prelude::*;
    pub struct Row {
        pub id: u32,
        pub label: Signal<String>,
    }
    impl View for Row {
        fn view(&self) -> El<'_> {
            row((text(self.id).w(64), text(&self.label))).w(960).h(32).bg(rgb(0xf0f0f0))
        }
    }
    pub struct Table {
        pub items: List<Row>,
        pub title: u32,
    }
    impl View for Table {
        fn view(&self) -> El<'_> {
            column((text(self.title), column(&self.items))).bg(rgb(0xffffff))
        }
    }
}

/// One table type, either way.
trait Shape: bough::app::View + Sized {
    type Row;
    fn make(n: usize) -> Self;
    fn row(id: u32) -> Self::Row;
    fn items(&mut self) -> &mut bough::list::List<Self::Row>;
    fn label(r: &mut Self::Row) -> &mut bough::signal::Signal<String>;
    fn title(&mut self) -> &mut u32;
}
macro_rules! shape {
    ($m:ident) => {
        impl Shape for $m::Table {
            type Row = $m::Row;
            fn make(n: usize) -> Self {
                $m::Table { items: bough::list::List::from_iter_in((0..n as u32).map(Self::row)), title: 0 }
            }
            fn row(id: u32) -> $m::Row {
                $m::Row { id, label: bough::signal::Signal::new(format!("Row label {id}")) }
            }
            fn items(&mut self) -> &mut bough::list::List<$m::Row> {
                &mut self.items
            }
            fn label(r: &mut $m::Row) -> &mut bough::signal::Signal<String> {
                &mut r.label
            }
            fn title(&mut self) -> &mut u32 {
                &mut self.title
            }
        }
    };
}
shape!(typed);
shape!(el);

fn cfg(font: &[u8]) -> Config {
    let mut cfg = Config::new(1280.0, 800.0, 1.0, font.to_vec());
    cfg.caps = Caps { instances: 1 << 17, pages: 1 << 13, glyphs: 1 << 20, runs: 1 << 17, spaces: 1 << 14, clips: 1 << 12, strikes: 16 };
    cfg
}

fn described<V: bough::app::View>(a: &App<V>) -> u64 {
    a.rt.lists.regs().iter().map(|r| r.stats.rows_described).sum()
}

#[derive(Clone, Copy, Debug)]
enum Op {
    Edit,
    Move,
    Splice,
    Parent,
}

/// (instructions, allocator calls, rows described) per turn of `op`, by slope.
fn measure<T: Shape>(font: &[u8], n: usize, op: Op) -> (u64, f64, f64) {
    let mut a = App::new(T::make(n), cfg(font), false);
    a.turn(|_| {});
    // a row in the window (a list region without a viewport mounts the rows meeting the visible rect)
    let mid = 5;
    let mut k = 0u32;
    let mut run = |reps: u32| {
        let (t0, c0, d0) = (tinstr(), CALLS.load(Relaxed), described(&a));
        for _ in 0..reps {
            k += 1;
            match op {
                Op::Edit => a.turn(|t| {
                    // rewritten in place (a `Signal`'s `DerefMut` queues its wires): the turn allocates nothing
                    // of the bench's own
                    let s: &mut String = T::label(t.items().edit(mid));
                    s.clear();
                    s.push_str(if k % 2 == 0 { "Row label even" } else { "Row label odd" });
                }),
                Op::Move => a.turn(|t| if k % 2 == 0 { t.items().move_to(mid, mid + 3) } else { t.items().move_to(mid + 3, mid) }),
                Op::Splice => {
                    a.turn(|t| {
                        t.items().insert(mid, T::row(1_000_000 + k));
                    });
                    a.turn(|t| {
                        t.items().remove(mid);
                    });
                }
                Op::Parent => a.turn(|t| *t.title() = k % 2), // two values: no glyph is new after the warm-up
            }
        }
        (tinstr() - t0, CALLS.load(Relaxed) - c0, described(&a) - d0)
    };
    run(8);
    let (lo, hi) = (run(16), run(48));
    if std::env::var_os("RAW").is_some() {
        eprintln!("{op:?} n={n}: 16 reps {lo:?}, 48 reps {hi:?}");
    }
    ((hi.0 - lo.0) / 32, (hi.1 as f64 - lo.1 as f64) / 32.0, (hi.2 as f64 - lo.2 as f64) / 32.0)
}

/// Instructions and allocator calls to construct and first-turn an `App` of `n` rows (minus 0 rows).
fn mount<T: Shape>(font: &[u8], n: usize) -> (u64, f64) {
    let one = |n: usize| {
        let t = T::make(n);
        let c = cfg(font);
        let (t0, c0) = (tinstr(), CALLS.load(Relaxed));
        let mut a = App::new(t, c, false);
        a.turn(|_| {});
        let d = (tinstr() - t0, CALLS.load(Relaxed) - c0);
        std::mem::forget(a); // teardown is not measured
        d
    };
    one(1);
    // the least of three (a first mount pays one-time costs: page faults, glyphs, table growth)
    let best = |n: usize| (0..3).map(|_| one(n)).min().unwrap();
    let (z, f) = (best(0), best(n));
    (f.0 - z.0, f.1 as f64 - z.1 as f64)
}

fn main() {
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    println!("rows,op,typed_instr,el_instr,el_over_typed,typed_allocs,el_allocs,typed_described,el_described");
    for n in std::env::args().skip(1).filter_map(|s| s.parse::<usize>().ok()) {
        let (tm, em) = (mount::<typed::Table>(&font, n), mount::<el::Table>(&font, n));
        println!("{n},mount,{},{},{:.2},{:.0},{:.0},,", tm.0, em.0, em.0 as f64 / tm.0.max(1) as f64, tm.1, em.1);
        for op in [Op::Edit, Op::Move, Op::Splice, Op::Parent] {
            let t = measure::<typed::Table>(&font, n, op);
            let e = measure::<el::Table>(&font, n, op);
            println!("{n},{op:?},{},{},{:.2},{:.2},{:.2},{:.2},{:.2}", t.0, e.0, e.0 as f64 / t.0.max(1) as f64, t.1, e.1, t.2, e.2);
        }
    }
}
