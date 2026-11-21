//! G8, the hot-path half: the inspector is not on it. One hover on the benchmark app, measured by the thread's
//! instruction counter, in a test binary that does not use `bough-inspect` at all — the same test compiles with
//! the `inspect` feature off and on, and the two medians are compared by the runner. The exact zero-delta
//! argument is structural (the off binary links no inspector symbol; `bough-x` is one compiled crate either way);
//! this test prints the numbers that show it.
use bough_guibench_x::rows::Rows;
use bough_inspect_app::lab::Event;
use bough_inspect_app::Lab;

fn median(mut v: Vec<u64>) -> u64 {
    v.sort_unstable();
    v[v.len() / 2]
}

#[test]
fn a_hover_costs_what_the_core_says() {
    let mut lab = Lab::new(Rows::new(3_000), 800, 600, 1);
    // warm: mount, first paint, the first rows
    lab.settle();
    for i in 0..200 {
        lab.input(&Event::Pointer {
            x: 100,
            y: (i % 18) * 32 + 10,
        });
        lab.settle();
    }
    let mut counts = Vec::with_capacity(2_000);
    for i in 0..2_000u32 {
        // two rows change under the pointer: a genuine hover, not a no-op
        let y = (i % 18) as i32 * 32 + 10;
        let (i0, c0) = bough_mem::pmu::instructions_cycles();
        lab.input(&Event::Pointer { x: 100, y });
        lab.settle();
        let (i1, _) = bough_mem::pmu::instructions_cycles();
        let _ = (i0, c0);
        counts.push(i1.saturating_sub(i0));
    }
    let m = median(counts.clone());
    println!(
        "hot-path hover: median {m} instructions (n=2000); pool median {}",
        median(counts)
    );
    // a sanity floor: the core's own sheet hover is about 6,800 instructions; this app's state is smaller
    assert!(m > 100, "{m} instructions is not a turn");
}
