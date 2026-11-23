//! The damage engine's frame cost, counted like `bough-dataflow`'s `sheet`: instructions and cycles of the
//! calling thread per frame (`begin_frame`, the frame's damage calls, `resolve`), bracket cost subtracted,
//! median and 95th percentile of `REPS` frames after a warm-up of the same script.
//!
//!   SCENE=stack REPS=64 cargo run --profile dist -p bough-damage --example damage
//!
//! Scenes (`SCENE`):
//! * `window` (default): 1280 x 800, a header and 24 opaque cards as covers, one glass pane and a glass header;
//! * `stack`: 1280 x 800, 64 covers and three overlapping glass panes stacked on each other (levels 5, 4, 3),
//!   so damage under the lowest cascades through the other two;
//! * `4k`: 3840 x 2160, 1024 covers in a grid and four layers (two stacked).
use bough_damage::*;
use bough_mem::pmu;
use std::hint::black_box;

struct Scene {
    w: i32,
    h: i32,
    covers: Vec<OpaqueCover>,
    layers: Vec<BlurLayer>,
    /// A point under the deepest glass, and the list a scroll moves.
    under: (i32, i32),
    list: Rect,
    theme: TileMask,
}

fn scene(name: &str) -> Scene {
    let (w, h) = if name == "4k" { (3840, 2160) } else { (1280, 800) };
    let mut covers = vec![OpaqueCover { z: 0, core_rect: Rect::new(0, 0, w, h) }, OpaqueCover { z: 900, core_rect: Rect::new(0, 0, w, 56) }];
    let (cols, rows) = match name {
        "4k" => (32, 32),
        "stack" => (8, 8),
        _ => (6, 4),
    };
    let (cw, ch) = ((w - 48) / cols, (h - 80) / rows);
    for r in 0..rows {
        for c in 0..cols {
            let x = 24 + c * cw;
            let y = 70 + r * ch;
            covers.push(OpaqueCover { z: 10 + (r * cols + c) as u32 * 4, core_rect: opaque_core(Rect::new(x, y, x + cw - 16, y + ch - 12), 14) });
        }
    }
    let mut layers = Vec::new();
    let top = 10 + (rows * cols) as u32 * 4 + 10;
    match name {
        "stack" => {
            layers.push(BlurLayer { id: 1, z: top, rect: Rect::new(300, 200, 800, 600), radius: 32, max_level: 5 });
            layers.push(BlurLayer { id: 2, z: top + 2, rect: Rect::new(600, 300, 1000, 700), radius: 16, max_level: 4 });
            layers.push(BlurLayer { id: 3, z: top + 4, rect: Rect::new(700, 150, 1100, 450), radius: 8, max_level: 3 });
        }
        "4k" => {
            layers.push(BlurLayer { id: 1, z: top, rect: Rect::new(900, 600, 2400, 1500), radius: 32, max_level: 5 });
            layers.push(BlurLayer { id: 2, z: top + 2, rect: Rect::new(2000, 1200, 3200, 1900), radius: 16, max_level: 4 });
            layers.push(BlurLayer { id: 3, z: top + 4, rect: Rect::new(200, 1700, 1000, 2100), radius: 8, max_level: 3 });
        }
        _ => {
            layers.push(BlurLayer { id: 1, z: top, rect: Rect::new(420, 250, 860, 560), radius: 14, max_level: 4 });
        }
    }
    layers.push(BlurLayer { id: 9, z: 901, rect: Rect::new(0, 0, w, 56), radius: 8, max_level: 3 });
    let mut theme = TileMask::new(w, h);
    for c in covers.iter().skip(2).step_by(2) {
        theme.mark_rect(c.core_rect);
    }
    let l0 = layers[0].rect;
    Scene { w, h, covers, layers, under: ((l0.x0 + l0.x1) / 2, (l0.y0 + l0.y1) / 2), list: Rect::new(0, 56, w / 3, h), theme }
}

fn commit(e: &mut DamageEngine, s: &Scene) {
    e.covers_clear();
    for c in &s.covers {
        e.cover_push(*c);
    }
    e.covers_commit();
    e.layers_clear();
    for l in &s.layers {
        e.layer_push(*l);
    }
    e.layers_commit();
}

