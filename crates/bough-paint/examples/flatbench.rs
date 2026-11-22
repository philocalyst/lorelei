//! The flat bank's benchmark: what the non-generic effects, the animation bank and retained groups cost.
//!
//! `flatbench [cards]` (default 300); `REPS` measured frames per cell (default 24). Two scenes at 1280 x 800:
//!
//! * **cards**: a scrolled world of styled cards (each two stacked drop shadows, a three-stop gradient
//!   fill, an inset shadow and a border: five records), a backdrop pane and a header over it;
//! * **pages**: two such pages as retained groups of a stage (what a page transition composites).
//!
//! Every cell runs on the CPU tile kernel with one core, with eight workers, and on Metal (the staged
//! frame), on separate scenes that receive the same script. Instructions are the frame thread's
//! (`bough_mem::pmu`). After every cell the retained surface is compared with the same scene redrawn
//! whole, and the Metal surface with the CPU's: mismatching words are counted and fail the run.
use bough_kernel::cpu::Pool;
use bough_mem::pmu;
use bough_paint::flat_scene::*;
use bough_paint::motion::{AnimBank, ClockNs, SpringSpec, TrackId};
use std::time::Instant;

const FW: u32 = 1280;
const FH: u32 = 800;

struct Cards {
    flat: FlatScene,
    /// Per card: its five nodes.
    cards: Vec<[NodeId; 5]>,
    pane: Option<NodeId>,
    luts: Vec<Lut>,
}
/// `n` cards in a grid 6 wide. `panned`: a scrolled world with a pane and a header (the cards scene);
/// otherwise a page's content (fixed, no backdrop).
fn cards(n: usize, panned: bool, w: u32, h: u32, media: usize) -> Cards {
    let sp = if panned { Space::Panned } else { Space::Fixed };
    let cap = n.div_ceil(64) * 64 + 64;
    let cfg = FlatConfig::empty()
        .with(EffectKind::Solid, Space::Fixed, 8)
        .with(EffectKind::Shadow, sp, 2 * cap)
        .with(EffectKind::Ramp, sp, cap)
        .with(EffectKind::Inset, sp, cap)
        .with(EffectKind::Border, sp, cap)
        .with(EffectKind::Glass, Space::Fixed, 4)
        .with(EffectKind::RRect, Space::Fixed, 8)
        .with(EffectKind::Group, Space::Fixed, 4)
        .media(media);
    let mut flat = FlatScene::new(w, h, cfg);
    flat.push(Node::new([0.0, 0.0, w as f32, h as f32], DrawCmd::Solid { color: 0xfff4_f1ee })).unwrap();
    let luts: Vec<Lut> = [[0xffe1_4f46u32, 0xffea_3393, 0xff99_48ec], [0xff38_bdf8, 0xff63_66f1, 0xffa8_55f7], [0xff34_d399, 0xff22_d3ee, 0xff3b_82f6], [0xfffb_bf24, 0xfff9_7316, 0xffef_4444]]
        .iter()
        .map(|c| flat.lut(&[(0.0, c[0]), (0.55, c[1]), (1.0, c[2])]).unwrap())
        .collect();
    let mut out = Vec::new();
    for i in 0..n {
        let (x, y) = (24.0 + 208.0 * (i % 6) as f32, 76.0 + 150.0 * (i / 6) as f32);
        let r = [x, y, x + 184.0, y + 126.0];
        let mk = |cmd| if panned { Node::new(r, cmd).panned() } else { Node::new(r, cmd) };
        let a = flat.push(mk(DrawCmd::Shadow { radius: 14.0, spread: -4.0, dx: 0.0, dy: 12.0, blur: 28.0, color: 0x4800_0000 })).unwrap();
        let b = flat.push(mk(DrawCmd::Shadow { radius: 14.0, spread: 0.0, dx: 0.0, dy: 2.0, blur: 5.0, color: 0x3800_0000 })).unwrap();
        let c = flat.push(mk(DrawCmd::Ramp { radius: 14.0, geo: RampGeo::Linear { angle_deg: 135.0 }, lut: luts[i % 4] })).unwrap();
        let d = flat.push(mk(DrawCmd::Inset { radius: 14.0, spread: 0.0, dx: 0.0, dy: 1.5, blur: 3.0, color: 0x70ff_ffff })).unwrap();
        let e = flat.push(mk(DrawCmd::Border { radius: 14.0, width: 1.0, color: 0x30ff_ffff, fill: 0 })).unwrap();
        out.push([a, b, c, d, e]);
    }
    let mut pane = None;
    if panned {
        pane = Some(flat.push(Node::new([420.0, 250.0, 860.0, 560.0], DrawCmd::Glass { radius: 22.0, blur: 14.0, brightness: 1.05, saturation: 1.4, contrast: 1.0, tint: 0x50ff_ffff })).unwrap());
        flat.push(Node::new([0.0, 0.0, w as f32, 56.0], DrawCmd::Solid { color: 0xff20_2430 })).unwrap();
    }
    Cards { flat, cards: out, pane, luts }
}

