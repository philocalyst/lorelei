//! Head-to-head benchmark for the non-generic flat core (`bough_flat::FlatWorld` over `bough-damage`)
//! directly beside `typed-core.rs`: same 640x400 card grid, same cells (`startup`, `idle`, `one`, `many`,
//! `theme`, plus `scroll` and `hover` which `typed-core`'s opaque-rect adapter could not even express!).
use bough_sys::perf;
use bough_flat::store::ROOT;
use bough_flat::{Effect, FlatWorld, NoExt, RecordDesc, Rect, Slot, SpaceId, Val};
use std::time::Instant;

const W: i32 = 640;
const H: i32 = 400;
const BG: u32 = 0xff14_2638;

fn card_color(base: u32, dark: bool) -> u32 {
    0xff00_0000 | ((base ^ if dark { 0x00bfdf } else { 0x22b080 }) & 0x00ff_ffff)
}

struct BenchWorld {
    world: FlatWorld,
    scroll: Slot<i32>,
    fg_x: Slot<i32>,
    motion: Slot<i32>,
    cell: &'static str,
}

fn build(n: usize, cell: &'static str) -> BenchWorld {
    let mut world = FlatWorld::new(W, H);
    let scroll = world.slot(0i32);
    let fg_x = world.slot(0i32);
    let motion = world.slot(0i32);

    let mut pal = vec![0u32; 512];
    for i in 0..256usize {
        let base = (i as u32).wrapping_mul(0x0077_9b83);
        pal[i] = card_color(base, false);
        pal[256 + i] = card_color(base, true);
    }
    world.set_palette(256, &pal);

    // Opaque background quad at root (z=0)
    let bg_rec = world.add_record(RecordDesc::rect(ROOT, Rect::new(0, 0, W, H), BG));

    let strip = cell == "scroll";
    let list_space: SpaceId = if strip {
        world.add_space(ROOT, Some(bg_rec), true, 0, scroll)
    } else {
        ROOT
    };

    for i in 0..n {
        let slot = if strip { i } else { i % 100 };
        let x = (slot % 10) as i32 * 64;
        let y = (slot / 10) as i32 * 40;
        let pal_idx = (i % 256) as u16;
        let is_last = i + 1 == n;

        let x_val: Val = match cell {
            "one" if is_last => {
                let mut b = world.tape.build();
                let a = b.konst(x as u32);
                let d = b.slot(fg_x);
                let sum = b.add(a, d);
                b.finish(sum)
            }
            "many" => {
                let mut b = world.tape.build();
                let a = b.konst(x as u32);
                let d = b.slot(motion);
                let sum = b.add(a, d);
                b.finish(sum)
            }
            _ => Val::Const(x as u32),
        };

        let site = if cell == "hover" {
            world.add_site(1)
        } else {
            bough_flat::NO_SITE
        };

        let fill: Val = if cell == "hover" {
            world.hover_pal(site, pal_idx, 0xfff0_b65d)
        } else if cell == "theme" {
            world.pal(pal_idx)
        } else {
            Val::Const(pal[pal_idx as usize])
        };

        let mut rec = RecordDesc::rect(list_space, Rect::new(x, y, x + 60, y + 36), fill).radius_px(6);
        rec.x = x_val;
        if cell == "hover" {
            rec = rec.hit(site, (i + 1) as u32);
        }
        // Stacked outer shadow on every 10th card — zero generic monomorphization!
        if i % 10 == 0 && cell != "startup" {
            rec = rec.effect(Effect::new().shadow(0, 0x3800_0000u32, 0, 2, 4, 0));
        }
        world.add_record(rec);
    }

    world.commit();
    world.frame();
    BenchWorld {
        world,
        scroll,
        fg_x,
        motion,
        cell,
    }
}

