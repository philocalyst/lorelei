//! Offscreen test host for the author scene in `scene.rs`.
#[cfg(feature = "full-effects")]
mod full_effects;
mod scene;

use bough::app::{App, Config};
use bough::backend::MemSurfaces;
use bough_sys::perf;
use scene::{BACKGROUND, HEIGHT, Scene, State, WIDTH};
use std::{
    env,
    hint::black_box,
    process::ExitCode,
    time::{Duration, Instant},
};

const TICK: f32 = 1.0 / 120.0;
const CELLS: &[&str] = &[
    "startup", "idle", "one", "many", "hover", "theme", "scroll", "resize",
];

fn mount(
    n: usize,
    strip: bool,
    state: State,
    ring: usize,
    size: (f32, f32),
    font: &[u8],
) -> App<Scene> {
    let mut cfg = Config::new(size.0, size.1, 1.0, font.to_owned()).once();
    cfg.ring = ring;
    let mut app = App::new(Scene::new(n, strip, state), cfg, false);
    app.target = Some(Box::new(MemSurfaces::new(
        size.0 as u32,
        size.1 as u32,
        ring,
    )));
    app
}

fn pointer(step: usize) -> (f32, f32) {
    let k = step % 40;
    let cell = if k < 20 { k } else { 39 - k };
    (
        (cell % 10) as f32 * 64.0 + 20.0,
        (cell / 10) as f32 * 40.0 + 20.0,
    )
}

fn update(a: &mut App<Scene>, cell: &str, step: usize, mutant: bool) {
    match cell {
        "one" | "many" => {
            if step % 40 == 0 || step % 40 == 12 {
                let target = if step.is_multiple_of(40) { 1 } else { -1 };
                a.turn(|s| {
                    s.foreground.set(target as f32 * 10.0);
                    if cell == "many" {
                        s.motion.set(target);
                    }
                });
            }
            a.tick(TICK);
        }
        "hover" => {
            let (x, y) = pointer(step);
            a.pointer_move(x, y);
        }
        "theme" => {
            if !mutant {
                a.turn(|s| {
                    s.theme.set(!*s.theme);
                });
            }
        }
        "scroll" => {
            let dy = if step % 40 < 20 { 11.0 } else { -11.0 };
            a.wheel_points(100.0, 100.0, dy);
        }
        "resize" => {
            let size = size_at(cell, Some(step));
            a.resize(size.0, size.1);
        }
        _ => unreachable!(),
    }
}

fn input(a: &mut App<Scene>, cell: &str, step: usize, mutant: bool) -> bool {
    update(a, cell, step, mutant);
    let drawn = a.frame();
    a.wait_presented();
    drawn
}

fn profile_one(n: usize, iterations: usize, font: &[u8]) {
    let mut app = mount(n, false, State::default(), 4, (WIDTH, HEIGHT), font);
    assert!(app.frame());
    app.wait_presented();
    let floor = perf::tinstr_overhead();
    for step in 0..iterations {
        let a = perf::tinstr();
        update(&mut app, "one", step, false);
        let b = perf::tinstr();
        app.frame();
        let c = perf::tinstr();
        app.wait_presented();
        let d = perf::tinstr();
        let staged = app.kernel.as_ref().map(|k| k.stats.staged).unwrap_or(0);
        println!(
            "facade_profile,{n},{step},{},{},{},{staged}",
            b.saturating_sub(a + floor),
            c.saturating_sub(b + floor),
            d.saturating_sub(c + floor)
        );
    }
}

fn size_at(cell: &str, step: Option<usize>) -> (f32, f32) {
    if cell == "resize" && step.is_some_and(|s| s % 2 == 0) {
        (704.0, 448.0)
    } else {
        (WIDTH, HEIGHT)
    }
}

fn checksum(px: &[u8]) -> u64 {
    px.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
    })
}

