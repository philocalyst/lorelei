//! The effects gates (card 2026-11-21, style-motion-media): layers, springs, video and shader paints on
//! adversarial scenes.
//!
//! * G1: a frame retained across random steps (state words, the clock, spring retargets, record rewrites,
//!   published video frames) equals a fresh scene's full redraw, in surfaces of ring ages 1 to 4.
//! * G2: it equals the whole-frame specification (`Scene::spec`: no tiles, no layer store, no damage).
//! * G3: the Metal executor's staged frame equals it too, bytes.
//! * The counts the card promises: an idle frame does nothing; a time-only frame visits the records in
//!   motion; a settled scene is not animating.
use bough_kernel::cpu::Pool;
use bough_kernel::scenes::Rng;
use bough_paint::expr::*;
use bough_paint::media::Synth;
use bough_paint::motion::{Response, Spring, SpringX};
use bough_paint::paint::*;
use bough_paint::scene::{ColId, Scene};

const KS: Response = Response::SNAPPY;
type Sp = SpringX<852, 21_889>;
type Pan = Off<Lane, I<Uw<0>>, I<Uw<1>>>;
type Any = Pm<Rec>;
type Themed = SelV<Eq<Uw<2>, Rec>, Pm<K<0xff6b3bff>>, MixV<Pmo<Rec>, Pmo<Rec>, Uw<3>>>;
type ACard = Over<Shadow<Off<Pan, Ki<0>, I<Rec>>, I<Rec>, I<Rec>, Any, Tab>, RRect<Pan, I<Rec>, Themed>>;
type Sprung = Off<Pan, Sp, Sp>;
type AGlass = Over<Backdrop<Sprung, I<Rec>, I<Rec>, Filt<Rec, Rec, Rec>>, Border<Sprung, I<Rec>, Ki<256>, Any, Any>>;
type AVideo = Video<Pan, I<Rec>, Rec, Rec, Rec>;
type AShader = Shader<Pan, I<Rec>, Rec, Uw<63>, Any, Any>;
type ADot = RRect<Sprung, I<Rec>, MixV<Any, Any, EOut<Prog<Uw<63>, Rec, Rec>>>>;
type ALoop = RRect<Pan, I<Rec>, MixV<Any, Any, Tri<Uw<63>, Rec, Rec>>>;

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
fn sprung(x: &Spring, y: &Spring) -> Sprung {
    Off { b: pan(), dx: x.operand(), dy: y.operand() }
}

#[derive(Clone)]
struct Glass {
    rect: [i32; 4],
    rad: i32,
    blur: i32,
    f: [u32; 3],
    line: u32,
    tint: u32,
    sx: Spring,
    sy: Spring,
}
impl Glass {
    fn rec(&self) -> AGlass {
        let b = sprung(&self.sx, &self.sy);
        Over(Backdrop { b, r: ri(self.rad), blur: ri(self.blur), f: V3 { a: Rec(self.f[0]), b: Rec(self.f[1]), c: Rec(self.f[2]) } }, Border { b, r: ri(self.rad), w: Ki, c: pm(self.line), fill: pm(self.tint) })
    }
}
#[derive(Clone)]
struct Dot {
    rect: [i32; 4],
    c: [u32; 2],
    t0: u32,
    dur: u32,
    sx: Spring,
    sy: Spring,
}
impl Dot {
    fn rec(&self) -> ADot {
        RRect { b: sprung(&self.sx, &self.sy), r: ri(q(6.0)), c: MixV { a: pm(self.c[0]), b: pm(self.c[1]), t: EOut { t: Prog { now: Uw, t0: Rec(self.t0), dur: Rec(self.dur) } } } }
    }
}
#[derive(Clone)]
struct Vid {
    rect: [i32; 4],
    rad: i32,
    op: u32,
    w: u32,
    h: u32,
    seed: u32,
    /// Frames published so far.
    n: u32,
}

/// Everything a scene is built from: a fresh scene from the same world shows the same pixels.
#[derive(Clone)]
struct World {
    seed: u64,
    glass: Vec<Glass>,
    dots: Vec<Dot>,
    vids: Vec<Vid>,
}
struct Built {
    s: Scene,
    glass: ColId<AGlass>,
    dots: ColId<ADot>,
    vids: ColId<AVideo>,
    src: Vec<Synth>,
}
fn vid_rec(v: &Vid, slot: u32) -> AVideo {
    Video { b: pan(), r: ri(v.rad), slot: Rec(slot), dims: Rec(v.w | v.h << 16), op: Rec(v.op) }
}

fn world(seed: u64, fw: u32, fh: u32) -> World {
    let mut g = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let (w, h) = (fw as f32, fh as f32);
    let mut wd = World { seed, glass: Vec::new(), dots: Vec::new(), vids: Vec::new() };
    for i in 0..7 {
        let (x, y) = (g.pos(-10.0, w - 60.0), g.pos(-10.0, h - 50.0));
        let (bw, bh) = (g.pos(30.0, 150.0), g.pos(24.0, 110.0));
        let blur = [0.0, 0.6, 2.0, 5.0, 11.0, 23.0, 40.0][i % 7] + g.f(0.0, 1.0);
        let f = [[256, 256, 256], [300, 420, 256], [200, 0, 300], [256, 256, 256]][g.u(4) as usize];
        // staggered: some springs have yet to start
        let mut sx = Spring::rest(q(g.f(-30.0, 30.0)));
        sx.retarget(KS, g.u(300), q(g.f(-40.0, 40.0)));
        wd.glass.push(Glass { rect: [q(x), q(y), q(x + bw), q(y + bh)], rad: q(g.f(0.0, 20.0)), blur: q(blur), f, line: g.color(false), tint: g.color(false) & 0x60ff_ffff, sx, sy: Spring::rest(0) });
    }
    for _ in 0..5 {
        let (x, y) = (g.pos(0.0, w - 20.0), g.pos(0.0, h - 20.0));
        wd.dots.push(Dot { rect: [q(x), q(y), q(x + 14.0), q(y + 14.0)], c: [g.color(true), g.color(false)], t0: g.u(200), dur: 100 + g.u(600), sx: Spring::rest(0), sy: Spring::rest(0) });
    }
    for i in 0..3 {
        let (x, y) = (g.pos(0.0, w - 80.0), g.pos(0.0, h - 60.0));
        let (vw, vh) = ([64, 96, 50][i], [48, 54, 38][i]);
        wd.vids.push(Vid { rect: [q(x), q(y), q(x + g.pos(40.0, 130.0)), q(y + g.pos(30.0, 100.0))], rad: q(g.f(0.0, 12.0)), op: [32768, 20000, 32768][i], w: vw, h: vh, seed: g.u(100), n: 1 });
    }
    wd
}

fn build(wd: &World, fw: u32, fh: u32) -> Built {
    let mut g = Rng(wd.seed.wrapping_mul(0x5851_f42d_4c95_7f2d) | 1);
    let mut s = Scene::new(fw, fh, 1 << 18);
    s.media = bough_paint::media::Pool::new(1 << 16);
    let cards = s.column::<ACard>(64);
    let glass = s.column::<AGlass>(16);
    let vids = s.column::<AVideo>(8);
    let shaders = s.column::<AShader>(8);
    let dots = s.column::<ADot>(16);
    let loops = s.column::<ALoop>(8);
    let (w, h) = (fw as f32, fh as f32);
    let mut src: Vec<Synth> = Vec::new();
    let card = |g: &mut Rng, s: &mut Scene| {
        let (x, y) = (g.pos(-20.0, w).round(), g.pos(-20.0, h).round());
        let rect = [q(x), q(y), q(x + g.pos(10.0, 140.0).round()), q(y + g.pos(10.0, 100.0).round())];
        let rad = q(g.f(0.0, 16.0));
        let o2 = g.u(2) == 0;
        let (c1, c2) = (g.color(true), g.color(o2));
        s.push(cards, rect, Over(Shadow { b: Off { b: pan(), dx: Ki, dy: ri(q(g.f(0.0, 8.0).round())) }, r: ri(rad), blur: ri(q(2.0 * g.f(0.5, 9.0))), c: pm(g.color(false)), tab: Tab::NONE }, RRect { b: pan(), r: ri(rad), c: SelV { c: Eq { a: Uw, b: Rec(g.u(6)) }, a: Pm { c: K }, b: MixV { a: Pmo { c: Rec(c1) }, b: Pmo { c: Rec(c2) }, t: Uw } } }));
    };
    // paint order interleaves every kind: cards beneath glass, glass over glass, things above glass
    let (mut gi, mut di, mut vi) = (0, 0, 0);
    for k in 0..46 {
        match k % 9 {
            0 | 1 | 4 | 6 => card(&mut g, &mut s),
            2 | 7 if gi < wd.glass.len() => {
                s.push(glass, wd.glass[gi].rect, wd.glass[gi].rec());
                gi += 1;
            }
            3 if vi < wd.vids.len() => {
                let v = &wd.vids[vi];
                let mut sy = Synth::new(v.w, v.h, v.seed);
                sy.seq = v.n - 1;
                let slot = sy.next(&mut s.media).expect("media pool");
                s.push(vids, v.rect, vid_rec(v, slot));
                src.push(sy);
                vi += 1;
            }
            5 if di < wd.dots.len() => {
                s.push(dots, wd.dots[di].rect, wd.dots[di].rec());
                di += 1;
            }
            8 => {
                let (x, y) = (g.pos(0.0, w - 60.0), g.pos(0.0, h - 40.0));
                let rect = [q(x), q(y), q(x + g.pos(20.0, 90.0)), q(y + g.pos(16.0, 60.0))];
                if k % 2 == 0 {
                    s.push(shaders, rect, Shader { b: pan(), r: ri(q(g.f(0.0, 10.0))), id: Rec(g.u(2)), now: Uw, c: pm(g.color(true)), c2: pm(g.color(false)) });
                } else {
                    s.push(loops, rect, RRect { b: pan(), r: ri(q(4.0)), c: MixV { a: pm(g.color(true)), b: pm(g.color(false)), t: Tri { now: Uw, phase: Rec(g.u(500)), period: Rec(150 + g.u(900)) } } });
                }
            }
            _ => card(&mut g, &mut s),
        }
    }
    assert!(gi == wd.glass.len() && di == wd.dots.len() && vi == wd.vids.len());
    Built { s, glass, dots, vids, src }
}

