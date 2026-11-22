use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;

thread_local! {
    static TRACK: Cell<Option<(u64, u64)>> = const { Cell::new(None) };
}
struct Counting;
#[global_allocator]
static ALLOC: Counting = Counting;
impl Counting {
    fn count(bytes: usize) {
        let _ = TRACK.try_with(|track| {
            if let Some((n, b)) = track.get() {
                track.set(Some((n.saturating_add(1), b.saturating_add(bytes as u64))));
            }
        });
    }
}
// SAFETY: all calls preserve the System allocator's arguments and return value.
// The counter uses const TLS and neither allocates nor panics for the measured volumes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        Self::count(layout.size());
        // SAFETY: the caller supplies a valid layout and we forward it unchanged.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        Self::count(layout.size());
        // SAFETY: the caller supplies a valid layout and we forward it unchanged.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        Self::count(new_size);
        // SAFETY: ptr and layout came from System, and the caller supplies the new size.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller supplies a live System allocation with its original layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}

pub const W: i32 = 128;
pub const H: i32 = 416;
pub const ROW: f32 = 48.0;
pub const HEADER: f32 = 32.0;
pub const HOT: u32 = 0x4466bb;
pub const LEAF: u32 = 0x227744;
pub fn bg(dark: bool) -> u32 {
    if dark { 0x112233 } else { 0xeeeeee }
}
pub fn scroll_indicator(y: f32) -> u32 {
    if y > 0.0 { 0x33aa55 } else { 0x444444 }
}
pub fn stripe(key: u32) -> u32 {
    if key % 2 == 0 { 0x789abc } else { 0x777777 }
}

#[derive(Clone, Copy, Debug)]
pub enum Op {
    Signal,
    Hover,
    Scroll,
    Insert,
    Theme,
    Idle,
}

pub trait Scene {
    type State;
    fn state(n: usize) -> Self::State;
    fn mount(state: Self::State) -> Self;
    fn op(&mut self, op: Op, k: usize);
    fn paint(&mut self) -> Vec<u32>;
    fn records(&self) -> usize;
}

fn digest(pixels: &[u32]) -> u64 {
    pixels.iter().fold(0xcbf29ce484222325, |h, p| {
        (h ^ u64::from(*p)).wrapping_mul(1099511628211)
    })
}

fn instructions() -> u64 {
    bough::sys::perf::thread_instructions()
}
fn measure<T>(allocations: bool, f: impl FnOnce() -> T) -> (T, (u64, u64, u64)) {
    if allocations {
        TRACK.set(Some((0, 0)));
    }
    let i0 = if allocations { 0 } else { instructions() };
    let value = black_box(f());
    let instr = if allocations { 0 } else { instructions() - i0 };
    let (n, bytes) = TRACK.replace(None).unwrap_or_default();
    (value, (instr, n, bytes))
}

fn median(mut v: Vec<u64>) -> u64 {
    v.sort_unstable();
    v[v.len() / 2]
}
fn summarize(instr: Vec<u64>, alloc: Vec<(u64, u64, u64)>) -> String {
    format!(
        "{}\t{}\t{}",
        if instr.is_empty() {
            "-".to_owned()
        } else {
            median(instr).to_string()
        },
        median(alloc.iter().map(|s| s.1).collect()),
        median(alloc.iter().map(|s| s.2).collect())
    )
}

const OPS: [Op; 6] = [
    Op::Signal,
    Op::Hover,
    Op::Scroll,
    Op::Insert,
    Op::Theme,
    Op::Idle,
];

fn verify<A: Scene>(rows: usize) {
    // Independent rectangles and colors. The two executors need no shared raster to agree.
    fn expected(rows: usize, step: usize, x: usize, y: usize) -> u32 {
        if x < 32 && y < HEADER as usize {
            return if step == 0 { 0x991122 } else { LEAF };
        }
        if (32..48).contains(&x) && y < HEADER as usize {
            return scroll_indicator(if step >= 3 { ROW } else { 0.0 });
        }
        if x < 100 && y >= HEADER as usize {
            let screen_row = (y - HEADER as usize) / ROW as usize;
            let index = screen_row + usize::from(step >= 3);
            let key = if step >= 4 {
                if index == 0 {
                    rows as u32
                } else {
                    (index - 1) as u32
                }
            } else {
                index as u32
            };
            return if step >= 2 && screen_row == 0 {
                HOT
            } else {
                stripe(key)
            };
        }
        bg(step >= 5)
    }
    fn compare(rows: usize, step: usize, pixels: &[u32]) {
        assert_eq!(pixels.len(), (W * H) as usize);
        if let Some((i, c)) = pixels
            .iter()
            .enumerate()
            .find(|(i, c)| **c != expected(rows, step, i % W as usize, i / W as usize))
        {
            panic!(
                "pixel mismatch at step {step}, {},{}: {:06x} vs {:06x}",
                i % W as usize,
                i / W as usize,
                c,
                expected(rows, step, i % W as usize, i / W as usize)
            );
        }
    }
    let mut app = A::mount(A::state(rows));
    let pixels = app.paint();
    compare(rows, 0, &pixels);
    let mut hashes = vec![digest(&pixels)];
    for (index, op) in OPS.into_iter().enumerate() {
        app.op(op, 0);
        let pixels = app.paint();
        compare(rows, index + 1, &pixels);
        hashes.push(digest(&pixels));
    }
    println!(
        "# {rows} pixel hashes {hashes:016x?} live records {}",
        app.records()
    );
}

pub fn run<A: Scene>() {
    let verify_only = std::env::args().any(|arg| arg == "--verify");
    let allocations_only = std::env::args().any(|arg| arg == "--allocations-only");
    if verify_only {
        for rows in [1_000, 100_000] {
            verify::<A>(rows);
        }
        return;
    }
    let (value, (_, n, b)) = measure(true, || vec![0u8; black_box(23)]);
    assert_eq!((value.len(), n, b), (23, 1, 23));
    if !allocations_only {
        assert!(instructions() > 0, "thread instruction counter unavailable");
    }
    let passes: &[bool] = if allocations_only {
        &[true]
    } else {
        &[false, true]
    };
    println!("rows\toperation\tthread_instructions\talloc_or_realloc\trequested_bytes");
    for rows in [1_000, 100_000] {
        let mut instr = Vec::new();
        let mut alloc = Vec::new();
        for &count in passes {
            for k in 0..17 {
                let state = A::state(rows);
                let (app, sample) = measure(count, || A::mount(state));
                black_box(&app);
                if k >= 4 {
                    if count {
                        alloc.push(sample);
                    } else {
                        instr.push(sample.0);
                    }
                }
            }
        }
        println!("{rows}\tMount\t{}", summarize(instr, alloc));
        verify::<A>(rows);
        for op in OPS {
            let mut instr = Vec::new();
            let mut alloc = Vec::new();
            for &count in passes {
                let mut app = A::mount(A::state(rows));
                // Warm ring and absolute offsets before sampling either instrumentation mode.
                for k in 0..70 {
                    let (_, sample) = measure(count, || app.op(op, k));
                    if k >= 10 {
                        if count {
                            alloc.push(sample);
                        } else {
                            instr.push(sample.0);
                        }
                    }
                }
            }
            println!("{rows}\t{op:?}\t{}", summarize(instr, alloc));
        }
    }
}
