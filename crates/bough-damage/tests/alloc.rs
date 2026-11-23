//! "Every vector is retained: a warm frame allocates nothing." A counting global allocator around frames of
//! every kind the engine has (covers and stacked layers recommitted, damage, moves, a mask, a renumbering, a
//! scroll with seams, a gated resolve, a report into a kept value), after one warm-up pass of the same script.
//! One test function: the counter is the process's.
use bough_damage::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

struct Counting;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static A: Counting = Counting;

fn frame(e: &mut DamageEngine, k: u32, theme: &TileMask, report: &mut DamageReport) -> u32 {
    let shift = (k % 5) as i32 * 7;
    // the driver hands its covers and layers over again (as after a move of one of them)
    e.covers_clear();
    e.cover_push(OpaqueCover { z: 30, core_rect: Rect::new(100 + shift, 300, 500, 600) });
    e.cover_push(OpaqueCover { z: 5, core_rect: Rect::new(0, 0, 1280, 64) });
    e.covers_commit();
    e.layers_clear();
    e.layer_push(BlurLayer { id: 1, z: 20, rect: Rect::new(200, 100, 700, 400), radius: 16, max_level: 4 });
    e.layer_push(BlurLayer { id: 2, z: 25, rect: Rect::new(600 + shift, 300, 900, 600), radius: 8, max_level: 3 });
    e.layer_push(BlurLayer { id: 3, z: 40, rect: Rect::new(0, 0, 1280, 64), radius: 32, max_level: 6 });
    e.layers_commit();
    e.begin_frame();
    match k % 4 {
        0 => {
            e.damage(3, Rect::new(150 + shift, 120, 260, 180), Work::Flat);
            e.damage(31, Rect::new(300, 310, 340, 350), Work::Shadow);
            e.damage_move(12, Rect::new(10, 500, 60, 540), Rect::new(1100, 20 + shift, 1150, 60), Work::Gradient);
        }
        1 => {
            if let Some(b) = e.scroll(8, 15, Rect::new(0, 64, 1280, 800), 0, -11 - shift) {
                e.seam(&b, 40, Rect::new(0, 0, 1280, 64));
                e.seam(&b, 20, Rect::new(200, 100, 700, 400));
            }
            e.damage(9, Rect::new(40, 700, 80, 760), Work::Flat);
        }
        2 => {
            e.damage_mask(theme);
            e.note_fanout(12);
        }
        _ => {
            e.damage(2, Rect::new(600, 350, 640, 380), Work::Flat);
            e.rebase_orders();
            e.damage_virtual(19, Rect::new(220, 120, 260, 140));
        }
    }
    let mut asked = 0u32;
    e.resolve(&mut |l, r| {
        asked += 1;
        if l.id == 2 && asked % 2 == 0 { None } else { Some(Rect { x1: r.x1.min(r.x0 + 40), ..r }) }
    });
    e.report_into(report);
    report.dirty_tiles + e.occluded_tiles_skipped() + e.blur_blocks().len() as u32 + e.blits().len() as u32
}

#[test]
fn a_warm_frame_of_every_kind_allocates_nothing() {
    let mut e = DamageEngine::new(1280, 800);
    let mut theme = TileMask::new(1280, 800);
    for k in 0..30 {
        theme.mark_rect(Rect::xywh(k * 41, (k * 97) % 760, 30, 20));
    }
    let mut report = DamageReport::default();
    let mut work = 0u64;
    // warm-up: every kind once, at every shift (capacities grow to their largest)
    for k in 0..20 {
        work += frame(&mut e, k, &theme, &mut report) as u64;
    }
    let before = ALLOCS.load(Ordering::Relaxed);
    for k in 0..200 {
        work += frame(&mut e, k, &theme, &mut report) as u64;
    }
    let after = ALLOCS.load(Ordering::Relaxed);
    assert!(work > 0 && !report.tiles.is_empty());
    assert_eq!(after - before, 0, "warm frames allocated {} times", after - before);
    // the negative control: a fresh report is a new value, and the counter sees it
    let r = e.report();
    assert!(ALLOCS.load(Ordering::Relaxed) > after && r.dirty_tiles == report.dirty_tiles);
}
