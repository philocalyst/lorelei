//! The effects and media benchmarks of the style-motion-media lane (card 2026-11-21).
//!
//! `fxbench effects [cards]` | `fxbench pane [cards]` (the effects scene with one glass panel and no
//! endless shader: what one moving thing costs) | `fxbench media [videos]`; `REPS` frames per cell (default 24). One command
//! runs each cell on the CPU tile kernel (one core) and on Metal (the staged frame), on separate scenes that
//! receive the same script, and once more on the CPU with `fx_nocache` (every layer redrawn every frame, no
//! restart from a stored prefix: the compositor that redraws its layers, as the control for what the cache
//! earns). Instructions are the frame thread's (`bough_mem::pmu`); the frame is `update` + `prepare` +
//! `shade` (CPU) or `update` + `staged` (Metal). A decoder's work (the synthetic source writing a frame) is
//! outside the timed region; publishing it (`Scene::set`) is inside. After every cell the retained surface
//! is compared with a fresh scene's full redraw, and the Metal surface with the CPU's: mismatches are
//! counted and fail the run.
//!
//! The scenes are tall worlds seen through a 1280 x 800 frame (word 1 scrolls): more cards or videos is more
//! world, as in an application, not more layers on one spot. `effects <n> dense` puts every card in the
//! frame instead (depth grows with n), reported apart.
use bough_audio::sync::Pacer;
use bough_audio::{Block, NullSink, Spec, Tone};
use bough_kernel::cpu::Pool;
use bough_kernel::scenes::Rng;
use bough_mem::pmu;
use bough_paint::expr::*;
use bough_paint::media::Synth;
use bough_paint::metal::Gpu;
use bough_paint::motion::{Response, Spring, SpringX};
use bough_paint::paint::*;
use bough_paint::scene::{ColId, FrameStats, Scene};
use std::time::Instant;

const FW: u32 = 1280;
const FH: u32 = 800;
const KS: Response = Response::SNAPPY;
type Sp = SpringX<852, 21_889>;
type Pan = Off<Lane, I<Uw<0>>, I<Uw<1>>>;
type Any = Pm<Rec>;
/// Hover (word 2 against the record's id) over a theme mix (word 3) of two opaque colours.
type Themed = SelV<Eq<Uw<2>, Rec>, Pm<K<0xff6b3bff>>, MixV<Pmo<Rec>, Pmo<Rec>, Uw<3>>>;
type Sprung = Off<Pan, Sp, Sp>;
type ACard = Over<Over<Shadow<Off<Pan, Ki<0>, Ki<1536>>, Ki<2560>, Ki<6144>, Any, Tab>, Shadow<Off<Pan, Ki<0>, Ki<512>>, Ki<2560>, Ki<1536>, Any, Tab>>, RRect<Pan, Ki<2560>, Themed>>;
type AGrad = Over<Shadow<Off<Pan, Ki<0>, Ki<1024>>, Ki<2560>, Ki<4096>, Any, Tab>, Grad<Pan, Ki<2560>, I<Rec>, I<Rec>, MixV<Pmo<Rec>, Pmo<Rec>, Uw<3>>, Pmo<Rec>>>;
type AGlass = Over<Backdrop<Sprung, Ki<3072>, I<Rec>, Filt<K<256>, K<360>, K<256>>>, Border<Sprung, Ki<3072>, Ki<256>, Any, Any>>;
type AShader = Shader<Pan, Ki<2048>, Rec, Uw<63>, Any, Any>;
type ADot = RRect<Sprung, Ki<1536>, Any>;
type AVideo = Video<Pan, Ki<2048>, Rec, Rec, Rec>;
type ABack = Solid<Lane, MixV<Pmo<Rec>, Pmo<Rec>, Uw<3>>>;

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
fn glass(blur: i32, x: &Spring, y: &Spring) -> AGlass {
    Over(Backdrop { b: sprung(x, y), r: Ki, blur: ri(blur), f: V3 { a: K, b: K, c: K } }, Border { b: sprung(x, y), r: Ki, w: Ki, c: pm(0x80ff_ffff), fill: pm(0x28ff_ffff) })
}
fn dot(c: u32, x: &Spring, y: &Spring) -> ADot {
    RRect { b: sprung(x, y), r: Ki, c: pm(c) }
}