fn block(g: &mut Rng) -> U {
    let mut u = U::default();
    u.w[0] = (g.u(40 * 256) as i32 - 20 * 256) as u32;
    u.w[1] = (g.u(40 * 256) as i32 - 20 * 256) as u32;
    u.w[2] = g.u(7);
    u.w[3] = [0, 32768, 12345][g.u(3) as usize];
    u
}
fn diff(a: &[u32], b: &[u32]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}
fn seeds() -> u64 {
    std::env::var("BOUGH_FX_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(6)
}
const STEPS: usize = 26;

/// One random step of the world and the retained scene. Returns its name.
fn step(g: &mut Rng, k: usize, u: &mut U, wd: &mut World, b: &mut Built) -> &'static str {
    let now = u.w[U_TIME];
    match [0, 2, 2, 4, 6, 1, 2, 3, 7, 5, 6, 2, 4, 2, 3, 7, 0, 5, 2, 6, 1, 2, 4, 2, 3, 7][k % 26] {
        0 => {
            let n = block(g);
            (u.w[0], u.w[1]) = (n.w[0], n.w[1]);
            "pan"
        }
        1 => {
            let n = block(g);
            (u.w[2], u.w[3]) = (n.w[2], n.w[3]);
            "hover and theme"
        }
        2 => {
            u.w[U_TIME] = now + 1 + g.u(40);
            "clock"
        }
        3 => {
            u.w[U_TIME] = now + 4000 + g.u(3000);
            "clock past every settle"
        }
        4 => {
            let i = g.u(wd.glass.len() as u32) as usize;
            let gl = &mut wd.glass[i];
            gl.sx.retarget(KS, now, q(g.f(-50.0, 50.0)));
            gl.sy.retarget(KS, now + g.u(60), q(g.f(-30.0, 30.0)));
            b.s.set(b.glass, i, gl.rect, gl.rec());
            u.w[U_TIME] = now + g.u(3) * 9;
            "glass retargeted"
        }
        5 => {
            let i = g.u(wd.dots.len() as u32) as usize;
            let d = &mut wd.dots[i];
            d.sx.retarget(KS, now, q(g.f(-60.0, 60.0)));
            (d.t0, d.dur) = (now + g.u(30), 80 + g.u(400));
            b.s.set(b.dots, i, d.rect, d.rec());
            "dot retargeted"
        }
        6 => {
            let i = g.u(wd.vids.len() as u32) as usize;
            let slot = b.src[i].next(&mut b.s.media).expect("media pool");
            wd.vids[i].n += 1;
            b.s.set(b.vids, i, wd.vids[i].rect, vid_rec(&wd.vids[i], slot));
            "video frame"
        }
        _ => "idle",
    }
}

#[test]
fn retained_equals_full_redraw_and_the_specification() {
    let (fw, fh) = (320u32, 250u32);
    let stride = fw as usize + 3;
    let words = stride * fh as usize;
    let pool = Pool::new(2);
    let (mut bad, mut idle, mut timed, mut restarts) = (0usize, 0usize, 0usize, 0u64);
    let (mut prefix, mut blocks, mut out_tiles, mut all_tiles) = (0u64, 0u64, 0u64, 0u64);
    for seed in 0..seeds() {
        let mut wd = world(seed, fw, fh);
        let mut b = build(&wd, fw, fh);
        let mut g = Rng(seed * 131 + 7);
        let mut u = block(&mut g);
        let nt = b.s.tx * b.s.ty;
        // surfaces of ring ages 1..=4: surface j is redrawn every (j + 1)-th frame from what went stale
        let mut ring = vec![vec![0u32; words]; 4];
        let mut stale = vec![vec![true; nt]; 4];
        for k in 0..STEPS {
            let name = if k == 0 { "mount" } else { step(&mut g, k, &mut u, &mut wd, &mut b) };
            let mut fs = b.s.update(&u);
            let st = b.s.prepare(&u);
            (fs.prefix_tiles, fs.blocks) = (st.prefix as u32, st.blk.iter().sum::<u64>() as u32);
            let mut tiles = Vec::new();
            b.s.damage.tiles(&mut tiles);
            b.s.damage.clear();
            b.s.media.reclaim();
            if name == "idle" {
                assert_eq!((fs.recs_visited, tiles.len(), fs.prefix_tiles, fs.blocks), (0, 0, 0, 0), "seed {seed} step {k}: an idle frame did work");
                idle += 1;
            }
            if name.starts_with("clock") {
                // a time-only frame visits the records in motion, never a whole column of records at rest
                assert!(fs.recs_visited as usize <= b.s.records(), "seed {seed} step {k}");
                timed += 1;
            }
            if name == "clock past every settle" {
                // only the endless loops (Tri) and the shader nodes still read the clock
                let endless = b.s.cols().iter().filter(|c| c.desc.name.contains("Tri") || c.desc.name.contains("Shader")).map(|c| c.len).sum::<usize>();
                assert_eq!(b.s.active_records(), endless, "seed {seed} step {k}: settled records are still active");
            }
            (prefix, blocks) = (prefix + fs.prefix_tiles as u64, blocks + fs.blocks as u64);
            if k > 0 {
                (out_tiles, all_tiles) = (out_tiles + tiles.len() as u64, all_tiles + nt as u64);
            }
            for s in stale.iter_mut() {
                tiles.iter().for_each(|&t| s[t as usize] = true);
            }
            // the reference: a fresh scene from the same world, every tile
            let mut f = build(&wd, fw, fh);
            let mut fresh = vec![0u32; words];
            f.s.frame(&u, &mut fresh, stride, &pool, 0);
            let mut d = [0usize; 5];
            for j in 0..4 {
                if k % (j + 1) != 0 {
                    continue;
                }
                let list: Vec<u32> = (0..nt as u32).filter(|&t| stale[j][t as usize]).collect();
                let st = b.s.shade(&u, &list, &mut ring[j], stride, &pool, (k % 2) * 2);
                restarts += st.tiles;
                stale[j].fill(false);
                d[j] = diff(&ring[j], &fresh);
            }
            if k % 5 == 0 || seed == 0 {
                let mut spec = vec![0u32; words];
                b.s.spec(&u, &mut spec, stride);
                d[4] = diff(&spec, &fresh);
            }
            if d != [0; 5] {
                eprintln!("seed {seed} step {k} ({name}): differing words at ring ages 1..4 {:?}, specification {}", &d[..4], d[4]);
                bad += 1;
            }
        }
    }
    eprintln!("fx exact: {} scenes x {STEPS} steps; {idle} idle frames did nothing, {timed} time-only frames; layer work {prefix} prefix tiles, {blocks} pyramid blocks; output {out_tiles} of {all_tiles} tiles; {restarts} tiles shaded; {bad} failing steps", seeds());
    assert!(prefix > 0 && blocks > 0 && out_tiles < all_tiles * 3 / 5);
    assert_eq!(bad, 0);
}

/// Springs: the leaf is continuous in value and (to the difference's resolution) in velocity across a
/// retarget, reaches its target exactly, and says when.
#[test]
fn springs_retarget_continuously_and_settle() {
    for (st, dm) in [(170.0, 26.0), (120.0, 8.0), (300.0, 40.0), (90.0, 30.0)] {
        let k = Response::new(st, dm);
        let mut s = Spring::rest(0);
        s.retarget(k, 1000, 200 * 256);
        let mut now = 1000;
        let mut g = Rng(st as u64);
        for _ in 0..40 {
            now += 5 + g.u(120);
            let (x0, v0) = (s.value(k, now), s.velocity(k, now));
            s.retarget(k, now, (g.u(600) as i32 - 300) * 256);
            let (x1, v1) = (s.value(k, now), s.velocity(k, now + 4));
            assert!((x1 - x0).abs() <= 2, "value jumped {x0} -> {x1} at a retarget ({st}, {dm})");
            // four ticks on, the velocity is the old one plus four ticks of the new target's pull (Q8 per ms)
            let pull = 4.0 * (st * (s.to - x1) as f64 / 1e3 - dm * v0) / 1e3;
            if s.dur > 12 {
                assert!((v1 - (v0 + pull)).abs() <= 0.04 * (v0.abs() + pull.abs()) + 5.0, "velocity jumped {v0} -> {v1} (expected {:.1}) at a retarget ({st}, {dm})", v0 + pull);
            }
        }
        // it arrives, exactly, and the step it takes there is under a quarter pixel
        let end = s.t0 + s.dur;
        assert!((s.value(k, end - 1) - s.to).abs() <= 64, "a visible step at the settle tick ({st}, {dm}): {}", s.value(k, end - 1) - s.to);
        assert_eq!(s.value(k, end), s.to);
        assert_eq!(s.value(k, end + 100_000), s.to);
        assert!(s.resting(end) && !s.resting(end - 1));
        // against the differential equation, integrated finely in floats: within a pixel throughout
        let mut s = Spring::rest(0);
        s.retarget(k, 0, 300 * 256);
        let (mut x, mut v) = (0.0f64, 0.0f64);
        let mut worst = 0.0f64;
        for t in 0..s.dur {
            for _ in 0..100 {
                let a = st * (300.0 - x) - dm * v;
                v += a * 1e-5;
                x += v * 1e-5;
            }
            worst = worst.max((s.value(k, t + 1) as f64 / 256.0 - x).abs());
        }
        eprintln!("spring ({st}, {dm}): response {k:?}, settles in {} ms, worst error against the ODE {worst:.3} px", s.dur);
        assert!(worst < 1.0, "({st}, {dm}): {worst}");
    }
}

