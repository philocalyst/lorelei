//! Staged paint benchmark for the published fx seam (`bough-paint`), offscreen, no window.
//!
//! Nine workload cells at 200 / 1000 / 5000 cards on one dense translucent card-stack scene with the
//! full-effects overlay layers (`manifest` says which contract parts the seam admits exactly, which are
//! reduced stand-ins and which are absent).
//!
//! ```text
//! fx_bench manifest                                  coverage of the exact contract, one row per part
//! fx_bench check   CELL CARDS [STEPS] [--mutant=input|damage] [--bare]
//! fx_bench measure CELL CARDS [SAMPLES] [--bringup] [--bare] [--nowarm]
//! fx_bench check|measure CARDS [STEPS]               legacy: the staged glass-over-video cell (`fx_video`)
//! CELL = startup idle one many hover theme scroll resize glass | video (legacy spring pane, brightness/saturation)
//! ```
//!
//! `check` never times anything: retained surfaces at ring ages 1..4 against an uncached full redraw of a
//! second fixture fed the same inputs, the whole-frame specification on the first frame, a fresh mount that
//! replays the inputs and draws once, independent expected-pixel constraints (an f64 compositor of the
//! translucent stack, shadows by their analytic B-spline profile, the gradient parameter, the BT.709 video
//! pixel, an opaque hover literal) and a changing checksum (idle: counters stay zero instead). With
//! `--mutant` an input or the damage is dropped from the retained side only: the check must fail (exit 101).
//! `measure` runs one unmeasured warm trace, then SAMPLES (default 240) steps and prints a row per step
//! plus nearest-rank percentiles (`fx_summary`).
use bough_kernel::cpu::Pool;
use bough_paint::expr::*;
use bough_paint::media::{Pool as Media, Synth};
use bough_paint::motion::{Response, Spring, SpringX};
use bough_paint::paint::*;
use bough_paint::scene::{ColId, Scene};
use bough_sys::perf;
use std::time::Instant;

const W0: u32 = 640;
const H0: u32 = 400;
const W1: u32 = 704;
const H1: u32 = 448;
const VIDEO: [i32; 4] = [256 * 256, 124 * 256, 512 * 256, 268 * 256];
const SEED: u32 = 41;
const HL: u32 = 0xff6b_3bff; // hover highlight, opaque (input byte order: R low)
const BG: u32 = 0xff14_2638;
const GDX: i32 = 700_000;
const GDY: i32 = -300_000;
const RB: i32 = 1536; // 6 px body radius
const TOL: f64 = 6.0;
const SPR2: Response = Response { decay: 754, omega: 30180 }; // Response::new(260, 23), asserted at start
const SNAPPY: Response = Response::SNAPPY;
const PANELS: [(f32, f32); 4] = [(14.0, 18.0), (514.0, 18.0), (328.0, 150.0), (482.0, 302.0)];
const SHADERS: [(f32, f32, u32); 3] = [(48.0, 128.0, 0), (486.0, 128.0, 1), (150.0, 330.0, 0)];
const CLIPR: [f32; 4] = [330.0, 320.0, 470.0, 392.0];
const CLIPK: [f32; 4] = [345.0, 325.0, 455.0, 385.0];

fn q(n: f32) -> i32 {
    (n * 256.0).round() as i32
}
fn ri(n: f32) -> I<Rec> {
    I { a: Rec(q(n) as u32) }
}
fn rgba(c: u32) -> Pm<Rec> {
    Pm { c: Rec(c) }
}
fn rect(x: f32, y: f32, w: f32, h: f32) -> [i32; 4] {
    [q(x), q(y), q(x + w), q(y + h)]
}

// ---- paint types -------------------------------------------------------------------------------
type Pan = Off<Lane, I<Uw<0>>, I<Uw<1>>>;
type Fg = Off<Pan, SpringX<852, 21_889>, Ki<0>>;
type Sh1<G> = Shadow<Grow<Off<G, Ki<0>, Ki<768>>, Ki<512>>, Ki<RB>, Ki<1536>, Pm<K<0x3800_0000>>, Tab>;
type Sh2<G> = Shadow<Grow<Off<G, Ki<512>, Ki<1280>>, Ki<-256>>, Ki<RB>, Ki<768>, Pm<K<0x4000_0000>>, Tab>;
type Card<G, C, D> = Over<Over<Sh1<G>, Sh2<G>>, Grad<G, Ki<RB>, Ki<GDX>, Ki<GDY>, C, D>>;
fn sh1<G: Copy>(g: G) -> Sh1<G> {
    Shadow { b: Grow { b: Off { b: g, dx: Ki, dy: Ki }, d: Ki }, r: Ki, blur: Ki, c: Pm { c: K }, tab: Tab::NONE }
}
fn sh2<G: Copy>(g: G) -> Sh2<G> {
    Shadow { b: Grow { b: Off { b: g, dx: Ki, dy: Ki }, d: Ki }, r: Ki, blur: Ki, c: Pm { c: K }, tab: Tab::NONE }
}
fn card<G: Copy, C, D>(g: G, c: C, d: D) -> Card<G, C, D> {
    Over(Over(sh1(g), sh2(g)), Grad { b: g, r: Ki, dx: Ki, dy: Ki, c, c2: d })
}
macro_rules! palette {
    ($t:ident, $f:ident, $a:literal, $b:literal) => {
        type $t<G> = Card<G, SelV<Eq<Uw<2>, Rec>, Pm<K<HL>>, Pm<Uw<$a>>>, SelV<Eq<Uw<2>, Rec>, Pm<K<HL>>, Pm<Uw<$b>>>>;
        fn $f<G: Copy>(g: G, id: u32) -> $t<G> {
            card(
                g,
                SelV { c: Eq { a: Uw, b: Rec(id) }, a: Pm { c: K }, b: Pm { c: Uw } },
                SelV { c: Eq { a: Uw, b: Rec(id) }, a: Pm { c: K }, b: Pm { c: Uw } },
            )
        }
    };
}
palette!(Pal0, pal0, 4, 8);
palette!(Pal1, pal1, 5, 9);
palette!(Pal2, pal2, 6, 10);
palette!(Pal3, pal3, 7, 11);

type Pg = Prog<Uw<63>, Rec, Rec>;
type RectQ = RectW<Rec, Rec, Rec, Rec>;
type Mg = MixR<RectQ, RectQ, Pg>;
type Sp2 = SpringX<754, 30180>;
type Mgs = Off<Mg, Sp2, Ki<0>>;
type Mc = Fade<MixV<Pm<Rec>, Pm<Rec>, Pg>, I2U<MixI<I<Rec>, I<Rec>, Pg>>>;
type MovP = Card<Mg, Mc, Mc>;
type MovS = Card<Mgs, Mc, Mc>;
type Back = RRect<Lane, I<Rec>, Pm<Rec>>;
type Shd = Shader<Lane, I<Rec>, Rec, Uw<40>, Pm<Rec>, Pm<Rec>>;
type ClipP = Clip<RRect<Lane, I<Rec>, Pm<Rec>>, RectQ>;
type PaintColor = Pm<Rec>;
type VideoPaint = Video<Lane, I<Rec>, Rec, Rec, Rec>;
type PBox<D> = Off<Lane, D, Ki<0>>;
type Glass<D> = Over<
    Backdrop<PBox<D>, I<Rec>, I<Rec>, Filt<Rec, Rec, Rec>>,
    Border<PBox<D>, I<Rec>, Ki<256>, PaintColor, PaintColor>,
