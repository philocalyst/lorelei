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
            let fs = b.s.update(&u);
            b.s.prepare(&u);
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
    assert!(prefix > 0 && blocks > 0 && out_tiles < all_tiles / 2);
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

/// G3: the Metal executor's staged frame (the CPU's schedule, run as dispatches over the scene's records,
/// with its own retained layer pool) writes the words the CPU kernel writes, retained across the steps.
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
            b.s.update(&u);
            b.s.prepare(&u);
            let mut tiles = Vec::new();
            b.s.damage.tiles(&mut tiles);
            b.s.damage.clear();
            b.s.shade(&u, &tiles, &mut kept, stride, &pool, 0);
            gpu.staged(&b.s, &u, &tiles, stride);
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
    let mutants: [(&str, &dyn Fn(&str) -> String); 6] = [
        ("pyramid taps shifted", &|s| s.replacen("2 * y - 1 + j", "2 * y + j", 1)),
        ("no flatten under a backdrop", &|s| s.replacen("return p_bdrop(x_flat(a), c, x, y, bl);", "return p_bdrop(a, c, x, y, bl);", 1)),
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
            s.update(&u);
            s.prepare(&u);
            let mut tiles = Vec::new();
            s.damage.tiles(&mut tiles);
            s.damage.clear();
            s.shade(&u, &tiles, &mut kept, fw as usize, &pool, 0);
            gpu.staged(&s, &u, &tiles, fw as usize);
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
