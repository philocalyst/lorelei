//! Round 3, independent review of the layers (retained == fresh), written without reading `tests/fx.rs`'s
//! walk beyond its public vocabulary.
//!
//! * `retained_walk_equals_fresh`: random walks over worlds of backdrops (overlapping, nested, over and
//!   under each other, one reading its blur and offset from the uniform block), with moves across tile
//!   and block edges, moves far enough to wrap every toroidal window, resizes (also to nothing),
//!   entering and leaving the frame, radius, blur (every pyramid level and every level seam), filter,
//!   tint, changes of records beneath and above, block pans. After every step a *fresh* scene built
//!   from the same world is the reference, for surfaces of ring ages 1 to 3; every few steps the
//!   whole-frame specification is too.
//! * `sweep_*`: deterministic one-pixel steps of a pane across tile edges, for every blur level, moving,
//!   growing/shrinking, and with a one-pixel spot beneath it stepping over every pixel around an edge.
//! * `frame_sizes_*`: the same walk on frames that are not tile multiples and on frames smaller than a tile.
//! * macOS: `gpu_staged_*` (Metal's retained staged frame equals the CPU's), and `probe_*` tests that
//!   drive the *layered* `Gpu::frame` and `Driver::frame` and say what they do.
//!
//! `BOUGH_SOLFX_SEEDS` (default 3) and `BOUGH_SOLFX_STEPS` (default 60) scale the walk.
use bough_kernel::cpu::Pool;
use bough_kernel::scenes::Rng;
use bough_paint::expr::*;
use bough_paint::paint::*;
use bough_paint::scene::{ColId, Scene};

type Pan = Off<Lane, I<Uw<0>>, I<Uw<1>>>;
type Any = Pm<Rec>;
/// Content under and over the panes.
type ABox = RRect<Lane, I<Rec>, Any>;
/// Hover-recoloured content over the panes: a change here must reach no layer.
type ATop = RRect<Lane, I<Rec>, SelV<Eq<Uw<2>, Rec>, Any, Any>>;
/// Frosted glass: a backdrop and a translucent tint.
type AGlass = Over<Backdrop<Lane, I<Rec>, I<Rec>, Filt<Rec, Rec, Rec>>, RRect<Lane, I<Rec>, Any>>;
/// A pane that slides with the block and takes its blur from word 4.
type ASlide = Backdrop<Pan, I<Rec>, I<Uw<4>>, Filt<K<256>, K<300>, K<256>>>;

fn q(v: f32) -> i32 {
    (v * 256.0).round() as i32
}
fn ri(v: i32) -> I<Rec> {
    I { a: Rec(v as u32) }
}
fn pm(c: u32) -> Any {
    Pm { c: Rec(c) }
}
fn pan() -> Pan {
    Off { b: Lane, dx: I { a: Uw }, dy: I { a: Uw } }
}

#[derive(Clone, Debug)]
struct Bx {
    rect: [i32; 4],
    rad: i32,
    c: u32,
}
#[derive(Clone, Debug)]
struct Gl {
    rect: [i32; 4],
    rad: i32,
    blur: i32,
    f: [u32; 3],
    tint: u32,
}
#[derive(Clone, Debug)]
struct Sl {
    rect: [i32; 4],
    rad: i32,
}
#[derive(Clone, Debug)]
struct World {
    bx: Vec<Bx>,
    gl: Vec<Gl>,
    sl: Vec<Sl>,
}
struct Built {
    s: Scene,
    bx: ColId<ABox>,
    top: ColId<ATop>,
    gl: ColId<AGlass>,
    sl: ColId<ASlide>,
}

/// Paint order of the world: B boxes, G glass, S slides. Boxes from `NB - NT` on are above every pane.
const NB: usize = 14;
const NT: usize = 2;
const ORDER: &str = "BBBB G BB S G BB G S BB G BB BB";

fn bx_rec(b: &Bx) -> ABox {
    RRect { b: Lane, r: ri(b.rad), c: pm(b.c) }
}
fn top_rec(b: &Bx) -> ATop {
    RRect { b: Lane, r: ri(b.rad), c: SelV { c: Eq { a: Uw, b: Rec(1) }, a: pm(b.c), b: pm(b.c ^ 0x00ff_ffff) } }
}
fn gl_rec(g: &Gl) -> AGlass {
    Over(Backdrop { b: Lane, r: ri(g.rad), blur: ri(g.blur), f: V3 { a: Rec(g.f[0]), b: Rec(g.f[1]), c: Rec(g.f[2]) } }, RRect { b: Lane, r: ri(g.rad), c: pm(g.tint) })
}
fn sl_rec(s: &Sl) -> ASlide {
    Backdrop { b: pan(), r: ri(s.rad), blur: I { a: Uw }, f: V3 { a: K, b: K, c: K } }
}