/// The frame clock follows the output: ticks come from presentation times, and a scene at rest owes no frame.
#[test]
fn animation_is_paced_by_the_output_and_stops() {
    use bough_paint::motion::FrameClock;
    let (fw, fh) = (320u32, 250u32);
    let pool = Pool::new(1);
    for hz in [60u64, 120, 144] {
        let mut wd = world(3, fw, fh);
        // no endless motion in this scene: every spring and transition ends
        let mut b = build(&wd, fw, fh);
        let mut px = vec![0u32; (fw * fh) as usize];
        let mut clock = FrameClock::default();
        let mut u = U::default();
        let (mut frames, mut out_ns) = (0u64, 5_000_000_000u64);
        // a turn retargets one glass; then the host draws a frame per output refresh while the scene asks
        u.w[U_TIME] = clock.present(out_ns);
        b.s.frame(&u, &mut px, fw as usize, &pool, 0);
        let gl = &mut wd.glass[0];
        gl.sx.retarget(KS, clock.now(), q(45.0));
        b.s.set(b.glass, 0, gl.rect, gl.rec());
        let endless = b.s.cols().iter().filter(|c| c.desc.name.contains("Tri") || c.desc.name.contains("Shader")).map(|c| c.len).sum::<usize>();
        loop {
            out_ns += 1_000_000_000 / hz;
            u.w[U_TIME] = clock.present(out_ns);
            b.s.frame(&u, &mut px, fw as usize, &pool, 0);
            frames += 1;
            if b.s.active_records() == endless || frames > 10_000 {
                break;
            }
        }
        let ms = (out_ns - 5_000_000_000) / 1_000_000;
        eprintln!("output at {hz} Hz: the motion took {frames} frames over {ms} ms ({:.1} per second)", frames as f64 * 1000.0 / ms as f64);
        // one frame per refresh, no more, no fewer; and it ended
        assert!(frames < 10_000);
        assert!((frames as f64 * 1000.0 / ms as f64 - hz as f64).abs() < 1.5, "{hz} Hz: {frames} frames in {ms} ms");
    }
}

/// The dilation of layer damage, pinned: a small record beneath a glass panel changes colour right at the
/// edge of a pyramid block, for every level's block edge and every blur level. The pixels of the next block
/// over read it through their [1 3 3 1] taps, so the retained frame is right only if the damage crossed
/// the edge (`mutant_layer_dilate` must fail here).
#[test]
fn a_change_at_a_block_edge_reaches_the_next_block() {
    type ABack = Solid<Lane, Pm<Rec>>;
    type ASpot = Solid<Lane, SelV<Eq<Uw<2>, Rec>, Pm<Rec>, Pm<Rec>>>;
    type APane = Backdrop<Lane, Ki<0>, I<Rec>, Filt<K<256>, K<256>, K<256>>>;
    let (fw, fh) = (640u32, 300u32);
    let pool = Pool::new(1);
    let mut bad = 0;
    let mut cases = 0;
    for blur in [1.0f32, 2.0, 4.5, 9.0, 18.0, 36.0] {
        for edge in [64, 128, 256, 512] {
            for side in [0, 1] {
                let build = || {
                    let mut s = Scene::new(fw, fh, 1 << 16);
                    let (back, spot, pane) = (s.column::<ABack>(1), s.column::<ASpot>(4), s.column::<APane>(1));
                    s.push(back, [0, 0, q(fw as f32), q(fh as f32)], Solid { b: Lane, c: Pm { c: Rec(0xff20_3040) } });
                    // a 3 x 3 spot whose last (or first) column is the pixel beside the block edge
                    let x = if side == 0 { edge - 3 } else { edge };
                    s.push(spot, [q(x as f32), q(125.0), q(x as f32 + 3.0), q(128.0)], Solid { b: Lane, c: SelV { c: Eq { a: Uw, b: Rec(1) }, a: Pm { c: Rec(0xffff_ffff) }, b: Pm { c: Rec(0xff00_0000) } } });
                    s.push(pane, [q(8.0), q(8.0), q(fw as f32 - 8.0), q(fh as f32 - 8.0)], Backdrop { b: Lane, r: Ki, blur: ri(q(blur)), f: V3 { a: K, b: K, c: K } });
                    s
                };
                let mut s = build();
                let mut kept = vec![0u32; (fw * fh) as usize];
                let mut u = U::default();
                s.frame(&u, &mut kept, fw as usize, &pool, 0);
                u.w[2] = 1;
                let fs = s.frame(&u, &mut kept, fw as usize, &pool, 0);
                let mut fresh = vec![0u32; (fw * fh) as usize];
                build().frame(&u, &mut fresh, fw as usize, &pool, 0);
                let d = diff(&kept, &fresh);
                cases += 1;
                if d != 0 {
                    eprintln!("blur {blur}, edge {edge}, side {side}: {d} differing words ({} pyramid blocks repaired)", fs.blocks);
                    bad += 1;
                }
                assert!((fs.tiles as usize) < s.tx * s.ty, "a spot's change redrew the whole frame");
            }
        }
    }
    eprintln!("block edges: {cases} cases, {bad} failing");
    assert_eq!(bad, 0);
}

/// A glass pane whose window begins exactly on a tile corner, half a pixel inside it: the tiles along its
/// edge lie wholly inside its window, so they restart from the stored prefix, and their edge pixels are
/// partly covered, so the prefix shows there. (Where a backdrop covers a pixel fully the prefix is replaced,
/// which is why the random scenes alone do not pin the restart.)
fn aligned_pane(fw: u32, fh: u32) -> Scene {
    type ABack = Grad<Lane, Ki<0>, I<Rec>, I<Rec>, Pmo<Rec>, Pm<Rec>>;
    type APane = Over<Backdrop<Lane, I<Rec>, I<Rec>, Filt<K<256>, K<300>, K<256>>>, RRect<Lane, I<Rec>, Pm<Rec>>>;
    type ATop = RRect<Lane, Ki<1024>, SelV<Eq<Uw<2>, Rec>, Pm<Rec>, Pm<Rec>>>;
    let mut s = Scene::new(fw, fh, 1 << 16);
    let (back, pane, top) = (s.column::<ABack>(1), s.column::<APane>(2), s.column::<ATop>(2));
    s.push(back, [0, 0, q(fw as f32), q(fh as f32)], Grad { b: Lane, r: Ki, dx: ri(180_000), dy: ri(90_000), c: Pmo { c: Rec(0xff10_80f0) }, c2: pm(0xfff0_e010) });
    for (x, blur) in [(32.5f32, 7.0f32), (64.5, 0.0)] {
        let rect = [q(x), q(32.5), q(x + 150.0), q(190.5)];
        s.push(pane, rect, Over(Backdrop { b: Lane, r: ri(q(14.0)), blur: ri(q(blur)), f: V3 { a: K, b: K, c: K } }, RRect { b: Lane, r: ri(q(14.0)), c: pm(0x30ff_ffff) }));
    }
    s.push(top, [q(70.0), q(70.0), q(110.0), q(100.0)], RRect { b: Lane, r: Ki, c: SelV { c: Eq { a: Uw, b: Rec(1) }, a: pm(0xc0ff_4000), b: pm(0x8000_40ff) } });
    s
}

/// The stored prefix changes time, never pixels: with and without it (the measurement switch that redraws
/// every layer and restarts nowhere) the frames are the specification's, and a change above the panes
/// touches no layer.
#[test]
fn the_stored_prefix_never_changes_pixels() {
    let (fw, fh) = (256u32, 224u32);
    let pool = Pool::new(1);
    let mut frames = Vec::new();
    for nocache in [false, true] {
        let mut s = aligned_pane(fw, fh);
        s.fx_nocache = nocache;
        let mut px = vec![0u32; (fw * fh) as usize];
        let mut u = U::default();
        s.frame(&u, &mut px, fw as usize, &pool, 0);
        u.w[2] = 1;
        let fs = s.frame(&u, &mut px, fw as usize, &pool, 0);
        if !nocache {
            assert_eq!((fs.prefix_tiles, fs.blocks), (0, 0), "a change above the panes reached their layers");
            assert!(fs.tiles > 0 && fs.raster.entries <= 3 * fs.tiles as u64, "the tiles above the panes did not restart from the prefix: {} entries in {} tiles", fs.raster.entries, fs.tiles);
        }
        let mut spec = vec![0u32; (fw * fh) as usize];
        s.spec(&u, &mut spec, fw as usize);
        assert_eq!(diff(&px, &spec), 0, "nocache {nocache}");
        frames.push(px);
    }
    assert_eq!(diff(&frames[0], &frames[1]), 0);
}

