//! G7, the idle-refresh half: an inspector refresh that finds nothing moved allocates nothing, with and without
//! a selection in the overlay. This is a test binary of its own so no other test's allocations are counted.
use bough_guibench_x::rows::Rows;
use bough_inspect::session::Session;
use bough_inspect::ui::Inspector;
use bough_inspect_app::Lab;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
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

#[test]
fn idle_refresh_allocates_nothing() {
    let mut s = Session::new(Lab::new(Rows::new(2_000), 800, 600, 1));
    let snap = s.snapshot().clone();
    let mut insp = Inspector::new(snap);
    for (selected, index) in [(false, 0usize), (true, 7)] {
        if selected {
            insp.select(index);
        }
        let _ = s.snapshot(); // one warm-up refresh, so the cache key is current
        ALLOCS.store(0, Ordering::Relaxed);
        for _ in 0..128 {
            let _ = s.snapshot();
        }
        let n = ALLOCS.load(Ordering::Relaxed);
        assert_eq!(
            n, 0,
            "selected={selected}: {n} allocations over 128 idle refreshes"
        );
    }
    // a change does rebuild: the gate above is not vacuously true (a mutation's own digest may warm the cache,
    // so invalidate and take one)
    s.set("hot", &bough_inspect::PropValue::Int(2)).unwrap();
    s.invalidate();
    ALLOCS.store(0, Ordering::Relaxed);
    let _ = s.snapshot();
    assert!(
        ALLOCS.load(Ordering::Relaxed) > 0,
        "a changed world rebuilds the description"
    );
}