fn gate(n: usize, cell: &str, ring: usize, font: &[u8], mutant: bool) -> u64 {
    let mut a = mount(
        n,
        cell == "scroll",
        State::default(),
        ring,
        (WIDTH, HEIGHT),
        font,
    );
    assert!(a.frame());
    a.wait_presented();
    let first = a.presented_pixels();
    let i = (39 * WIDTH as usize + 63) * 4;
    let bg = BACKGROUND.to_le_bytes();
    assert_eq!(&first[i..i + 4], &[bg[0], bg[1], bg[2], 255]);
    assert_ne!(
        &first[16 * 4 + 16 * WIDTH as usize * 4..][..4],
        &[bg[0], bg[1], bg[2], 255]
    );
    assert!(
        first == a.reference_pixels(),
        "first retained redraw differs"
    );
    let mut changed = false;
    let mut previous = checksum(&first);
    let mut witness = previous;
    let last_step = if matches!(cell, "idle" | "startup") {
        None
    } else {
        Some(19)
    };
    if cell == "idle" {
        assert!(!a.dirty());
        assert!(!a.frame());
    }
    if let Some(last) = last_step {
        for step in 0..=last {
            input(&mut a, cell, step, mutant);
            let pixels = a.presented_pixels();
            assert!(
                pixels == a.reference_pixels(),
                "retained redraw differs at step {step}"
            );
            let hash = checksum(&pixels);
            changed |= hash != previous;
            previous = hash;
            witness = witness.rotate_left(5) ^ hash;
        }
        assert!(changed, "trace never changed pixels");
    }
    if matches!(cell, "one" | "many") {
        a.tick(1.0);
        a.frame();
        a.wait_presented();
    }
    let mut fresh = mount(
        n,
        cell == "scroll",
        a.owner.state(),
        ring,
        size_at(cell, last_step),
        font,
    );
    assert!(fresh.frame());
    fresh.wait_presented();
    if cell == "hover" || cell == "scroll" {
        let (x, y) = if cell == "hover" {
            pointer(last_step.unwrap())
        } else {
            (100.0, 100.0)
        };
        fresh.pointer_move(x, y);
        fresh.frame();
        fresh.wait_presented();
    }
    let actual = a.presented_pixels();
    let expected = fresh.presented_pixels();
    assert_eq!(actual.len(), expected.len());
    let mut differences = actual
        .chunks_exact(4)
        .zip(expected.chunks_exact(4))
        .enumerate()
        .filter(|(_, (a, b))| a != b);
    if let Some((pixel, (actual, expected))) = differences.next() {
        let width = size_at(cell, last_step).0 as usize;
        panic!(
            "fresh mount differs at rest, {} pixels, first at ({}, {}): {actual:?} versus {expected:?}",
            differences.count() + 1,
            pixel % width,
            pixel / width
        );
    }
    witness
}

fn damage(a: &App<Scene>, size: (f32, f32)) -> f64 {
    let (w, h) = (size.0 as usize, size.1 as usize);
    let mut area = 0;
    for (y, &row) in a.debug_rows().iter().take(h.div_ceil(32)).enumerate() {
        for x in 0..w.div_ceil(32) {
            if row & (1 << x) != 0 {
                area += (w - x * 32).min(32) * (h - y * 32).min(32);
            }
        }
    }
    area as f64 / (w * h) as f64
}

fn percentiles(mut count: Vec<u64>) -> [u64; 3] {
    count.sort_unstable();
    [50, 95, 99].map(|p| count[(count.len() * p).div_ceil(100) - 1])
}