/// R1 (slice 7): the resident plan. The same dispatches every frame, the schedule decided on the GPU from
/// the frame's events, nothing read back: retained across the steps, drawn into a surface that is shown only
/// every 1st to 4th frame (the tiles damaged since it was last drawn are a comparison of generations), it
/// writes the words a fresh CPU redraw writes. With and without the scene routing events itself. Then source
/// mutations of the plan's kernels, each of which must be seen.
#[cfg(target_os = "macos")]
#[test]
fn gpu_resident_plan_equals_the_cpu() {
    use bough_paint::metal::{Gpu, source_fx};
    let (fw, fh) = (320u32, 250u32);
    let stride = fw as usize + 3;
    let words = stride * fh as usize;
    let pool = Pool::new(2);
    // Every `unsafe { Gpu::.. }` below: the scene is declared before the executor and outlives it; the arena
    // and the media pool are written only between the executor's calls, each of which waits for the GPU.
    let run = |seed: u64, src: Option<&dyn Fn(&str) -> String>| -> Option<(usize, [u32; 4], usize)> {
        let mut wd = world(seed, fw, fh);
        let mut b = build(&wd, fw, fh);
        b.s.route_off = seed % 2 == 0;
        let mut gpu = match src {
            None => unsafe { Gpu::new(&b.s, words, 256) }?,
            Some(f) => {
                let descs: Vec<&bough_paint::scene::Desc> = b.s.cols().iter().map(|c| c.desc).collect();
                let lays: Vec<(usize, usize)> = b.s.cols().iter().map(|c| (c.base, c.desc.words)).collect();
                let good = source_fx(&descs, &lays, b.s.tab_region().0, (fw, fh), b.s.media.words().len());
                let bad = f(&good);
                assert!(bad != good, "the mutation did not apply");
                unsafe { Gpu::with_source(&b.s, words, 256, &bad) }?
            }
        };
        let age = 1 + (seed as usize / 2) % 4;
        let mut g = Rng(seed * 131 + 7);
        let mut u = block(&mut g);
        let mut kept = vec![0u32; words];
        let (mut bad, mut ran, mut shown, mut drawn) = (0, [0u32; 4], 0u32, 0);
        for k in 0..STEPS {
            let name = if k == 0 { "mount" } else { step(&mut g, k, &mut u, &mut wd, &mut b) };
            b.s.update(&u);
            let mut tiles = Vec::new();
            b.s.damage.tiles(&mut tiles);
            b.s.damage.clear();
            let draw = k % age == 0 || k + 1 == STEPS;
            let (_, era) = gpu.resident(&mut b.s, &u, &tiles, draw.then_some(shown), stride);
            b.s.media.reclaim();
            for (a, n) in ran.iter_mut().zip(gpu.resident_stats()) {
                *a += n;
            }
            if !draw {
                continue;
            }
            (shown, drawn) = (era, drawn + 1);
            build(&wd, fw, fh).s.frame(&u, &mut kept, stride, &pool, 0);
            let d = diff(gpu.out(words), &kept);
            if d != 0 && src.is_none() {
                eprintln!("seed {seed} step {k} ({name}), surface age {age}: resident gpu differs from cpu in {d} words");
            }
            bad += (d != 0) as usize;
        }
        Some((bad, ran, drawn))
    };
    let (mut bad, mut ran, mut drawn) = (0, [0u32; 4], 0);
    for seed in 0..seeds().max(8) {
        let Some((b, r, d)) = run(seed, None) else {
            eprintln!("no Metal device: skipped");
            return;
        };
        bad += b;
        drawn += d;
        ran.iter_mut().zip(r).for_each(|(a, n)| *a += n);
    }
    eprintln!("fx gpu resident: {} scenes x {STEPS} steps, {drawn} drawn frames at surface ages 1..4; ran {} prefix jobs and {} output tiles in {} dispatches; {bad} failing frames", seeds().max(8), ran[0], ran[2], ran[3]);
    assert!(ran[0] > 0);
    assert_eq!(bad, 0);
    let mutants: [(&str, &dyn Fn(&str) -> String); 6] = [
        ("a prefix tile never stamps its block changed", &|s| s.replace("metal::atomic_store_explicit(&CHG[J[5]], RP[0], metal::memory_order_relaxed); // prefix stamps changed", "")),
        ("a pyramid block never stamps itself changed", &|s| s.replace("metal::atomic_store_explicit(&CHG[s], gen, metal::memory_order_relaxed); // block stamps changed", "")),
        ("a pixel looks only at the block beneath its first tap", &|s| s.replace("by <= (2 * y + 2) >> s1 && !due; by++) {", "by <= (2 * y - 1) >> s1 && !due; by++) {").replace("bx <= (2 * x + 2) >> s1 && !due; bx++) {", "bx <= (2 * x - 1) >> s1 && !due; bx++) {")),
        ("the reach pass does not lower the tile keys", &|s| s.replace("metal::atomic_fetch_min_explicit(&KT[4u * t], z, metal::memory_order_relaxed); // reach lowers the tile key", "")),
        ("the reach pass damages no output", &|s| s.replace("{ metal::atomic_store_explicit(&KT[4u * t + 2u], gen, metal::memory_order_relaxed); }", "{ }")),
        ("any cover certifies", &|s| s.replace("if (c.x <= ox && c.y <= oy && c.z >= ox + 32 && c.w >= oy + 32) { return; } // every event is under this cover", "if (c.x < c.z) { return; }")),
    ];
    for (name, f) in mutants {
        let failing: usize = (0..seeds().max(8)).map(|seed| run(seed, Some(f)).unwrap().0).sum();
        eprintln!("resident mutant \"{name}\": {failing} failing frames");
        assert!(failing > 0, "the mutant \"{name}\" was not seen");
    }
}

/// G3: the Metal executor's staged frame (the CPU's schedule, run as dispatches over the scene's records and
/// the shared layer pool), retained across the steps, writes the words a fresh CPU redraw writes.
/// Then source mutations of the layer and media glue, each of which must be seen.
#[cfg(target_os = "macos")]
#[test]
fn gpu_staged_frame_equals_the_cpu() {
    use bough_paint::metal::{Gpu, source_fx};
    let (fw, fh) = (320u32, 250u32);
    let stride = fw as usize + 3;
    let words = stride * fh as usize;
    let pool = Pool::new(2);
    // Every `unsafe { Gpu::.. }` below: the scene is declared before the executor and outlives it; the arena
    // and the media pool are written only between the executor's calls, each of which waits for the GPU.
    let run = |seed: u64, src: Option<&dyn Fn(&str) -> String>| -> Option<(usize, usize, [u32; 4])> {
        let mut wd = world(seed, fw, fh);
        let mut b = build(&wd, fw, fh);
        let mut gpu = match src {
            None => unsafe { Gpu::new(&b.s, words, 256) }?,
            Some(f) => {
                let descs: Vec<&bough_paint::scene::Desc> = b.s.cols().iter().map(|c| c.desc).collect();
                let lays: Vec<(usize, usize)> = b.s.cols().iter().map(|c| (c.base, c.desc.words)).collect();
                let good = source_fx(&descs, &lays, b.s.tab_region().0, (fw, fh), b.s.media.words().len());
                let bad = f(&good);
                assert!(bad != good, "the mutation did not apply");
                unsafe { Gpu::with_source(&b.s, words, 256, &bad) }?
            }
        };
        let mut g = Rng(seed * 131 + 7);
        let mut u = block(&mut g);
        let mut kept = vec![0u32; words];
        let (mut bad, mut d_all, mut ran) = (0, 0, [0u32; 4]);
        for k in 0..STEPS {
            let name = if k == 0 { "mount" } else { step(&mut g, k, &mut u, &mut wd, &mut b) };
            // The retained scene is drawn by Metal alone (the layers' pool is shared memory now: a CPU
            // `prepare` on the same scene would repair what a broken GPU stage left). The reference is a
            // fresh scene drawn whole by the CPU kernel.
            b.s.update_eager(&u);
            let mut tiles = Vec::new();
            b.s.damage.tiles(&mut tiles);
            b.s.damage.clear();
            gpu.staged(&b.s, &u, &tiles, stride);
            build(&wd, fw, fh).s.frame(&u, &mut kept, stride, &pool, 0);
            b.s.media.reclaim();
            for (a, n) in ran.iter_mut().zip(gpu.staged_stats) {
                *a += n;
            }
            let d = diff(gpu.out(words), &kept);
            if d != 0 && src.is_none() {
                eprintln!("seed {seed} step {k} ({name}): gpu differs from cpu in {d} words");
            }
            bad += (d != 0) as usize;
            d_all += d;
        }
        Some((bad, d_all, ran))
    };
    let (mut bad, mut ran) = (0, [0u32; 4]);
    for seed in 0..seeds() {
        let Some((b, _, r)) = run(seed, None) else {
            eprintln!("no Metal device: skipped");
            return;
        };
        bad += b;
        ran.iter_mut().zip(r).for_each(|(a, n)| *a += n);
    }
    eprintln!("fx gpu exact: {} scenes x {STEPS} steps; ran {} prefix jobs, {} pyramid blocks, {} output tiles in {} dispatches; {bad} failing steps", seeds(), ran[0], ran[1], ran[2], ran[3]);
    assert!(ran[0] > 0 && ran[1] > 0);
    assert_eq!(bad, 0);
    // (No "flatten dropped" mutant: every job now starts at its topmost layer record from that record's stored,
    // already 8-bit prefix, so the flatten in the shade path is a no-op there; what it stood for is covered by
    // "prefix not stored" and "no restart".)
    let mutants: [(&str, &dyn Fn(&str) -> String); 5] = [
        ("pyramid taps shifted", &|s| s.replacen("2 * y - 1 + j", "2 * y + j", 1)),
        ("prefix not stored", &|s| s.replace("if (x_in(r, x, y)) { POOL[fx_at(LT, dest, 0u, x, y)] = x_store(a0); }", "")),
        ("no restart from the stored prefix", &|s| s.replace("a0 = x_unstore(POOL[fx_at(LT, init, 0u, x, y)]);", "")),
        ("chroma planes swapped", &|s| s.replacen("return vd_rgb(y, cb, cr);", "return vd_rgb(y, cr, cb);", 1)),
        ("coarser level never mixed", &|s| s.replacen("if (c_frac(c) != 0u) {", "if (false) {", 1)),
    ];
    // the aligned pane: tiles that restart from a stored prefix which shows through partial coverage
    let aligned = |src: Option<&dyn Fn(&str) -> String>| -> usize {
        let (fw, fh) = (256u32, 224u32);
        let words = (fw * fh) as usize;
        let mut s = aligned_pane(fw, fh);
        let mut gpu = match src {
            None => unsafe { Gpu::new(&s, words, 64) }.unwrap(),
            Some(f) => {
                let descs: Vec<&bough_paint::scene::Desc> = s.cols().iter().map(|c| c.desc).collect();
                let lays: Vec<(usize, usize)> = s.cols().iter().map(|c| (c.base, c.desc.words)).collect();
                unsafe { Gpu::with_source(&s, words, 64, &f(&source_fx(&descs, &lays, s.tab_region().0, (fw, fh), s.media.words().len()))) }.unwrap()
            }
        };
        let (mut kept, mut u, mut d) = (vec![0u32; words], U::default(), 0);
        for hover in [0, 1, 0] {
            u.w[2] = hover;
            s.update_eager(&u);
            let mut tiles = Vec::new();
            s.damage.tiles(&mut tiles);
            s.damage.clear();
            gpu.staged(&s, &u, &tiles, fw as usize);
            aligned_pane(fw, fh).frame(&u, &mut kept, fw as usize, &pool, 0);
            d += diff(gpu.out(words), &kept);
        }
        d
    };
    assert_eq!(aligned(None), 0, "the aligned pane differs between the executors");
    for (name, f) in mutants {
        if name.starts_with("no restart") {
            let d = aligned(Some(f));
            eprintln!("fx gpu mutant {name}: {d} differing words on the aligned pane");
            assert!(d > 0, "{name} was not caught");
            continue;
        }
        let (b, d, _) = run(2, Some(f)).unwrap();
        eprintln!("fx gpu mutant {name}: {d} differing words in {b} of {STEPS} steps");
        assert!(d > 0, "{name} was not caught");
    }
}

