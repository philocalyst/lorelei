//! The flat core's warm paths allocate nothing: a counting global allocator around turns and frames of each
//! kind, after a warm-up of the same kinds. One test function: the counter is the process's.
mod common;
use bough_mem::count::{Counting, allocs};
use bough_flat::*;
use common::*;

#[global_allocator]
static A: Counting = Counting;

fn run(s: &mut Scene, kind: usize, k: u32) {
    match kind {
        // a signal update: a state word with many readers (every label), three digits wide
        0 => {
            s.w.set(s.count, 100 + (k * 37) % 900);
        }
        // hover from row to row
        1 => {
            let (_, y) = s.row_center((k % 8) as usize);
            s.w.pointer_move(40, y);
        }
        // scroll, to and fro and far
        2 => {
            let at = [7, 19, 3, 160, 164, 90, 0, 33][(k % 8) as usize];
            s.w.set(s.scroll, at);
        }
        // theme switch
        3 => {
            s.w.set_theme(k % 2);
        }
        // selection (an expression per row)
        4 => {
            s.w.set(s.selected, k % 9);
        }
        // a card with shadows moving under glass, and its shadow growing
        5 => {
            s.w.set(s.card_x, 200 + ((k * 53) % 300) as i32);
            s.w.set(s.shadow_blur, 4 + k % 16);
        }
        // an animation in flight: enter the header, tick, leave, tick
        6 => {
            if k % 12 == 0 {
                s.w.pointer_move(500, 50);
            } else if k % 12 == 6 {
                s.w.pointer_leave();
            }
            let t = s.w.get(s.w.time_slot());
            s.w.set_time(t + 16);
        }
        // clip_overflow toggled, and what is under covers and glass recoloured
        _ => {
            s.w.set(s.clip, k % 2 == 0);
            s.w.set(s.buried, 0xff00_0000 | k * 977);
            s.w.set(s.hidden, 0xff00_0000 | k * 131);
        }
    }
}

#[test]
fn warm_turns_and_frames_allocate_nothing() {
    const NAMES: [&str; 8] = ["signal update", "hover", "scroll", "theme switch", "selection", "moving shadow under glass", "animation", "clip_overflow and hidden colours"];
    let mut s = Scene::new(300);
    s.w.frame();
    // warm-up: every kind, twice round, so every retained vector has reached its size
    for round in 0..2 {
        for kind in 0..NAMES.len() {
            for k in 0..24 {
                run(&mut s, kind, k + round);
                s.w.frame();
            }
        }
    }
    let mut seen = [0usize; 8];
    let mut frames = [0u32; 8];
    for kind in 0..NAMES.len() {
        let before = allocs();
        let mut busy = 0;
        for k in 0..48 {
            run(&mut s, kind, k);
            let i = s.w.frame();
            busy += !i.idle as u32;
        }
        seen[kind] = allocs() - before;
        frames[kind] = busy;
    }
    for kind in 0..NAMES.len() {
        assert!(frames[kind] >= 24, "{}: only {} frames did any work", NAMES[kind], frames[kind]);
        assert_eq!(seen[kind], 0, "{}: {} allocations over 48 warm turns and frames", NAMES[kind], seen[kind]);
    }
    // several turns to a frame, the kinds interleaved: once to let the damage list reach the size four turns
    // need, then measured
    let interleaved = |s: &mut Scene| {
        for k in 0..64 {
            run(s, (k % 6) as usize, k);
            s.w.settle();
            if k % 4 == 3 {
                s.w.frame();
            }
        }
    };
    interleaved(&mut s);
    let before = allocs();
    interleaved(&mut s);
    interleaved(&mut s);
    assert_eq!(allocs() - before, 0, "interleaved turns");
    // and the result is still right (checked last: the oracles allocate)
    s.w.check(&mut NoExt).expect("the scene still holds its oracles");
    // the counter counts: a cold edit does allocate
    let before = allocs();
    s.push_row();
    s.w.frame();
    assert!(allocs() - before > 0);
}