>;
type GlassW = Glass<I<Rec>>;
type GlassS = Glass<SpringX<852, 21_889>>;

fn pan() -> Pan {
    Off { b: Lane, dx: I { a: Uw }, dy: I { a: Uw } }
}
fn fg(s: &Spring) -> Fg {
    Off { b: pan(), dx: s.operand(), dy: Ki }
}
fn rectq(r: [i32; 4]) -> RectQ {
    RectW { a: Rec(r[0] as u32), b: Rec(r[1] as u32), c: Rec(r[2] as u32), d: Rec(r[3] as u32) }
}
fn pg(t0: u32, dur: u32) -> Pg {
    Prog { now: Uw, t0: Rec(t0), dur: Rec(dur) }
}
fn mg(m: &Mv) -> Mg {
    MixR { a: rectq(m.a.r), b: rectq(m.b.r), t: pg(m.t0, m.dur) }
}
fn mc(m: &Mv, s: usize) -> Mc {
    let (ca, cb) = if s == 0 { (m.a.c1, m.b.c1) } else { (m.a.c2, m.b.c2) };
    Fade {
        a: MixV { a: rgba(ca), b: rgba(cb), t: pg(m.t0, m.dur) },
        k: I2U { a: MixI { a: I { a: Rec(m.a.op as u32) }, b: I { a: Rec(m.b.op as u32) }, t: pg(m.t0, m.dur) } },
    }
}
fn mov_p(m: &Mv) -> MovP {
    card(mg(m), mc(m, 0), mc(m, 1))
}
fn mov_s(m: &Mv) -> MovS {
    card(Off { b: mg(m), dx: m.spr.operand(), dy: Ki }, mc(m, 0), mc(m, 1))
}
fn panel<D: Copy>(dx: D, f: (u32, u32, u32)) -> Glass<D> {
    let b = Off { b: Lane, dx, dy: Ki };
    Over(
        Backdrop { b, r: ri(12.0), blur: ri(6.0), f: V3 { a: Rec(f.0), b: Rec(f.1), c: Rec(f.2) } },
        Border { b, r: ri(12.0), w: Ki, c: rgba(0xaae5_f6ff), fill: rgba(0x5267_89a9) },
    )
}
fn video(slot: u32) -> VideoPaint {
    Video { b: Lane, r: ri(10.0), slot: Rec(slot), dims: Rec(256 | 144 << 16), op: Rec(32768) }
}

