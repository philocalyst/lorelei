//! Broad words (`World::broad`): a change of a word the application declares broad (a theme) damages the
//! spaces of the columns that read it, one mark a column, and evaluates no record.
//!
//! * the expression of a reading record is not evaluated by the turn, at any number of exposed rows;
//! * the damage is a superset of the page scan's, and retained == full redraw == `Immediate` at every step of
//!   a script of theme changes, scrolls, edits and pointer moves on the sheet fixture.
//!
//! `--features mutant_broad` must fail this file.
#[path = "../scenes/sheet.rs"]
mod sheet;
use bough_x::prelude::*;
use bough_x::soft::{Immediate, Soft};
use std::cell::Cell;

thread_local! {
    static EVALS: Cell<usize> = const { Cell::new(0) };
}

lens!(Mode: u32 = 0);

/// Rows whose fill is a function of a block word and of the row.
struct Rows(U, u32);
impl View for Rows {
    fn block(&self) -> &U {
        &self.0
    }
    fn view<S: Sink<Self>>(&self, sink: &mut S) {
        sink.list(
            &List {
                n: self.1,
                row_h: 4,
                x: 0,
                y: 0,
                shape: &[Rect::new(0, 0, 64, 3)],
                edits: &FIXED,
            },
            |row| {
                Fill((un::<Mode>(), rec(row as u32)).map(|(m, r): (u32, u32)| {
                    EVALS.with(|n| n.set(n.get() + 1));
                    Color(0xff00_0000 | (r * 0x01_0305) ^ (m * 0x40_4040))
                }))
            },
        );
    }
}

/// The bit of the block word `change` writes.
fn word_of<V: View>(v: &mut V, change: impl Fn(&mut V)) -> u64 {
    let before = *v.block();
    change(v);
    let after = *v.block();
    change(v);
    let mut m = 0u64;
    for (i, (a, b)) in before.w.iter().zip(after.w.iter()).enumerate() {
        m |= ((a != b) as u64) << i;
    }
    assert!(m.count_ones() == 1, "one word: {m:b}");
    m
}

#[test]
fn a_broad_change_evaluates_no_record() {
    let mut counts = Vec::new();
    for rows in [64, 4096] {
        for broad in [false, true] {
            let mut view = Rows(U::default(), rows as u32);
            let mut world = World::<_, Soft>::new(64, rows * 4, 1);
            world.mount(&view);
            if broad {
                world.broad = word_of(&mut view, |v| v.0.set::<Mode>(v.0.get::<Mode>() ^ 1));
            }
            world.tiles.clear();
            EVALS.with(|n| n.set(0));
            view.0.set::<Mode>(1);
            world.settle(&view);
            assert!(world.tiles.any(), "rows {rows}: the change left no damage");
            assert_eq!(world.fast_stats.broad_hits, broad as u64);
            counts.push(EVALS.with(Cell::get));
        }
    }
    // [scan at 64, broad at 64, scan at 4096, broad at 4096]
    assert!(
        counts[0] >= 64 && counts[2] >= 4096 && counts[1] == 0 && counts[3] == 0,
        "evaluations in the turn: {counts:?}"
    );
}

#[test]
fn broad_damage_holds_the_scans_and_the_frame_is_the_same() {
    let (w, h) = (sheet::W, sheet::H);
    let mut v = sheet::Sheet::new(2000);
    let theme = word_of(&mut v, |v| v.toggle_theme());
    let (mut broad, mut scan) = (
        World::<_, Soft>::new(w, h, sheet::NWORDS),
        World::<_, Soft>::new(w, h, sheet::NWORDS),
    );
    broad.broad = theme;
    broad.mount(&v);
    scan.mount(&v);
    let mut retained = Soft::new(w, h);
    retained.repair(&broad.tiles, |s| broad.paint(s));
    broad.tiles.clear();
    scan.tiles.clear();
    let mut seed = 0xb40ad_u64;
    let mut below = |n: usize| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((seed >> 33) as usize) % n
    };
    for step in 0..90 {
        match below(4) {
            0 => {
                v.toggle_theme();
                broad.settle(&v);
                scan.settle(&v);
            }
            1 => {
                v.scroll_by([44, -44, 7, 900, -900][below(5)]);
                broad.settle(&v);
                scan.settle(&v);
            }
            2 => {
                v.edit(below(2000), below(sheet::COLS), below(100_000) as f32);
                broad.settle(&v);
                scan.settle(&v);
            }
            _ => {
                let (x, y) = (below(w as usize) as i32, below(h as usize) as i32);
                broad.pointer_words(x, y);
                scan.pointer_words(x, y);
            }
        }
        assert!(
            broad
                .tiles
                .bits
                .iter()
                .zip(&scan.tiles.bits)
                .all(|(b, s)| b & s == *s),
            "step {step}: the broad damage misses a tile the scan damages"
        );
        retained.repair(&broad.tiles, |s| broad.paint(s));
        broad.tiles.clear();
        scan.tiles.clear();
        let mut full = Soft::new(w, h);
        broad.paint(&mut full);
        assert!(
            retained.px == full.px,
            "step {step}: retained != full redraw"
        );
        let mut now = Soft::new(w, h);
        v.view(&mut Immediate::new(&mut now, &broad.shown));
        assert!(full.px == now.px, "step {step}: stores != immediate");
    }
    assert!(broad.fast_stats.broad_hits > 20 && scan.fast_stats.broad_hits == 0);
}