struct Pages {
    stage: FlatScene,
    groups: [GroupId; 2],
    nodes: [NodeId; 2],
    card: NodeId,
}
fn pages(n: usize) -> Pages {
    let per = surface_words(FW, FH, 5) + 64;
    let cfg = FlatConfig::empty().with(EffectKind::Solid, Space::Fixed, 1).with(EffectKind::Group, Space::Fixed, 2).media(2 * per + 64);
    let mut stage = FlatScene::new(FW, FH, cfg);
    stage.push(Node::new([0.0, 0.0, FW as f32, FH as f32], DrawCmd::Solid { color: 0xff10_1010 })).unwrap();
    let (a, b) = (cards(n.min(30), false, FW, FH, 0), cards(n.min(30), false, FW, FH, 0));
    let card = a.cards[7][2];
    let groups = [stage.group(a.flat, 5).unwrap(), stage.group(b.flat, 5).unwrap()];
    let full = [0.0, 0.0, FW as f32, FH as f32];
    let nodes = [stage.push(Node::new(full, DrawCmd::Group { radius: 0.0, blur: 0.0, group: groups[0] })).unwrap(), stage.push(Node::new(full, DrawCmd::Group { radius: 0.0, blur: 0.0, group: groups[1] }).opacity(0.0)).unwrap()];
    Pages { stage, groups, nodes, card }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Cell {
    Idle,
    Hover,
    HoverGlass,
    Spring,
    Scroll,
    Glass,
    PageIdle,
    CrossFade,
    Slide,
    BlurReveal,
    PageEdit,
}
impl Cell {
    fn name(self) -> &'static str {
        match self {
            Cell::Idle => "idle",
            Cell::Hover => "restyle one card",
            Cell::HoverGlass => "restyle a card beneath the pane",
            Cell::Spring => "one card on a spring",
            Cell::Scroll => "scroll 11 px",
            Cell::Glass => "pane moves 1 px",
            Cell::PageIdle => "pages idle",
            Cell::CrossFade => "page cross-fade",
            Cell::Slide => "page slide",
            Cell::BlurReveal => "page blur-reveal",
            Cell::PageEdit => "one card of a page",
        }
    }
    fn paged(self) -> bool {
        matches!(self, Cell::PageIdle | Cell::CrossFade | Cell::Slide | Cell::BlurReveal | Cell::PageEdit)
    }
}
enum World {
    Cards(Cards, AnimBank, Vec<TrackId>),
    Pages(Pages),
}
impl World {
    fn flat(&mut self) -> &mut FlatScene {
        match self {
            World::Cards(c, ..) => &mut c.flat,
            World::Pages(p) => &mut p.stage,
        }
    }
}
/// Frame `f` of a cell's script.
fn turn(w: &mut World, cell: Cell, f: usize) {
    match (w, cell) {
        (World::Cards(c, ..), Cell::Hover | Cell::HoverGlass) => {
            let id = c.cards[if cell == Cell::Hover { 0 } else { 8 }][2];
            let DrawCmd::Ramp { radius, geo, .. } = c.flat.node(id).cmd else { unreachable!() };
            c.flat.set(id, DrawCmd::Ramp { radius, geo, lut: c.luts[f % 4] });
        }
        (World::Cards(c, bank, tracks), Cell::Spring) => {
            let now = ClockNs::from_ms(1_000 + 16 * f as u64);
            if f % 30 == 1 {
                for &t in tracks.iter() {
                    bank.spring_to(t, SpringSpec::BOUNCY, if (f / 30) % 2 == 0 { -40.0 } else { 0.0 }, now);
                }
            }
            let flat = &mut c.flat;
            bank.tick(now, |tag, v| flat.apply(tag, v));
        }
        (World::Cards(c, ..), Cell::Scroll) => c.flat.set_pan(0.0, -11.0 * f as f32),
        (World::Cards(c, ..), Cell::Glass) => c.flat.set_xf(c.pane.unwrap(), f as f32, 0.0, 1.0),
        (World::Pages(p), Cell::CrossFade) => p.stage.set_opacity(p.nodes[1], ((f % 40) as f32 + 1.0) / 41.0),
        (World::Pages(p), Cell::Slide) => {
            let x = (f % 50) as f32 * 24.0;
            p.stage.set_opacity(p.nodes[1], 1.0);
            p.stage.set_xf(p.nodes[1], FW as f32 - x, 0.0, 1.0);
            // (whole pixels, as the page router moves its pages)
            p.stage.set_xf(p.nodes[0], (-0.3 * x).round(), 0.0, 1.0);
        }
        (World::Pages(p), Cell::BlurReveal) => {
            let t = ((f % 40) as f32 + 1.0) / 41.0;
            p.stage.set_opacity(p.nodes[1], t);
            p.stage.set_prop(p.nodes[1], Prop::Blur, 24.0 * (1.0 - t));
            p.stage.set_xf(p.nodes[1], 0.0, 0.0, 1.06 - 0.06 * t);
            p.stage.set_prop(p.nodes[0], Prop::Blur, 24.0 * t);
        }
        (World::Pages(p), Cell::PageEdit) => {
            let g = p.groups[0];
            let id = p.card;
            let DrawCmd::Ramp { radius, geo, lut } = p.stage.child(g).node(id).cmd else { unreachable!() };
            let _ = lut;
            let l = p.stage.child_mut(g).lut(&[(0.0, 0xff00_0000 | (f as u32 * 0x050309) & 0xff_ffff), (1.0, 0xffff_ffff)]).unwrap();
            p.stage.child_mut(g).set(id, DrawCmd::Ramp { radius, geo, lut: l });
        }
        _ => {}
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Exec {
    Cpu1,
    Cpu8,
    Metal,
}
#[derive(Default)]
struct Row {
    wall: Vec<f64>,
    instr: Vec<u64>,
    tiles: u64,
    lowered: u64,
    group_tiles: u64,
    group_blocks: u64,
    frames: u64,
}
fn med<T: Copy + PartialOrd>(v: &mut [T]) -> T {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}
fn make(cell: Cell, n: usize) -> World {
    if cell.paged() {
        World::Pages(pages(n))
    } else {
        let c = cards(n, true, FW, FH, 0);
        let mut bank = AnimBank::new();
        let tracks = c.cards[1].iter().map(|&id| bank.track(Prop::Dy.tag(id), 0.0)).collect();
        World::Cards(c, bank, tracks)
    }
}
fn run(cell: Cell, exec: Exec, n: usize, reps: usize, pool: &Pool) -> (Row, usize, Vec<u32>) {
    let mut w = make(cell, n);
    let words = (FW * FH) as usize;
    // SAFETY: `w` (the scene) is declared before the executor and outlives it, and is written only
    // between the executor's calls, each of which waits for the GPU.
    #[cfg(target_os = "macos")]
    let mut gpu = if exec == Exec::Metal { Some(unsafe { FlatGpu::new(w.flat(), 256) }.expect("no Metal device")) } else { None };
    let mut out = vec![0u32; words];
    let mut row = Row::default();
    let warm = 6;
    let extra = if exec == Exec::Cpu8 { 7 } else { 0 };
    for f in 0..warm + reps {
        let (i0, _) = pmu::instructions_cycles();
        let t0 = Instant::now();
        if f > 0 {
            turn(&mut w, cell, f);
        }
        let st = match exec {
            #[cfg(target_os = "macos")]
            Exec::Metal => gpu.as_mut().unwrap().frame(w.flat(), pool, 7),
            _ => w.flat().frame(&mut out, FW as usize, pool, extra),
        };
        let wall = t0.elapsed().as_secs_f64();
        let (i1, _) = pmu::instructions_cycles();
        if f >= warm {
            row.wall.push(wall);
            row.instr.push(i1 - i0);
            row.tiles += st.frame.tiles as u64;
            row.lowered += st.lowered as u64;
            row.group_tiles += st.group_tiles as u64;
            row.group_blocks += st.group_blocks as u64;
            row.frames += 1;
        }
    }
    // validity: the retained surface against the same scene redrawn whole
    #[cfg(target_os = "macos")]
    let surface: Vec<u32> = match gpu.as_ref() {
        Some(g) => g.out().to_vec(),
        None => out,
    };
    #[cfg(not(target_os = "macos"))]
    let surface = out;
    #[cfg(target_os = "macos")]
    drop(gpu);
    let mut full = vec![0u32; words];
    w.flat().scene_mut().invalidate();
    w.flat().frame(&mut full, FW as usize, pool, 7);
    let bad = surface.iter().zip(&full).filter(|(a, b)| a != b).count();
    (row, bad, surface)
}

fn main() {
    let n: usize = std::env::args().nth(1).and_then(|v| v.parse().ok()).unwrap_or(300);
    let reps: usize = std::env::var("REPS").ok().and_then(|v| v.parse().ok()).unwrap_or(24);
    let only = std::env::var("CELL").ok();
    let pool = Pool::new(8);
    println!("# flatbench: {n} cards ({} records) in a scrolled world, and two pages as retained groups, {FW} x {FH}; {reps} frames per cell", 5 * n + 3);
    println!("# archetypes a flat scene can instantiate: {} (fixed and panned), whatever is drawn", 2 * KINDS);
    println!("scene\tcell\texec\tms/frame (median)\tms (min)\tk instr/frame\ttiles/frame\trecords/frame\tgroup tiles/frame\tgroup blocks/frame\tmismatches");
    let mut total_bad = 0;
    for cell in [Cell::Idle, Cell::Hover, Cell::HoverGlass, Cell::Spring, Cell::Scroll, Cell::Glass, Cell::PageIdle, Cell::CrossFade, Cell::Slide, Cell::BlurReveal, Cell::PageEdit] {
        if only.as_ref().is_some_and(|o| !cell.name().contains(o.as_str())) {
            continue;
        }
        let mut cpu: Option<Vec<u32>> = None;
        for exec in [Exec::Cpu1, Exec::Cpu8, Exec::Metal] {
            if cfg!(not(target_os = "macos")) && exec == Exec::Metal {
                continue;
            }
            let (mut row, mut bad, surface) = run(cell, exec, n, reps, &pool);
            // the executors agree
            match (&cpu, exec) {
                (None, _) => cpu = Some(surface),
                (Some(c), _) => bad += c.iter().zip(&surface).filter(|(a, b)| a != b).count(),
            }
            total_bad += bad;
            let f = row.frames.max(1) as f64;
            let min = row.wall.iter().cloned().fold(f64::MAX, f64::min);
            println!(
                "{}\t{}\t{:?}\t{:.3}\t{:.3}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{}",
                if cell.paged() { "pages" } else { "cards" },
                cell.name(),
                exec,
                med(&mut row.wall) * 1e3,
                min * 1e3,
                med(&mut row.instr) as f64 / 1e3,
                row.tiles as f64 / f,
                row.lowered as f64 / f,
                row.group_tiles as f64 / f,
                row.group_blocks as f64 / f,
                bad
            );
        }
    }
    if std::env::var("BOUGH_PAINT_PROF").is_ok() {
        // nanoseconds and tile entries per column of the last scenes (a measurement switch)
        let p = bough_paint::scene::PROF.lock().unwrap();
        let names = archetypes();
        for (i, (ns, n)) in p.iter().enumerate().filter(|(_, v)| v.1 > 0) {
            println!("# column {i}: {:.1} ms, {n} entries, {:.0} ns/entry ({})", *ns as f64 / 1e6, *ns as f64 / *n as f64, names.iter().flatten().map(|d| d.name).nth(i).unwrap_or("?").len());
        }
    }
    println!("# mismatching words in total: {total_bad}");
    assert_eq!(total_bad, 0);
}