/// A scene and what its script moves.
struct World {
    s: Scene,
    glass: ColId<AGlass>,
    dots: ColId<ADot>,
    vids: ColId<AVideo>,
    g_rect: Vec<[i32; 4]>,
    g_blur: Vec<i32>,
    g_sx: Vec<Spring>,
    d_rect: Vec<[i32; 4]>,
    d_col: Vec<u32>,
    d_sx: Vec<Spring>,
    v_rect: Vec<[i32; 4]>,
    v_dim: Vec<(u32, u32)>,
    src: Vec<Synth>,
    /// Per video: its audio (a null sink playing a tone, pumped at the tile's own device rate), the pacer
    /// that picks its frame from that clock, and when its next audio block is due (host ns) and how long
    /// a block lasts on its device.
    av: Vec<(NullSink<Box<dyn FnMut(&mut Block) + Send>>, Pacer, u64, u64)>,
    /// Frames published by the pacers, and refreshes at which no tile had a new frame.
    av_published: u64,
    av_quiet: u64,
    /// Ids of a card above every glass panel and of one beneath a glass panel (hover targets).
    above: u32,
    beneath: u32,
}

/// Cards with two shadows or a shadow and a gradient, a glass panel over every twelfth cell, shader
/// paints, sprung dots. 12 columns; `dense`: every row folded into the frame.
fn effects(n: usize, dense: bool, lean: bool) -> World {
    let mut g = Rng(0x5eed_f00d);
    let mut s = Scene::new(FW, FH, (n * 64).max(1 << 16));
    let back = s.column::<ABack>(1);
    let cards = s.column::<ACard>(n + n / 8 + 8);
    let grads = s.column::<AGrad>(n);
    let glassc = s.column::<AGlass>(n / 8 + 4);
    let shaders = s.column::<AShader>(n / 16 + 4);
    let dots = s.column::<ADot>(n / 4 + 4);
    let vids = s.column::<AVideo>(1);
    s.push(back, [0, 0, q(FW as f32), q(FH as f32)], Solid { b: Lane, c: MixV { a: Pmo { c: Rec(0xff1a_1410) }, b: Pmo { c: Rec(0xffe8_ecf0) }, t: Uw } });
    let mut w = World { s, glass: glassc, dots, vids, g_rect: vec![], g_blur: vec![], g_sx: vec![], d_rect: vec![], d_col: vec![], d_sx: vec![], v_rect: vec![], v_dim: vec![], src: vec![], av: vec![], av_published: 0, av_quiet: 0, above: 0, beneath: 0 };
    let rows_in_frame = 8;
    for i in 0..n {
        let (cx, row) = (i % 12, i / 12);
        let row = if dense { row % rows_in_frame } else { row };
        let (x, y) = (14.0 + cx as f32 * 105.0 + (i % 5) as f32 * 0.25, 14.0 + row as f32 * 98.0);
        let rect = [q(x), q(y), q(x + 96.0), q(y + 84.0)];
        let id = i as u32 + 1;
        if i % 3 == 0 {
            w.s.push(grads, rect, Over(Shadow { b: Off { b: pan(), dx: Ki, dy: Ki }, r: Ki, blur: Ki, c: pm(0x5000_0000), tab: Tab::NONE }, Grad { b: pan(), r: Ki, dx: ri(400_000), dy: ri(250_000), c: MixV { a: Pmo { c: Rec(g.color(true)) }, b: Pmo { c: Rec(g.color(true)) }, t: Uw }, c2: Pmo { c: Rec(g.color(true)) } }));
        } else {
            w.s.push(
                cards,
                rect,
                Over(Over(Shadow { b: Off { b: pan(), dx: Ki, dy: Ki }, r: Ki, blur: Ki, c: pm(0x3800_0000), tab: Tab::NONE }, Shadow { b: Off { b: pan(), dx: Ki, dy: Ki }, r: Ki, blur: Ki, c: pm(0x4000_0000), tab: Tab::NONE }), RRect { b: pan(), r: Ki, c: SelV { c: Eq { a: Uw, b: Rec(id) }, a: Pm { c: K }, b: MixV { a: Pmo { c: Rec(g.color(true)) }, b: Pmo { c: Rec(g.color(true)) }, t: Uw } } }),
            );
        }
        if i % 12 == 5 && (!lean || i == 5) {
            // a glass panel over the two cards to its left and this one (they are beneath it in paint order)
            let r = [q(x - 150.0), q(y + 20.0), q(x + 60.0), q(y + 70.0)];
            let (sx, sy) = (Spring::rest(0), Spring::rest(0));
            let blur = [q(6.0), q(12.0), q(3.0)][w.g_rect.len() % 3];
            w.s.push(glassc, r, glass(blur, &sx, &sy));
            (w.g_rect.push(r), w.g_blur.push(blur), w.g_sx.push(sx));
            // and a chip on top of the panel: something above the glass
            let chip = 1_000_000 + w.g_rect.len() as u32;
            w.s.push(
                cards,
                [q(x - 110.0), q(y + 30.0), q(x - 50.0), q(y + 60.0)],
                Over(
                    Over(Shadow { b: Off { b: pan(), dx: Ki, dy: Ki }, r: Ki, blur: Ki, c: pm(0x3800_0000), tab: Tab::NONE }, Shadow { b: Off { b: pan(), dx: Ki, dy: Ki }, r: Ki, blur: Ki, c: pm(0x4000_0000), tab: Tab::NONE }),
                    RRect { b: pan(), r: Ki, c: SelV { c: Eq { a: Uw, b: Rec(chip) }, a: Pm { c: K }, b: MixV { a: Pmo { c: Rec(g.color(true)) }, b: Pmo { c: Rec(g.color(true)) }, t: Uw } } },
                ),
            );
        }
        if i % 16 == 9 && !lean {
            w.s.push(shaders, [q(x + 8.0), q(y + 44.0), q(x + 88.0), q(y + 76.0)], Shader { b: pan(), r: Ki, id: Rec((i / 16 % 2) as u32), now: Uw, c: pm(g.color(true)), c2: pm(g.color(false)) });
        }
        if i % 4 == 2 {
            let r = [q(x + 70.0), q(y + 6.0), q(x + 86.0), q(y + 22.0)];
            let c = g.color(true);
            w.s.push(dots, r, dot(c, &Spring::rest(0), &Spring::rest(0)));
            (w.d_rect.push(r), w.d_col.push(c), w.d_sx.push(Spring::rest(0)));
        }
    }
    // the foreground subject: the last record, a chip on the first glass panel (nothing is above it)
    w.s.push(
        cards,
        [q(430.0), q(40.0), q(490.0), q(70.0)],
        Over(Over(Shadow { b: Off { b: pan(), dx: Ki, dy: Ki }, r: Ki, blur: Ki, c: pm(0x3800_0000), tab: Tab::NONE }, Shadow { b: Off { b: pan(), dx: Ki, dy: Ki }, r: Ki, blur: Ki, c: pm(0x4000_0000), tab: Tab::NONE }), RRect { b: pan(), r: Ki, c: SelV { c: Eq { a: Uw, b: Rec(2_000_000) }, a: Pm { c: K }, b: MixV { a: Pmo { c: Rec(0xff40_80c0) }, b: Pmo { c: Rec(0xffc0_8040) }, t: Uw } } }),
    );
    // hover targets: that chip, and the plain card in cell 4 beneath the first panel
    (w.above, w.beneath) = (2_000_000, 5);
    w
}

