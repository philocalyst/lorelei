//! Integration Card 3: what each operation of the vertical slice (`examples/slice.rs`) costs on the legacy
//! runtime (`bough::raw::App`) and on the lowered one (`bough::lower::App`): process instructions, medians.
//!
//!   BOUGH_REPAIR=kernel cargo run --release -p bough --example card3_cost            # table to stdout
//!   BOUGH_REPAIR=kernel cargo run --release -p bough --example card3_cost -- tsv     # tab-separated
//!
//! Instruction counts are the process's (every thread), read around the call; 60 repetitions after 10 warm-up
//! ones, the median. "turn" is the input call alone; "turn+frame" adds the frame that repaints what it damaged.
//! Both runtimes are headless with a ring of 4 surfaces at 640 x 480 points, scale 2. The legacy runtime draws
//! with whatever `BOUGH_REPAIR` selects (set `kernel` for its CPU kernel; the default is its Metal presenter, whose
//! instructions are mostly the driver's); the lowered runtime draws with its own integer compositor on this thread.
//! So the "turn" columns compare the runtimes; the "turn+frame" columns also compare two different rasters.
#[path = "slice.rs"]
#[allow(dead_code)]
mod slice;

use bough::app::{Config, Repair};
use bough::input::Button;
use slice::{ROW_H, Slice};

fn instrs() -> u64 {
    bough::sys::perf::snap().instrs
}
fn med(mut v: Vec<u64>) -> u64 {
    v.sort();
    v[v.len() / 2]
}
fn cfg() -> Config {
    let mut c = Config::new(640.0, 480.0, 2.0, bough::ui::default_font());
    c.ring = 4;
    c
}

/// What the benchmark needs of a runtime.
trait Rt: Sized {
    const NAME: &'static str;
    fn new(n: usize) -> Self {
        Self::of(Slice::new(n))
    }
    /// Mount a view that is already built (building 100,000 items is the application's cost, not a runtime's).
    fn of(v: Slice) -> Self;
    fn frame(&mut self);
    fn turn(&mut self, f: impl FnOnce(&mut Slice));
    fn pointer_move(&mut self, x: f32, y: f32);
    fn click(&mut self, x: f32, y: f32);
    fn resize(&mut self, w: f32, h: f32);
}
struct Legacy(bough::raw::App<Slice>);
impl Rt for Legacy {
    const NAME: &'static str = "legacy";
    fn of(v: Slice) -> Self {
        let mut a = bough::raw::App::new(v, cfg(), true);
        a.repair = Repair::from_env();
        Legacy(a)
    }
    fn frame(&mut self) {
        self.0.frame();
        self.0.wait_presented();
    }
    fn turn(&mut self, f: impl FnOnce(&mut Slice)) {
        self.0.turn(f)
    }
    fn pointer_move(&mut self, x: f32, y: f32) {
        self.0.pointer_move(x, y)
    }
    fn click(&mut self, x: f32, y: f32) {
        self.0.pointer_down(x, y, Button::Primary);
        self.0.pointer_up(x, y, Button::Primary);
    }
    fn resize(&mut self, w: f32, h: f32) {
        self.0.resize(w, h)
    }
}
struct Lowered(bough::lower::App<Slice>);
impl Rt for Lowered {
    const NAME: &'static str = "lowered";
    fn of(v: Slice) -> Self {
        Lowered(bough::lower::App::new(v, cfg()))
    }
    fn frame(&mut self) {
        self.0.frame();
    }
    fn turn(&mut self, f: impl FnOnce(&mut Slice)) {
        self.0.turn(f)
    }
    fn pointer_move(&mut self, x: f32, y: f32) {
        self.0.pointer_move(x, y)
    }
    fn click(&mut self, x: f32, y: f32) {
        self.0.pointer_down(x, y, Button::Primary);
        self.0.pointer_up(x, y, Button::Primary);
    }
    fn resize(&mut self, w: f32, h: f32) {
        self.0.resize(w, h)
    }
}

struct Typed(bough::typed::App<Slice>);
impl Rt for Typed {
    const NAME: &'static str = "typed";
    fn of(v: Slice) -> Self {
        match bough::typed::App::new(v, cfg()) {
            Ok(a) => Typed(a),
            Err((_, why)) => panic!("the slice is not admitted to the typed path: {why:?}"),
        }
    }
    fn frame(&mut self) {
        self.0.frame();
    }
    fn turn(&mut self, f: impl FnOnce(&mut Slice)) {
        self.0.turn(f)
    }
    fn pointer_move(&mut self, x: f32, y: f32) {
        self.0.pointer_move(x, y)
    }
    fn click(&mut self, x: f32, y: f32) {
        self.0.pointer_down(x, y, Button::Primary);
        self.0.pointer_up(x, y, Button::Primary);
    }
    fn resize(&mut self, w: f32, h: f32) {
        self.0.resize(w, h)
    }
}

fn tinstr() -> u64 {
    bough::sys::perf::thread_instructions()
}
/// (turn alone, turn + frame) medians of `op(app, k)`. The turn is bracketed with the calling thread's counter
/// and the bracket's own cost is subtracted (Card 4's method); turn + frame with the process counter, likewise
/// net of its bracket (the legacy runtime may draw on other threads).
fn cell<R: Rt>(app: &mut R, mut op: impl FnMut(&mut R, u32)) -> (u64, u64) {
    let (ot, os) = (bough::sys::perf::tinstr_overhead(), bough::sys::perf::snap_overhead());
    for k in 0..10 {
        op(app, k);
        app.frame();
    }
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for k in 10..70 {
        let i0 = instrs();
        let t0 = tinstr();
        op(app, k);
        let t1 = tinstr();
        app.frame();
        let i2 = instrs();
        a.push((t1 - t0).saturating_sub(ot));
        b.push((i2 - i0).saturating_sub(os + ot));
    }
    (med(a), med(b))
}

