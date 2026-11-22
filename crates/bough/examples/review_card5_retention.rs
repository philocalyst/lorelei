//! Text retention through maintenance boundaries and 10,000 mixed turns.
use bough::{prelude::*, typed::App};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default)]
struct Heap {
    allocations: u64,
    requested: usize,
    net: isize,
}
thread_local! {
    static HEAP: Cell<Option<Heap>> = const { Cell::new(None) };
}

fn account(requested: Option<usize>, removed: usize) {
    let _ = HEAP.try_with(|track| {
        if let Some(mut heap) = track.get() {
            if let Some(bytes) = requested {
                heap.allocations += 1;
                heap.requested += bytes;
                heap.net += bytes as isize;
            }
            heap.net -= removed as isize;
            track.set(Some(heap));
        }
    });
}

struct Counting;
#[global_allocator]
static ALLOCATOR: Counting = Counting;
// SAFETY: each call forwards exactly the caller's pointer and layout to `System`. The TLS counter does not
// allocate and counts only completed operations.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller supplies the layout and we forward it unchanged.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            account(Some(layout.size()), 0);
        }
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller supplies the layout and we forward it unchanged.
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            account(Some(layout.size()), 0);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        // SAFETY: the caller's pointer and original layout are forwarded to the allocating system.
        unsafe { System.dealloc(p, layout) };
        account(None, layout.size());
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // SAFETY: the original allocation and size request are forwarded to the allocating system.
        let next = unsafe { System.realloc(p, layout, size) };
        if !next.is_null() {
            account(Some(size), layout.size());
        }
        next
    }
}

struct Item(u32);
impl View for Item {
    fn view(&self) -> impl Element<Self> + '_ {
        row((text(self.0).font(13.0, 20.0),))
            .w(80.0)
            .h(20.0)
            .bg(rgb(0x223344))
            .hover_bg(rgb(0x334455))
    }
}

struct Model {
    title: Signal<Color>,
    rows: List<Item>,
    scroll: Signal<f32>,
}
impl View for Model {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            block(80.0, 20.0).bg(sig!(self.title)),
            list(sig!(self.rows))
                .scroll(sig!(self.scroll))
                .estimate(20.0)
                .w(80.0)
                .h(160.0),
        ))
        .w(80.0)
        .h(180.0)
    }
}

fn app(n: u32) -> App<Model> {
    let mut cfg = bough::app::Config::new(80.0, 180.0, 1.0, bough::ui::default_font());
    cfg.ring = 4;
    App::new(
        Model {
            title: Signal::new(rgb(0x778899)),
            rows: List::from_iter_in((0..n).map(|i| Item(i * 3 + 100))),
            scroll: Signal::new(0.0),
        },
        cfg,
    )
    .unwrap_or_else(|(_, why)| panic!("refused: {why:?}"))
}

fn maintenance() {
    let mut app = app(1_000);
    for _ in 0..4 {
        app.frame();
    }
    let expected = app.presented_pixels();
    assert!(
        expected
            .chunks_exact(4)
            .any(|p| p == [255, 255, 255, 255] || p == [0, 0, 0, 255])
    );
    let bracket = bough::sys::perf::tinstr_overhead();
    let mut steady = Vec::new();
    for turn in 1..=650 {
        let before = bough::sys::perf::thread_instructions();
        app.turn(|_| {});
        let raw = bough::sys::perf::thread_instructions() - before;
        let p = app.pass_stats();
        if p.rows == 0 {
            steady.push(raw.saturating_sub(bracket));
        } else {
            println!(
                "maintenance\t{turn}\t{}\t{}\t{}\t{}",
                app.describes(),
                raw.saturating_sub(bracket),
                p.rows,
                p.lookups
            );
            assert!(!app.frame(), "unchanged rows damaged at turn {turn}");
            assert_eq!(app.presented_pixels(), expected);
        }
    }
    steady.sort_unstable();
    println!(
        "steady\t{}\t{}\tmax\t{}",
        steady.len(),
        steady[steady.len() / 2],
        steady.last().unwrap()
    );
    assert_eq!(
        steady.len(),
        650,
        "unchanged descriptions must not revisit the rows"
    );
    // An independent app has a different run store. No oracle touches the measured run store until here.
    let mut fresh = self::app(1_000);
    fresh.frame();
    assert_eq!(app.reference_pixels(), fresh.presented_pixels());
}

fn mixed(n: u32) {
    let mut app = app(n);
    app.frame();
    let mut peak = 0;
    let mut retained = 0;
    println!(
        "mixed rows\tturns\talloc_or_realloc\trequested bytes\tnet bytes cumulative\tpeak net bytes\tlive records\tpages\tworld bytes"
    );
    for end in (1_000..=10_000).step_by(1_000) {
        let mut window = Heap::default();
        for turn in end - 1_000..end {
            HEAP.set(Some(Heap::default()));
            match turn % 8 {
                0 => app.turn(|_| {}),
                1 => app.pointer_move(8.0, 26.0 + 20.0 * ((turn / 8) % 3) as f32),
                2 => app.turn(|v| {
                    v.title.set(rgb(0x334455 + ((turn / 8) % 4) as u32));
                }),
                3 => app.wheel(8.0, 30.0, 0.0, 1.0),
                4 => app.turn(|v| {
                    v.rows.remove(0);
                    v.rows.insert(0, Item(901 + ((turn / 8) % 8) as u32));
                }),
                5 => app.turn(|v| {
                    v.scroll.set(0.0);
                }),
                6 => app.turn(|v| v.rows.edit(0).0 ^= 1),
                _ => app.turn(|v| v.rows.swap(1, 2)),
            }
            let heap = HEAP.replace(None).unwrap();
            window.allocations += heap.allocations;
            window.requested += heap.requested;
            window.net += heap.net;
            retained += heap.net;
            peak = peak.max(retained);
        }
        let (records, pages, bytes) = app.world_stats();
        println!(
            "{n}\t{end}\t{}\t{}\t{retained}\t{peak}\t{records}\t{pages}\t{bytes}",
            window.allocations, window.requested
        );
    }
    app.check().unwrap();
}

fn main() {
    maintenance();
    for n in [1_000, 100_000] {
        mixed(n);
    }
}
