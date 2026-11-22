//! What each operation of the vertical slice costs on the lowered runtime, on the typed path, and on the flat
//! core: process and thread instructions, medians. The harness is `card3_cost`'s (same cells, same brackets).
//!
//!   cargo run --release -p bough --example flat_cost            # table
//!   cargo run --release -p bough --example flat_cost -- tsv
//!
//! Columns:
//! * `lowered`: `bough::lower::App<Slice>` (the prelude slice described as data onto `World<V, B>`).
//! * `typed`: `bough::typed::App<Slice>` (the prelude slice on the typed-dataflow path).
//! * `flat`: `bough::rethink::FlatApp<Slice>` (the same prelude slice, described as data onto the flat core).
//! * `flat-ui`: `examples/flat_slice.rs`, the slice written against the flat core's bindings. Its contract is
//!   narrower and is not the other columns': the built-in 5 x 7 face instead of shaped text, every row
//!   materialised (no virtual list), layout computed at mount. "insert" and "resize" have no cell there.
//!
//! All four are headless at 640 x 480 points, scale 2, one thread. lowered/typed repaint with the lowered
//! canvas over a ring of 4 surfaces; flat/flat-ui repaint one surface with the flat reference raster. So the
//! "turn" columns compare the runtimes; "turn+frame" also compares rasters and ring sizes.
#[path = "deep_view.rs"]
#[allow(dead_code)]
mod deep_view;
#[path = "flat_deep.rs"]
#[allow(dead_code)]
mod flat_deep;
#[path = "flat_slice.rs"]
#[allow(dead_code)]
mod flat_slice;
#[path = "slice.rs"]
#[allow(dead_code)]
mod slice;

use bough::app::Config;
use bough::input::Button;
use slice::{ROW_H, Slice};

