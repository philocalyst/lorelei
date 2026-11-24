//! Gate D1 (`designs/reactivity/final/10-quality.md` §4): the author surface's predictable mistakes fail to
//! compile with messages in the design's own vocabulary, and the message text is under snapshot test
//! (`tests/ui/*.stderr`; regenerate with `TRYBUILD=overwrite` and review the diff).
//!
//! Negative control: a snapshot whose text no longer matches fails the gate (remove an
//! `on_unimplemented` note from `el/` and this test fails).
#[test]
fn the_diagnostics_corpus() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/pass_*.rs");
    t.compile_fail("tests/ui/fail_*.rs");
}