/// A glass panel dragged across tile edges, a pixel or a few at a time, under and over other panels: its halo
/// (whole tiles, where it flattens the accumulator) changes whenever it crosses one, and the tiles it left
/// and entered must be redrawn (`mutant_halo_damage` must fail here; so must `mutant_bins_stale`).
#[test]
fn a_halo_crossing_tile_edges_stays_exact() {
    type ABack = Grad<Lane, Ki<0>, I<Rec>, I<Rec>, Pmo<Rec>, Pm<Rec>>;
    type AVeil = RRect<Lane, Ki<2048>, Pm<Rec>>;
    type APane = Over<Backdrop<Lane, I<Rec>, I<Rec>, Filt<K<256>, K<256>, K<256>>>, RRect<Lane, I<Rec>, Pm<Rec>>>;
    let (fw, fh) = (384u32, 288u32);
    let pool = Pool::new(1);
    let pane = |x: f32, y: f32, blur: f32| -> ([i32; 4], APane) { ([q(x), q(y), q(x + 90.0), q(y + 60.0)], Over(Backdrop { b: Lane, r: ri(q(8.0)), blur: ri(q(blur)), f: V3 { a: K, b: K, c: K } }, RRect { b: Lane, r: ri(q(8.0)), c: pm(0x20ff_ffff) })) };
    let build = |pos: &[(f32, f32)]| {
        let mut s = Scene::new(fw, fh, 1 << 16);
        let (back, veil, panes) = (s.column::<ABack>(1), s.column::<AVeil>(8), s.column::<APane>(4));
        s.push(back, [0, 0, q(fw as f32), q(fh as f32)], Grad { b: Lane, r: Ki, dx: ri(150_000), dy: ri(110_000), c: Pmo { c: Rec(0xff20_60e0) }, c2: pm(0xffe0_c020) });
        // translucent veils everywhere: the accumulator is not on the 8-bit grid, so a flatten shows
        for k in 0..6 {
            s.push(veil, [q(20.0 + 55.0 * k as f32), q(10.0 + 37.0 * k as f32), q(140.0 + 55.0 * k as f32), q(120.0 + 37.0 * k as f32)], RRect { b: Lane, r: Ki, c: pm(0x5a30_9060 + 0x0f_1f07 * k) });
        }
        for (i, &(x, y)) in pos.iter().enumerate() {
            let (r, p) = pane(x, y, [3.0, 9.0, 20.0][i]);
            s.push(panes, r, p);
        }
        (s, panes)
    };
    let mut pos = [(40.3f32, 50.7f32), (120.0, 90.0), (200.5, 140.2)];
    let (mut s, panes) = build(&pos);
    let (mut kept, u) = (vec![0u32; (fw * fh) as usize], U::default());
    s.frame(&u, &mut kept, fw as usize, &pool, 0);
    let (mut bad, mut g) = (0, Rng(77));
    for step in 0..60 {
        let i = g.u(3) as usize;
        pos[i].0 += g.f(-9.0, 9.0);
        pos[i].1 += g.f(-7.0, 7.0);
        let (r, p) = pane(pos[i].0, pos[i].1, [3.0, 9.0, 20.0][i]);
        s.set(panes, i, r, p);
        s.frame(&u, &mut kept, fw as usize, &pool, 0);
        let mut fresh = vec![0u32; (fw * fh) as usize];
        build(&pos).0.frame(&u, &mut fresh, fw as usize, &pool, 0);
        let d = diff(&kept, &fresh);
        if d != 0 {
            eprintln!("halo crossing step {step} (pane {i}): {d} differing words");
            bad += 1;
        }
    }
    assert_eq!(bad, 0);
}

/// B1, B2: the mixed model. Frames alternate between the CPU kernel and Metal at random on one pool and one
/// surface and stay equal to a fresh redraw; a change of a word declared broad visits no record, rebuilds
/// no bins, enumerates no layer, and is right. (`mutant_broad_layers` must fail.)
#[cfg(target_os = "macos")]
#[test]
fn executors_alternate_and_a_broad_word_costs_nothing_to_find() {
    use bough_paint::mixed::{Mixed, Ran};
    let (fw, fh) = (320u32, 250u32);
    let stride = fw as usize;
    let pool = Pool::new(1);
    let (mut bad, mut broad, mut ran) = (0, 0, [0u32; 3]);
    for seed in 0..seeds() {
        let mut wd = world(seed, fw, fh);
        let mut b = build(&wd, fw, fh);
        // word 3 is the theme's mix: no geometry reads it
        b.s.broad = 1 << 3;
        // SAFETY: the scene is declared before the executor and outlives it; it is written between calls.
        let Some(mut mx) = (unsafe { Mixed::new(&b.s, stride) }) else {
            eprintln!("no Metal device: skipped");
            return;
        };
        let mut g = Rng(seed * 977 + 3);
        let mut u = block(&mut g);
        for k in 0..STEPS + 8 {
            let name = if k == 0 {
                "mount"
            } else if k % 5 == 3 {
                // the theme alone
                u.w[3] = [0, 32768, 12345, 20000][g.u(4) as usize] ^ (k as u32 & 1);
                "theme"
            } else {
                step(&mut g, k, &mut u, &mut wd, &mut b)
            };
            let before = b.s.bins_epoch;
            let force = [Some(Ran::Cpu), Some(Ran::Gpu), None][g.u(3) as usize];
            let (fs, r) = mx.frame(&mut b.s, &u, &pool, force);
            b.s.media.reclaim();
            ran[r as usize] += 1;
            if name == "theme" && fs.broad {
                assert_eq!((fs.recs_visited, b.s.bins_epoch, fs.prefix_tiles), (0, before, 0), "seed {seed} step {k}: a broad change was looked for");
                broad += 1;
            }
            let mut f = build(&wd, fw, fh);
            let mut fresh = vec![0u32; stride * fh as usize];
            f.s.frame(&u, &mut fresh, stride, &pool, 0);
            let d = diff(mx.out(), &fresh);
            if d != 0 {
                eprintln!("seed {seed} step {k} ({name}, ran {r:?}): {d} differing words");
                bad += 1;
            }
        }
    }
    eprintln!("mixed: {} scenes; frames idle / cpu / gpu {ran:?}; {broad} broad changes found without a record; {bad} failing steps", seeds());
    assert!(broad > 0 && ran[1] > 0 && ran[2] > 0);
    assert_eq!(bad, 0);
}