/// Blurs (px) that land on every level and on both sides of every seam between levels
/// (`BD_SIGMA` = 330 Q8 = 1.289 px; level `l` begins at 330 * 2^(l - 1)).
const BLURS: [f32; 24] = [0.0, 0.05, 0.4, 1.0, 1.28, 1.29, 1.3, 1.9, 2.57, 2.6, 3.7, 5.1, 5.2, 7.5, 10.2, 10.4, 15.0, 20.5, 20.7, 30.0, 41.0, 82.0, 200.0, 0.2];
const FILTERS: [[u32; 3]; 5] = [[256, 256, 256], [300, 420, 256], [200, 0, 300], [0, 256, 4096], [256, 256, 256]];

fn world(seed: u64, fw: u32, fh: u32) -> World {
    let mut g = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let (w, h) = (fw as f32, fh as f32);
    let mut wd = World { bx: Vec::new(), gl: Vec::new(), sl: Vec::new() };
    for i in 0..NB {
        // small high-contrast content (a blur of it is far from flat), some right at tile edges
        let (x, y) = if g.u(3) == 0 { (32.0 * g.u(fw / 32 + 1) as f32 - g.u(2) as f32, 32.0 * g.u(fh / 32 + 1) as f32 - g.u(2) as f32) } else { (g.pos(-10.0, w), g.pos(-10.0, h)) };
        let (bw, bh) = if g.u(3) == 0 { (g.pos(1.0, 4.0), g.pos(1.0, 4.0)) } else { (g.pos(3.0, 70.0), g.pos(3.0, 60.0)) };
        let rad = [0, q(2.0), q(9.0), q(9999.0)][g.u(4) as usize];
        let opaque = i < 3 || g.u(3) != 0;
        wd.bx.push(Bx { rect: [q(x), q(y), q(x + bw), q(y + bh)], rad, c: g.color(opaque) });
    }
    let pane = |x: f32, y: f32, bw: f32, bh: f32, blur: f32, g: &mut Rng| {
        let rect = [q(x), q(y), q(x + bw), q(y + bh)];
        Gl { rect, rad: [0, q(6.0), q(14.0), q(40.0)][g.u(4) as usize], blur: q(blur), f: FILTERS[g.u(5) as usize], tint: g.color(false) & 0x50ff_ffff }
    };
    let (x0, y0) = (g.pos(0.0, w * 0.4), g.pos(0.0, h * 0.4));
    let (w0, h0) = (g.pos(w * 0.3, w * 0.6), g.pos(h * 0.3, h * 0.6));
    wd.gl.push(pane(x0, y0, w0, h0, BLURS[g.u(24) as usize], &mut g));
    // overlapping the first, partly outside it
    wd.gl.push(pane(x0 + w0 * 0.5, y0 + h0 * 0.3, g.pos(30.0, w * 0.5), g.pos(24.0, h * 0.5), BLURS[g.u(24) as usize], &mut g));
    // nested: wholly inside the second
    let r1 = wd.gl[1].rect;
    wd.gl.push(pane(r1[0] as f32 / 256.0 + 6.0, r1[1] as f32 / 256.0 + 5.0, 18.0, 15.0, BLURS[g.u(24) as usize], &mut g));
    // a big one over (nearly) everything
    wd.gl.push(pane(g.pos(0.0, 9.0), g.pos(0.0, 9.0), w - 4.0, h - 4.0, BLURS[g.u(24) as usize], &mut g));
    for _ in 0..2 {
        let (x, y) = (g.pos(0.0, w * 0.5), g.pos(0.0, h * 0.5));
        wd.sl.push(Sl { rect: [q(x), q(y), q(x + g.pos(20.0, w * 0.5)), q(y + g.pos(16.0, h * 0.5))], rad: q(g.f(0.0, 20.0)) });
    }
    wd
}