/// A grid of video tiles (six to a row) over a card backdrop, a glass panel over the second and third
/// tiles of every other row, and sprung dots.
fn media(nv: usize) -> World {
    let mut g = Rng(0x0bad_cafe);
    let mut s = Scene::new(FW, FH, 1 << 18);
    s.media = bough_paint::media::Pool::new(nv * 3 * bough_paint::media::frame_words(160, 90) + 4096);
    let back = s.column::<ABack>(1);
    let cards = s.column::<ACard>(nv * 2 + 8);
    let vids = s.column::<AVideo>(nv + 2);
    let glassc = s.column::<AGlass>(nv / 6 + 4);
    let dots = s.column::<ADot>(nv + 4);
    s.push(back, [0, 0, q(FW as f32), q(FH as f32)], Solid { b: Lane, c: MixV { a: Pmo { c: Rec(0xff10_1418) }, b: Pmo { c: Rec(0xffe8_ecf0) }, t: Uw } });
    let mut w = World { s, glass: glassc, dots, vids, g_rect: vec![], g_blur: vec![], g_sx: vec![], d_rect: vec![], d_col: vec![], d_sx: vec![], v_rect: vec![], v_dim: vec![], src: vec![], av: vec![], av_published: 0, av_quiet: 0, above: 0, beneath: 0 };
    for i in 0..nv {
        let (cx, row) = (i % 6, i / 6);
        let (x, y) = (12.0 + cx as f32 * 212.0, 12.0 + row as f32 * 132.0);
        w.s.push(
            cards,
            [q(x - 4.0), q(y - 4.0), q(x + 204.0), q(y + 124.0)],
            Over(
                Over(Shadow { b: Off { b: pan(), dx: Ki, dy: Ki }, r: Ki, blur: Ki, c: pm(0x3800_0000), tab: Tab::NONE }, Shadow { b: Off { b: pan(), dx: Ki, dy: Ki }, r: Ki, blur: Ki, c: pm(0x4000_0000), tab: Tab::NONE }),
                RRect { b: pan(), r: Ki, c: SelV { c: Eq { a: Uw, b: Rec(i as u32 + 1) }, a: Pm { c: K }, b: MixV { a: Pmo { c: Rec(g.color(true)) }, b: Pmo { c: Rec(g.color(true)) }, t: Uw } } },
            ),
        );
        // every third source is a small "animated image" of another size and rate
        let (vw, vh) = if i % 3 == 2 { (64, 64) } else { (160, 90) };
        let rect = [q(x), q(y), q(x + 200.0), q(y + 112.5)];
        let mut src = Synth::new(vw, vh, i as u32);
        let slot = src.next(&mut w.s.media).expect("media pool");
        w.s.push(vids, rect, Video { b: pan(), r: Ki, slot: Rec(slot), dims: Rec(vw | vh << 16), op: Rec(if i % 5 == 4 { 26_000 } else { 32_768 }) });
        (w.v_rect.push(rect), w.v_dim.push((vw, vh)), w.src.push(src));
        // its sound: 48 kHz, 10 ms blocks, on a device up to 0.5 % fast or slow (each tile its own)
        let mut tone = Tone::new(220 + 20 * i as u32, 48_000, 6000);
        let render: Box<dyn FnMut(&mut Block) + Send> = Box::new(move |b: &mut Block| tone.fill(b));
        let skew = (i as i64 % 11 - 5) * 10_000;
        w.av.push((NullSink::new(Spec::STEREO_48K, 480, render), Pacer::new(30, 1), 0, (10_000_000 + skew) as u64));
        if i % 12 == 2 {
            let r = [q(x - 120.0), q(y + 30.0), q(x + 110.0), q(y + 96.0)];
            let (sx, sy) = (Spring::rest(0), Spring::rest(0));
            w.s.push(glassc, r, glass(q(6.0), &sx, &sy));
            (w.g_rect.push(r), w.g_blur.push(q(6.0)), w.g_sx.push(sx));
        }
        let r = [q(x + 176.0), q(y + 8.0), q(x + 192.0), q(y + 24.0)];
        let c = g.color(true);
        w.s.push(dots, r, dot(c, &Spring::rest(0), &Spring::rest(0)));
        (w.d_rect.push(r), w.d_col.push(c), w.d_sx.push(Spring::rest(0)));
    }
    // hover target: the card behind the second video
    (w.above, w.beneath) = (2, 2);
    w
}