// ---- cells ---------------------------------------------------------------------------------------
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cell {
    Startup,
    Idle,
    One,
    Many,
    Hover,
    Theme,
    Scroll,
    Resize,
    Glass,
    /// The earlier staged cell: spring pane retargeted twice per 40 ticks, brightness 0.8 / saturation 1.4.
    Video,
}
impl Cell {
    fn parse(s: &str) -> Option<Cell> {
        Some(match s {
            "startup" => Cell::Startup,
            "idle" => Cell::Idle,
            "one" => Cell::One,
            "many" => Cell::Many,
            "hover" => Cell::Hover,
            "theme" => Cell::Theme,
            "scroll" => Cell::Scroll,
            "resize" => Cell::Resize,
            "glass" => Cell::Glass,
            "video" => Cell::Video,
            _ => return None,
        })
    }
    fn name(self) -> &'static str {
        match self {
            Cell::Startup => "startup",
            Cell::Idle => "idle",
            Cell::One => "one",
            Cell::Many => "many",
            Cell::Hover => "hover",
            Cell::Theme => "theme",
            Cell::Scroll => "scroll",
            Cell::Resize => "resize",
            Cell::Glass => "glass",
            Cell::Video => "video",
        }
    }
    /// Cells whose frames must differ from the previous one at every step.
    fn every_frame(self) -> bool {
        matches!(self, Cell::Hover | Cell::Theme | Cell::Scroll | Cell::Resize)
    }
    fn video_moves(self) -> bool {
        matches!(self, Cell::Glass | Cell::Video)
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mutant {
    None,
    Input,
    Damage,
}

fn hash(mut x: u32) -> u32 {
    x = x.wrapping_add(0x9e37_79b9);
    x = (x ^ (x >> 16)).wrapping_mul(0x85eb_ca6b);
    x = (x ^ (x >> 13)).wrapping_mul(0xc2b2_ae35);
    x ^ (x >> 16)
}
/// Shared palette word (input byte order, alpha preserved: stop 0 0xd4, stop 1 0xb8).
fn pal_color(generation: u32, p: u32, s: u32) -> u32 {
    let a = if s == 0 { 0xd4 } else { 0xb8 };
    (a << 24) | (hash(generation * 8 + p * 2 + s) & 0x00ff_ffff)
}
fn lerp_color(a: u32, b: u32, t: f64) -> u32 {
    let mut o = 0;
    for sh in [0, 8, 16, 24] {
        let (x, y) = (((a >> sh) & 255) as f64, ((b >> sh) & 255) as f64);
        o |= ((x + (y - x) * t).round() as u32 & 255) << sh;
    }
    o
}

#[derive(Clone, Copy, Debug)]
struct Seg {
    r: [i32; 4],
    op: i32,
    c1: u32,
    c2: u32,
}
#[derive(Clone, Copy, Debug)]
struct Mv {
    a: Seg,
    b: Seg,
    t0: u32,
    dur: u32,
    spr: Spring,
}

struct Cols {
    back: ColId<Back>,
    p0: ColId<Pal0<Pan>>,
    p1: ColId<Pal1<Pan>>,
    p2: ColId<Pal2<Pan>>,
    p3: ColId<Pal3<Pan>>,
    fore: ColId<Pal3<Fg>>,
    movp: ColId<MovP>,
    movs: ColId<MovS>,
    shaders: ColId<Shd>,
    clip: ColId<ClipP>,
    video: ColId<VideoPaint>,
    gw: ColId<GlassW>,
    gs: ColId<GlassS>,
}

/// Everything a replay of the inputs determines, apart from the scene.
struct St {
    cell: Cell,
    cards: usize,
    mutant: Mutant,
    bare: bool,
    w: u32,
    h: u32,
    fore: Spring,
    pane: Spring,
    pane_dx: i32,
    src: Synth,
    vseq: u32,
    mv: Vec<Mv>,
    slot: Vec<(bool, usize)>,
    spr_state: Vec<Spring>,
    /// repeat the 100 ms retarget sequence every 40 ticks (measure: keeps >= 200 active frames)
    repeat: bool,
}

fn base_pos(cell: Cell, i: usize) -> (f32, f32) {
    if cell == Cell::Scroll {
        let (col, row) = (i % 10, i / 10);
        let l = (row % 8) as f32 * 0.25;
        (col as f32 * 64.0 + l, row as f32 * 40.0 + l)
    } else {
        let slot = i % 100;
        let l = ((i / 100) % 8) as f32 * 0.25;
        ((slot % 10) as f32 * 64.0 + l, (slot / 10) as f32 * 40.0 + l)
    }
}
fn base_rect(cell: Cell, i: usize) -> [i32; 4] {
    let (x, y) = base_pos(cell, i);
    rect(x, y, 60.0, 36.0)
}
fn mover_params(cell: Cell, i: usize) -> Mv {
    let p = (i % 4) as u32;
    let r0 = base_rect(cell, i);
    let (dx, dy) = (4.0 + (i % 5) as f32, (i % 3) as f32 - 1.0);
    let a = Seg { r: r0, op: 32768, c1: pal_color(0, p, 0), c2: pal_color(0, p, 1) };
    let b = Seg { r: [r0[0] + q(dx), r0[1] + q(dy), r0[2] + q(dx + 8.0), r0[3] + q(dy)], op: 22_938, c1: pal_color(101, p, 0), c2: pal_color(101, p, 1) };
    Mv { a, b, t0: 8 + ((i * 7) % 64) as u32, dur: 400, spr: Spring::rest(0) }
}
/// The final segment after the interruption at 100 ms (also the independent expected end state).
fn final_seg(cell: Cell, i: usize) -> Seg {
    let p = (i % 4) as u32;
    let r0 = base_rect(cell, i);
    let dx = -(2.0 + (i % 3) as f32);
    Seg { r: [r0[0] + q(dx), r0[1] + q(1.0), r0[2] + q(dx + 4.0), r0[3] + q(1.0)], op: 29_491, c1: pal_color(202, p, 0), c2: pal_color(202, p, 1) }
}
const FORE_END: f32 = -10.0;
const SPR_END: f32 = -3.0;

fn overlays(bare: bool) -> Vec<[f32; 4]> {
    if bare {
        return Vec::new();
    }
    let mut v = Vec::new();
    for (x, y, _) in SHADERS {
        v.push([x, y, x + 96.0, y + 60.0]);
    }
    v.push([256.0, 124.0, 512.0, 268.0]);
    for (x, y) in PANELS {
        v.push([x - 36.0, y - 4.0, x + 148.0, y + 80.0]);
    }
    v.push(CLIPR);
    v
}
fn hits(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}
fn free_cells(bare: bool) -> Vec<usize> {
    let o = overlays(bare);
    (0..100)
        .filter(|c| {
            let (x, y) = ((c % 10) as f32 * 64.0 - 6.0, (c / 10) as f32 * 40.0 - 6.0);
            o.iter().all(|r| !hits([x, y, x + 74.0, y + 52.0], *r))
        })
        .collect()
}
fn hover_cell(fc: &[usize], k: usize) -> Option<usize> {
    if k == 0 {
        return None;
    }
    let m = (k - 1) % 38;
    let idx = if m < 20 { m } else { 38 - m };
    Some(fc[idx])
}
fn top_card(cards: usize, cell: usize) -> usize {
    cell + 100 * ((cards - 1 - cell) / 100)
}
fn scroll_px(cards: usize, k: usize) -> i32 {
    let rows = cards.div_ceil(10);
    let max = (rows * 40).saturating_sub(H0 as usize).max(1) as i32;
    let period = 2 * (max / 11).max(1) as usize;
    let m = k % period;
    let half = period / 2;
    (if m <= half { m } else { period - m }) as i32 * 11
}
fn dim_at(cell: Cell, k: usize) -> (u32, u32) {
    if cell == Cell::Resize && k % 2 == 1 { (W1, H1) } else { (W0, H0) }
}
fn pane_q8(k: usize) -> i32 {
    (28.0 * 256.0 * (std::f64::consts::TAU * k as f64 / 60.0).sin()).round() as i32
}

fn build(st: &mut St) -> (Scene, Cols) {
    let n = st.cards;
    let mut s = Scene::new(st.w, st.h, n * 96 + (1 << 17));
    s.media = Media::new(1 << 18);
    let many = st.cell == Cell::Many;
    let cap = if many { 64 } else { n / 4 + 64 };
    let c = Cols {
        back: s.column::<Back>(1),
        p0: s.column::<Pal0<Pan>>(cap),
        p1: s.column::<Pal1<Pan>>(cap),
        p2: s.column::<Pal2<Pan>>(cap),
        p3: s.column::<Pal3<Pan>>(cap),
        fore: s.column::<Pal3<Fg>>(1),
        movp: s.column::<MovP>(if many { n / 2 + 64 } else { 1 }),
        movs: s.column::<MovS>(if many { n / 2 + 64 } else { 1 }),
        shaders: s.column::<Shd>(3),
        clip: s.column::<ClipP>(1),
        video: s.column::<VideoPaint>(1),
        gw: s.column::<GlassW>(4),
        gs: s.column::<GlassS>(4),
    };
    s.push(c.back, rect(0.0, 0.0, st.w as f32, st.h as f32), RRect { b: Lane, r: ri(0.0), c: rgba(BG) });
    let first_mount = st.mv.is_empty() && many;
    if many {
        st.slot.clear();
        if first_mount {
            st.spr_state = vec![Spring::rest(0); n];
        }
    }
    let (mut np, mut ns) = (0, 0);
    for i in 0..n {
        let r = base_rect(st.cell, i);
        let id = i as u32 + 1;
        if many {
            if first_mount {
                st.mv.push(mover_params(st.cell, i));
            }
            let m = st.mv[i];
            if i % 2 == 0 {
                let m = Mv { spr: st.spr_state[i], ..m };
                s.push(c.movs, r, mov_s(&m));
                st.slot.push((true, ns));
                ns += 1;
            } else {
                s.push(c.movp, r, mov_p(&m));
                st.slot.push((false, np));
                np += 1;
            }
        } else if st.cell == Cell::One && i == n - 1 {
            assert_eq!(i % 4, 3);
            s.push(c.fore, r, pal3(fg(&st.fore), id));
        } else {
            match i % 4 {
                0 => s.push(c.p0, r, pal0(pan(), id)),
                1 => s.push(c.p1, r, pal1(pan(), id)),
                2 => s.push(c.p2, r, pal2(pan(), id)),
                _ => s.push(c.p3, r, pal3(pan(), id)),
            };
        }
    }
    if !st.bare {
        for (x, y, id) in SHADERS {
            let (c1, c2) = if x < 100.0 || id == 1 { (0xaa30_86d8, 0xaac2_78bd) } else { (0xaa66_c0a0, 0xaa20_40c0) };
            s.push(c.shaders, rect(x, y, 96.0, 60.0), Shader { b: Lane, r: ri(10.0), id: Rec(id), now: Uw, c: rgba(c1), c2: rgba(c2) });
        }
        s.push(
            c.clip,
            rect(CLIPR[0], CLIPR[1], CLIPR[2] - CLIPR[0], CLIPR[3] - CLIPR[1]),
            Clip { p: RRect { b: Lane, r: ri(14.0), c: rgba(0x9040_a0ff) }, k: rectq(rect(CLIPK[0], CLIPK[1], CLIPK[2] - CLIPK[0], CLIPK[3] - CLIPK[1])) },
        );
        st.src.shown = None;
        st.src.seq = st.vseq;
        let slot = st.src.next(&mut s.media).expect("video pool");
        s.push(c.video, VIDEO, video(slot));
        for (j, (x, y)) in PANELS.into_iter().enumerate() {
            let rc = rect(x, y, 112.0, 76.0);
            if st.cell == Cell::Video {
                s.push(c.gs, rc, panel(st.pane.operand(), (205, 358, 256)));
            } else {
                let dx = if j == 2 { st.pane_dx } else { 0 };
                s.push(c.gw, rc, panel(I { a: Rec(dx as u32) }, (256, 256, 256)));
            }
        }
    }
    (s, c)
}

struct Fixture {
    st: St,
    scene: Scene,
    cols: Cols,
    fc: Vec<usize>,
}
impl Fixture {
    fn new(cell: Cell, cards: usize, mutant: Mutant, bare: bool) -> Fixture {
        let (w, h) = dim_at(cell, 0);
        let mut st = St {
            cell,
            cards,
            mutant,
            bare,
            w,
            h,
            fore: Spring::rest(0),
            pane: Spring::rest(0),
            pane_dx: 0,
            src: Synth::new(256, 144, SEED),
            vseq: 0,
            mv: Vec::new(),
            slot: Vec::new(),
            spr_state: Vec::new(),
            repeat: false,
        };
        let (scene, cols) = build(&mut st);
        let fc = free_cells(bare);
        assert!(fc.len() >= 20, "fewer than 20 hoverable cells");
        Fixture { st, scene, cols, fc }
    }
    /// Apply the inputs of tick `k` (words, record rewrites, a remount). True when the scene was replaced.
    fn step(&mut self, k: usize, u: &mut U) -> bool {
        let now = (k * 1_000 / 120) as u32;
        u.w[U_TIME] = now;
        let drop_input = self.st.mutant == Mutant::Input && k >= 2;
        let cell = self.st.cell;
        if cell == Cell::Idle {
            // the idle scene has a frozen shader clock; the mutant wakes it every frame
            if drop_input {
                u.w[40] = now;
            }
        } else if matches!(cell, Cell::Many | Cell::Glass | Cell::Video) {
            // shader clock runs only where motion is the workload; elsewhere a continually animated shader
            // would be a second moving subject (one, hover, theme, scroll, resize, startup keep it frozen)
            u.w[40] = now;
        }
        let mut remount = false;
        match cell {
            Cell::One => {
                let ph = if self.st.repeat { k % 40 } else { k };
                if k > 0 && (ph == 1 || ph == 12) {
                    let target = if ph == 1 { 10.0 } else { FORE_END };
                    self.st.fore.retarget(SNAPPY, now, q(target));
                    if !(drop_input && ph == 12) {
                        let n = self.st.cards;
                        let r = base_rect(cell, n - 1);
                        self.scene.set(self.cols.fore, 0, r, pal3(fg(&self.st.fore), n as u32));
                    }
                }
            }
            Cell::Many => {
                let rep = self.st.repeat && k > 12;
                let phase = if k == 12 || (rep && k % 40 == 12) { 2 } else if rep && k % 40 == 1 { 1 } else { 0 };
                if k == 1 {
                    for i in (0..self.st.cards).step_by(2) {
                        self.st.spr_state[i].retarget(SPR2, now, q(6.0));
                        self.set_mover(i);
                    }
                }
                if phase != 0 && !drop_input {
                    for i in 0..self.st.cards {
                        let m = &mut self.st.mv[i];
                        let p = ((now as f64 - m.t0 as f64) / m.dur as f64).clamp(0.0, 1.0);
                        let mut cur = m.a;
                        for l in 0..4 {
                            cur.r[l] = (m.a.r[l] as f64 + (m.b.r[l] - m.a.r[l]) as f64 * p).round() as i32;
                        }
                        cur.op = (m.a.op as f64 + (m.b.op - m.a.op) as f64 * p).round() as i32;
                        cur.c1 = lerp_color(m.a.c1, m.b.c1, p);
                        cur.c2 = lerp_color(m.a.c2, m.b.c2, p);
                        m.a = cur;
                        m.b = if phase == 2 { final_seg(cell, i) } else { mover_params(cell, i).b };
                        m.t0 = now + ((i * 7) % 64) as u32;
                        m.dur = 400;
                        if i % 2 == 0 {
                            self.st.spr_state[i].retarget(SPR2, now, q(if phase == 2 { SPR_END } else { 6.0 }));
                        }
                        self.set_mover(i);
                    }
                }
            }
            Cell::Hover => {
                if !drop_input {
                    u.w[2] = hover_cell(&self.fc, k).map_or(0, |c| top_card(self.st.cards, c) as u32 + 1);
                }
            }
            Cell::Theme => {
                if !drop_input {
                    for p in 0..4 {
                        u.w[4 + p] = pal_color(k as u32, p as u32, 0);
                        u.w[8 + p] = pal_color(k as u32, p as u32, 1);
                    }
                }
            }
            Cell::Scroll => {
                if !drop_input {
                    u.w[1] = (-scroll_px(self.st.cards, k) * 256) as u32;
                }
            }
            Cell::Resize => {
                let (w, h) = dim_at(cell, k);
                if (w, h) != (self.st.w, self.st.h) && !drop_input {
                    self.st.w = w;
                    self.st.h = h;
                    let (s, c) = build(&mut self.st);
                    self.scene = s;
                    self.cols = c;
                    remount = true;
                }
            }
            Cell::Glass => {
                if !drop_input && !self.st.bare {
                    self.st.pane_dx = pane_q8(k);
                    let (x, y) = PANELS[2];
                    self.scene.set(self.cols.gw, 2, rect(x, y, 112.0, 76.0), panel(I { a: Rec(self.st.pane_dx as u32) }, (256, 256, 256)));
                }
            }
            Cell::Video => {
                if matches!(k % 40, 1 | 12) && !self.st.bare {
                    let target = if k % 40 == 1 { 10.0 } else { -10.0 };
                    self.st.pane.retarget(SNAPPY, now, q(target));
                    let (x, y) = PANELS[2];
                    self.scene.set(self.cols.gs, 2, rect(x, y, 112.0, 76.0), panel(self.st.pane.operand(), (205, 358, 256)));
                }
            }
            Cell::Startup | Cell::Idle => {}
        }
        if cell.video_moves() && !self.st.bare && k != 0 && k % 4 == 0 && !drop_input {
            let slot = self.st.src.next(&mut self.scene.media).expect("video pool");
            self.st.vseq = self.st.src.seq - 1;
            self.scene.set(self.cols.video, 0, VIDEO, video(slot));
        }
        remount
    }
    fn set_mover(&mut self, i: usize) {
        let (spring, at) = self.st.slot[i];
        let r = base_rect(self.st.cell, i);
        let m = Mv { spr: self.st.spr_state[i], ..self.st.mv[i] };
        if spring {
            self.scene.set(self.cols.movs, at, r, mov_s(&m));
        } else {
            self.scene.set(self.cols.movp, at, r, mov_p(&m));
        }
    }
}

// ---- independent expectations ------------------------------------------------------------------
/// What a card shows at a probe pixel, from the harness's own state and no renderer value.
#[derive(Clone, Copy)]
struct Truth {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    c1: u32,
    c2: u32,
    op: f64,
    hover: bool,
}
fn rgb(c: u32) -> [f64; 3] {
    [(c & 255) as f64, ((c >> 8) & 255) as f64, ((c >> 16) & 255) as f64]
}
/// CDF of the quadratic B-spline blur kernel in units of its width `w` (two sigma): exact piecewise cubic.
fn cdf3(u: f64) -> f64 {
    let u = u.clamp(-1.5, 1.5);
    ((u + 1.5).powi(3) - 3.0 * (u + 0.5).max(0.0).powi(3) + 3.0 * (u - 0.5).max(0.0).powi(3)) / 6.0
}
fn cov(p: f64, lo: f64, hi: f64, w: f64) -> f64 {
    cdf3((p - lo) / w) - cdf3((p - hi) / w)
}
/// Composite a card stack over the background at pixel (px, py) (sample at its centre). `None` when the pixel
/// is too near a card's rounded corner or edge for the separable shadow profile.
fn expected(stack: &[Truth], px: i32, py: i32) -> Option<[f64; 3]> {
    let (x, y) = (px as f64 + 0.5, py as f64 + 0.5);
    let mut acc = rgb(BG);
    for t in stack {
        if x - t.x0 < 9.0 || t.x1 - x < 9.0 || y - t.y0 < 9.0 || t.y1 - y < 9.0 {
            return None;
        }
        for (g, ox, oy, w, a) in [(2.0, 0.0, 3.0, 6.0, 0x38 as f64 / 255.0), (-1.0, 2.0, 5.0, 3.0, 0x40 as f64 / 255.0)] {
            let c = cov(x, t.x0 - g + ox, t.x1 + g + ox, w) * cov(y, t.y0 - g + oy, t.y1 + g + oy, w);
            for ch in &mut acc {
                *ch *= 1.0 - a * c;
            }
        }
        if t.hover {
            acc = rgb(HL);
            continue;
        }
        let (cx, cy) = ((t.x0 + t.x1) / 2.0, (t.y0 + t.y1) / 2.0);
        let (ox, oy) = (((x - cx) * 16.0).floor(), ((y - cy) * 16.0).floor());
        let s = ((ox * GDX as f64 + oy * GDY as f64) / 32768.0).floor();
        let tt = (s + 16384.0).clamp(0.0, 32768.0) / 32768.0;
        let (a1, a2) = ((t.c1 >> 24) as f64 / 255.0, (t.c2 >> 24) as f64 / 255.0);
        let (p1, p2) = (rgb(t.c1), rgb(t.c2));
        let a = (a1 * (1.0 - tt) + a2 * tt) * t.op;
        for ch in 0..3 {
            let pm = (p1[ch] * a1 * (1.0 - tt) + p2[ch] * a2 * tt) * t.op;
            acc[ch] = pm + acc[ch] * (1.0 - a);
        }
    }
    Some(acc)
}
/// The BT.709 limited-range colour of the synthetic frame `seq` at a column whose chroma bars stay constant
/// around it and outside its bouncing box, row 126 of the tile. Returns (rgb, tile column).
fn video_expected(seq: u32) -> Option<([f64; 3], i32)> {
    let (w, h, seed) = (256usize, 144usize, SEED as usize);
    let (boxx, boxy) = ((seq as usize * 3 + seed * 7) % w, (seq as usize * 2 + seed * 5) % h);
    let j = 126usize;
    let bar = |i: usize| ((i + seq as usize * 2) * 8 / w + seed) % 8;
    for lx in 40..200usize {
        if (lx - 5..=lx + 5).any(|i| bar(i) != bar(lx)) || lx.abs_diff(boxx) < w / 8 + 6 || j.abs_diff(boxy) < h / 8 + 6 {
            continue;
        }
        let b = bar(lx);
        let y = (40 + b * 24 + (j * 16 / h)) as f64;
        let cb = (64 + b * 20) as f64;
        let cr = (200 - b * 18) as f64 + ((j / 2) * 2 * 12 / h) as f64;
        let l = 1.164 * (y - 16.0);
        return Some(([l + 1.793 * (cr - 128.0), l - 0.213 * (cb - 128.0) - 0.533 * (cr - 128.0), l + 2.112 * (cb - 128.0)], lx as i32));
    }
    None
}

impl Fixture {
    fn stack_truth(&self, cell: usize, k: usize, last: bool, hover: Option<usize>) -> Option<Vec<Truth>> {
        let st = &self.st;
        let generation = if st.cell == Cell::Theme { k as u32 } else { 0 };
        let mut v = Vec::new();
        let mut i = cell;
        while i < st.cards {
            let (x, y) = base_pos(st.cell, i);
            let (mut x0, mut y0, mut x1, mut y1) = (x as f64, y as f64, x as f64 + 60.0, y as f64 + 36.0);
            let p = (i % 4) as u32;
            let (mut c1, mut c2, mut op) = (pal_color(generation, p, 0), pal_color(generation, p, 1), 1.0);
            match st.cell {
                Cell::One if i == st.cards - 1 => {
                    if k != 0 && !last {
                        return None;
                    }
                    if k != 0 {
                        x0 += FORE_END as f64;
                        x1 += FORE_END as f64;
                    }
                }
                Cell::Many => {
                    if k != 0 && !last {
                        return None;
                    }
                    if k != 0 {
                        let f = final_seg(st.cell, i);
                        let sp = if i % 2 == 0 { SPR_END as f64 } else { 0.0 };
                        (x0, y0, x1, y1) = (f.r[0] as f64 / 256.0 + sp, f.r[1] as f64 / 256.0, f.r[2] as f64 / 256.0 + sp, f.r[3] as f64 / 256.0);
                        (c1, c2, op) = (f.c1, f.c2, f.op as f64 / 32768.0);
                    }
                }
                _ => {}
            }
            v.push(Truth { x0, y0, x1, y1, c1, c2, op, hover: hover == Some(i) });
            i += 100;
        }
        Some(v)
    }
    /// Probe pixels (x, y, expected rgb) for the frame after tick `k`. `last`: the motion has settled (one and
    /// many are only checked at the first frame and when settled).
    fn probes(&self, k: usize, last: bool) -> Vec<(i32, i32, [f64; 3])> {
        let st = &self.st;
        let mut out = Vec::new();
        if st.cell == Cell::Scroll {
            let sc = scroll_px(st.cards, k) as f64;
            let o = overlays(st.bare);
            let mut got = 0;
            'o: for row in 0..st.cards.div_ceil(10) {
                for col in 0..10 {
                    let i = row * 10 + col;
                    let (x, y) = base_pos(st.cell, i);
                    let (y0, y1) = (y as f64 - sc, y as f64 + 36.0 - sc);
                    if y0 < 4.0 || y1 > H0 as f64 - 4.0 || o.iter().any(|r| hits([x - 8.0, y0 as f32 - 8.0, x + 68.0, y1 as f32 + 8.0], *r)) {
                        continue;
                    }
                    let p = (i % 4) as u32;
                    let t = Truth { x0: x as f64, y0, x1: x as f64 + 60.0, y1, c1: pal_color(0, p, 0), c2: pal_color(0, p, 1), op: 1.0, hover: false };
                    let (px, py) = (x as i32 + 30, (y0 + 18.0) as i32);
                    if let Some(e) = expected(&[t], px, py) {
                        out.push((px, py, e));
                        got += 1;
                        if got == 3 {
                            break 'o;
                        }
                    }
                }
            }
            return out;
        }
        let fc = &self.fc;
        let hov = if st.cell == Cell::Hover { hover_cell(fc, k).map(|c| top_card(st.cards, c)) } else { None };
        let mut cells = vec![fc[0], fc[fc.len() / 2], fc[fc.len() - 1]];
        if st.cell == Cell::Hover {
            cells.extend(hover_cell(fc, k));
            cells.extend(hover_cell(fc, k.saturating_sub(1)));
        }
        if st.cell == Cell::One {
            cells.push(99); // the foreground's cell: its centre pixel lies just outside the last pane
        }
        for c in cells {
            if let Some(stack) = self.stack_truth(c, k, last, hov) {
                let (px, py) = ((c % 10) as i32 * 64 + 30, (c / 10) as i32 * 40 + 18);
                if let Some(e) = expected(&stack, px, py) {
                    out.push((px, py, e));
                }
            }
        }
        if !st.bare {
            let seq = if st.cell.video_moves() { (k / 4) as u32 } else { 0 };
            if let Some((e, lx)) = video_expected(seq) {
                out.push((256 + lx, 250, e));
            }
        }
        out
    }
}