fn run<R: Rt>() -> Vec<(&'static str, u64, u64)> {
    let mut out = Vec::new();
    let mount = |n: usize| {
        let v: Vec<(u64, u64)> = (0..7)
            .map(|_| {
                let v = Slice::new(n);
                let i0 = instrs();
                let mut app = R::of(v);
                let i1 = instrs();
                app.frame();
                (i1 - i0, instrs() - i0)
            })
            .collect();
        (
            med(v.iter().map(|x| x.0).collect()),
            med(v.iter().map(|x| x.1).collect()),
        )
    };
    let (a, b) = mount(1000);
    out.push(("mount, 1,000 rows", a, b));
    let (a, b) = mount(100_000);
    out.push(("mount, 100,000 rows", a, b));
    let mut app = R::new(1000);
    app.frame();
    let (a, b) = cell(&mut app, |app, k| {
        app.pointer_move(320.0, 200.0 + (k % 2) as f32 * ROW_H)
    });
    out.push(("hover, one row to the next", a, b));
    let (a, b) = cell(&mut app, |app, k| {
        app.turn(|s| {
            s.title.set(format!("Slice {k}"));
        })
    });
    out.push(("one signal (the title)", a, b));
    let (a, b) = cell(&mut app, |app, _| app.click(331.0, 24.0));
    out.push(("insert at 0 (click)", a, b));
    let (a, b) = cell(&mut app, |app, k| {
        app.turn(|s| {
            s.scroll.set((k % 40) as f32 * ROW_H);
        })
    });
    out.push(("scroll by one row", a, b));
    let (a, b) = cell(&mut app, |app, _| app.turn(|s| s.toggle_theme()));
    out.push(("theme (opaque state)", a, b));
    let (a, b) = cell(&mut app, |app, k| {
        app.resize(640.0 + (k % 2) as f32 * 40.0, 480.0)
    });
    out.push(("resize by 40 pt", a, b));
    let (a, b) = cell(&mut app, |_, _| {});
    out.push(("still frame", a, b));
    out
}

fn main() {
    let tsv = std::env::args().any(|a| a == "tsv");
    let (old, low, new) = (run::<Legacy>(), run::<Lowered>(), run::<Typed>());
    let load = std::process::Command::new("sysctl").args(["-n", "vm.loadavg"]).output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let r = |a: u64, b: u64| if a == 0 { f64::NAN } else { b as f64 / a as f64 };
    if tsv {
        println!("cell\tlegacy_turn\tlowered_turn\ttyped_turn\ttyped_over_legacy\tlegacy_turn_frame\tlowered_turn_frame\ttyped_turn_frame\ttyped_over_legacy_frame");
    } else {
        println!("legacy repair: {:?}; load average: {load}", Repair::from_env());
        println!("{:30} {:>10} {:>10} {:>10} {:>7}   {:>10} {:>10} {:>10} {:>7}", "cell (instructions, net)", "legacy", "lowered", "typed", "t/leg", "legacy+f", "lowered+f", "typed+f", "t/leg");
    }
    for ((o, l), n) in old.iter().zip(&low).zip(&new) {
        if tsv {
            println!("{}\t{}\t{}\t{}\t{:.3}\t{}\t{}\t{}\t{:.3}", o.0, o.1, l.1, n.1, r(o.1, n.1), o.2, l.2, n.2, r(o.2, n.2));
        } else {
            println!("{:30} {:>10} {:>10} {:>10} {:>6.2}x   {:>10} {:>10} {:>10} {:>6.2}x", o.0, o.1, l.1, n.1, r(o.1, n.1), o.2, l.2, n.2, r(o.2, n.2));
        }
    }
    // where a typed re-description's instructions go: the view and its conversion alone, then a whole
    // re-description of an unchanged view (conversion again, layout words, the typed compare, the rows)
    if let Ok(mut t) = bough::typed::App::new(Slice::new(1000), cfg()) {
        t.frame();
        let mut v: Vec<(u64, u64)> = (0..41).map(|_| t.profile()).collect();
        v.sort();
        let (c, d) = v[20];
        let line = format!("typed, thread instructions: view + conversion of the fixed part {c}; a re-description of an unchanged view {d}");
        if tsv {
            println!("# {line}");
        } else {
            println!("{line}");
        }
    }
    // where a lowered turn's instructions go (the title edit): view and lowering, layout and emission, the world
    let mut app = bough::lower::App::new(Slice::new(1000), cfg());
    app.frame();
    app.stats.profile = true;
    for k in 0..50 {
        app.turn(|s| {
            s.title.set(format!("Profile {k}"));
        });
    }
    let st = app.stats.stage.map(|v| v / 50);
    let line = format!(
        "lowered title edit, thread instructions per turn: view+lower {} / layout+emit {} / world compare {} ({} records)",
        st[0],
        st[1],
        st[2],
        app.record_count()
    );
    if tsv {
        println!("# {line}");
    } else {
        println!("{line}");
    }
    if tsv {
        println!(
            "# load average: {load}; legacy repair: {:?}",
            Repair::from_env()
        );
    }
}