/// What one frame of a cell does to the block and the world before it is drawn.
#[derive(Clone, Copy, PartialEq)]
enum Cell {
    Idle,
    /// One dot's spring retargeted every 40 frames; the clock advances 8 ms a frame.
    OneSpring,
    /// Every dot's spring retargeted (staggered) every 40 frames.
    AllSprings,
    Scroll,
    HoverAbove,
    HoverBeneath,
    Theme,
    /// One glass panel's rect moved one pixel a frame over a still scene.
    GlassMoves,
    /// One video (not under glass) publishes a frame every frame.
    OneVideo,
    /// A video under a glass panel publishes a frame every frame.
    VideoUnderGlass,
    /// Every video publishes on every fourth frame (30 fps content at 120 Hz).
    AllVideos,
    /// Every video shows the frame its own audio clock is playing at each 120 Hz refresh (30 fps content).
    AvPlayback,
    /// The first glass panel moves a pixel a frame while the videos beneath it play (every fourth frame).
    GlassOverVideo,
}
impl Cell {
    fn name(self) -> &'static str {
        match self {
            Cell::Idle => "idle",
            Cell::OneSpring => "one spring",
            Cell::AllSprings => "all springs",
            Cell::Scroll => "scroll 11 px",
            Cell::HoverAbove => "hover above glass",
            Cell::HoverBeneath => "hover beneath glass",
            Cell::Theme => "theme",
            Cell::GlassMoves => "glass moves 1 px",
            Cell::OneVideo => "one video frame",
            Cell::VideoUnderGlass => "video under glass",
            Cell::AllVideos => "all videos (every 4th)",
            Cell::AvPlayback => "a/v playback (audio clocks)",
            Cell::GlassOverVideo => "glass over playing video",
        }
    }
}

