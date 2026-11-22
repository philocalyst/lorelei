//! TASK 1: a leaf's site is the line the leaf was written on, not the row/list call it shares.
//!
//! The view below writes three leaves on three lines. The test asserts each reported `file:line:col` against
//! *this file's own text* (the same shape as the review's `rows_source_points_at_the_real_description_call_site`).
//! A mutant (`mutant_same_site`) makes every leaf the row's site; the gate must fail with it on.
use bough_dataflow::el::App;
use bough_dataflow::el::prelude::*;
use bough_dataflow::reflect::Reflect;
use bough_dataflow::soft::Soft;

const SRC: &str = include_str!("leaf_sites.rs");

struct Leaves;
impl View for Leaves {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            text("alpha"),
            text("beta"),
            div().w(40.0).h(12.0).bg(rgb(0x102030)),
        ))
        .w_pct(1.0)
        .h_pct(1.0)
        .bg(rgb(0x445566))
    }
}

/// The expected `(line, column)` of the first occurrence of `needle` in this test's own source.
fn site(needle: &str) -> (u32, u32) {
    for (i, line) in SRC.lines().enumerate() {
        if let Some(c) = line.find(needle) {
            return ((i + 1) as u32, (c + 1) as u32);
        }
    }
    panic!("{needle:?} is not in this test's source");
}

#[test]
fn sibling_leaves_report_their_own_written_lines() {
    let app = App::<Leaves, Soft>::new(Leaves, 240, 120, 1.0);
    let r = Reflect::learn(&app.world, &app.v);
    let items = r.items(app.world.root(), &app.world.shown);
    // The reflected text is the first content operand, at most twelve bytes.
    let by_text = |want: &str| {
        items
            .iter()
            .find(|i| i.text().as_deref() == Some(want))
            .unwrap_or_else(|| {
                panic!(
                    "{want} was not described: {:?}",
                    items.iter().map(|i| i.text()).collect::<Vec<_>>()
                )
            })
    };
    let a = by_text("alpha");
    let b = by_text("beta");
    assert_eq!(
        (
            a.location().file(),
            a.location().line(),
            a.location().column()
        ),
        (
            file!(),
            site("text(\"alpha\")").0,
            site("text(\"alpha\")").1
        ),
        "the first text leaf names its own call site"
    );
    assert_eq!(
        (
            b.location().file(),
            b.location().line(),
            b.location().column()
        ),
        (file!(), site("text(\"beta\")").0, site("text(\"beta\")").1),
        "the second text leaf names its own call site"
    );
    assert_ne!(
        a.location().line(),
        b.location().line(),
        "two leaves written on two lines do not share one location"
    );
    // A painted box's record names its `.bg(..)` call. (A method call's caller column is the method name.)
    let root_bg = site(".bg(rgb(0x445566))").0;
    let fill = items
        .iter()
        .find(|i| i.kind() == "fill" && i.location().line() != root_bg)
        .expect("the inner painted box is a record");
    let (line, col) = site(".bg(rgb(0x102030))");
    assert_eq!(
        (
            fill.location().file(),
            fill.location().line(),
            fill.location().column()
        ),
        (file!(), line, col + 1),
        "the box's fill names the `.bg(..)` that wrote it, got {:?}",
        fill.location()
    );
    // Distinct leaves have distinct sites.
    let mut lines: Vec<u32> = items
        .iter()
        .filter(|i| i.kind() == "label" || i.kind() == "fill")
        .map(|i| i.location().line())
        .collect();
    lines.sort_unstable();
    lines.dedup();
    assert!(lines.len() >= 3, "at least three distinct sites: {lines:?}");
}
