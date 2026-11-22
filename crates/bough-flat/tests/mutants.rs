//! Negative controls of the flat core and the damage engine. Each test breaks one rule (a bit of
//! `bough_damage::mutant`) and requires the oracle that rests on the rule to fail, on a scene where the same
//! operation holds with the rule intact.
//!
//!   cargo test --release -p bough-flat --features mutants --test mutants
#![cfg(feature = "mutants")]
mod common;
use bough_damage::mutant;
use bough_flat::*;
use common::*;

/// Mount, run `op` with the rule intact (must hold), then on a second scene with `bit` set (must not).
fn must_fail(bit: u32, rows: u32, prepare: impl Fn(&mut Scene), op: impl Fn(&mut Scene)) -> String {
    let mut control = Scene::new(rows);
    control.w.check(&mut NoExt).expect("the mount holds");
    prepare(&mut control);
    control.w.check(&mut NoExt).expect("the preparation holds");
    op(&mut control);
    control.w.check(&mut NoExt).expect("the operation holds with the rule intact");

    let mut s = Scene::new(rows);
    s.w.check(&mut NoExt).expect("the mount holds");
    prepare(&mut s);
    s.w.check(&mut NoExt).expect("the preparation holds");
    s.w.engine.mutants = bit;
    op(&mut s);
    let r = s.w.check(&mut NoExt);
    s.w.engine.mutants = 0;
    match r {
        Err(e) => e,
        Ok(i) => panic!("the mutant {bit:#x} was not caught: {i:?}"),
    }
}

#[test]
fn a_cover_that_culls_whatever_its_order_fails_the_redraw_oracle() {
    // a row is above the list's opaque panel: hovering it must repaint it
    let e = must_fail(mutant::OCCLUSION_ANY_Z, 12, |_| {}, |s| {
        let (_, y) = s.row_center(4);
        s.w.pointer_move(40, y);
    });
    assert!(e.starts_with("retained != full redraw"), "{e}");
}

#[test]
fn a_move_that_forgets_where_the_record_was_fails_the_redraw_oracle() {
    let e = must_fail(mutant::NO_OLD_WIN, 6, |_| {}, |s| {
        s.w.set(s.card_x, 150);
    });
    assert!(e.starts_with("retained != full redraw"), "{e}");
}

#[test]
fn damage_that_reaches_glass_undilated_fails_the_redraw_oracle() {
    // the hidden box is beside the glass, outside it, inside what it reads
    let e = must_fail(mutant::NO_BLUR_DILATE, 6, |s| { s.w.set(s.cover_on, 120u32); }, |s| {
        s.w.set(s.hidden, 0xff00_ff00u32);
    });
    assert!(e.starts_with("retained != full redraw"), "{e}");
}

#[test]
fn a_scroll_without_its_edge_strip_fails_the_redraw_oracle() {
    let e = must_fail(mutant::NO_SCROLL_STRIP, 40, |_| {}, |s| {
        s.w.set(s.scroll, 10);
    });
    assert!(e.starts_with("retained != full redraw"), "{e}");
}

#[test]
fn a_scroll_without_seams_under_fixed_records_fails_the_redraw_oracle() {
    let e = must_fail(mutant::NO_SCROLL_SEAM, 40, |_| {}, |s| {
        s.w.set(s.scroll, 10);
    });
    assert!(e.starts_with("retained != full redraw"), "{e}");
}

#[test]
fn a_dependency_row_that_loses_a_reader_fails_the_fresh_mount_oracle() {
    // `count` is read by every row's label and by the card's: the last reader is never told
    let e = must_fail(mutant::CSR_DROP_LAST, 6, |_| {}, |s| {
        s.w.set(s.count, 77);
    });
    assert!(e.starts_with("retained != fresh mount"), "the stores themselves are stale, so a redraw of them agrees: {e}");
}

#[test]
fn a_window_without_its_effects_fails_the_redraw_oracle() {
    // the shadow grows: the pixels it newly covers are outside the box
    let e = must_fail(mutant::NO_EFFECT_HULL, 6, |_| {}, |s| {
        s.w.set(s.shadow_blur, 23);
    });
    assert!(e.starts_with("retained != full redraw"), "{e}");
    // and a moving shadow leaves itself behind
    let e = must_fail(mutant::NO_EFFECT_HULL, 6, |_| {}, |s| {
        s.w.set(s.card_x, 480);
    });
    assert!(e.starts_with("retained != full redraw"), "{e}");
}

#[test]
fn damage_culled_before_the_glass_between_fails_the_redraw_oracle() {
    // the hidden box is wholly under an opaque cover that is above the glass beside it: its own tiles are
    // rightly skipped, but the glass reads it
    let e = must_fail(mutant::CULL_BEFORE_BLUR, 6, |_| {}, |s| {
        s.w.set(s.hidden, 0xff00_ff00u32);
    });
    assert!(e.starts_with("retained != full redraw"), "{e}");
}

#[test]
fn every_mutant_bit_has_a_test_here() {
    // the list in `bough_damage::mutant`, so a new bit without a control is noticed
    let all = mutant::OCCLUSION_ANY_Z | mutant::NO_OLD_WIN | mutant::NO_BLUR_DILATE | mutant::NO_SCROLL_STRIP | mutant::CSR_DROP_LAST | mutant::NO_EFFECT_HULL | mutant::NO_SCROLL_SEAM | mutant::CULL_BEFORE_BLUR;
    assert_eq!(all, 0xff);
}