/// S1: a pan and a hover answered by index. A world 7,000 px tall seen through a 320 x 250 frame: cards keyed
/// by id, sprung glass and dots, all scrolled by words 0 and 1. With the declarations (`Scene::scroll`,
/// `Scene::keyed`) a pan visits the records near the old and new viewport and a hover the records of two ids;
/// the frames must be those of the same scene without the declarations, of a fresh redraw, and of the
/// specification. (`mutant_scroll_band` and `mutant_key_old` must fail.)
#[test]
fn a_pan_and_a_hover_visit_what_they_can_reach() {
    type AKey = Over<Shadow<Off<Pan, Ki<0>, Ki<768>>, Ki<1536>, Ki<2560>, Any, Tab>, RRect<Pan, Ki<1536>, SelV<Eq<Uw<2>, Rec>, Pm<K<0xff6b3bff>>, Pmo<Rec>>>>;
    let (fw, fh) = (320u32, 250u32);
    let stride = fw as usize;
    let pool = Pool::new(1);
    #[derive(Clone)]
    struct W {
        glass: Vec<Glass>,
        dots: Vec<Dot>,
    }
    let build = |w: &W, declare: bool| {
        let mut g = Rng(0xfeed);
        let mut s = Scene::new(fw, fh, 1 << 18);
        let (cards, glass, dots) = (s.column::<AKey>(256), s.column::<AGlass>(16), s.column::<ADot>(32));
        // endless paints all through the world: the clock does not move them, so those outside the frame
        // are parked (slice 9) and must animate again once a pan brings them into view
        let loops = s.column::<ALoop>(64);
        if declare {
            s.scroll(loops, 0, 1);
            s.scroll(cards, 0, 1);
            s.scroll(glass, 0, 1);
            s.scroll(dots, 0, 1);
            s.keyed(cards, 2);
        }
        let (mut gi, mut di) = (0, 0);
        for i in 0..200usize {
            let (x, y) = (8.0 + (i % 3) as f32 * 104.0 + g.f(0.0, 6.0), 6.0 + (i / 3) as f32 * 104.0 + g.f(0.0, 9.0));
            let rect = [q(x.round()), q(y.round()), q(x.round() + 92.0), q(y.round() + 88.0)];
            let id = 1 + (i as u32 % 40);
            s.push(cards, rect, Over(Shadow { b: Off { b: pan(), dx: Ki, dy: Ki }, r: Ki, blur: Ki, c: pm(0x5000_0000), tab: Tab::NONE }, RRect { b: pan(), r: Ki, c: SelV { c: Eq { a: Uw, b: Rec(id) }, a: Pm { c: K }, b: Pmo { c: Rec(g.color(true)) } } }));
            if declare {
                s.set_key(cards, i, id);
            }
            if i % 5 == 1 {
                s.push(loops, [q(x.round() + 10.0), q(y.round() + 60.0), q(x.round() + 50.0), q(y.round() + 80.0)], RRect { b: pan(), r: ri(q(4.0)), c: MixV { a: pm(g.color(true)), b: pm(g.color(true)), t: Tri { now: Uw, phase: Rec(i as u32 * 7), period: Rec(90) } } });
            }
            if i % 25 == 7 && gi < w.glass.len() {
                s.push(glass, w.glass[gi].rect, w.glass[gi].rec());
                gi += 1;
            }
            if i % 16 == 3 && di < w.dots.len() {
                s.push(dots, w.dots[di].rect, w.dots[di].rec());
                di += 1;
            }
        }
        (s, glass, dots)
    };
    let mut g = Rng(4242);
    let mut w = W { glass: Vec::new(), dots: Vec::new() };
    for k in 0..8 {
        let (x, y) = (g.pos(10.0, 180.0), 60.0 + k as f32 * 850.0);
        w.glass.push(Glass { rect: [q(x), q(y), q(x + 120.0), q(y + 70.0)], rad: q(10.0), blur: q([2.0, 7.0, 15.0][k % 3]), f: [256, 320, 256], line: 0x80ff_ffff, tint: 0x30ff_ffff, sx: Spring::rest(0), sy: Spring::rest(0) });
    }
    for k in 0..12 {
        let (x, y) = (g.pos(0.0, 280.0), 30.0 + k as f32 * 560.0);
        w.dots.push(Dot { rect: [q(x), q(y), q(x + 14.0), q(y + 14.0)], c: [g.color(true), g.color(false)], t0: 0, dur: 1, sx: Spring::rest(0), sy: Spring::rest(0) });
    }
    let (mut s, glass, dots) = build(&w, true);
    let mut most_active = 0;
    let (mut kept, mut u) = (vec![0u32; stride * fh as usize], U::default());
    s.frame(&u, &mut kept, stride, &pool, 0);
    let (mut bad, mut pans, mut hovers, mut pan_recs, mut hover_recs) = (0, 0u32, 0u32, 0u32, 0u32);
    for step in 0..90 {
        let now = u.w[U_TIME];
        let kind = [0, 0, 1, 2, 0, 3, 1, 4, 0, 5][step % 10];
        match kind {
            // a pan: a few pixels, a page, or a jump across the world (and back up)
            0 => {
                let dy = [11.0, -11.0, 240.0, 3100.0, -2900.0, 1.0, 600.0][g.u(7) as usize];
                let y = ((u.w[1] as i32) as f32 / 256.0 - dy).clamp(-6800.0, 0.0);
                (u.w[0], u.w[1]) = ((q(g.f(-4.0, 4.0).round())) as u32, q(y.round()) as u32);
            }
            1 => u.w[2] = g.u(42),
            2 => u.w[U_TIME] = now + 5 + g.u(60),
            3 => {
                let i = g.u(w.dots.len() as u32) as usize;
                w.dots[i].sx.retarget(KS, now, q(g.f(-40.0, 40.0)));
                w.dots[i].sy.retarget(KS, now, q(g.f(-300.0, 300.0)));
                s.set(dots, i, w.dots[i].rect, w.dots[i].rec());
            }
            4 => {
                let i = g.u(w.glass.len() as u32) as usize;
                w.glass[i].sy.retarget(KS, now, q(g.f(-200.0, 200.0)));
                s.set(glass, i, w.glass[i].rect, w.glass[i].rec());
            }
            _ => {
                // a pan and a hover and the clock in one turn
                (u.w[1], u.w[2], u.w[U_TIME]) = ((u.w[1] as i32 - q(37.0)).max(q(-6800.0)) as u32, g.u(42), now + 16);
            }
        }
        let fs = s.frame(&u, &mut kept, stride, &pool, 0);
        most_active = most_active.max(s.active_records());
        if kind == 0 {
            (pans, pan_recs) = (pans + 1, pan_recs + fs.recs_visited);
        }
        if kind == 1 {
            (hovers, hover_recs) = (hovers + 1, hover_recs + fs.recs_visited);
        }
        let mut plain = vec![0u32; stride * fh as usize];
        build(&w, false).0.frame(&u, &mut plain, stride, &pool, 0);
        let mut d = [diff(&kept, &plain), 0];
        if step % 9 == 0 {
            let mut spec = vec![0u32; stride * fh as usize];
            s.spec(&u, &mut spec, stride);
            d[1] = diff(&spec, &plain);
        }
        if d != [0, 0] {
            eprintln!("scroll/hover step {step} (kind {kind}): {} words differ from the undeclared fresh scene, specification {}", d[0], d[1]);
            bad += 1;
        }
    }
    eprintln!("parked clock readers: 40 endless paints in the world, at most {most_active} records in an active set");
    assert!(most_active < 30, "endless paints outside the frame are in the active set");
    eprintln!("scroll and hover by index: {} records in the scene; a pan visited {:.1} records on average, a hover {:.1}; {bad} failing steps", s.records(), pan_recs as f32 / pans as f32, hover_recs as f32 / hovers as f32);
    // a hover is the records of two ids (five cards each); a pan is the records near two viewports
    assert!(hover_recs <= hovers * 10 && pan_recs < pans * 60, "the indexes did not narrow the visits");
    assert_eq!(bad, 0);
}

// ---- slice 8: palette tables and the broad clock ----
type PS = Pal<PalIx<Uw<4>, Rec>>;
type PT = Pal<PalIx<Uw<5>, Rec>>;
type PCol = MixV<Pm<PS>, Pm<PT>, Uw<6>>;
type LCol = MixV<Pm<Rec>, Pm<Rec>, Uw<6>>;
type PGlassF = Filt<K<256>, K<300>, K<256>>;
/// One description of the paletted scene: a backdrop, cards whose colour is one of two slots by a hover word,
/// two glass panes. `lit`: `None` builds it over palette slots, `Some(tables, a, b)` with every slot
/// replaced by its colour in tables `a` and `b`.
fn pal_scene(fw: u32, fh: u32, lit: Option<(&[[u32; 16]], usize, usize)>) -> Scene {
    let mut s = Scene::new(fw, fh, 1 << 16);
    let mut g = Rng(0x9a1e77e);
    let cards: Vec<([i32; 4], u32, u32, u32)> = (0..14)
        .map(|i| {
            let (x, y) = (8.0 + (i % 5) as f32 * 62.5 + (g.next() % 9) as f32 * 0.5, 10.0 + (i / 5) as f32 * 74.0 + (g.next() % 7) as f32);
            ([q(x), q(y), q(x + 56.0), q(y + 60.0)], i as u32 + 1, (g.next() % 16) as u32, (g.next() % 16) as u32)
        })
        .collect();
    let panes = [([q(40.5), q(30.5), q(200.0), q(120.0)], 7.0f32, 3u32), ([q(150.0), q(100.0), q(300.5), q(215.5)], 3.0, 9)];
    match lit {
        None => {
            type ABack = Solid<Lane, MixV<Pmo<PS>, Pmo<PT>, Uw<6>>>;
            type ACardP = RRect<Lane, I<Rec>, SelV<Eq<Uw<2>, Rec>, PCol, PCol>>;
            type AGl = Over<Backdrop<Lane, I<Rec>, I<Rec>, PGlassF>, RRect<Lane, I<Rec>, PCol>>;
            let (back, card, glass) = (s.column::<ABack>(1), s.column::<ACardP>(16), s.column::<AGl>(2));
            let pc = |slot: u32| -> PCol { MixV { a: Pm { c: Pal(PalIx { t: Uw, s: Rec(slot) }) }, b: Pm { c: Pal(PalIx { t: Uw, s: Rec(slot) }) }, t: Uw } };
            s.push(back, [0, 0, q(fw as f32), q(fh as f32)], Solid { b: Lane, c: MixV { a: Pmo { c: Pal(PalIx { t: Uw, s: Rec(0) }) }, b: Pmo { c: Pal(PalIx { t: Uw, s: Rec(0) }) }, t: Uw } });
            for (i, &(r, id, a, b)) in cards.iter().enumerate() {
                if i == 7 {
                    for &(pr, blur, slot) in &panes {
                        s.push(glass, pr, Over(Backdrop { b: Lane, r: ri(q(10.0)), blur: ri(q(blur)), f: V3 { a: K, b: K, c: K } }, RRect { b: Lane, r: ri(q(10.0)), c: pc(slot) }));
                    }
                }
                s.push(card, r, RRect { b: Lane, r: ri(q(6.0)), c: SelV { c: Eq { a: Uw, b: Rec(id) }, a: pc(a), b: pc(b) } });
            }
            s.clock(6, 7, 8);
            s.broad = 1 << 4 | 1 << 5 | 1 << 6;
        }
        Some((tabs, ta, tb)) => {
            type ABack = Solid<Lane, MixV<Pmo<Rec>, Pmo<Rec>, Uw<6>>>;
            type ACardL = RRect<Lane, I<Rec>, SelV<Eq<Uw<2>, Rec>, LCol, LCol>>;
            type AGl = Over<Backdrop<Lane, I<Rec>, I<Rec>, PGlassF>, RRect<Lane, I<Rec>, LCol>>;
            let (back, card, glass) = (s.column::<ABack>(1), s.column::<ACardL>(16), s.column::<AGl>(2));
            let lc = |slot: u32| -> LCol { MixV { a: Pm { c: Rec(tabs[ta][slot as usize]) }, b: Pm { c: Rec(tabs[tb][slot as usize]) }, t: Uw } };
            s.push(back, [0, 0, q(fw as f32), q(fh as f32)], Solid { b: Lane, c: MixV { a: Pmo { c: Rec(tabs[ta][0]) }, b: Pmo { c: Rec(tabs[tb][0]) }, t: Uw } });
            for (i, &(r, id, a, b)) in cards.iter().enumerate() {
                if i == 7 {
                    for &(pr, blur, slot) in &panes {
                        s.push(glass, pr, Over(Backdrop { b: Lane, r: ri(q(10.0)), blur: ri(q(blur)), f: V3 { a: K, b: K, c: K } }, RRect { b: Lane, r: ri(q(10.0)), c: lc(slot) }));
                    }
                }
                s.push(card, r, RRect { b: Lane, r: ri(q(6.0)), c: SelV { c: Eq { a: Uw, b: Rec(id) }, a: lc(a), b: lc(b) } });
            }
        }
    }
    s
}