fn build(wd: &World, fw: u32, fh: u32) -> Built {
    let mut s = Scene::new(fw, fh, 1 << 18);
    let bx = s.column::<ABox>(NB);
    let top = s.column::<ATop>(NT);
    let gl = s.column::<AGlass>(8);
    let sl = s.column::<ASlide>(4);
    let (mut b, mut g, mut sx) = (0, 0, 0);
    for ch in ORDER.chars() {
        match ch {
            'B' => {
                let r = &wd.bx[b];
                if b >= NB - NT {
                    s.push(top, r.rect, top_rec(r));
                } else {
                    s.push(bx, r.rect, bx_rec(r));
                }
                b += 1;
            }
            'G' => {
                s.push(gl, wd.gl[g].rect, gl_rec(&wd.gl[g]));
                g += 1;
            }
            'S' => {
                s.push(sl, wd.sl[sx].rect, sl_rec(&wd.sl[sx]));
                sx += 1;
            }
            _ => {}
        }
    }
    assert_eq!((b, g, sx), (NB, wd.gl.len(), wd.sl.len()), "ORDER does not mount every record");
    Built { s, bx, top, gl, sl }
}

fn diff(a: &[u32], b: &[u32]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}
fn first_diff(a: &[u32], b: &[u32], stride: usize, fw: usize) -> String {
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        if x != y && i % stride < fw {
            let (px, py) = (i % stride, i / stride);
            return format!("first at ({px},{py}) tile ({},{}): {x:08x} vs fresh {y:08x}", px / 32, py / 32);
        }
    }
    String::new()
}
fn env(k: &str, d: usize) -> usize {
    std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}
fn block(g: &mut Rng) -> U {
    let mut u = U::default();
    u.w[0] = (g.u(40 * 256) as i32 - 20 * 256) as u32;
    u.w[1] = (g.u(40 * 256) as i32 - 20 * 256) as u32;
    u.w[2] = g.u(3);
    u.w[4] = q(BLURS[g.u(24) as usize]) as u32;
    u
}

fn shift(r: [i32; 4], dx: i32, dy: i32) -> [i32; 4] {
    [r[0] + dx, r[1] + dy, r[2] + dx, r[3] + dy]
}
/// Keeps a rect a legal record lane (admission is +-2^15 px) and, unless `empty`, non-empty.
fn sane(mut r: [i32; 4], empty: bool) -> [i32; 4] {
    let lim = 20_000 * 256;
    for v in r.iter_mut() {
        *v = (*v).clamp(-lim, lim);
    }
    if !empty {
        r[2] = r[2].max(r[0] + 256);
        r[3] = r[3].max(r[1] + 256);
    }
    r
}