fn check_probes(px: &[u32], stride: usize, probes: &[(i32, i32, [f64; 3])], what: &str) {
    for &(x, y, e) in probes {
        let p = px[y as usize * stride + x as usize];
        let got = [((p >> 16) & 255) as f64, ((p >> 8) & 255) as f64, (p & 255) as f64];
        assert!((0..3).all(|c| (got[c] - e[c]).abs() <= TOL), "{what}: independent constraint at ({x},{y}): got {got:?} expected {e:.1?}");
    }
}
fn checksum(px: &[u32]) -> u64 {
    px.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &v| (h ^ v as u64).wrapping_mul(0x100_0000_01b3))
}

// ---- the frame driver --------------------------------------------------------------------------
struct Ring {
    surfaces: Vec<Vec<u32>>,
    stale: Vec<Vec<bool>>,
}
impl Ring {
    fn new(ages: usize) -> Ring {
        Ring { surfaces: vec![vec![0; (W1 * H1) as usize]; ages], stale: vec![Vec::new(); ages] }
    }
    fn reset(&mut self, tiles: usize) {
        for s in &mut self.stale {
            *s = vec![true; tiles];
        }
        for s in &mut self.surfaces {
            s.fill(0);
        }
    }
}
struct FrameOut {
    ns: u128,
    instr: u64,
    cpu: u64,
    rss: u64,
    foot: u64,
    tiles: usize,
    tiles_total: usize,
    recs: u32,
    prefix: u32,
    blocks: u32,
    active: usize,
    animating: bool,
}

