//! Staged paint benchmark for the published fx seam. This drives the renderer without a window.
use bough_kernel::cpu::Pool;
use bough_paint::expr::*;
use bough_paint::media::Synth;
use bough_paint::motion::{Response, Spring, SpringX};
use bough_paint::paint::*;
use bough_paint::scene::{ColId, Scene};
use bough_sys::perf;
use std::time::Instant;

const W: usize = 640;
const H: usize = 400;
const VIDEO: [i32; 4] = [256 * 256, 124 * 256, 512 * 256, 268 * 256];
const SEED: u32 = 41;
type PaintColor = Pm<Rec>;
type VideoPaint = Video<Lane, I<Rec>, Rec, Rec, Rec>;
type SpringOperand = SpringX<852, 21_889>;
type PanelBox = Off<Lane, SpringOperand, SpringOperand>;
type Glass = Over<
    Backdrop<PanelBox, I<Rec>, I<Rec>, Filt<Rec, Rec, Rec>>,
    Border<PanelBox, I<Rec>, Ki<256>, PaintColor, PaintColor>,
>;

fn q(n: f32) -> i32 {
    (n * 256.0).round() as i32
}
fn ri(n: f32) -> I<Rec> {
    I {
        a: Rec(q(n) as u32),
    }
}
fn rgba(c: u32) -> PaintColor {
    Pm { c: Rec(c) }
}
fn rect(x: f32, y: f32, w: f32, h: f32) -> [i32; 4] {
    [q(x), q(y), q(x + w), q(y + h)]
}

fn panel(x: &Spring) -> Glass {
    let b = Off {
        b: Lane,
        dx: x.operand(),
        dy: Spring::rest(0).operand(),
    };
    Over(
        Backdrop {
            b,
            r: ri(12.0),
            blur: ri(6.0),
            f: V3 {
                a: Rec(205),
                b: Rec(358),
                c: Rec(256),
            },
        },
        Border {
            b,
            r: ri(12.0),
            w: Ki,
            c: rgba(0xaae5f6ff),
            fill: rgba(0x526789a9),
        },
    )
}

fn video(slot: u32) -> VideoPaint {
    Video {
        b: Lane,
        r: ri(10.0),
        slot: Rec(slot),
        dims: Rec(256 | 144 << 16),
        op: Rec(32768),
    }
}

struct Fixture {
    scene: Scene,
    video: ColId<VideoPaint>,
    panels: ColId<Glass>,
    x: Spring,
    src: Synth,
}

impl Fixture {
    fn new(cards: usize) -> Self {
        let mut scene = Scene::new(W as u32, H as u32, 1 << 20);
        scene.media = bough_paint::media::Pool::new(1 << 18);
        let card = scene.column::<RRect<Lane, I<Rec>, PaintColor>>(cards + 1);
        let shaders = scene.column::<Shader<Lane, I<Rec>, Rec, Uw<63>, PaintColor, PaintColor>>(2);
        let video_col = scene.column::<VideoPaint>(1);
        let panels = scene.column::<Glass>(4);
        scene.push(
            card,
            rect(0.0, 0.0, W as f32, H as f32),
            RRect {
                b: Lane,
                r: ri(0.0),
                c: rgba(0xff142638),
            },
        );
        for i in 0..cards {
            let slot = i % 100;
            let layer = ((i / 100) % 8) as f32 * 0.25;
            let card_rect = rect(
                (slot % 10) as f32 * 64.0 + layer,
                (slot / 10) as f32 * 40.0 + layer,
                60.0,
                36.0,
            );
            let rgb = (i as u32).wrapping_mul(0x0077_9b83) & 0xffffff;
            scene.push(
                card,
                card_rect,
                RRect {
                    b: Lane,
                    r: ri(6.0),
                    c: rgba(0xd400_0000 | rgb),
                },
            );
        }
        for (id, x) in [48.0, 486.0].into_iter().enumerate() {
            scene.push(
                shaders,
                rect(x, 128.0, 96.0, 60.0),
                Shader {
                    b: Lane,
                    r: ri(10.0),
                    id: Rec(id as u32),
                    now: Uw,
                    c: rgba(0xaa3086d8),
                    c2: rgba(0xaac278bd),
                },
            );
        }
        let mut src = Synth::new(256, 144, SEED);
        let slot = src.next(&mut scene.media).expect("video pool");
        scene.push(video_col, VIDEO, video(slot));
        let x = Spring::rest(0);
        for (a, b) in [(14.0, 18.0), (514.0, 18.0), (328.0, 150.0), (482.0, 302.0)] {
            scene.push(panels, rect(a, b, 112.0, 76.0), panel(&x));
        }
        Self {
            scene,
            video: video_col,
            panels,
            x,
            src,
        }
    }