struct Meter {
    bracket: [u64; 2],
}
impl Meter {
    fn new() -> Meter {
        let mut v: Vec<[u64; 2]> = (0..64)
            .map(|_| {
                let a = pmu::sample();
                let d = pmu::sample().since(&a);
                [d.v[0], d.v[1]]
            })
            .collect();
        let mut bracket = [0; 2];
        for (i, b) in bracket.iter_mut().enumerate() {
            v.sort_by_key(|s| s[i]);
            *b = v[v.len() / 2][i];
        }
        Meter { bracket }
    }
    /// Run `frame(k)` for a warm-up and then `reps` times; print the medians.
    fn phase(&self, name: &str, reps: usize, e: &mut DamageEngine, s: &Scene, mut frame: impl FnMut(&mut DamageEngine, &Scene, usize)) {
        for k in 0..16 {
            frame(e, s, k);
        }
        let mut rows: Vec<(u64, u64, u32)> = Vec::with_capacity(reps);
        for k in 0..reps {
            let a = pmu::sample();
            frame(e, s, 16 + k);
            let d = pmu::sample().since(&a);
            rows.push((d.v[0].saturating_sub(self.bracket[0]), d.v[1].saturating_sub(self.bracket[1]), black_box(e.mask().count())));
        }
        let pick = |f: &dyn Fn(&(u64, u64, u32)) -> u64| {
            let mut x: Vec<u64> = rows.iter().map(f).collect();
            x.sort();
            (x[x.len() / 2], x[(x.len() * 95 / 100).min(x.len() - 1)])
        };
        let (i50, i95) = pick(&|r| r.0);
        let (c50, _) = pick(&|r| r.1);
        let (t50, _) = pick(&|r| r.2 as u64);
        println!("damage\t{name}\t{reps}\t{i50}\t{i95}\t{c50}\t{t50}");
    }
}

fn env(name: &str, d: usize) -> usize {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}

fn main() {
    let reps = env("REPS", 64);
    let name = std::env::var("SCENE").unwrap_or_else(|_| "window".into());
    assert!(matches!(name.as_str(), "window" | "stack" | "4k"), "SCENE is window, stack or 4k");
    let s = scene(&name);
    let m = Meter::new();
    println!("# damage scene={name} window={}x{} covers={} layers={} reps={reps}", s.w, s.h, s.covers.len(), s.layers.len());
    println!("scene\tphase\tsteps\tinstr_p50\tinstr_p95\tcycles_p50\ttiles_p50");
    let mut e = DamageEngine::new(s.w, s.h);
    commit(&mut e, &s);
    let (ux, uy) = s.under;
    let top = s.layers[0].z;
    let mut report = DamageReport::default();
    m.phase("floor", reps, &mut e, &s, |_, _, _| {});
    m.phase("idle", reps, &mut e, &s, |e, _, _| {
        e.begin_frame();
        e.resolve_ungated();
    });
    // two rows of a hover in the list, under no glass
    m.phase("hover", reps, &mut e, &s, |e, s, k| {
        e.begin_frame();
        let y = 100 + (k % 8) as i32 * 28;
        e.damage(top + 6, Rect::new(16, y, s.list.x1 - 16, y + 28), Work::Flat);
        e.damage(top + 6, Rect::new(16, y + 28, s.list.x1 - 16, y + 56), Work::Flat);
        e.resolve_ungated();
    });
    // a card beneath the deepest glass recoloured: its output, and the layers stacked over it
    m.phase("under_glass", reps, &mut e, &s, |e, _, k| {
        e.begin_frame();
        e.damage(12, Rect::xywh(ux - 60 + (k % 4) as i32, uy - 40, 120, 80), Work::Gradient);
        e.resolve_ungated();
    });
    // the same, with a gate that finds the backdrop unchanged every other frame
    m.phase("under_glass_gated", reps, &mut e, &s, |e, _, k| {
        e.begin_frame();
        e.damage(12, Rect::xywh(ux - 60, uy - 40, 120, 80), Work::Flat);
        e.resolve(&mut |_, r| if k % 2 == 0 { None } else { Some(r) });
    });
    // a card with a shadow moves 40 px across the glass's edge
    m.phase("move", reps, &mut e, &s, |e, s, k| {
        e.begin_frame();
        let l = s.layers[0].rect;
        let x = l.x0 - 100 + (k % 2) as i32 * 40;
        e.damage_move(14, Rect::xywh(x, uy, 180, 120).expand(20), Rect::xywh(x + 40, uy, 180, 120).expand(20), Work::Shadow);
        e.resolve_ungated();
    });
    // the list scrolls 11 px under the glass header: a blit, its strip, the header's seam
    m.phase("scroll", reps, &mut e, &s, |e, s, _| {
        e.begin_frame();
        if let Some(b) = e.scroll(10, top, s.list, 0, -11) {
            e.seam(&b, 900, Rect::new(0, 0, s.w, 56));
            e.seam(&b, 901, Rect::new(0, 0, s.w, 56));
        }
        e.resolve_ungated();
    });
    // a theme switch: the tiles of every other card as a mask, every layer's backdrop
    m.phase("theme", reps, &mut e, &s, |e, s, _| {
        e.begin_frame();
        e.damage_mask(&s.theme);
        e.resolve_ungated();
    });
    // a cover moved: the driver hands every cover and layer over again, then one damage
    m.phase("recommit", reps, &mut e, &s, |e, s, k| {
        commit(e, s);
        e.begin_frame();
        e.damage(14, Rect::xywh(30 + (k % 3) as i32, 80, 200, 140), Work::Flat);
        e.resolve_ungated();
    });
    // the report a tool reads, filled into a kept value
    m.phase("report", reps, &mut e, &s, |e, _, _| {
        e.begin_frame();
        e.damage(12, Rect::xywh(ux - 60, uy - 40, 120, 80), Work::Flat);
        e.resolve_ungated();
        e.report_into(&mut report);
        black_box(&report);
    });
}