fn drive(f: &mut Fixture, u: &mut U, k: usize, ring: &mut Ring, pool: &Pool, overhead: u64, tiles: &mut Vec<u32>, candidates: &mut Vec<u32>) -> FrameOut {
    let a0 = perf::snap();
    let start = Instant::now();
    let reset = f.step(k, u) || k == 0;
    if reset {
        ring.reset(f.scene.tx * f.scene.ty);
    }
    let fs = f.scene.update(u);
    f.scene.prepare(u);
    f.scene.damage.tiles(tiles);
    f.scene.damage.clear();
    if f.st.mutant == Mutant::Damage && k >= 2 {
        tiles.clear();
        if reset {
            for lag in &mut ring.stale {
                lag.fill(false);
            }
        }
    }
    let stride = f.st.w as usize;
    for (age, lag) in ring.stale.iter_mut().enumerate() {
        for &t in tiles.iter() {
            lag[t as usize] = true;
        }
        if k % (age + 1) != 0 {
            continue;
        }
        candidates.clear();
        candidates.extend(lag.iter().enumerate().filter_map(|(t, &stale)| stale.then_some(t as u32)));
        f.scene.shade(u, candidates, &mut ring.surfaces[age], stride, pool, 0);
        lag.fill(false);
    }
    f.scene.media.reclaim();
    let ns = start.elapsed().as_nanos();
    let a1 = perf::snap();
    FrameOut {
        ns,
        instr: a1.instrs.saturating_sub(a0.instrs).saturating_sub(overhead),
        cpu: a1.cpu_ns - a0.cpu_ns,
        rss: a1.rss,
        foot: a1.footprint,
        tiles: tiles.len(),
        tiles_total: f.scene.tx * f.scene.ty,
        recs: fs.recs_visited,
        prefix: fs.prefix_tiles,
        blocks: fs.blocks,
        active: f.scene.active_records(),
        animating: f.scene.animating(),
    }
}