/// P1 / P2 (slice 8): a scene whose colours are slots of resident palette tables, retained across table
/// switches, timed cross-fades on the scene's derived clock word, hovers that choose between two slots, and
/// table rewrites, equals the same scene with every slot replaced by its literal colour (drawn fresh, and by
/// the specification); on the CPU kernel and on Metal's resident plan. A fade frame visits no record.
#[test]
fn palette_tables_equal_their_literal_colours() {
    let (bad, bad_gpu, fades, switches, device) = palette_run(false);
    eprintln!("palette tables: 4 resident tables, 90 steps, {fades} fade frames with 0 records visited, {switches} switches; {bad} failing steps on the CPU, {bad_gpu} on Metal{}", if device { "" } else { " (no device)" });
    assert!(fades > 5 && switches > 0);
    assert_eq!((bad, bad_gpu), (0, 0));
    if device {
        // the Metal read ignores which table: must be seen
        let (_, bad_gpu, ..) = palette_run(true);
        eprintln!("palette mutant \"the Metal read ignores the table\": {bad_gpu} failing steps");
        assert!(bad_gpu > 0);
    }
}
fn palette_run(mutate: bool) -> (usize, usize, usize, usize, bool) {
    let (fw, fh) = (320u32, 250u32);
    let words = (fw * fh) as usize;
    let pool = Pool::new(2);
    let mut g = Rng(0x5eed_9a1);
    let ntab = 4;
    let mut tabs: Vec<[u32; 16]> = (0..ntab).map(|_| std::array::from_fn(|_| g.next() as u32 | if g.next() % 3 == 0 { 0xff00_0000 } else { 0x4000_0000 })).collect();
    let mut s = pal_scene(fw, fh, None);
    let mut sg = pal_scene(fw, fh, None);
    for (t, c) in tabs.iter().enumerate() {
        s.set_palette(t, c);
        sg.set_palette(t, c);
    }
    // SAFETY: `sg` is declared before the executor and outlives it; it is written only between its calls.
    let mut gpu = if mutate {
        let descs: Vec<&bough_paint::scene::Desc> = sg.cols().iter().map(|c| c.desc).collect();
        let lays: Vec<(usize, usize)> = sg.cols().iter().map(|c| (c.base, c.desc.words)).collect();
        let good = bough_paint::metal::source_fx(&descs, &lays, sg.tab_region().0, (fw, fh), sg.media.words().len());
        let bad = good.replace("+ int(i)) : 0u; }", "+ int(i & 255u)) : 0u; }");
        assert!(bad != good, "the mutation did not apply");
        unsafe { bough_paint::metal::Gpu::with_source(&sg, words, 64, &bad) }
    } else {
        unsafe { bough_paint::metal::Gpu::new(&sg, words, 64) }
    };
    let (mut kept, mut fresh, mut spec) = (vec![0u32; words], vec![0u32; words], vec![0u32; words]);
    let mut u = U::default();
    (u.w[4], u.w[5], u.w[7], u.w[8]) = (0, 1, 0, 0);
    let (mut bad, mut bad_gpu, mut fades, mut era, mut switches) = (0, 0, 0, 0u32, 0);
    for k in 0..90 {
        let name = match if k == 0 { 99 } else { g.next() % 8 } {
            0 => {
                // a cross-fade to another table, driven by the clock
                (u.w[4], u.w[5], u.w[7], u.w[8]) = (u.w[5], (g.next() % ntab as u64) as u32, u.w[U_TIME], 40);
                "fade starts"
            }
            1 => {
                u.w[2] = (g.next() % 16) as u32;
                "hover"
            }
            2 => {
                // an instant switch of both tables
                (u.w[4], u.w[5]) = ((g.next() % ntab as u64) as u32, (g.next() % ntab as u64) as u32);
                switches += 1;
                "switch"
            }
            3 if k % 3 == 0 => {
                let t = (g.next() % ntab as u64) as usize;
                tabs[t][(g.next() % 16) as usize] = g.next() as u32 | 0x2000_0000;
                s.set_palette(t, &tabs[t]);
                sg.set_palette(t, &tabs[t]);
                "table rewritten"
            }
            99 => "mount",
            _ => "tick",
        };
        u.w[U_TIME] += 8;
        let was_animating = s.animating();
        let fs = s.frame(&u, &mut kept, fw as usize, &pool, 0);
        // the literal scene under the same block with the clock's word filled in by hand
        let mut ul = u;
        ul.w[6] = bough_paint::scene::clock_at(u.w[U_TIME], u.w[7], u.w[8]);
        let mut lit = pal_scene(fw, fh, Some((&tabs, u.w[4] as usize, u.w[5] as usize)));
        lit.frame(&ul, &mut fresh, fw as usize, &pool, 0);
        lit.spec(&ul, &mut spec, fw as usize);
        let (d1, d2) = (diff(&kept, &fresh), diff(&fresh, &spec));
        if d1 + d2 != 0 && !mutate {
            eprintln!("palette step {k} ({name}): retained paletted vs literal {d1} words, literal vs spec {d2}");
        }
        bad += (d1 + d2 != 0) as usize;
        // a frame of a running fade is a broad fact: no record is asked
        if name == "tick" && was_animating && fs.mask >> 6 & 1 != 0 {
            fades += 1;
            assert!(fs.broad && fs.recs_visited == 0, "a fade frame visited {} records", fs.recs_visited);
        }
        if let Some(gpu) = gpu.as_mut() {
            sg.update(&u);
            let mut tiles = Vec::new();
            sg.damage.tiles(&mut tiles);
            sg.damage.clear();
            if !tiles.is_empty() || sg.owes_layers() {
                let block = *sg.block();
                era = gpu.resident(&mut sg, &block, &tiles, Some(era), fw as usize).1;
            }
            let d = diff(gpu.out(words), &fresh);
            if d != 0 && !mutate {
                eprintln!("palette step {k} ({name}): metal vs literal {d} words");
            }
            bad_gpu += (d != 0) as usize;
        }
    }
    // a finished fade leaves nothing animating
    u.w[U_TIME] += 400;
    s.frame(&u, &mut kept, fw as usize, &pool, 0);
    assert!(!s.animating(), "a finished fade still owes frames");
    (bad, bad_gpu, fades, switches, gpu.is_some())
}

#[test]
fn spring_hull_matches_discrete_extrema() {
    let mut rng = Rng(0xf510);
    let mut cases = 0;
    for (decay, omega) in [(852, 21_889), (961, 0), (204, 0x8000_059b), (100, 4_000_000), (0, 0)] {
        let response = Response { decay, omega };
        for i in 0..20u32 {
            let t0 = if i % 2 == 0 { 1000 } else { u32::MAX - 10 };
            let from = (rng.next() % 50_000) as i32 - 25_000;
            let target = (rng.next() % 100_000) as i32 - 50_000;
            let mut spring = Spring::launch(response, t0, from, 20.0 - f64::from(i), target);
            if i % 3 == 0 {
                spring.retarget(response, t0.wrapping_add(230), target + 5120);
            }
            let operand: Spr<Uw<63>, Rec, Rec, I<Rec>, I<Rec>, I<Rec>, Rec, Rec> = Spr { now: Uw, t0: Rec(spring.t0), dur: Rec(spring.dur), to: ri(spring.to), a0: ri(spring.a0), b0: ri(spring.b0), decay: Rec(decay), omega: Rec(omega) };
            for elapsed in [-77, 0, (spring.dur / 2) as i32, spring.dur as i32 + 1] {
                let mut u = U::default();
                u.w[U_TIME] = spring.t0.wrapping_add_signed(elapsed);
                let actual = operand.time_hull(&Cx::of(&u));
                let mut expected = (spring.to, spring.to);
                for tick in elapsed..spring.dur as i32 {
                    u.w[U_TIME] = spring.t0.wrapping_add_signed(tick);
                    let v = operand.eval(&Cx::of(&u));
                    expected = (expected.0.min(v), expected.1.max(v));
                }
                assert_eq!(actual, Some(expected), "elapsed {elapsed}, {spring:?}, {response:?}");
                cases += 1;
            }
        }
    }
    eprintln!("spring hull: {cases} remaining-motion intervals match the discrete leaf extrema");
}