/// One random step of the world and the retained scene. Returns its name and whether it touched only
/// records above every layer (which must reach no layer) or nothing at all.
fn step(g: &mut Rng, u: &mut U, wd: &mut World, b: &mut Built, fw: u32, fh: u32) -> (&'static str, bool, bool) {
    let (w, h) = (fw as i32, fh as i32);
    let sub = |g: &mut Rng| if g.u(2) == 0 { g.u(256) as i32 } else { 0 };
    let tile_jump = |g: &mut Rng| [31, 32, 33, 63, 64, 65, 95, -31, -32, -33, -64, -97][g.u(12) as usize] * 256 + sub(g);
    let k = g.u(28);
    let name: &'static str;
    let pi = g.u(wd.gl.len() as u32 + wd.sl.len() as u32) as usize;
    let on_gl = pi < wd.gl.len();
    let si = pi - wd.gl.len().min(pi);
    let old = if on_gl { wd.gl[pi].rect } else { wd.sl[si].rect };
    let mut nr = old;
    macro_rules! geo {
        ($name:literal, $f:expr) => {{
            let f: &dyn Fn(&mut Rng, [i32; 4]) -> [i32; 4] = &$f;
            nr = sane(f(g, old), false);
            name = $name;
        }};
    }
    match k {
        0 | 1 => geo!("move one pixel", |g: &mut Rng, r| shift(r, [-256, 256, 0, 0][g.u(4) as usize] + [0, 0, 77, -45][g.u(4) as usize] * (g.u(2) as i32), [0, 0, -256, 256][g.u(4) as usize])),
        2 | 3 => geo!("move across tiles", |g: &mut Rng, r| shift(r, tile_jump(g), if g.u(2) == 0 { 0 } else { tile_jump(g) })),
        4 => geo!("move far (wraps the torus)", |g: &mut Rng, r| shift(r, (100 + g.u(400) as i32) * 256 * if g.u(2) == 0 { 1 } else { -1 }, (g.u(300) as i32 - 150) * 256)),
        5 | 6 => geo!("grow", |g: &mut Rng, r| [r[0] - if g.u(2) == 0 { 256 * g.u(40) as i32 } else { 0 }, r[1] - if g.u(2) == 0 { 256 * g.u(40) as i32 } else { 0 }, r[2] + 256 * g.u(70) as i32 + sub(g), r[3] + 256 * g.u(50) as i32]),
        7 | 8 => geo!("shrink", |g: &mut Rng, r| [r[0] + 256 * g.u(30) as i32, r[1] + 256 * g.u(20) as i32 + sub(g), r[2] - 256 * g.u(40) as i32, r[3] - 256 * g.u(30) as i32]),
        9 => {
            let tiny = g.u(3);
            nr = sane([old[0], old[1], old[0] + if tiny == 0 { 256 } else { 256 * 40 }, old[1] + if tiny == 1 { 256 } else { 256 * 30 }], false);
            name = "resize to a sliver";
        }
        10 => {
            // empty (width zero): the pane leaves the layer table of live windows
            nr = sane([old[0], old[1], old[0], old[3]], true);
            name = "collapse to empty";
        }
        11 | 12 => {
            let (dx, dy) = [(w + 60, 0), (-(w + 200), 0), (0, h + 60), (0, -(h + 90)), (3 * w, 2 * h)][g.u(5) as usize];
            nr = sane(shift(old, dx * 256, dy * 256), false);
            name = "leave the frame";
        }
        13 | 14 => {
            // enter: back into the frame at a random place, with a random size
            let (x, y, bw, bh) = (g.u(w as u32) as i32 - 20, g.u(h as u32) as i32 - 20, 1 + g.u(90) as i32, 1 + g.u(70) as i32);
            nr = sane([q(x as f32), q(y as f32), q((x + bw) as f32) + sub(g), q((y + bh) as f32) + sub(g)], false);
            name = "enter the frame";
        }
        15 => {
            if on_gl {
                let new = [0, q(0.5), q(3.0), q(14.0), q(9999.0), q(((wd.gl[pi].rect[2] - wd.gl[pi].rect[0]) / 512) as f32)][g.u(6) as usize];
                wd.gl[pi].rad = new;
                name = "radius";
            } else {
                wd.sl[si].rad = [0, q(7.0), q(9999.0)][g.u(3) as usize];
                name = "radius (slide)";
            }
        }
        16 | 17 => {
            if on_gl {
                // all levels, and across a seam by a hair
                wd.gl[pi].blur = q(BLURS[g.u(24) as usize]) + g.u(2) as i32;
                name = "blur";
            } else {
                u.w[4] = q(BLURS[g.u(24) as usize]) as u32;
                name = "blur (slide, block word)";
            }
        }
        18 => {
            if on_gl {
                wd.gl[pi].f = FILTERS[g.u(5) as usize];
                wd.gl[pi].tint = g.color(false) & 0x50ff_ffff;
                name = "filter and tint";
            } else {
                *u = block(g);
                name = "block (slide pans and blurs)";
            }
        }
        19 | 20 => {
            // content beneath: move a box to an edge, one pixel wide, or recolour it
            let i = g.u((NB - NT) as u32) as usize;
            let bxr = &mut wd.bx[i];
            match g.u(4) {
                0 => {
                    let (x, y) = (32 * g.u(w as u32 / 32 + 1) as i32 - g.u(2) as i32, 32 * g.u(h as u32 / 32 + 1) as i32 - g.u(2) as i32);
                    bxr.rect = [q(x as f32), q(y as f32), q(x as f32 + 1.0 + g.u(3) as f32), q(y as f32 + 1.0 + g.u(3) as f32)];
                }
                1 => {
                    let opaque = g.u(2) == 0;
                    bxr.c = g.color(opaque);
                }
                2 => bxr.rect = sane(shift(bxr.rect, [-256, 256, 0, 32 * 256][g.u(4) as usize], [0, 0, -256, 256][g.u(4) as usize]), false),
                _ => bxr.rad = [0, q(2.0), q(9999.0)][g.u(3) as usize],
            }
            let bxc = wd.bx[i].clone();
            b.s.set(b.bx, i, bxc.rect, bx_rec(&bxc));
            return ("content beneath", false, false);
        }
        21 => {
            // above every pane: no layer may be reached
            let i = NB - NT + g.u(NT as u32) as usize;
            let bxr = &mut wd.bx[i];
            bxr.rect = sane(shift(bxr.rect, (g.u(80) as i32 - 40) * 256, (g.u(80) as i32 - 40) * 256), false);
            bxr.c = g.color(true);
            let bxc = wd.bx[i].clone();
            b.s.set(b.top, i - (NB - NT), bxc.rect, top_rec(&bxc));
            return ("content above", true, false);
        }
        22 => {
            u.w[2] = g.u(3);
            return ("hover (records above)", true, false);
        }
        23 | 24 => {
            *u = block(g);
            return ("block", false, false);
        }
        25 => {
            u.w[U_TIME] += 1 + g.u(40);
            return ("clock", true, false);
        }
        _ => return ("idle", false, true),
    }
    if on_gl {
        wd.gl[pi].rect = nr;
        let gc = wd.gl[pi].clone();
        b.s.set(b.gl, pi, gc.rect, gl_rec(&gc));
    } else {
        wd.sl[si].rect = nr;
        let sc = wd.sl[si].clone();
        b.s.set(b.sl, si, sc.rect, sl_rec(&sc));
    }
    (name, false, false)
}