    fn step(&mut self, k: usize, u: &mut U) {
        let now = (k * 1_000 / 120) as u32;
        u.w[U_TIME] = now;
        if k == 1 || k == 12 {
            let target = if k == 1 { 10.0 } else { -10.0 };
            self.x.retarget(Response::SNAPPY, now, q(target));
            self.scene.set(
                self.panels,
                2,
                rect(328.0, 150.0, 112.0, 76.0),
                panel(&self.x),
            );
        }
        if k != 0 && k.is_multiple_of(4) {
            let slot = self.src.next(&mut self.scene.media).expect("video pool");
            self.scene.set(self.video, 0, VIDEO, video(slot));
        }
    }
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [mode, cards] = &args[..] else {
        panic!("usage: fx_bench check|measure CARDS");
    };
    let cards = cards.parse::<usize>().unwrap();
    assert!([200, 1000, 5000].contains(&cards));
    assert!(mode == "check" || mode == "measure");
    let check = mode == "check";
    let mut f = Fixture::new(cards);
    let mut u = U::default();
    let pool = Pool::new(0);
    let overhead = perf::snap_overhead();
    let mut surfaces = vec![vec![0; W * H]; if check { 4 } else { 1 }];
    let mut stale = vec![vec![true; f.scene.tx * f.scene.ty]; surfaces.len()];
    let mut candidates = Vec::with_capacity(stale[0].len());
    let mut tiles = Vec::with_capacity(stale[0].len());
    let mut witness = 0;
    let mut reference = check.then(|| Fixture::new(cards));
    let mut reference_words = U::default();
    let mut reference_pixels = if check { vec![0; W * H] } else { Vec::new() };
    for k in 0..=20 {
        let a0 = perf::snap();
        let start = Instant::now();
        f.step(k, &mut u);
        let fs = f.scene.update(&u);
        f.scene.prepare(&u);
        f.scene.damage.tiles(&mut tiles);
        f.scene.damage.clear();
        for (age, lag) in stale.iter_mut().enumerate() {
            for &t in &tiles {
                lag[t as usize] = true;
            }
            if k % (age + 1) != 0 {
                continue;
            }
            candidates.clear();
            candidates.extend(
                lag.iter()
                    .enumerate()
                    .filter_map(|(t, &stale)| stale.then_some(t as u32)),
            );
            f.scene
                .shade(&u, &candidates, &mut surfaces[age], W, &pool, 0);
            lag.fill(false);
        }
        f.scene.media.reclaim();
        let ns = start.elapsed().as_nanos();
        let a1 = perf::snap();
        if let Some(reference) = &mut reference {
            reference.step(k, &mut reference_words);
            reference.scene.invalidate();
            reference.scene.fx_nocache = true;
            reference
                .scene
                .frame(&reference_words, &mut reference_pixels, W, &pool, 0);
            reference.scene.media.reclaim();
            if k == 0 {
                // Keep the independent shader and backdrop reference at every size. The
                // uncached full draw checks all subsequent frames and ring ages.
                let mut exact = vec![0; W * H];
                f.scene.spec(&u, &mut exact, W);
                assert!(exact == reference_pixels, "independent full frame differs");
            }
            for (age, px) in surfaces.iter().enumerate() {
                if k % (age + 1) == 0 {
                    assert!(*px == reference_pixels, "age {}, frame {k}", age + 1);
                }
            }
            let px = reference_pixels[178 * W + 365];
            if k == 0 {
                witness = px;
            }
            if k == 20 {
                assert_ne!(px, witness, "glass over video never changed");
            }
        } else if k > 0 {
            let instr = a1.instrs.saturating_sub(a0.instrs).saturating_sub(overhead);
            println!(
                "fx_video,{cards},{k},{ns},{instr},{},{},{},{},{},{},{}",
                a1.cpu_ns - a0.cpu_ns,
                a1.rss,
                a1.footprint,
                tiles.len(),
                fs.recs_visited,
                fs.prefix_tiles,
                fs.blocks
            );
        }
    }
    if check {
        println!("gfx gate video cards={cards} ages=1..4");
    }
}