/// Frame `f` of `cell`: the untimed part (a decoder's work). Returns the slots to publish.
fn decode(cell: Cell, f: usize, w: &mut World) -> Vec<(usize, u32)> {
    let which: Vec<usize> = match cell {
        Cell::OneVideo => vec![3],
        Cell::VideoUnderGlass => vec![1],
        Cell::AllVideos | Cell::GlassOverVideo if f % 4 == 0 => (0..w.src.len()).collect(),
        _ => vec![],
    };
    let mut out = Vec::new();
    if cell == Cell::AvPlayback {
        // the instant this refresh is shown; each tile's device has delivered its blocks up to it
        let present = f as u64 * 1_000_000_000 / 120;
        for i in 0..w.av.len() {
            let (sink, pacer, due, block_ns) = &mut w.av[i];
            while *due <= present {
                sink.pump(480, *due);
                *due += *block_ns;
            }
            if let Some(frame) = pacer.at(&sink.clock(), present) {
                w.src[i].seq = frame as u32;
                if let Some(slot) = w.src[i].next(&mut w.s.media) {
                    out.push((i, slot));
                    w.av_published += 1;
                }
            }
        }
        w.av_quiet += out.is_empty() as u64;
        return out;
    }
    for i in which {
        if i < w.src.len() {
            if let Some(slot) = w.src[i].next(&mut w.s.media) {
                out.push((i, slot));
            }
        }
    }
    out
}
/// Frame `f` of `cell`: the turn (timed).
fn turn(cell: Cell, f: usize, u: &mut U, w: &mut World, slots: &[(usize, u32)]) {
    let now = u.w[U_TIME];
    for &(i, slot) in slots {
        let (vw, vh) = w.v_dim[i];
        w.s.set(w.vids, i, w.v_rect[i], Video { b: pan(), r: Ki, slot: Rec(slot), dims: Rec(vw | vh << 16), op: Rec(if i % 5 == 4 { 26_000 } else { 32_768 }) });
    }
    match cell {
        Cell::Idle => {}
        Cell::OneSpring | Cell::AllSprings => {
            u.w[U_TIME] = now + 8;
            if f % 40 == 1 {
                let n = if cell == Cell::OneSpring { 1 } else { w.d_sx.len() };
                for i in 0..n.min(w.d_sx.len()) {
                    let to = if (f / 40 + i) % 2 == 0 { q(-40.0) } else { q(0.0) };
                    w.d_sx[i].retarget(KS, now + (i as u32 % 16) * 6, to);
                    w.s.set(w.dots, i, w.d_rect[i], dot(w.d_col[i], &w.d_sx[i], &Spring::rest(0)));
                }
            }
        }
        Cell::Scroll => u.w[1] = u.w[1].wrapping_sub(11 * 256),
        Cell::HoverAbove => u.w[2] = if f % 2 == 0 { w.above } else { 0 },
        Cell::HoverBeneath => u.w[2] = if f % 2 == 0 { w.beneath } else { 0 },
        Cell::Theme => u.w[3] = if f % 2 == 0 { 32_768 } else { 0 },
        Cell::GlassMoves | Cell::GlassOverVideo => {
            if !w.g_rect.is_empty() {
                let d = if (f / 60) % 2 == 0 { 256 } else { -256 };
                (w.g_rect[0][0], w.g_rect[0][2]) = (w.g_rect[0][0] + d, w.g_rect[0][2] + d);
                w.s.set(w.glass, 0, w.g_rect[0], glass(w.g_blur[0], &w.g_sx[0], &Spring::rest(0)));
            }
        }
        Cell::OneVideo | Cell::VideoUnderGlass | Cell::AllVideos | Cell::AvPlayback => {}
    }
}