#[derive(Default)]
struct Tally {
    steps: usize,
    bad: usize,
    prefix: u64,
    blocks: u64,
    out: u64,
    all: u64,
    idle: usize,
    spec_checked: usize,
}

/// The walk: retained scene against fresh scenes, in surfaces of ring ages 1..=3, and the specification.
fn walk(seed: u64, fw: u32, fh: u32, steps: usize, t: &mut Tally) {
    let stride = fw as usize + 3;
    let words = stride * fh as usize;
    let pool = Pool::new(2);
    let mut wd = world(seed, fw, fh);
    let mut b = build(&wd, fw, fh);
    let mut g = Rng(seed * 977 + 13);
    let mut u = block(&mut g);
    let nt = b.s.tx * b.s.ty;
    let mut ring = vec![vec![0u32; words]; 3];
    let mut stale = vec![vec![true; nt]; 3];
    let mut log: Vec<&str> = Vec::new();
    for k in 0..steps {
        let (name, above_only, idle) = if k == 0 { ("mount", false, false) } else { step(&mut g, &mut u, &mut wd, &mut b, fw, fh) };
        log.push(name);
        let fs = b.s.update(&u);
        b.s.prepare(&u);
        let mut tiles = Vec::new();
        b.s.damage.tiles(&mut tiles);
        b.s.damage.clear();
        if idle {
            assert_eq!((fs.recs_visited, tiles.len(), fs.prefix_tiles, fs.blocks), (0, 0, 0, 0), "seed {seed} {fw}x{fh} step {k}: an idle frame did work");
            t.idle += 1;
        }
        if above_only && name != "clock" {
            assert_eq!((fs.prefix_tiles, fs.blocks), (0, 0), "seed {seed} {fw}x{fh} step {k} ({name}): a change above every pane reached a layer");
        }
        (t.prefix, t.blocks) = (t.prefix + fs.prefix_tiles as u64, t.blocks + fs.blocks as u64);
        if k > 0 {
            (t.out, t.all) = (t.out + tiles.len() as u64, t.all + nt as u64);
        }
        for s in stale.iter_mut() {
            tiles.iter().for_each(|&x| s[x as usize] = true);
        }
        let mut f = build(&wd, fw, fh);
        let mut fresh = vec![0u32; words];
        f.s.frame(&u, &mut fresh, stride, &pool, 0);
        let mut d = [0usize; 4];
        let mut first = String::new();
        for j in 0..3 {
            if k % (j + 1) != 0 {
                continue;
            }
            let list: Vec<u32> = (0..nt as u32).filter(|&x| stale[j][x as usize]).collect();
            b.s.shade(&u, &list, &mut ring[j], stride, &pool, 0);
            stale[j].fill(false);
            d[j] = diff(&ring[j], &fresh);
            if d[j] != 0 && first.is_empty() {
                first = first_diff(&ring[j], &fresh, stride, fw as usize);
            }
        }
        if k % 3 == 0 {
            let mut spec = vec![0u32; words];
            b.s.spec(&u, &mut spec, stride);
            d[3] = diff(&spec, &fresh);
            t.spec_checked += 1;
        }
        t.steps += 1;
        if d != [0; 4] {
            let tail: Vec<_> = log.iter().rev().take(4).rev().collect();
            eprintln!("seed {seed} {fw}x{fh} step {k} ({name}; after {tail:?}): differing words at ring ages 1..3 {:?}, fresh vs spec {}; {first}", &d[..3], d[3]);
            t.bad += 1;
        }
    }
}