fn measure(n: usize, cell: &str, ring: usize, samples: usize, font: &[u8]) {
    let mut app = (cell != "startup").then(|| {
        let mut a = mount(
            n,
            cell == "scroll",
            State::default(),
            ring,
            (WIDTH, HEIGHT),
            font,
        );
        a.frame();
        a.wait_presented();
        a
    });
    let mut walls = Vec::with_capacity(samples);
    let mut draws = 0;
    let mut fraction = 0.0;
    let mut batch_instructions = 0;
    let mut batch_cpu = 0;
    let mut rss = 0;
    let mut footprint = 0;
    let floor = perf::snap_overhead();
    for i in 0..samples {
        let a0 = perf::snap();
        let t = Instant::now();
        let mut started = None;
        let drawn = match cell {
            "startup" => {
                let mut a = mount(n, false, State::default(), ring, (WIDTH, HEIGHT), font);
                assert!(a.frame());
                a.wait_presented();
                started = Some(a);
                true
            }
            "idle" => {
                std::thread::sleep(Duration::from_secs(1));
                let a = app.as_ref().unwrap();
                assert!(!a.dirty());
                false
            }
            _ => input(app.as_mut().unwrap(), cell, i, false),
        };
        let ns = t.elapsed().as_nanos() as u64;
        let a1 = perf::snap();
        let instr = a1.instrs.saturating_sub(a0.instrs).saturating_sub(floor);
        let cpu = a1.cpu_ns.saturating_sub(a0.cpu_ns);
        batch_instructions += instr;
        batch_cpu += cpu;
        rss = rss.max(a1.rss);
        footprint = footprint.max(a1.footprint);
        let f = if drawn {
            draws += 1;
            if cell == "resize" {
                f64::NAN
            } else if cell == "startup" {
                1.0
            } else {
                damage(app.as_ref().unwrap(), size_at(cell, Some(i)))
            }
        } else {
            0.0
        };
        fraction += f;
        black_box(started);
        walls.push(ns);
        println!(
            "step,{n},{cell},{i},{ns},{instr},{cpu},{},{},{f:.5},{drawn}",
            a1.rss, a1.footprint
        );
    }
    let [p50, p95, p99] = percentiles(walls);
    println!(
        "summary,{n},{cell},{samples},{draws},{p50},{p95},{p99},{},{},{rss},{footprint},{:.5}",
        batch_instructions / samples as u64,
        batch_cpu / samples as u64,
        fraction / samples as f64
    );
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let [mode, n, cell, count] = &args[..] else {
        eprintln!("usage: bough-fx-bench check|measure|cold|mutant CARDS CELL SAMPLES");
        return ExitCode::FAILURE;
    };
    let n: usize = n.parse().expect("card count");
    let count: usize = count.parse().expect("sample count");
    assert!([200, 1000, 5000].contains(&n) && count > 0);
    if cell == "video" || mode == "full-effects" {
        #[cfg(feature = "full-effects")]
        eprintln!(
            "unavailable in the author frontend for {:#?}",
            full_effects::OPERATIONS
        );
        #[cfg(not(feature = "full-effects"))]
        eprintln!(
            "unavailable in the author frontend. See scripts/build-fx-seam.sh for the staged probe."
        );
        return ExitCode::from(2);
    }
    assert!(CELLS.contains(&cell.as_str()));
    let font = bough::ui::default_font();
    match mode.as_str() {
        "check" | "mutant" => {
            assert!(mode != "mutant" || cell == "theme");
            for ring in 1..=4 {
                let witness = gate(n, cell, ring, &font, mode == "mutant");
                println!("gate,{n},{cell},{ring},{witness:016x}");
            }
        }
        "measure" => measure(n, cell, 4, count, &font),
        "profile" => profile_one(n, count, &font),
        "profile-mount" => profile_one(n, 0, &font),
        "cold" => {
            assert_eq!((cell.as_str(), count), ("startup", 1));
            let mut app = mount(n, false, State::default(), 4, (WIDTH, HEIGHT), &font);
            assert!(app.frame());
            app.wait_presented();
            let ns = perf::since_exec_ns();
            let process = perf::snap();
            println!(
                "spawn_to_first,{n},{ns},{},{},{},{}",
                process.instrs, process.cpu_ns, process.rss, process.footprint
            );
        }
        _ => return ExitCode::FAILURE,
    }
    ExitCode::SUCCESS
}