fn rank(v: &mut [u64], p: f64) -> u64 {
    v.sort_unstable();
    v[((p * v.len() as f64).ceil() as usize).clamp(1, v.len()) - 1]
}

struct Opts {
    check: bool,
    cell: Cell,
    legacy: bool,
    cards: usize,
    steps: usize,
    mutant: Mutant,
    bare: bool,
    bringup: bool,
    warm: bool,
    single: bool,
}

fn manifest() {
    let rows: &[(&str, &str, &str)] = &[
        ("cards: 640x400 RGBA, 10x10 grid, 64x40, quarter-pixel layer stagger, insertion order", "exact", "Grad/Shadow records; card i in cell i%100, last card is the foreground"),
        ("body: translucent rounded 60x36 with deterministic palette and asymmetric two-stop gradient", "exact", "Grad, stop alphas 0xd4/0xb8, direction (700000,-300000) in the seam's units"),
        ("two outer shadows, distinct spread/offset/blur", "reduced", "Shadow (spline blur: blur is two sigma) with spread via Grow; spreads +2/-1 px, offsets (0,3)/(2,5), blur 6/3 px"),
        ("shared procedural PNG tile every 16 cards", "MISSING", "no image/texture paint in the seam; no tile is drawn"),
        ("foreground card as motion subject", "exact", "last card"),
        ("scroll strip with mounted rows limited by the framework API", "reduced", "ten-column strip panned by uniform words; all cards stay mounted (no virtualisation API in the seam), so footprint does not follow exposure"),
        ("stroked cubic and even-odd filled path", "MISSING", "no path primitive"),
        ("rounded clips and alpha mask", "reduced", "Clip is a rectangle (one rect-clipped rounded fill is drawn); rounded-rect clip and alpha mask MISSING"),
        ("filtered group: blur 3, saturate 1.4, brightness 0.8, drop-shadow sigma 4 offset (3,5)", "MISSING", "no subtree group; the legacy `video` cell puts brightness 0.8 / saturation 1.4 on each pane's backdrop as a stand-in; `glass` panes have no filter"),
        ("three fills: hash-noise plasma, circle-minus-circle SDF, moving sine-ramp uniform", "reduced", "three Shader records using the two built-in nodes (stripes, ripple), clock word moving; plasma, SDF and sine-ramp MISSING (author shader registration not built)"),
        ("four 112x76 frosted panels, radius 12, backdrop sigma 6 px", "exact", "Backdrop + Border; sigma 6; tint is the translucent border fill"),
        ("256x144 video, 120 frames 30 fps, 4:2:0 limited BT.709, new frame per four ticks", "reduced", "Synth pattern (bars + box), NV12 BT.709; new frame each 4 ticks in glass/video; not a 120-frame loop and no codec (decode is the generator)"),
        ("fixed virtual timestamps", "exact", "tick k at k*1000/120 ms (U_TIME)"),
        ("cell startup (fresh process)", "reduced", "`measure startup N` times mount + first frame in-process; fx-seam-run.sh also spawns fresh processes"),
        ("cell idle", "exact", "frozen shader clock; zero recs/tiles/layer work and no active records asserted"),
        ("cell one (interrupt at 100 ms by 10 px)", "reduced", "spring to +10 px at tick 1, retargeted to -10 px at tick 12"),
        ("cell many (stagger translation/opacity/colour/width, interrupt at 100 ms, spring k=260 c=23 on every other card)", "exact", "Prog on rect, colour and opacity; every record rewritten at tick 12"),
        ("cell hover (20 distinct cells then reverse)", "reduced", "highlight by uniform word compare, opaque flat colour; walks only cells free of overlays"),
        ("cell theme (palette replaced every frame)", "exact", "eight shared palette words rewritten each tick, alpha preserved"),
        ("cell scroll (11 px per frame then reverse)", "reduced", "see strip"),
        ("cell resize 640x400 <-> 704x448 without remount", "MISSING", "Scene has a fixed surface; each size change remounts the scene from the live state (the row is a remount cost)"),
        ("cell glass over video", "reduced", "pane translates by a sine each tick, video each 4th tick; no group filter"),
        ("GPU timestamps, hardware GPU", "MISSING", "this adapter drives the CPU tile kernel on one core; the seam's Metal executor is not driven here"),
    ];
    println!("part\tstatus\tnote");
    for (a, b, c) in rows {
        println!("{a}\t{b}\t{c}");
    }
}