#[test]
fn retained_walk_equals_fresh() {
    let mut t = Tally::default();
    for seed in 0..env("BOUGH_SOLFX_SEEDS", 3) as u64 {
        walk(seed, 300, 230, env("BOUGH_SOLFX_STEPS", 60), &mut t);
    }
    report("walk 300x230", &t);
}
/// Frames that are not tile multiples, and frames of one tile or less (levels clamp to a few pixels).
#[test]
fn frame_sizes_equal_fresh() {
    let mut t = Tally::default();
    for (i, (fw, fh)) in [(257u32, 193u32), (96, 64), (33, 31), (31, 70), (20, 12), (512, 40)].into_iter().enumerate() {
        for seed in 0..env("BOUGH_SOLFX_SEEDS", 3) as u64 {
            walk(100 + i as u64 * 10 + seed, fw, fh, env("BOUGH_SOLFX_STEPS", 60) / 2, &mut t);
        }
    }
    report("frame sizes", &t);
}
fn report(what: &str, t: &Tally) {
    eprintln!("solfx {what}: {} steps ({} idle, {} against the specification); layer work {} prefix tiles, {} pyramid blocks; output {} of {} tiles; {} failing steps", t.steps, t.idle, t.spec_checked, t.prefix, t.blocks, t.out, t.all, t.bad);
    assert!(t.prefix > 0 && t.blocks > 0, "the walk never exercised a layer");
    assert!(t.out < t.all, "no damage ever stayed local");
    assert_eq!(t.bad, 0);
}

// ---------------------------------------------------------------------------------------------------
// Deterministic sweeps: one pixel at a time, every level.

/// A fixed world, then the pane under test (glass 1, overlapping glass 0, under glass 2 and the big pane,
/// which are made small/mild so they do not hide it) takes `blur`.
fn sweep_world(fw: u32, fh: u32, blur: f32) -> World {
    let mut wd = world(7, fw, fh);
    wd.gl[3].rect = [q(0.0), q(0.0), q(0.0), q(0.0)];
    wd.gl[1].blur = q(blur);
    wd.gl[0].blur = q(BLURS[(blur * 3.0) as usize % 24]);
    wd.gl[2].blur = q(blur / 2.0);
    wd.gl[1].tint = 0x1800_0000;
    wd.gl[1].f = [256, 256, 256];
    wd
}
fn pair(fw: u32, fh: u32, stride: usize, wd: &World, b: &mut Built, u: &U, kept: &mut [u32], pool: &Pool, what: &str, bad: &mut usize, t: &mut Tally) {
    let fs = b.s.update(u);
    b.s.prepare(u);
    let mut tiles = Vec::new();
    b.s.damage.tiles(&mut tiles);
    b.s.damage.clear();
    b.s.shade(u, &tiles, kept, stride, pool, 0);
    let mut f = build(wd, fw, fh);
    let mut fresh = vec![0u32; kept.len()];
    f.s.frame(u, &mut fresh, stride, pool, 0);
    t.prefix += fs.prefix_tiles as u64;
    t.blocks += fs.blocks as u64;
    t.out += tiles.len() as u64;
    t.all += (b.s.tx * b.s.ty) as u64;
    t.steps += 1;
    let d = diff(kept, &fresh);
    if d != 0 {
        *bad += 1;
        t.bad += 1;
        eprintln!("{what}: {d} differing words; {}", first_diff(kept, &fresh, stride, fw as usize));
    }
}
const SWEEP_BLURS: [f32; 12] = [0.0, 0.6, 1.3, 1.95, 2.6, 3.9, 5.2, 7.7, 10.4, 15.0, 20.7, 60.0];

fn sweep(kind: &str, step: &dyn Fn(&mut World, usize), n: usize) {
    let (fw, fh) = (230u32, 200u32);
    let stride = fw as usize + 1;
    let pool = Pool::new(1);
    let mut t = Tally::default();
    let mut bad = 0;
    for &blur in &SWEEP_BLURS {
        let mut wd = sweep_world(fw, fh, blur);
        let mut b = build(&wd, fw, fh);
        let u = U::default();
        let mut kept = vec![0u32; stride * fh as usize];
        pair(fw, fh, stride, &wd, &mut b, &u, &mut kept, &pool, "mount", &mut bad, &mut t);
        for i in 0..n {
            step(&mut wd, i);
            let gc = wd.gl[1].clone();
            b.s.set(b.gl, 1, gc.rect, gl_rec(&gc));
            let (bi, bc) = (0usize, wd.bx[0].clone());
            b.s.set(b.bx, bi, bc.rect, bx_rec(&bc));
            pair(fw, fh, stride, &wd, &mut b, &u, &mut kept, &pool, &format!("{kind}: blur {blur} step {i}"), &mut bad, &mut t);
        }
    }
    eprintln!("sweep {kind}: {} frames, {} prefix tiles, {} blocks, output {} of {} tiles, {bad} failing", t.steps, t.prefix, t.blocks, t.out, t.all);
    assert!(t.prefix > 0);
    assert_eq!(bad, 0);
}