#[derive(Default)]
struct Row {
    instr: Vec<u64>,
    wall: Vec<f64>,
    gpu: Vec<f64>,
    tiles: u64,
    recs: u64,
    prefix: u64,
    blocks: u64,
    layer_px: u64,
    active: usize,
    frames: u64,
    /// Pixels the scene's layers hold (every level).
    layers_px: u64,
    /// Instructions of `Scene::update` alone (the turn's raster side).
    turn: Vec<u64>,
    /// Under the chooser: frames idle / on the CPU / on the GPU, and the two device constants it measured.
    ran: [u32; 3],
    consts: (f64, f64),
}
fn med<T: Copy + PartialOrd>(v: &[T]) -> Option<T> {
    let mut v = v.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v.get(v.len() / 2).copied()
}
fn load() -> String {
    let o = std::process::Command::new("uptime").output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    o.split("load averages:").nth(1).unwrap_or("?").trim().to_string()
}

#[derive(Clone, Copy, PartialEq)]
enum Exec {
    Cpu,
    CpuNoCache,
    Metal,
    /// The chooser (`mixed::Mixed`): per change, CPU kernel or Metal.
    Mixed,
}
/// Run `cell` for `reps` frames on a fresh world. Returns the row and the mismatches found.
fn run(make: &dyn Fn() -> World, cell: Cell, exec: Exec, reps: usize, pool: &Pool) -> (Row, usize) {
    let mut w = make();
    w.s.fx_nocache = exec == Exec::CpuNoCache;
    // the theme's mix word (word 3) is broad: no geometry reads it
    w.s.broad = 1 << 3;
    let words = (FW * FH) as usize;
    // SAFETY: `w` (the scene) is declared before the executor and outlives it; the arena and the media pool
    // are written only between the executor's calls, each of which waits for the GPU.
    // SAFETY: as the executor below.
    let mut mixed = if exec == Exec::Mixed { Some(unsafe { bough_paint::mixed::Mixed::new(&w.s, FW as usize) }.expect("no Metal device")) } else { None };
    let mut gpu = if exec == Exec::Metal { Some(unsafe { Gpu::new(&w.s, words, 256) }.expect("no Metal device")) } else { None };
    let mut out = vec![0u32; words];
    let mut u = U::default();
    let mut tiles = Vec::new();
    let mut row = Row::default();
    // mount, then warm frames of the script (settle caches, first-touch pages), then the measured ones
    let warm = 8;
    for f in 0..warm + reps {
        let slots = if f == 0 { vec![] } else { decode(cell, f, &mut w) };
        let (i0, _) = pmu::instructions_cycles();
        let t0 = Instant::now();
        if f > 0 {
            turn(cell, f, &mut u, &mut w, &slots);
        }
        let mut gs = 0.0;
        let fs: FrameStats = if let Some(mx) = mixed.as_mut() {
            let (fs, ran) = mx.frame(&mut w.s, &u, pool, None);
            gs = mx.last_gpu;
            if f >= warm {
                row.ran[ran as usize] += 1;
            }
            tiles.resize(fs.tiles as usize, 0);
            if f == 0 {
                mx.calibrate(&w.s, &u, pool);
                row.consts = (mx.cpu_ns_per_weight, mx.gpu_floor_ns);
            }
            fs
        } else {
            let (j0, _) = pmu::instructions_cycles();
            let fs = w.s.update(&u);
            let (j1, _) = pmu::instructions_cycles();
            if f >= warm {
                row.turn.push(j1 - j0);
            }
            w.s.damage.tiles(&mut tiles);
            w.s.damage.clear();
            match gpu.as_mut() {
                None => {
                    w.s.prepare(&u);
                    w.s.shade(&u, &tiles, &mut out, FW as usize, pool, 0);
                }
                Some(g) => gs = g.staged(&w.s, &u, &tiles, FW as usize).gpu,
            }
            fs
        };
        if cell == Cell::AvPlayback && f > 0 && slots.is_empty() && exec != Exec::CpuNoCache {
            // T1: a refresh at which no tile has a new frame is an idle frame (the control redraws its
            // layers regardless: that is what it is)
            assert_eq!((tiles.len(), fs.recs_visited), (0, 0), "a refresh with no new video frame did work");
        }
        let wall = t0.elapsed().as_secs_f64();
        let (i1, _) = pmu::instructions_cycles();
        w.s.media.reclaim();
        if f >= warm {
            row.instr.push(i1 - i0);
            row.wall.push(wall);
            row.gpu.push(gs);
            row.tiles += tiles.len() as u64;
            row.recs += fs.recs_visited as u64;
            row.prefix += fs.prefix_tiles as u64;
            row.blocks += fs.blocks as u64;
            row.layer_px += fs.layer_px;
            row.frames += 1;
        }
    }
    row.active = w.s.active_records();
    if cell == Cell::AvPlayback && exec == Exec::Cpu {
        // T2: every frame an audio clock crossed was published once (the pacers' last frames say how many)
        let crossed: u64 = w.av.iter().map(|a| a.1.shown().map_or(0, |f| f + 1)).sum();
        let audio: Vec<String> = w.av.iter().take(6).map(|a| format!("{}", a.0.position())).collect();
        println!("# a/v: {} tiles over {} refreshes: {} frames crossed by the audio clocks, {} published, {} refreshes with none; audio frames delivered by the first tiles: {}", w.av.len(), warm + reps - 1, crossed, w.av_published, w.av_quiet, audio.join(" "));
        assert_eq!(crossed, w.av_published);
    }
    row.layers_px = w.s.layers.list.iter().map(|l| l.lv.iter().map(|v| ((v.r[2] - v.r[0]) * (v.r[3] - v.r[1])) as u64).sum::<u64>()).sum();
    // validity: the retained surface against a full redraw of the same scene state (a second surface, every
    // tile, through the same scene: `invalidate` forgets what the surface shows)
    let surface: Vec<u32> = match (gpu.as_ref(), mixed.as_ref()) {
        (Some(g), _) => g.out(words).to_vec(),
        (_, Some(m)) => m.out().to_vec(),
        _ => out,
    };
    drop(mixed);
    let mut full = vec![0u32; words];
    w.s.fx_nocache = false;
    w.s.invalidate();
    w.s.frame(&u, &mut full, FW as usize, pool, 0);
    let bad = surface.iter().zip(&full).filter(|(a, b)| a != b).count();
    (row, bad)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let scene = args.get(1).map(String::as_str).unwrap_or("effects");
    let n: usize = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(if scene == "media" { 24 } else { 300 });
    let dense = args.get(3).is_some_and(|v| v == "dense");
    let reps: usize = std::env::var("REPS").ok().and_then(|v| v.parse().ok()).unwrap_or(24);
    let pool = Pool::new(1);
    let lean = scene == "pane";
    let make: Box<dyn Fn() -> World> = if scene == "media" { Box::new(move || media(n)) } else { Box::new(move || effects(n, dense, lean)) };
    let cells: &[Cell] = match scene {
        "media" => &[Cell::Idle, Cell::OneVideo, Cell::VideoUnderGlass, Cell::AllVideos, Cell::AvPlayback, Cell::Scroll, Cell::HoverAbove, Cell::Theme, Cell::AllSprings, Cell::GlassOverVideo],
        "pane" => &[Cell::Idle, Cell::OneSpring, Cell::AllSprings, Cell::HoverAbove, Cell::HoverBeneath, Cell::GlassMoves],
        _ => &[Cell::Idle, Cell::OneSpring, Cell::AllSprings, Cell::Scroll, Cell::HoverAbove, Cell::HoverBeneath, Cell::Theme, Cell::GlassMoves],
    };
    let w = make();
    println!("# fxbench {scene} {n}{}; {FW}x{FH}; reps {reps}; load at start: {}", if dense { " dense" } else { "" }, load());
    println!("# scene: {} records in {} columns, {} layers, {} glass / {} dots / {} videos; media pool {} slots", w.s.records(), w.s.cols().len(), w.s.layers.list.len(), w.g_rect.len(), w.d_rect.len(), w.v_rect.len(), w.s.media.stats().0);
    drop(w);
    println!("scene\tn\tcell\texecutor\ttiles/frame\trecs/frame\tprefix/frame\tblocks/frame\tlayer_px/frame\tlayers_px\tactive\tinstr_med_K\tinstr_min_K\tinstr_mean_K\twall_med_us\tgpu_med_us\tmismatch\tturn_med\tran_idle/cpu/gpu\tcpu_ns_per_weight\tgpu_floor_ns");
    if scene == "media" {
        // T3: what the pacer costs per tile per refresh
        let mut sink = NullSink::new(Spec::STEREO_48K, 480, |_: &mut Block| {});
        (0..100u64).for_each(|k| sink.pump(480, k * 10_000_000));
        let (clock, mut pacer, mut n) = (sink.clock(), Pacer::new(30, 1), 0u64);
        let (i0, _) = pmu::instructions_cycles();
        for k in 0..100_000u64 {
            n += pacer.at(&clock, std::hint::black_box(k * 8_333)).is_some() as u64;
        }
        let (i1, _) = pmu::instructions_cycles();
        println!("# pacer: {:.1} instructions per call ({n} frame changes in 100000 calls)", (i1 - i0) as f64 / 1e5);
    }
    let mut bad_total = 0;
    for &cell in cells {
        for (exec, name) in [(Exec::Cpu, "cpu1"), (Exec::CpuNoCache, "cpu1-nocache"), (Exec::Metal, "metal"), (Exec::Mixed, "mixed")] {
            // `NOCONTROL=1`: skip the redraw-every-layer control (seconds per frame in deep scenes)
            if exec == Exec::CpuNoCache && std::env::var("NOCONTROL").is_ok() {
                continue;
            }
            let (r, bad) = run(&*make, cell, exec, reps, &pool);
            bad_total += bad;
            let f = r.frames.max(1) as f64;
            println!(
                "{scene}\t{n}{}\t{}\t{name}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{:.0}\t{}\t{}\t{:.1}\t{:.1}\t{:.1}\t{:.0}\t{:.0}\t{bad}\t{}\t{}/{}/{}\t{:.1}\t{:.0}",
                if dense { "d" } else { "" },
                cell.name(),
                r.tiles as f64 / f,
                r.recs as f64 / f,
                r.prefix as f64 / f,
                r.blocks as f64 / f,
                r.layer_px as f64 / f,
                r.layers_px,
                r.active,
                med(&r.instr).unwrap_or(0) as f64 / 1e3,
                r.instr.iter().copied().min().unwrap_or(0) as f64 / 1e3,
                r.instr.iter().sum::<u64>() as f64 / r.instr.len().max(1) as f64 / 1e3,
                med(&r.wall).unwrap_or(0.0) * 1e6,
                med(&r.gpu).unwrap_or(0.0) * 1e6,
                med(&r.turn).map_or("-".to_string(), |t| t.to_string()),
                r.ran[0],
                r.ran[1],
                r.ran[2],
                r.consts.0,
                r.consts.1,
            );
        }
    }
    println!("# mismatching words in total: {bad_total}; load at end: {}", load());
    if bad_total != 0 {
        std::process::exit(1);
    }
}
