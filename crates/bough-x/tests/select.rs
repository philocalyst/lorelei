//! Gates G4, G5 and G7 of the incremental-operators card.
//!
//! * G4: a column whose records declare a key (`sel`) or a reach (`near`) leaves, at every step of a random
//!   script, exactly the damage its own page scan leaves (the same world with the bounded deltas off), and
//!   retained == full redraw == immediate == fresh mount, and the same pixels as the opaque form.
//! * G7: once the index exists, a change of the selection evaluates two records, whatever is exposed; with a
//!   description pass between every change the scan answers every time (the fallback, forced).
//! * G5: with sweeps handed over as rects, the rects marked as tiles are the tiles of the other mode.
//!
//! `--features mutant_keyed`, `mutant_near` and `mutant_swept` must each fail this file.
#[path = "../scenes/anim.rs"]
mod anim;
#[path = "../scenes/grid.rs"]
mod grid;
use bough_x::prelude::*;
use bough_x::soft::{Immediate, Soft};
use grid::*;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}
fn diff(a: &[u32], b: &[u32]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

const H: i32 = 328;
const ROWS: u32 = 5000;

/// The three worlds of the gate after the same inputs: the declared form with its bounded deltas, the same
/// with them off, and the opaque form.
struct Trio {
    g: Grid<true>,
    fast: World<Grid<true>, Soft>,
    scan: World<Grid<true>, Soft>,
    og: Grid<false>,
    opaque: World<Grid<false>, Soft>,
    retained: Soft,
}
impl Trio {
    fn new() -> Trio {
        let (g, og) = (Grid::<true>::new(ROWS, H), Grid::<false>::new(ROWS, H));
        let mut t = Trio {
            fast: World::new(W, H, NWORDS),
            scan: World::new(W, H, NWORDS),
            opaque: World::new(W, H, NWORDS),
            retained: Soft::new(W, H),
            g,
            og,
        };
        t.scan.no_fast = true;
        t.fast.mount(&t.g);
        t.scan.mount(&t.g);
        t.opaque.mount(&t.og);
        t
    }
    fn settle(&mut self) {
        self.og.u = self.g.u;
        self.fast.settle(&self.g);
        self.scan.settle(&self.g);
        self.opaque.settle(&self.og);
    }
    fn pointer(&mut self, x: i32, y: i32) {
        self.fast.pointer_words(x, y);
        self.scan.pointer_words(x, y);
        self.opaque.pointer_words(x, y);
    }
    fn check(&mut self, what: &str, fresh: bool) {
        assert!(
            self.fast.tiles.bits == self.scan.tiles.bits,
            "{what}: the bounded delta's damage is not the scan's"
        );
        assert_eq!(
            self.scan.fast_stats,
            Default::default(),
            "{what}: the scan world took a bounded delta"
        );
        self.retained
            .repair(&self.fast.tiles, |s| self.fast.paint(s));
        self.fast.tiles.clear();
        self.scan.tiles.clear();
        self.opaque.tiles.clear();
        let mut full = Soft::new(W, H);
        self.fast.paint(&mut full);
        assert!(
            self.retained.px == full.px,
            "{what}: retained != full redraw ({} pixels)",
            diff(&self.retained.px, &full.px)
        );
        let mut now = Soft::new(W, H);
        self.g.view(&mut Immediate::new(&mut now, &self.fast.shown));
        assert!(
            full.px == now.px,
            "{what}: stores != immediate ({} pixels)",
            diff(&full.px, &now.px)
        );
        let mut other = Soft::new(W, H);
        self.opaque.paint(&mut other);
        assert!(
            full.px == other.px,
            "{what}: declared != opaque form ({} pixels)",
            diff(&full.px, &other.px)
        );
        if fresh {
            let mut w2 = World::<_, Soft>::new(W, H, NWORDS);
            w2.mount(&self.g);
            w2.pointer_words(
                self.fast.shown.w[bough_x::expr::U_PTR_X] as i32,
                self.fast.shown.w[bough_x::expr::U_PTR_Y] as i32,
            );
            let mut f = Soft::new(W, H);
            w2.paint(&mut f);
            assert!(
                full.px == f.px,
                "{what}: retained != fresh mount ({} pixels)",
                diff(&full.px, &f.px)
            );
        }
    }
}

#[test]
fn bounded_deltas_leave_the_scans_damage_and_the_same_pixels() {
    let mut t = Trio::new();
    let mut rng = Rng(0x5e1ec7 | 1);
    t.check("mount", true);
    let exposed = (H / ROW_H) as u32;
    for step in 1..=3000usize {
        let top = (t.g.u.get::<Scroll>() / ROW_H) as u32;
        let what = format!("step {step}");
        match rng.below(12) {
            // select an exposed cell, any cell, or none
            0..=3 => {
                let id = match rng.below(8) {
                    0 => NONE,
                    1 => rng.below((ROWS as usize) * CELLS) as u32,
                    _ => Grid::<true>::cell(
                        top + rng.below(exposed as usize + 2) as u32,
                        rng.below(CELLS) as u32,
                    ),
                };
                t.g.u.set::<Sel>(id);
                t.settle();
            }
            // the pointer: near the glowing cells, on them, far away, off the window
            4..=7 => {
                let (x, y) = match rng.below(6) {
                    0 => (
                        -5000 + rng.below(10_000) as i32,
                        -5000 + rng.below(10_000) as i32,
                    ),
                    1 => (
                        i32::MAX - rng.below(3) as i32,
                        i32::MIN + rng.below(3) as i32,
                    ),
                    2 => (
                        CELLS as i32 * CELL_W + rng.below(600) as i32,
                        top as i32 * ROW_H + rng.below(H as usize) as i32,
                    ),
                    // just around the reach of some cell
                    3 => {
                        let c = Grid::<true>::centre(
                            top as usize + rng.below(exposed as usize),
                            rng.below(CELLS),
                        );
                        (
                            c[0] + [-256, -255, 255, 256, 0][rng.below(5)],
                            c[1] + [-256, -255, 255, 256, 0][rng.below(5)],
                        )
                    }
                    _ => (
                        rng.below(W as usize) as i32,
                        rng.below(H as usize * 4) as i32,
                    ),
                };
                t.pointer(x, y);
            }
            8 => {
                let th = t.g.u.get::<Theme>() ^ 1;
                t.g.u.set::<Theme>(th);
                t.settle();
            }
            // the selection and the theme in one turn: not a change of the key's word alone
            9 => {
                let th = t.g.u.get::<Theme>() ^ 1;
                t.g.u.set::<Theme>(th);
                t.g.u.set::<Sel>(Grid::<true>::cell(
                    top + rng.below(exposed as usize) as u32,
                    0,
                ));
                t.settle();
            }
            // a list pass: rows enter and leave, slots are reused
            _ => {
                t.g.scroll_by(
                    [ROW_H, -ROW_H, 3 * ROW_H, -40 * ROW_H, 500 * ROW_H, 6][rng.below(6)],
                );
                // sometimes with a selection in the same turn
                if rng.below(3) == 0 {
                    t.g.u.set::<Sel>(Grid::<true>::cell(
                        top + rng.below(exposed as usize) as u32,
                        3,
                    ));
                }
                t.settle();
            }
        }
        t.check(&what, step % 97 == 0);
    }
    let s = t.fast.fast_stats;
    assert!(
        s.key_hits > 200 && s.key_builds > 20 && s.key_scans > 20,
        "every keyed route was taken: {s:?}"
    );
    assert!(
        s.near_hits > 200 && s.near_builds > 20,
        "every reach route was taken: {s:?}"
    );
}

/// G7. The grid exposes `H / ROW_H` rows of eight keyed cells; a change of the selection evaluates two.
#[test]
fn a_selection_change_evaluates_two_records_whatever_is_exposed() {
    for h in [328, 2056, 8200] {
        let mut g = Grid::<true>::new(100_000, h);
        let mut w = World::<_, Soft>::new(W, h, NWORDS);
        w.mount(&g);
        w.tiles.clear();
        let rows = (h / ROW_H) as u32;
        // the first change of only the key's word is the scan's, the second builds the index
        for k in 0..2u32 {
            g.u.set::<Sel>(Grid::<true>::cell(k, 0));
            w.settle(&g);
        }
        assert_eq!(
            (
                w.fast_stats.key_scans,
                w.fast_stats.key_builds,
                w.fast_stats.key_hits
            ),
            (1, 1, 1)
        );
        let before = w.fast_stats;
        for k in 0..64u32 {
            w.tiles.clear();
            g.u.set::<Sel>(Grid::<true>::cell(k * 7 % rows, k % CELLS as u32));
            w.settle(&g);
            assert!(w.tiles.count() >= 1);
        }
        let d = w.fast_stats;
        assert_eq!(
            (d.key_hits - before.key_hits, d.key_builds, d.key_scans),
            (64, 1, 1)
        );
        assert_eq!(
            d.key_records - before.key_records,
            2 * 64,
            "two records per change at {h} px"
        );
        // to a key no record has, and back: one record each way
        g.u.set::<Sel>(NONE);
        w.settle(&g);
        assert_eq!(w.fast_stats.key_records - d.key_records, 1);

        // the adversarial case: a description pass between every change. The index is never worth building,
        // and is not built: every change is the scan's.
        let d = w.fast_stats;
        for k in 0..32u32 {
            g.scroll_by(if k % 2 == 0 { ROW_H } else { -ROW_H });
            w.settle(&g);
            g.u.set::<Sel>(Grid::<true>::cell(k % rows, 1));
            w.settle(&g);
        }
        let e = w.fast_stats;
        assert_eq!(
            (
                e.key_hits - d.key_hits,
                e.key_builds - d.key_builds,
                e.key_scans - d.key_scans
            ),
            (0, 0, 32)
        );
    }
}

/// G5. A frame with records in motion, handed over as rects: marked as tiles, they are the other mode's tiles.
#[test]
fn swept_rects_are_the_tiles_of_the_other_mode() {
    use anim::{Anim, BANNER, ROW_H as AROW};
    let h = 400;
    let mut a = Anim::new(10_000, h);
    let mut bits = World::<_, Soft>::new(anim::W, h, anim::NWORDS);
    let mut rects = World::<_, Soft>::new(anim::W, h, anim::NWORDS);
    rects.tiles.sweep_rects = true;
    for w in [&mut bits, &mut rects] {
        w.tick(1000);
        w.mount(&a);
        w.tiles.clear();
    }
    let _ = BANNER;
    let mut rng = Rng(77);
    let mut t = 1000u32;
    let mut handed = 0usize;
    for step in 0..600 {
        match rng.below(6) {
            0 => {
                a.toggle(t);
                bits.settle(&a);
                rects.settle(&a);
            }
            1 => {
                a.scroll_by([3 * AROW, -AROW, 7][rng.below(3)]);
                bits.settle(&a);
                rects.settle(&a);
            }
            _ => {}
        }
        t += [16, 16, 33, 250][rng.below(4)];
        bits.tick(t);
        rects.tick(t);
        assert!(bits.tiles.swept.is_empty());
        assert_eq!(
            rects.tiles.sweeps.len() >= 1,
            !rects.tiles.swept.is_empty() || rects.tiles.sweeps.is_empty(),
            "step {step}"
        );
        assert_eq!(
            bits.tiles.any(),
            rects.tiles.any(),
            "step {step}: one mode has damage and the other none"
        );
        handed += rects.tiles.swept.len();
        rects.tiles.settle_swept();
        assert!(
            bits.tiles.bits == rects.tiles.bits,
            "step {step}: the rects are not the tiles"
        );
        bits.tiles.clear();
        rects.tiles.clear();
        assert!(!rects.tiles.any());
    }
    assert!(
        handed > 600,
        "a rect was handed over nearly every frame: {handed}"
    );
}

/// The anim fixture with its glowing cells declaring their reach paints what it paints without.
#[test]
fn the_anim_fixture_with_declared_reach_paints_the_same() {
    use anim::Anim;
    let h = 600;
    let (mut a, mut b) = (Anim::new(5000, h), Anim::new(5000, h));
    b.near = true;
    let (mut wa, mut wb) = (
        World::<_, Soft>::new(anim::W, h, anim::NWORDS),
        World::<_, Soft>::new(anim::W, h, anim::NWORDS),
    );
    wa.mount(&a);
    wb.mount(&b);
    let mut retained = Soft::new(anim::W, h);
    let mut rng = Rng(0xa11);
    for step in 0..400 {
        if rng.below(5) == 0 {
            let dy = [anim::ROW_H * 3, -anim::ROW_H, 11][rng.below(3)];
            a.scroll_by(dy);
            b.scroll_by(dy);
            wa.settle(&a);
            wb.settle(&b);
        } else {
            let (x, y) = (
                rng.below(anim::W as usize + 400) as i32 - 200,
                rng.below(h as usize + 400) as i32 - 200,
            );
            wa.pointer_words(x, y);
            wb.pointer_words(x, y);
        }
        retained.repair(&wb.tiles, |s| wb.paint(s));
        let (ca, cb) = (wa.tiles.count(), wb.tiles.count());
        assert!(
            wa.tiles.bits == wb.tiles.bits,
            "step {step}: opaque form damages {ca} tiles, declared {cb}"
        );
        wa.tiles.clear();
        wb.tiles.clear();
        let (mut fa, mut fb) = (Soft::new(anim::W, h), Soft::new(anim::W, h));
        wa.paint(&mut fa);
        wb.paint(&mut fb);
        assert!(
            fa.px == fb.px,
            "step {step}: declared reach paints other pixels"
        );
        assert!(retained.px == fb.px, "step {step}: retained != full redraw");
    }
    assert!(wb.fast_stats.near_hits > 100 && wa.fast_stats.near_hits == 0);
    // the sequence `anim_x` counts, at its exposure
    let h = 10288;
    let (mut a, mut b) = (Anim::new(100_000, h), Anim::new(100_000, h));
    b.near = true;
    let (mut wa, mut wb) = (
        World::<_, Soft>::new(anim::W, h, anim::NWORDS),
        World::<_, Soft>::new(anim::W, h, anim::NWORDS),
    );
    wa.tick(1000);
    wb.tick(1000);
    wa.mount(&a);
    wb.mount(&b);
    // frames and fades first, as the example runs them
    for k in 0..200u32 {
        if k % 8 == 0 {
            a.toggle(wa.now());
            b.toggle(wb.now());
            wa.settle(&a);
            wb.settle(&b);
        }
        wa.tick(1000 + k * 16);
        wb.tick(1000 + k * 16);
    }
    wa.tiles.clear();
    wb.tiles.clear();
    for k in 0..64i32 {
        let (x, y) = (700 + (k % 16) * 9, 100 + (k % 5) * 7);
        wa.pointer_words(x, y);
        wb.pointer_words(x, y);
        assert!(
            wa.tiles.bits == wb.tiles.bits,
            "event {k}: opaque {} tiles, declared {}",
            wa.tiles.count(),
            wb.tiles.count()
        );
        wa.tiles.clear();
        wb.tiles.clear();
    }
}