#[test]
fn sweep_move_x_across_every_edge() {
    sweep("move x", &|wd, i| wd.gl[1].rect = shift(wd.gl[1].rect, if i < 100 { 256 } else { -256 } + (i as i32 % 3) * 21, 0), 110);
}
#[test]
fn sweep_move_y_across_every_edge() {
    sweep("move y", &|wd, i| wd.gl[1].rect = shift(wd.gl[1].rect, 0, if i < 90 { 256 } else { -256 }), 100);
}
#[test]
fn sweep_move_diagonal_by_tile_less_one() {
    // 31 px a step: the window passes a tile every step and wraps its torus every few
    sweep("diagonal 31", &|wd, i| wd.gl[1].rect = shift(wd.gl[1].rect, if i % 2 == 0 { 31 * 256 } else { -29 * 256 + 100 }, 13 * 256), 14);
}
#[test]
fn sweep_resize_right_and_bottom() {
    sweep("resize", &|wd, i| {
        let r = &mut wd.gl[1].rect;
        if i < 70 {
            r[2] += 256;
            r[3] += 128 * (i as i32 % 2);
        } else {
            r[2] = (r[2] - 256).max(r[0] + 256);
            r[3] = (r[3] - 128).max(r[1] + 256);
        }
    }, 130);
}
#[test]
fn sweep_resize_left_and_top() {
    sweep("resize from the origin side", &|wd, i| {
        let r = &mut wd.gl[1].rect;
        if i < 50 {
            r[0] -= 256;
            r[1] -= 256 * (i as i32 % 2);
        } else {
            r[0] = (r[0] + 256).min(r[2] - 256);
            r[1] = (r[1] + 256).min(r[3] - 256);
        }
    }, 100);
}
/// The pane steps in and out of the frame across each edge, one pixel a step.
#[test]
fn sweep_enter_and_leave_every_side() {
    sweep("enter/leave", &|wd, i| {
        let (dx, dy) = [(1, 0), (0, 1), (-1, 0), (0, -1)][(i / 120).min(3)];
        // from the frame's middle out past each side in turn (and back in) at 3 px a step
        let k = (i % 120) as i32;
        let s = if k < 60 { 1 } else { -1 };
        wd.gl[1].rect = shift(wd.gl[1].rect, 3 * s * dx * 256, 3 * s * dy * 256);
    }, 400);
}
/// A one-pixel spot beneath the pane steps over every pixel around the edges of tiles, for every level.
#[test]
fn sweep_content_beneath_over_every_edge_pixel() {
    sweep("spot beneath", &|wd, i| {
        let (x, y) = (20 + i as i32 % 100, 60 + (i as i32 / 100) * 64);
        wd.bx[0] = Bx { rect: [q(x as f32), q(y as f32), q(x as f32 + 1.0), q(y as f32 + 1.0)], rad: 0, c: 0xffff_ffff };
    }, 200);
}

// ---------------------------------------------------------------------------------------------------
// Metal.

#[cfg(target_os = "macos")]
mod gpu {
    use super::*;
    use bough_paint::metal::Gpu;

    /// The retained staged frame (CPU schedule run as dispatches) equals the CPU's retained frame, and
    /// both equal fresh. Every `unsafe { Gpu::new(..) }`: the scene is declared before the executor and
    /// outlives it; its arena is written between the executor's calls, each of which waits for the GPU.
    #[test]
    fn gpu_staged_equals_cpu_and_fresh() {
        let (fw, fh) = (300u32, 230u32);
        let stride = fw as usize + 3;
        let words = stride * fh as usize;
        let pool = Pool::new(2);
        let (mut bad, mut steps) = (0, 0);
        for seed in 0..env("BOUGH_SOLFX_SEEDS", 3) as u64 {
            let mut wd = world(seed, fw, fh);
            let mut b = build(&wd, fw, fh);
            let Some(mut gpu) = (unsafe { Gpu::new(&b.s, words, 256) }) else {
                eprintln!("no Metal device: skipped");
                return;
            };
            let mut g = Rng(seed * 977 + 13);
            let mut u = block(&mut g);
            let mut kept = vec![0u32; words];
            for k in 0..env("BOUGH_SOLFX_STEPS", 60) {
                let (name, _, _) = if k == 0 { ("mount", false, false) } else { step(&mut g, &mut u, &mut wd, &mut b, fw, fh) };
                b.s.update(&u);
                b.s.prepare(&u);
                let mut tiles = Vec::new();
                b.s.damage.tiles(&mut tiles);
                b.s.damage.clear();
                b.s.shade(&u, &tiles, &mut kept, stride, &pool, 0);
                gpu.staged(&b.s, &u, &tiles, stride);
                let mut f = build(&wd, fw, fh);
                let mut fresh = vec![0u32; words];
                f.s.frame(&u, &mut fresh, stride, &pool, 0);
                // the staged frame writes the damaged tiles only: compare there, and the CPU's whole
                let (dg, dc) = (diff(gpu.out(words), &fresh), diff(&kept, &fresh));
                steps += 1;
                if dg != 0 || dc != 0 {
                    eprintln!("seed {seed} step {k} ({name}): gpu differs from fresh in {dg} words, cpu in {dc}; {}", first_diff(gpu.out(words), &fresh, stride, fw as usize));
                    bad += 1;
                }
            }
        }
        eprintln!("solfx gpu staged: {steps} steps, {bad} failing");
        assert_eq!(bad, 0);
    }