fn main() {
    assert_eq!(Response::new(260.0, 23.0), SPR2, "constant spring response drifted");
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("manifest") {
        return manifest();
    }
    let mut pos = Vec::new();
    let mut o = Opts { check: false, cell: Cell::Video, legacy: false, cards: 0, steps: 0, mutant: Mutant::None, bare: false, bringup: false, warm: true, single: false };
    for a in &args {
        match a.as_str() {
            "--bare" => o.bare = true,
            "--bringup" => o.bringup = true,
            "--nowarm" => o.warm = false,
            "--single" => o.single = true, // one-shot sequence only (about 70 active ticks for one/many)
            "--mutant=input" => o.mutant = Mutant::Input,
            "--mutant=damage" => o.mutant = Mutant::Damage,
            s if s.starts_with("--") => panic!("unknown flag {s}"),
            s => pos.push(s.to_string()),
        }
    }
    let usage = "usage: fx_bench manifest | check|measure CELL CARDS [N] [flags] | check|measure CARDS [N] (legacy video)";
    let [mode, rest @ ..] = &pos[..] else { panic!("{usage}") };
    assert!(mode == "check" || mode == "measure", "{usage}");
    o.check = mode == "check";
    let (cards, n) = match rest.first().map(|s| s.parse::<usize>()) {
        Some(Ok(c)) => {
            o.legacy = true;
            (c, rest.get(1))
        }
        _ => {
            o.cell = Cell::parse(rest.first().expect(usage)).expect(usage);
            (rest.get(1).expect(usage).parse().unwrap(), rest.get(2))
        }
    };
    assert!([200, 1000, 5000].contains(&cards));
    o.cards = cards;
    let default = if o.legacy {
        20
    } else if o.check {
        if matches!(o.cell, Cell::One | Cell::Many) { 100 } else { 48 }
    } else {
        240
    };
    o.steps = n.map(|s| s.parse().unwrap()).unwrap_or(default);
    let floor = if o.check || o.legacy || o.bringup { 20 } else { 240 };
    let need = if o.cell == Cell::Startup && !o.check { 1 } else { floor };
    assert!(o.steps >= need, "need at least {need} samples");
    if o.check && matches!(o.cell, Cell::One | Cell::Many) {
        assert!(o.steps >= 100, "one/many settle after about 70 ticks: use 100 or more steps");
    }
    if o.cell == Cell::Startup {
        return startup(&o);
    }
    if o.check {
        run_check(&o);
    } else {
        run_measure(&o);
    }
}

#[allow(clippy::too_many_arguments)]
fn row(name: &str, cards: usize, k: usize, r: &FrameOut, seq: u32, cum: &mut (u64, u64, u64)) {
    // fields: 0 name, 1 cards, 2 step, 3 wall_ns, 4 instrs, 5 cpu_ns, 6 rss, 7 footprint, 8 tiles_damaged,
    // 9 recs_visited, 10 prefix_tiles, 11 blocks, 12 tiles_total, 13 active_records, 14 animating,
    // 15 cum_tiles_drawn, 16 submitted, 17 video_seq, 18 cum_frames_drawn, 19 cum_tile_px (32x32, upper bound)
    cum.0 += (r.tiles > 0) as u64;
    cum.1 += r.tiles as u64;
    cum.2 += r.tiles as u64 * 1024;
    println!(
        "{name},{cards},{k},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{seq},{},{}",
        r.ns, r.instr, r.cpu, r.rss, r.foot, r.tiles, r.recs, r.prefix, r.blocks, r.tiles_total, r.active, r.animating as u8, cum.1, (r.tiles > 0) as u8, cum.0, cum.2
    );
}
const REFUSED: [(&str, &str); 5] = [
    ("png_tile", "no image paint in the seam: shared PNG tile not drawn"),
    ("group_filter", "no subtree group: blur/saturate/brightness/drop-shadow group not composited"),
    ("path_mask", "no path or alpha-mask primitive; clip is rectangular only"),
    ("shader_exact", "plasma, SDF and sine-ramp shaders absent; built-in stripes/ripple substituted"),
    ("resize_retained", "fixed-size Scene: resize remounts, retained resize unsupported"),
];
fn refusals(o: &Opts) {
    for (part, why) in REFUSED {
        if part == "resize_retained" && o.cell != Cell::Resize {
            continue;
        }
        println!("fx_refused,{},{},{part},{why}", o.cell.name(), o.cards);
    }
}

fn init_words(u: &mut U) {
    for p in 0..4 {
        u.w[4 + p] = pal_color(0, p as u32, 0);
        u.w[8 + p] = pal_color(0, p as u32, 1);
    }
}

fn startup(o: &Opts) {
    let pool = Pool::new(0);
    let overhead = perf::snap_overhead();
    let mut surface = vec![0u32; (W0 * H0) as usize];
    let mut all = Vec::new();
    let mut cum = (0, 0, 0);
    refusals(o);
    for k in 0..o.steps {
        let a0 = perf::snap();
        let start = Instant::now();
        let mut f = Fixture::new(Cell::Startup, o.cards, Mutant::None, o.bare);
        let mut u = U::default();
        init_words(&mut u);
        let fs = f.scene.update(&u);
        f.scene.prepare(&u);
        let mut tiles = Vec::new();
        f.scene.damage.tiles(&mut tiles);
        f.scene.shade(&u, &tiles, &mut surface, W0 as usize, &pool, 0);
        f.scene.media.reclaim();
        let ns = start.elapsed().as_nanos();
        let a1 = perf::snap();
        if o.check {
            check_probes(&surface, W0 as usize, &f.probes(0, true), "startup");
            let mut exact = vec![0; (W0 * H0) as usize];
            f.scene.spec(&u, &mut exact, W0 as usize);
            assert!(exact == surface, "first frame differs from the whole-frame specification");
            continue;
        }
        let r = FrameOut {
            ns,
            instr: a1.instrs.saturating_sub(a0.instrs).saturating_sub(overhead),
            cpu: a1.cpu_ns - a0.cpu_ns,
            rss: a1.rss,
            foot: a1.footprint,
            tiles: tiles.len(),
            tiles_total: f.scene.tx * f.scene.ty,
            recs: fs.recs_visited,
            prefix: fs.prefix_tiles,
            blocks: fs.blocks,
            active: f.scene.active_records(),
            animating: f.scene.animating(),
        };
        all.push((ns as u64, r.instr));
        row("fx_startup", o.cards, k, &r, 0, &mut cum);
    }
    if o.check {
        println!("gfx gate startup cards={} probes=ok spec=ok", o.cards);
    } else {
        summary(o, &mut all);
    }
}

