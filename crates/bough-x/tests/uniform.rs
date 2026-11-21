//! A column of records with no constants of their own (an expression of the block alone, such as a theme
//! colour or a pointer-driven tint shared by every row) is differentiated once per change, not once per record.
//!
//! * the number of evaluations of the expression does not depend on how many rows are exposed;
//! * retained == full redraw == fresh mount at every step of a script that changes and does not change what
//!   the column shows.
//!
//! `--features mutant_uniform` must fail this file.
use bough_x::prelude::*;
use bough_x::soft::Soft;
use std::cell::Cell;

thread_local! {
    static EVALS: Cell<usize> = const { Cell::new(0) };
}

const W: i32 = 64;

/// `n` rows, each one fill whose colour is a function of the pointer's x alone.
struct Tinted(U, u32);

impl View for Tinted {
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
                shape: &[Rect::new(0, 0, W, 3)],
                edits: &FIXED,
            },
            |_| {
                Fill(un::<PtrX>().map(|x| {
                    EVALS.with(|n| n.set(n.get() + 1));
                    // bands of 8 px: most pointer moves change nothing
                    Color(0xff10_2030 + ((x as u32 / 8) & 7) * 0x0f_0f0f)
                }))
            },
        );
    }
}

#[test]
fn evaluations_do_not_grow_with_exposed_rows() {
    let mut counts = Vec::new();
    for (rows, x) in [(8, 3), (64, 3), (512, 3), (8, 40), (64, 40), (512, 40)] {
        let view = Tinted(U::default(), rows as u32);
        let mut world = World::<_, Soft>::new(W, rows * 4, 0);
        world.mount(&view);
        world.pointer_words(1, 0);
        world.tiles.clear();
        EVALS.with(|n| n.set(0));
        world.pointer_words(x, 0);
        // x = 3 stays in the band (no damage), x = 40 leaves it (every row damaged)
        assert_eq!(world.tiles.any(), x == 40, "rows {rows}, x {x}");
        counts.push(EVALS.with(Cell::get));
    }
    assert!(
        counts.iter().all(|&c| c == counts[0]) && counts[0] <= 2,
        "evaluations per change at 8, 64, 512 rows, unchanged then changed: {counts:?}"
    );
}

#[test]
fn retained_equals_full_redraw_and_a_fresh_mount() {
    let rows = 150;
    let h = rows * 4;
    let view = Tinted(U::default(), rows as u32);
    let mut world = World::<_, Soft>::new(W, h, 0);
    world.mount(&view);
    let mut retained = Soft::new(W, h);
    retained.repair(&world.tiles, |s| world.paint(s));
    world.tiles.clear();
    let mut seed = 0x51ed_u64;
    let (mut changed, mut same) = (0, 0);
    for step in 0..300 {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let x = ((seed >> 33) % 64) as i32;
        world.pointer_words(x, (step % 7) as i32);
        if world.tiles.any() {
            changed += 1
        } else {
            same += 1
        }
        retained.repair(&world.tiles, |s| world.paint(s));
        world.tiles.clear();
        let mut full = Soft::new(W, h);
        world.paint(&mut full);
        assert!(
            retained.px == full.px,
            "step {step}: retained != full redraw"
        );
        let mut fresh = World::<_, Soft>::new(W, h, 0);
        fresh.mount(&view);
        fresh.pointer_words(x, (step % 7) as i32);
        let mut f = Soft::new(W, h);
        fresh.paint(&mut f);
        assert!(full.px == f.px, "step {step}: retained != fresh mount");
    }
    assert!(
        changed > 50 && same > 20,
        "the script covers both: {changed} changed, {same} unchanged"
    );
}