    /// PROBE: what the layered resident `Gpu::frame` does. Expected (by its assertion): refuses. The test
    /// passes if it refuses *with that message* and fails if it silently draws something.
    #[test]
    fn probe_layered_gpu_frame() {
        let (fw, fh) = (128u32, 96u32);
        let wd = world(1, fw, fh);
        let b = build(&wd, fw, fh);
        // SAFETY: the scene owns the input storage and outlives this executor.
        let mut gpu = unsafe { Gpu::new(&b.s, (fw * fh) as usize, 256) }.expect("Metal device required for resident refusal probe");
        let u = U::default();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            gpu.frame(&u, None, fw as usize);
        }));
        match r {
            Err(e) => {
                let m = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                eprintln!("probe: Gpu::frame on a layered scene panics: {m}");
                assert!(m.contains("staged"));
            }
            Ok(()) => panic!("Gpu::frame on a layered scene returned: it drew without scheduling the layers"),
        }
        // and the catch-up path the driver uses between executors
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| gpu.catch_up(&u, fw as usize)));
        eprintln!("probe: Gpu::catch_up on a layered scene {}", if r.is_err() { "panics" } else { "returns" });
    }

    /// PROBE: the driver on a layered scene, through the executor it chooses. On the CPU path it must run
    /// the layers' stages (`Scene::prepare`) between `update` and `shade`; on the GPU path it must use
    /// `staged`. A driver that does neither draws stale or empty layers (or panics).
    fn drive(gpu: bool, name: &str) {
        use bough_paint::driver::{Driver, Ran};
        use bough_paint::plan::Whole;
        let (fw, fh) = (200u32, 150u32);
        let pool = Pool::new(1);
        let mut wd = world(5, fw, fh);
        let b = build(&wd, fw, fh);
        let (cols, gc, bc) = (b.gl, b.gl, b.bx);
        let _ = (cols, bc);
        let whole = Whole { gpu, block: u64::MAX, rebin_on: u64::MAX, cut: true, pick: false, cpu_tiles: 0, words: 0 };
        let Built { s, .. } = b;
        let mut d = Driver::new(s, whole, 1);
        assert_eq!(d.gpu().is_some(), gpu, "requested executor must be present");
        let mut g = Rng(99);
        let mut u = block(&mut g);
        let mut bad = 0;
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for k in 0..14 {
                if k > 0 {
                    let i = g.u(wd.gl.len() as u32) as usize;
                    wd.gl[i].rect = sane(shift(wd.gl[i].rect, (g.u(70) as i32 - 35) * 256, (g.u(50) as i32 - 25) * 256), false);
                    wd.gl[i].blur = q(BLURS[g.u(24) as usize]);
                    d.set(gc, i, wd.gl[i].rect, gl_rec(&wd.gl[i]));
                    u = block(&mut g);
                }
                let ran = d.frame(&mut u, None);
                assert_eq!(ran, if gpu { Ran::Gpu } else { Ran::Cpu });
                let mut f = build(&wd, fw, fh);
                let mut fresh = vec![0u32; (fw * fh) as usize];
                f.s.frame(&u, &mut fresh, fw as usize, &pool, 0);
                let df = diff(d.pixels(), &fresh);
                if df != 0 {
                    eprintln!("probe {name}: frame {k}: driver surface differs from fresh in {df} words; {}", first_diff(d.pixels(), &fresh, fw as usize, fw as usize));
                    bad += 1;
                }
            }
        }));
        if let Err(e) = r {
            let m = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
            panic!("probe {name}: Driver::frame panicked: {m}");
        }
        assert_eq!(bad, 0, "probe {name}: the driver's frames differ from fresh");
    }
    #[test]
    fn probe_driver_cpu_path_on_layers() {
        drive(false, "driver cpu-only");
    }
    #[test]
    fn probe_driver_gpu_path_on_layers() {
        drive(true, "driver gpu-resident");
    }
}
