//! Paint-stage diagnostic using the typed core's opaque rects and arithmetic placement.
use bough_sys::perf;
use bough_x::prelude::*;
use bough_x::soft::Soft;
use bough_x::store::Tiles;
use std::time::Instant;

const W: i32 = 640;
const H: i32 = 400;
const BG: u32 = 0xff14_2638;
lens!(Theme: u32 = 0);
lens!(Started: u32 = 1);
lens!(Direction: u32 = 2);

struct Canvas {
    count: usize,
    cell: &'static str,
    state: U,
}

fn themed((base, phase): (u32, u32)) -> Color {
    Color(0xff00_0000 | ((base ^ if phase == 0 { 0x22b080 } else { 0x00bfdf }) & 0xffffff))
}

impl View for Canvas {
    fn block(&self) -> &U {
        &self.state
    }

    fn view<S: Sink<Self>>(&self, sink: &mut S) {
        sink.row(|| ([Rect::new(0, 0, W, H)], Fill(rec(Color(BG)))));
        for i in 0..self.count {
            let x = (i % 10) as i32 * 64;
            let y = (i % 100 / 10) as i32 * 40;
            let rect = Rect::new(x, y, 60, 36);
            let base = (i as u32).wrapping_mul(0x0077_9b83);
            match self.cell {
                "one" | "many" if self.cell == "many" || i + 1 == self.count => {
                    let c = themed((base, 0));
                    let dest = (un::<Direction>(), rec(base)).map(|(d, base)| themed((base, d)));
                    sink.row(|| {
                        (
                            [rect],
                            Fill(Tween::<300, _, _, _>(rec(c), dest, un::<Started>())),
                        )
                    });
                }
                "theme" => sink.row(|| ([rect], Fill((rec(base), un::<Theme>()).map(themed)))),
                _ => sink.row(|| ([rect], Fill(rec(themed((base, 0)))))),
            }
        }
    }
}

fn make(n: usize, cell: &'static str) -> (Canvas, World<Canvas, Soft>) {
    let scene = Canvas {
        count: n,
        cell,
        state: U::default(),
    };
    let mut world = World::new(W, H, 3);
    world.tick(1_000);
    world.mount(&scene);
    (scene, world)
}

fn draw_at(surface: &mut Soft, world: &World<Canvas, Soft>, current: &Tiles) {
    surface.repair(current, |output| world.paint(output));
}

fn run(n: usize, cell: &'static str, check: bool, ring: usize) {
    let overhead = perf::snap_overhead();
    let start = Instant::now();
    let a0 = perf::snap();
    let (mut scene, mut world) = make(n, cell);
    let mut surfaces: Vec<_> = (0..ring).map(|_| Soft::new(W, H)).collect();
    let mut damage: Vec<_> = (0..ring).map(|_| Tiles::new(W, H)).collect();
    for d in &mut damage {
        d.mark_all();
    }
    draw_at(&mut surfaces[0], &world, &damage[0]);
    let a1 = perf::snap();
    println!(
        "core_startup,{n},{cell},{ring},{},{},{},{}",
        start.elapsed().as_nanos(),
        a1.instrs - a0.instrs - overhead,
        a1.rss,
        a1.footprint
    );
    let initial_pixel = surfaces[0].px[363 * W as usize + 580];
    if check {
        assert_eq!(surfaces[0].px[39 * W as usize + 63], BG);
        let mut full = Soft::new(W, H);
        world.paint(&mut full);
        assert!(surfaces[0].px == full.px);
    }
    damage[0].clear();
    world.tiles.clear();
    if cell == "idle" {
        let a0 = perf::snap();
        std::thread::sleep(std::time::Duration::from_secs(1));
        let a1 = perf::snap();
        println!(
            "core_idle,{n},{},{},{}",
            a1.instrs.saturating_sub(a0.instrs).saturating_sub(overhead),
            a1.rss,
            a1.footprint
        );
        assert!(!world.tiles.any());
        return;
    }
    if cell == "startup" {
        return;
    }
    let mut changed = false;
    for step in 0..20 {
        let a0 = perf::snap();
        let start = Instant::now();
        let now = 1_001 + (step as u32 * 1_000) / 120;
        world.tick(now);
        if cell == "theme" || step == 0 || step == 12 {
            if cell == "theme" {
                scene.state.set::<Theme>((step + 1) as u32 % 2);
            } else {
                scene.state.set::<Started>(now);
                scene.state.set::<Direction>(u32::from(step == 0));
            }
            world.settle(&scene);
        }
        for d in &mut damage {
            world.tiles.each(|rect| d.mark(rect));
        }
        let slot = (step + 1) % ring;
        draw_at(&mut surfaces[slot], &world, &damage[slot]);
        let elapsed = start.elapsed().as_nanos();
        let a1 = perf::snap();
        let instr = a1.instrs.saturating_sub(a0.instrs).saturating_sub(overhead);
        println!(
            "core_step,{n},{cell},{ring},{step},{elapsed},{instr},{},{},{},{}",
            a1.cpu_ns - a0.cpu_ns,
            a1.rss,
            a1.footprint,
            damage[slot].count()
        );
        if check {
            let mut full = Soft::new(W, H);
            world.paint(&mut full);
            assert!(surfaces[slot].px == full.px, "ring {ring}, step {step}");
            changed |= full.px[363 * W as usize + 580] != initial_pixel;
        }
        damage[slot].clear();
        world.tiles.clear();
    }
    if check {
        assert!(changed, "sample pixel never changed");
        world.tick(2_000);
        let mut fresh = World::<Canvas, Soft>::new(W, H, 3);
        fresh.tick(2_000);
        fresh.mount(&scene);
        let (mut a, mut b) = (Soft::new(W, H), Soft::new(W, H));
        world.paint(&mut a);
        fresh.paint(&mut b);
        assert!(a.px == b.px, "fresh mount differs");
    }
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [mode, count, cell] = &args[..] else {
        panic!("usage: typed-core check|measure CARD_COUNT CELL");
    };
    let count: usize = count.parse().unwrap();
    assert!([200, 1000, 5000].contains(&count));
    let cell = match cell.as_str() {
        "startup" => "startup",
        "idle" => "idle",
        "one" => "one",
        "many" => "many",
        "theme" => "theme",
        _ => {
            eprintln!("unavailable in the opaque-rect adapter: {cell}");
            std::process::exit(2);
        }
    };
    match mode.as_str() {
        "check" => {
            for ring in 1..=4 {
                run(count, cell, true, ring);
            }
        }
        "measure" => run(count, cell, false, 4),
        _ => std::process::exit(2),
    }
}
