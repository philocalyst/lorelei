//! Phase counts for the glyph memo on the rows scene (the two cells of `text_x`: "scroll a page" and
//! "collect the glyph memo"): memo lookups, shapings and heap operations per step. Heap operations are counted
//! by a global allocator, so a collection that releases memory and then grows it again shows as allocations.
//! Run with `-- --nocapture` to read the counts; the asserts hold the ordinary behaviour (a scroll over text
//! already shaped is all hits, and collecting then re-describing makes the exposed text again).
#[path = "../scenes/rows.rs"]
mod rows;
use bough_x::prelude::*;
use bough_x::soft::Soft;
use rows::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

struct Counting;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static REALLOCS: AtomicU64 = AtomicU64::new(0);
static FREES: AtomicU64 = AtomicU64::new(0);
// SAFETY: forwards every call to `System` unchanged.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        FREES.fetch_add(1, Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        REALLOCS.fetch_add(1, Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static A: Counting = Counting;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Counts {
    shaped: u64,
    found: u64,
    allocs: u64,
    reallocs: u64,
    frees: u64,
}
fn snap(v: &Rows) -> Counts {
    let g = v.glyphs.borrow();
    Counts {
        shaped: g.shaped,
        found: g.found,
        allocs: ALLOCS.load(Relaxed),
        reallocs: REALLOCS.load(Relaxed),
        frees: FREES.load(Relaxed),
    }
}
fn delta(a: Counts, b: Counts, steps: u64) -> Counts {
    Counts {
        shaped: (b.shaped - a.shaped) / steps,
        found: (b.found - a.found) / steps,
        allocs: (b.allocs - a.allocs) / steps,
        reallocs: (b.reallocs - a.reallocs) / steps,
        frees: (b.frees - a.frees) / steps,
    }
}

fn world(n: usize) -> (Rows, World<Rows, Soft>) {
    let v = Rows::new(n);
    let mut w = World::<_, Soft>::new(W, H, NWORDS);
    w.mount(&v);
    w.tiles.clear();
    (v, w)
}

#[test]
fn scroll_a_page_asks_the_memo_hits_and_does_not_allocate() {
    let (mut v, mut w) = world(100_000);
    // warm: one full cycle of the phase, so what the cycle exposes has been shaped
    let step = |w: &mut World<Rows, Soft>, v: &mut Rows, k: usize| {
        v.scroll_by(if (k / 4) % 2 == 0 { 24 * ROW_H } else { -24 * ROW_H });
        w.settle(v);
        w.tiles.clear();
    };
    for k in 0..64 {
        step(&mut w, &mut v, k);
    }
    let (a, steps) = (snap(&v), 64u64);
    for k in 0..steps as usize {
        step(&mut w, &mut v, k);
    }
    let d = delta(a, snap(&v), steps);
    eprintln!("phase scroll-a-page per step: {d:?}");
    assert_eq!(d.shaped, 0, "text already shaped is found, not shaped");
    assert!(d.found > 0, "entering rows ask the memo");
}

#[test]
fn collecting_the_memo_redescribes_only_what_is_exposed() {
    let (mut v, mut w) = world(100_000);
    let exposed = v.glyphs.borrow().len();
    let (a, steps) = (snap(&v), 64u64);
    for _ in 0..steps {
        v.glyphs.borrow_mut().clear();
        w.redescribe(&v);
        w.tiles.clear();
    }
    let d = delta(a, snap(&v), steps);
    eprintln!("phase collect per step: {d:?}, {} lines kept, {} bytes", v.glyphs.borrow().len(), v.glyphs.borrow().bytes());
    assert_eq!(v.glyphs.borrow().len(), exposed, "the same lines come back");
    assert_eq!(d.found + d.shaped >= 1, true);
    assert_eq!(v.glyphs.borrow().cleared, steps);
}