fn run(n: usize, cell: &'static str, check: bool, steps: usize) {
    let overhead = perf::snap_overhead();
    let start = Instant::now();
    let a0 = perf::snap();
    let mut bw = build(n, cell);
    let a1 = perf::snap();
    println!(
        "flat_startup,{n},{cell},{},{},{},{}",
        start.elapsed().as_nanos(),
        a1.instrs.saturating_sub(a0.instrs).saturating_sub(overhead),
        a1.rss,
        a1.footprint
    );
    if check {
        assert_eq!(bw.world.pixels()[39 * W as usize + 63], BG);
        assert_eq!(bw.world.records(), n + 1);
    }
    if cell == "startup" {
        return;
    }
    if cell == "idle" {
        let a0 = perf::snap();
        bw.world.settle();
        let info = bw.world.frame();
        let a1 = perf::snap();
        println!(
            "flat_idle,{n},{},{},{},{}",
            a1.instrs.saturating_sub(a0.instrs).saturating_sub(overhead),
            a1.rss,
            a1.footprint,
            info.dirty_tiles
        );
        assert_eq!(info.dirty_tiles, 0);
        return;
    }

    let mut total_instr = 0u64;
    let mut total_ns = 0u128;
    let mut total_repair = 0.0f64;
    let mut any_dirty = false;
    for step in 0..steps {
        let a0 = perf::snap();
        let t0 = Instant::now();
        match bw.cell {
            "one" => {
                let off = if step % 2 == 0 { 10 } else { -10 };
                bw.world.set(bw.fg_x, off);
                bw.world.settle();
            }
            "many" => {
                let off = if step % 2 == 0 { 8 } else { -8 };
                bw.world.set(bw.motion, off);
                bw.world.settle();
            }
            "theme" => {
                let th = bw.world.theme_slot();
                let cur = bw.world.get(th);
                bw.world.set(th, 1 - cur);
                bw.world.settle();
            }
            "scroll" => {
                let cur = bw.world.get(bw.scroll);
                let dy = if step % 40 < 20 { 11 } else { -11 };
                bw.world.set(bw.scroll, (cur + dy).max(0));
                bw.world.settle();
            }
            "hover" => {
                let k = step % 40;
                let c = if k < 20 { k } else { 39 - k };
                let x = (c % 10) as i32 * 64 + 20;
                let y = (c / 10) as i32 * 40 + 20;
                bw.world.pointer_move(x, y);
                bw.world.settle();
            }
            _ => {}
        }
        let info = bw.world.frame();
        let elapsed = t0.elapsed().as_nanos();
        let a1 = perf::snap();
        let instr = a1.instrs.saturating_sub(a0.instrs).saturating_sub(overhead);
        total_instr += instr;
        total_ns += elapsed;
        total_repair += info.repair_percent as f64;
        any_dirty |= info.dirty_tiles > 0;
        if check && step < 6 {
            let full = bw.world.full_redraw(&mut NoExt);
            assert_eq!(
                bw.world.pixels(),
                full.px.as_slice(),
                "flat-core pixel mismatch on cell={cell} step={step}"
            );
        }
    }
    if check {
        assert!(any_dirty, "flat-core cell={cell} never produced any dirty tiles!");
    }
    println!(
        "flat_summary,{n},{cell},{steps},{},{},{:.2}",
        total_ns / steps as u128,
        total_instr / steps as u64,
        total_repair / steps as f64
    );
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [mode, count, cell, rest @ ..] = &args[..] else {
        eprintln!("usage: flat-core check|measure CARD_COUNT CELL [STEPS]");
        std::process::exit(2);
    };
    let steps = rest.first().and_then(|s| s.parse().ok()).unwrap_or(20);
    let count: usize = count.parse().unwrap_or(200);
    let cell: &'static str = match cell.as_str() {
        "startup" => "startup",
        "idle" => "idle",
        "one" => "one",
        "many" => "many",
        "theme" => "theme",
        "scroll" => "scroll",
        "hover" => "hover",
        other => {
            eprintln!("unknown cell: {other}");
            std::process::exit(2);
        }
    };
    run(count, cell, mode == "check", steps);
}