fn instrs() -> u64 {
    bough::sys::perf::snap().instrs
}
fn tinstr() -> u64 {
    bough::sys::perf::thread_instructions()
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

trait Rt: Sized {
    fn of(v: Slice) -> Self;
    fn frame(&mut self);
    fn turn(&mut self, f: impl FnOnce(&mut Slice));
    fn pointer_move(&mut self, x: f32, y: f32);
    fn click(&mut self, x: f32, y: f32);
    fn resize(&mut self, w: f32, h: f32);
}
macro_rules! rt {
    ($name:ident, $t:ty, $new:expr) => {
        struct $name($t);
        impl Rt for $name {
            fn of(v: Slice) -> Self {
                $name($new(v))
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
    };
}
rt!(Lowered, bough::lower::App<Slice>, |v| bough::lower::App::new(v, cfg()));
rt!(Typed, bough::typed::App<Slice>, |v| match bough::typed::App::new(v, cfg()) {
    Ok(a) => a,
    Err((_, why)) => panic!("the slice is not admitted to the typed path: {why:?}"),
});
rt!(Flat, bough::rethink::FlatApp<Slice>, |v| bough::rethink::FlatApp::new(v, cfg()));

/// (turn alone, turn + frame) medians of `op(k)` then `frame()`, net of the brackets' own cost.
fn cell(mut op: impl FnMut(u32), mut frame: impl FnMut()) -> (u64, u64) {
    let (ot, os) = (bough::sys::perf::tinstr_overhead(), bough::sys::perf::snap_overhead());
    for k in 0..10 {
        op(k);
        frame();
    }
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for k in 10..70 {
        let i0 = instrs();
        let t0 = tinstr();
        op(k);
        let t1 = tinstr();
        frame();
        let i2 = instrs();
        a.push((t1 - t0).saturating_sub(ot));
        b.push((i2 - i0).saturating_sub(os + ot));
    }
    (med(a), med(b))
}

const CELLS: [&str; 9] = [
    "mount, 1,000 rows",
    "mount, 100,000 rows",
    "hover, one row to the next",
    "one signal (the title)",
    "insert at 0 (click)",
    "scroll by one row",
    "theme",
    "resize by 40 pt",
    "still frame",
];

fn run<R: Rt>() -> Vec<Option<(u64, u64)>> {
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
        Some((med(v.iter().map(|x| x.0).collect()), med(v.iter().map(|x| x.1).collect())))
    };
    out.push(mount(1000));
    out.push(mount(100_000));
    let app = std::cell::RefCell::new(R::of(Slice::new(1000)));
    app.borrow_mut().frame();
    let mut c = |op: &dyn Fn(&mut R, u32)| Some(cell(|k| op(&mut app.borrow_mut(), k), || app.borrow_mut().frame()));
    out.push(c(&|a, k| a.pointer_move(320.0, 200.0 + (k % 2) as f32 * ROW_H)));
    out.push(c(&|a, k| {
        a.turn(|s| {
            s.title.set(format!("Slice {k}"));
        })
    }));
    out.push(c(&|a, _| a.click(331.0, 24.0)));
    out.push(c(&|a, k| {
        a.turn(|s| {
            s.scroll.set((k % 40) as f32 * ROW_H);
        })
    }));
    out.push(c(&|a, _| a.turn(|s| s.toggle_theme())));
    out.push(c(&|a, k| a.resize(640.0 + (k % 2) as f32 * 40.0, 480.0)));
    out.push(c(&|_, _| {}));
    out
}

fn run_ui() -> Vec<Option<(u64, u64)>> {
    use flat_slice::FlatSlice;
    let mut out = Vec::new();
    let mount = |n: usize| {
        let v: Vec<(u64, u64)> = (0..7)
            .map(|_| {
                let i0 = instrs();
                let mut s = FlatSlice::new(n, 1280, 960, 2.0);
                let i1 = instrs();
                s.ui.frame();
                (i1 - i0, instrs() - i0)
            })
            .collect();
        Some((med(v.iter().map(|x| x.0).collect()), med(v.iter().map(|x| x.1).collect())))
    };
    out.push(mount(1000));
    out.push(mount(100_000));
    let s = std::cell::RefCell::new(FlatSlice::new(1000, 1280, 960, 2.0));
    s.borrow_mut().ui.frame();
    // the flat core splits a turn in two: the write (here) and `settle` (the first half of `frame`). To
    // compare with the other columns' "turn", the op settles; the frame then only resolves and repaints.
    let mut c = |op: &dyn Fn(&mut FlatSlice, u32)| {
        Some(cell(
            |k| {
                let mut s = s.borrow_mut();
                op(&mut s, k);
                s.ui.world.settle();
            },
            || {
                s.borrow_mut().ui.frame();
            },
        ))
    };
    out.push(c(&|s, k| s.ui.pointer_move(640, 400 + (k % 2) as i32 * 56)));
    out.push(c(&|s, k| {
        let t = s.title;
        s.ui.world.state.set_str(t, if k % 2 == 0 { "Slice even" } else { "Slice odd" });
    }));
    out.push(None);
    out.push(c(&|s, k| {
        s.ui.world.set(s.scroll, (k % 40) as i32 * 56);
    }));
    out.push(c(&|s, k| s.ui.world.set_theme(k % 2)));
    out.push(None);
    out.push(c(&|_, _| {}));
    out
}

fn main() {
    let tsv = std::env::args().any(|a| a == "tsv");
    let cols = [("lowered", run::<Lowered>()), ("typed", run::<Typed>()), ("flat", run::<Flat>()), ("flat-ui", run_ui())];
    let load = std::process::Command::new("sysctl").args(["-n", "vm.loadavg"]).output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let show = |v: Option<(u64, u64)>, f: fn((u64, u64)) -> u64| v.map_or("-".to_string(), |x| f(x).to_string());
    if tsv {
        println!("cell\t{}\t{}", cols.iter().map(|c| format!("{}_turn", c.0)).collect::<Vec<_>>().join("\t"), cols.iter().map(|c| format!("{}_turn_frame", c.0)).collect::<Vec<_>>().join("\t"));
    } else {
        println!("instructions, net of the brackets; load average: {load}");
        println!("{:28} {:>11} {:>11} {:>11} {:>11}   {:>11} {:>11} {:>11} {:>11}", "cell", "lowered", "typed", "flat", "flat-ui", "lowered+f", "typed+f", "flat+f", "flat-ui+f");
    }
    for (k, name) in CELLS.iter().enumerate() {
        let t: Vec<String> = cols.iter().map(|c| show(c.1[k], |x| x.0)).collect();
        let f: Vec<String> = cols.iter().map(|c| show(c.1[k], |x| x.1)).collect();
        if tsv {
            println!("{name}\t{}\t{}", t.join("\t"), f.join("\t"));
        } else {
            println!("{name:28} {:>11} {:>11} {:>11} {:>11}   {:>11} {:>11} {:>11} {:>11}", t[0], t[1], t[2], t[3], f[0], f[1], f[2], f[3]);
        }
    }
    // what the flat columns' frames were made of
    let mut app = bough::rethink::FlatApp::new(Slice::new(1000), cfg());
    app.frame();
    let mut line = String::from("flat (prelude slice), last frame of each kind:");
    app.pointer_move(320.0, 200.0);
    app.frame();
    app.pointer_move(320.0, 200.0 + ROW_H);
    app.frame();
    line += &format!(" hover {} tiles {:.1}% {:?};", app.last.dirty_tiles, app.last.repair_percent, app.last.choice);
    app.turn(|s| {
        s.scroll.set(5.0 * ROW_H);
    });
    app.frame();
    app.turn(|s| {
        s.scroll.set(6.0 * ROW_H);
    });
    app.frame();
    line += &format!(" scroll by a row {} tiles {:.1}% blits {} {:?};", app.last.dirty_tiles, app.last.repair_percent, app.last.blits, app.last.choice);
    app.turn(|s| s.toggle_theme());
    app.frame();
    line += &format!(" theme {} tiles {:.1}% {:?}", app.last.dirty_tiles, app.last.repair_percent, app.last.choice);
    println!("{}{line}", if tsv { "# " } else { "" });
    deep(tsv);
    if tsv {
        println!("# load average: {load}");
    }
}

/// The deep view (sixteen levels, 900 x 1400 points at scale 1): the same four turns on each runtime.
fn deep(tsv: bool) {
    use deep_view::Deep;
    let dcfg = || {
        let mut c = Config::new(900.0, 1400.0, 1.0, bough::ui::default_font());
        c.ring = 4;
        c
    };
    let names = ["count += 1 (5 readers)", "fade (4 readers)", "shift (12 readers)", "hover, level to level"];
    let mut rows: Vec<Vec<String>> = names.iter().map(|n| vec![n.to_string()]).collect();
    let mut push = |cells: Vec<(u64, u64)>| {
        for (r, c) in rows.iter_mut().zip(cells) {
            r.push(format!("{} / {}", c.0, c.1));
        }
    };
    macro_rules! prelude {
        ($app:expr) => {{
            let app = std::cell::RefCell::new($app);
            app.borrow_mut().frame();
            let mut c = |op: &dyn Fn(u32)| cell(|k| op(k), || { app.borrow_mut().frame(); });
            vec![
                c(&|_| app.borrow_mut().turn(|d| d.inc())),
                c(&|k| app.borrow_mut().turn(|d| { d.fade.set(0.3 + (k % 50) as f32 / 100.0); })),
                c(&|k| app.borrow_mut().turn(|d| { d.shift.set((k % 9) as f32); })),
                c(&|k| app.borrow_mut().pointer_move(300.0, 200.0 + (k % 2) as f32 * 400.0)),
            ]
        }};
    }
    let mut head = vec!["cell (turn / turn+frame)".to_string(), "lowered".to_string()];
    push(prelude!(bough::lower::App::new(Deep::new(), dcfg())));
    match bough::typed::App::new(Deep::new(), dcfg()) {
        Ok(app) => {
            head.push("typed".to_string());
            push(prelude!(app));
        }
        Err((_, why)) => println!("{}the deep view is not admitted to the typed path: {why:?}", if tsv { "# " } else { "" }),
    }
    head.push("flat".to_string());
    push(prelude!(bough::rethink::FlatApp::new(Deep::new(), dcfg())));
    head.push("flat-ui".to_string());
    {
        let (ui, v) = flat_deep::mount(900, 1400);
        let ui = std::cell::RefCell::new(ui);
        ui.borrow_mut().frame();
        let mut c = |op: &dyn Fn(&mut bough::rethink::ui::FlatUi, u32)| {
            cell(
                |k| {
                    let mut ui = ui.borrow_mut();
                    op(&mut ui, k);
                    ui.world.settle();
                },
                || {
                    ui.borrow_mut().frame();
                },
            )
        };
        push(vec![
            c(&|ui, k| {
                ui.world.set(v.count, k);
            }),
            c(&|ui, k| {
                ui.world.set(v.fade, 76 + (k % 50) * 2);
            }),
            c(&|ui, k| {
                ui.world.set(v.shift, (k % 9) as i32);
            }),
            c(&|ui, k| ui.pointer_move(300, 200 + (k % 2) as i32 * 400)),
        ]);
    }
    println!("{}deep view, instructions:", if tsv { "# " } else { "" });
    let sep = if tsv { "\t" } else { "   " };
    println!("{}", head.iter().map(|h| format!("{h:>24}")).collect::<Vec<_>>().join(sep));
    for r in rows {
        println!("{}", r.iter().map(|h| format!("{h:>24}")).collect::<Vec<_>>().join(sep));
    }
}