#[test]
fn spring_hull_parks_and_wakes_at_output_ages_one_to_four() {
    type DotPaint = RRect<Sprung, Ki<768>, Pmo<K<0xffe0_8040>>>;
    let paint = |sy| DotPaint { b: sprung(&Spring::rest(0), &sy), r: Ki, c: Pmo { c: K } };
    let rect = [q(12.0), q(800.0), q(32.0), q(812.0)];
    let initial = Spring::launch(KS, 800, 0, 0.0, q(-568.0));
    let (fw, fh, stride) = (64, 96, 69);
    let words = stride * fh as usize;
    let pool = Pool::new(1);
    let make = |motion, indexed| {
        let mut scene = Scene::new(fw, fh, 16_384);
        let col = scene.column::<DotPaint>(1);
        scene.push(col, rect, paint(motion));
        if indexed {
            scene.scroll(col, 0, 1);
        }
        (scene, col)
    };
    for age in 1..=4 {
        let (mut cpu, col) = make(initial, true);
        let (mut metal, metal_col) = make(initial, true);
        metal.route_off = true;
        // SAFETY: `metal` outlives the executor. Writes take place between synchronous calls.
        let mut gpu = unsafe { bough_paint::metal::Gpu::new(&metal, words, 16) }.expect("headless Metal");
        let mut ring = vec![vec![0; words]; age];
        let mut pending: Vec<_> = (0..age)
            .map(|_| {
                let mut d = bough_paint::scene::Damage::new(cpu.tx, cpu.ty);
                d.all();
                d
            })
            .collect();
        let mut era = 0;
        let mut tiles = Vec::new();
        let mut u = U::default();
        let mut motion = initial;
        let mut fresh = vec![0; words];
        for step in 0..60 {
            let i = step % age;
            u.w[U_TIME] = if step < 3 { 100 + step as u32 } else { 800 + (step as u32 - 3) * 24 };
            if step == 2 {
                u.w[1] = q(-160.5) as u32;
            }
            if step == 39 {
                u.w[1] = 0;
            }
            if step == 40 {
                motion.retarget(KS, u.w[U_TIME] + 48, q(-760.0));
                cpu.set(col, 0, rect, paint(motion));
                metal.set(metal_col, 0, rect, paint(motion));
            }
            let stats = cpu.update(&u);
            if step == 0 || step == 39 {
                assert!(!cpu.animating(), "age {age}, tick {step} should be parked");
            }
            if step == 1 {
                assert_eq!(stats.recs_visited, 0);
            }
            if step == 2 || step == 40 {
                assert_eq!(cpu.active_records(), 1, "age {age}, tick {step} should wake by its new hull");
            }
            let block = *cpu.block();
            cpu.prepare(&block);
            for d in &mut pending {
                for (a, b) in d.bits.iter_mut().zip(&cpu.damage.bits) {
                    *a |= *b;
                }
            }
            cpu.damage.clear();
            pending[i].tiles(&mut tiles);
            pending[i].clear();
            cpu.shade(&block, &tiles, &mut ring[i], stride, &pool, 0);
            let (mut oracle, _) = make(motion, false);
            oracle.frame(&u, &mut fresh, stride, &pool, 0);
            assert_eq!(diff(&ring[i], &fresh), 0, "CPU age {age}, tick {step}");
            metal.update(&u);
            metal.damage.tiles(&mut tiles);
            metal.damage.clear();
            let block = *metal.block();
            let draw = (step + 1) % age == 0;
            if !tiles.is_empty() || draw {
                let next = gpu.resident(&mut metal, &block, &tiles, draw.then_some(era), stride).1;
                if draw {
                    era = next;
                    assert_eq!(diff(gpu.out(words), &fresh), 0, "Metal age {age}, tick {step}");
                }
            }
        }
    }
    eprintln!("motion hull: 240 CPU frames and 125 Metal draws at ages 1..4, zero mismatching words");
}

#[test]
fn resident_layers_have_no_dispatch_when_all_events_are_above_them() {
    use bough_paint::metal::Gpu;
    let (fw, fh) = (128, 96);
    let words = (fw * fh) as usize;
    let pool = Pool::new(1);
    let make = || {
        let mut scene = Scene::new(fw, fh, 16_384);
        let under = scene.column::<Solid<Lane, Pmo<Uw<4>>>>(1);
        let glass = scene.column::<Backdrop<Lane, Ki<1024>, Ki<1536>, Filt<K<256>, K<256>, K<256>>>>(8);
        let above = scene.column::<Solid<Lane, Pmo<Uw<5>>>>(1);
        scene.push(under, [0, 0, q(128.0), q(96.0)], Solid { b: Lane, c: Pmo { c: Uw } });
        for i in 0..8 {
            scene.push(glass, [q(16.0 + i as f32), q(16.0), q(96.0), q(80.0)], Backdrop { b: Lane, r: Ki, blur: Ki, f: V3 { a: K, b: K, c: K } });
        }
        scene.push(above, [q(40.0), q(40.0), q(46.0), q(46.0)], Solid { b: Lane, c: Pmo { c: Uw } });
        scene
    };
    let mut scene = make();
    scene.route_off = true;
    // SAFETY: `scene` outlives `gpu` and writes occur between synchronous calls.
    let mut gpu = unsafe { Gpu::new(&scene, words, 64) }.expect("headless Metal");
    let mut u = U::default();
    let mut era = 0;
    let mut fresh = vec![0; words];
    let mut tiles = Vec::new();
    for step in 0..8u32 {
        if step % 2 == 0 {
            u.w[4] = 0x0020_4060 * (step / 2 + 1);
        }
        if step % 2 == 1 {
            u.w[5] = 0x0000_4040 * step;
        }
        scene.update(&u);
        scene.damage.tiles(&mut tiles);
        scene.damage.clear();
        let block = *scene.block();
        era = gpu.resident(&mut scene, &block, &tiles, Some(era), fw as usize).1;
        let stat = gpu.resident_stats();
        if step % 2 == 1 {
            assert_eq!(stat[0], 0, "above-glass frame repaired a prefix");
            assert_eq!(stat[3], 3, "above-glass frame encoded a layer dispatch");
        }
        let mut oracle = make();
        oracle.frame(&u, &mut fresh, fw as usize, &pool, 0);
        assert_eq!(diff(gpu.out(words), &fresh), 0, "step {step}");
    }
    eprintln!("rank seeds: 4 above-glass frames used 3 output/reset dispatches each, 8 exact frames");
}

#[test]
fn resident_commands_follow_changed_slots() {
    use bough_paint::metal::Gpu;
    use objc2_metal::{MTLCreateSystemDefaultDevice, MTLDevice, MTLGPUFamily};
    type Pane = Backdrop<Lane, Ki<1024>, Ki<1024>, Filt<K<256>, K<240>, K<256>>>;
    let pane = || Pane { b: Lane, r: Ki, blur: Ki, f: V3 { a: K, b: K, c: K } };
    let (fw, fh) = (640, 192);
    let words = (fw * fh) as usize;
    let pool = Pool::new(1);
    let left = |width: f32| [q(96.0), q(64.0), q(96.0 + width), q(128.0)];
    let right = |width: f32| [q(416.0), q(64.0), q(416.0 + width), q(128.0)];
    let make = |switched| {
        let mut s = Scene::new(fw, fh, 16_384);
        s.broad = 1 << 4;
        let bg = s.column::<Solid<Lane, Pmo<Uw<4>>>>(1);
        s.push(bg, [0, 0, q(640.0), q(192.0)], Solid { b: Lane, c: Pmo { c: Uw } });
        let col = s.column::<Pane>(3);
        s.push(col, left(32.0), pane());
        s.push(col, if switched { right(128.0) } else { left(128.0) }, pane());
        s.push(col, if switched { left(32.0) } else { right(32.0) }, pane());
        (s, col)
    };
    let (mut s, col) = make(false);
    s.route_off = true;
    let device = MTLCreateSystemDefaultDevice().expect("headless Metal");
    // SAFETY: `s` outlives the executor and changes only between its synchronous calls.
    let mut gpu = unsafe { Gpu::new(&s, words, 64) }.expect("headless Metal");
    let mut u = U::default();
    let (mut era, mut tiles, mut expected) = (0, Vec::new(), vec![0; words]);
    for step in 0..5u32 {
        let switched = step % 2 == 1;
        u.w[4] = 0x00e0_2040 - 0x0010_0703 * step;
        if step > 0 {
            s.set(col, 1, if switched { right(128.0) } else { left(128.0) }, pane());
            s.set(col, 2, if switched { left(32.0) } else { right(32.0) }, pane());
        }
        s.update(&u);
        s.damage.tiles(&mut tiles);
        s.damage.clear();
        let block = *s.block();
        era = gpu.resident(&mut s, &block, &tiles, Some(era), fw as usize).1;
        let (mut oracle, _) = make(switched);
        oracle.frame(&u, &mut expected, fw as usize, &pool, 0);
        assert_eq!(diff(gpu.out(words), &expected), 0, "slot reassignment at step {step}");
        assert_eq!(gpu.resident_shape().0, 2);
        if device.supportsFamily(MTLGPUFamily::Apple4) {
            assert_eq!(gpu.resident_stats()[3], 5, "the admitted frame did not use the cached sequence");
        }
    }
    eprintln!("resident commands: 5 exact frames through two ranks and four slot reassignments");
}

#[test]
fn a_prefix_change_reaches_the_next_checkpoint_without_a_pyramid_change() {
    type Pane = Backdrop<Lane, Ki<1024>, Ki<1536>, Filt<K<256>, K<256>, K<256>>>;
    let pane = || Pane { b: Lane, r: Ki, blur: Ki, f: V3 { a: K, b: K, c: K } };
    let (fw, fh) = (128, 96);
    let words = (fw * fh) as usize;
    let pixel = [q(40.0), q(28.0), q(41.0), q(29.0)];
    let pool = Pool::new(1);
    let build = || {
        let mut scene = Scene::new(fw, fh, 16_384);
        let bg = scene.column::<Solid<Lane, Pmo<K<0xff40_4040>>>>(1);
        scene.push(bg, [0, 0, q(128.0), q(96.0)], Solid { b: Lane, c: Pmo { c: K } });
        let change = scene.column::<Solid<Lane, Pmo<Uw<4>>>>(1);
        scene.push(change, pixel, Solid { b: Lane, c: Pmo { c: Uw } });
        let glass = scene.column::<Pane>(2);
        scene.push(glass, [q(44.0), q(32.0), q(111.0), q(85.0)], pane());
        scene.push(glass, [q(49.0), q(33.0), q(113.0), q(82.0)], pane());
        scene
    };
    let mut scene = build();
    let mut u = U::default();
    u.w[4] = 0x0040_4040;
    let (mut kept, mut fresh, mut spec) = (vec![0; words], vec![0; words], vec![0; words]);
    scene.frame(&u, &mut kept, fw as usize, &pool, 0);
    u.w[4] += 1;
    let fs = scene.frame(&u, &mut kept, fw as usize, &pool, 0);
    let mut oracle = build();
    oracle.frame(&u, &mut fresh, fw as usize, &pool, 0);
    oracle.spec(&u, &mut spec, fw as usize);
    assert_eq!(diff(&fresh, &spec), 0);
    assert_eq!(diff(&kept, &fresh), 0, "the first prefix's unmagnified change must reach the next prefix");
    assert_eq!(fs.raster.prefix, 2);
    assert_eq!(fs.raster.blk, fs.raster.blk_same, "a single one-byte impulse should round away in the pyramid");
    eprintln!("prefix checkpoint: two prefix tiles changed, every recomputed pyramid block was unchanged");
}