fn summary(o: &Opts, all: &mut [(u64, u64)]) {
    let mut ns: Vec<u64> = all.iter().map(|a| a.0).collect();
    let mut ins: Vec<u64> = all.iter().map(|a| a.1).collect();
    let basis = if all.len() < 100 { "observed_max" } else { "nearest_rank" };
    let p = |v: &mut Vec<u64>| (rank(v, 0.5), rank(v, 0.95), rank(v, 0.99), *v.last().unwrap());
    let (a, b) = (p(&mut ns), p(&mut ins));
    println!(
        "fx_summary,{},{},{},ns_p50,{},p95,{},p99,{},max,{},instr_p50,{},p95,{},p99,{},max,{},p99_basis,{basis}",
        o.cell.name(), o.cards, all.len(), a.0, a.1, a.2, a.3, b.0, b.1, b.2, b.3
    );
}

fn run_measure(o: &Opts) {
    let pool = Pool::new(0);
    let overhead = perf::snap_overhead();
    let mut warm = o.warm && !o.legacy;
    loop {
        let mut f = Fixture::new(o.cell, o.cards, Mutant::None, o.bare);
        f.st.repeat = !o.single;
        let mut u = U::default();
        init_words(&mut u);
        let mut ring = Ring::new(1);
        let mut active_n = 0;
        let (mut tiles, mut cand) = (Vec::new(), Vec::new());
        let mut all = Vec::new();
        let mut idle_bad = 0;
        let mut cum = (0u64, 0u64, 0u64);
        if !warm {
            refusals(o);
        }
        let name = format!("fx_{}", o.cell.name());
        for k in 0..=o.steps {
            let r = drive(&mut f, &mut u, k, &mut ring, &pool, overhead, &mut tiles, &mut cand);
            if k == 0 || warm {
                continue;
            }
            all.push((r.ns as u64, r.instr));
            active_n += (r.active > 0 || r.tiles > 0) as usize;
            if o.cell == Cell::Idle && (r.recs | r.prefix | r.blocks | r.tiles as u32 | r.active as u32 | r.animating as u32) != 0 {
                idle_bad += 1;
            }
            row(&name, o.cards, k, &r, f.st.vseq, &mut cum);
        }
        if warm {
            warm = false;
            continue;
        }
        println!("fx_active,{},{},total,{},active,{active_n},repeat,{}", o.cell.name(), o.cards, all.len(), !o.single);
        if matches!(o.cell, Cell::One | Cell::Many) && !o.single && !o.bringup && active_n < 200 {
            println!("fx_invalid,{},fewer than 200 active frames", o.cell.name());
        }
        summary(o, &mut all);
        if idle_bad > 0 {
            println!("fx_invalid,idle,{idle_bad} steps did work");
            std::process::exit(2);
        }
        return;
    }
}

fn run_check(o: &Opts) {
    let pool = Pool::new(0);
    let overhead = perf::snap_overhead();
    refusals(o);
    let mut f = Fixture::new(o.cell, o.cards, o.mutant, o.bare);
    let mut r = Fixture::new(o.cell, o.cards, Mutant::None, o.bare);
    let (mut u, mut ru) = (U::default(), U::default());
    init_words(&mut u);
    init_words(&mut ru);
    let mut ring = Ring::new(4);
    let (mut tiles, mut cand) = (Vec::new(), Vec::new());
    let mut ref_px = vec![0u32; (W1 * H1) as usize];
    let (mut first_sum, mut last_sum, mut prev, mut changed, mut probes_n) = (0, 0, 0, 0usize, 0usize);
    let mut idle_work = 0u32;
    for k in 0..=o.steps {
        let out = drive(&mut f, &mut u, k, &mut ring, &pool, overhead, &mut tiles, &mut cand);
        if k > 0 && o.cell == Cell::Idle {
            idle_work |= out.recs | out.prefix | out.blocks | out.tiles as u32 | out.active as u32 | out.animating as u32;
        }
        r.step(k, &mut ru);
        r.scene.invalidate();
        r.scene.fx_nocache = true;
        let stride = r.st.w as usize;
        r.scene.frame(&ru, &mut ref_px, stride, &pool, 0);
        r.scene.media.reclaim();
        let n = stride * r.st.h as usize;
        if k == 0 {
            let mut exact = vec![0; n];
            f.scene.spec(&u, &mut exact, stride);
            assert!(exact == ref_px[..n], "independent full frame differs");
        }
        for (age, px) in ring.surfaces.iter().enumerate() {
            if k % (age + 1) == 0 {
                assert!(px[..n] == ref_px[..n], "retained differs from full redraw: age {}, frame {k}", age + 1);
            }
        }
        let last = k == o.steps;
        let pr = f.probes(k, last);
        probes_n += pr.len();
        check_probes(&ring.surfaces[0], stride, &pr, &format!("{} frame {k}", o.cell.name()));
        let sum = checksum(&ref_px[..n]);
        if k == 0 {
            first_sum = sum;
        } else if sum != prev {
            changed += 1;
        } else if o.cell.every_frame() {
            panic!("{}: frame {k} did not change", o.cell.name());
        }
        prev = sum;
        last_sum = sum;
    }
    if o.cell == Cell::Idle {
        assert_eq!(idle_work, 0, "idle frames did framework work");
        assert_eq!(first_sum, last_sum, "idle frame changed");
    } else {
        assert!(changed > 0, "frame checksum never changed");
        assert!(changed >= o.steps / 4 || matches!(o.cell, Cell::One | Cell::Many | Cell::Video), "too few changing frames: {changed}");
    }
    assert!(probes_n > 0, "no independent constraint was evaluated");
    // fresh mount: replay the inputs without drawing, draw once, compare with the retained frame
    let mut m = Fixture::new(o.cell, o.cards, Mutant::None, o.bare);
    let mut mu = U::default();
    init_words(&mut mu);
    for k in 0..=o.steps {
        m.step(k, &mut mu);
    }
    let stride = m.st.w as usize;
    let mut fresh = vec![0u32; stride * m.st.h as usize];
    m.scene.fx_nocache = true;
    m.scene.frame(&mu, &mut fresh, stride, &pool, 0);
    assert!(ring.surfaces[0][..fresh.len()] == fresh[..], "fresh mount differs from the retained frame");
    println!(
        "gfx gate {} cards={} ages=1..4 steps={} probes={} changed_frames={} fresh=ok{}",
        o.cell.name(), o.cards, o.steps, probes_n, changed, if o.legacy { " (legacy)" } else { "" }
    );
}
