//! A hard line break followed by a combining mark ("a\n\u{300}b": Enter, then an accent). harfrust merges a
//! continuation character into the preceding cluster even when that cluster is a newline, so upstream parley 0.11.1
//! builds a newline cluster of several characters (a debug assertion in `process_clusters`; in release the mark's
//! glyphs, and the newline's .notdef, are drawn on the newline's line). UAX #29 (GB4/GB5) always ends a grapheme
//! after CR / LF, and UAX #14 breaks the line there (LB4-LB6; LB9 does not attach a mark across a break, LB10 makes
//! the orphan mark AL): the mark is a degenerate cluster at the start of the next line. The vendored parley shapes
//! each side of such a break on its own (vendor/parley/src/shape/mod.rs `shape_segment`); upstream still has the
//! bug, so `parley_twin::random_mixed` skips these inputs and this test covers them.
mod common {
    pub mod twin;
}
use common::twin::{dejavu, grafted, Case};
use icu_properties::props::{GeneralCategory, GeneralCategoryGroup};
use icu_properties::CodePointMapData;

fn is_newline(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

fn is_mark(c: char) -> bool {
    GeneralCategoryGroup::Mark.contains(CodePointMapData::<GeneralCategory>::new().get(c))
}

/// (text, lines). parley breaks after `\r` and after `\n` separately (upstream behaviour, the same as for "a\r\nb"),
/// so "\r\n" makes two hard breaks.
const CASES: &[(&str, usize)] = &[
    ("a\n\u{300}b", 2),
    ("a\r\n\u{300}b", 3),
    ("\n\u{300}", 2),
    ("a\n\u{300}\u{301}b", 2),
    ("\u{5E9}\u{5DC}\u{5D5}\u{5DD}\n\u{5B0}\u{5D0}", 2),
    ("x\u{2028}\u{300}y", 2),
    ("a\n\u{300}\n\u{301}b\n", 4),
    ("a\n\u{200D}b", 2),
    // VT, FF and NEL are mandatory breaks to the analysis (the mark after one is split off, its own cluster), but
    // parley's line breaker ends lines at LF / CR / LS / PS only (upstream behaviour, the same as for "a\u{B}b").
    ("a\u{B}\u{300}b", 1),
    ("a\u{C}\u{300}b", 1),
    ("a\u{85}\u{300}b", 1),
    ("a\u{2029}\u{300}b", 2),
];

#[test]
fn newline_then_mark() {
    let mut g = grafted::Eng::new(&dejavu(), true);
    for &(text, lines) in CASES {
        for width in [None, Some(1.0), Some(300.0)] {
            let c = Case::plain(text, "DejaVu Sans", 14.0, width);
            let l = g.layout(&c);
            let ctx = format!("{text:?} at {width:?}");
            assert_eq!(l.len(), lines, "{ctx}: line count");
            // The same text without its marks breaks into the same number of lines.
            let bare: String = text.chars().filter(|&c| !is_mark(c)).collect();
            assert_eq!(g.layout(&Case::plain(&bare, "DejaVu Sans", 14.0, width)).len(), lines, "{ctx}: without marks");

            let mut clusters = Vec::new();
            let mut hard = 0;
            for (li, line) in l.lines().enumerate() {
                let mut glyphs = 0;
                for run in line.runs() {
                    for cl in run.clusters() {
                        let r = cl.text_range();
                        assert!(!r.is_empty(), "{ctx}: empty cluster");
                        assert!(line.text_range().start <= r.start && r.end <= line.text_range().end, "{ctx}: cluster {r:?} outside line {li}");
                        if is_newline(cl.source_char()) {
                            // A newline cluster is the newline alone: no glyphs (it draws nothing), not a ligature.
                            assert!(cl.glyphs().count() == 0 && !cl.is_ligature_start(), "{ctx}: newline cluster {r:?} holds glyphs");
                        }
                        hard += cl.is_hard_line_break() as usize;
                        glyphs += cl.glyphs().count();
                        clusters.push((r, li));
                    }
                }
                // DejaVu Sans has one glyph for every non-newline character here: each is drawn on its own line.
                let drawn = text[line.text_range()].chars().filter(|&c| !is_newline(c)).count();
                assert!(glyphs >= drawn, "{ctx}: line {li} draws {glyphs} glyphs for {drawn} characters");
            }
            assert_eq!(lines, hard + 1, "{ctx}: lines = hard breaks + 1");
            // Every character is covered by exactly one cluster; a mark sits on a later line than the newline before it.
            let mut newline_line = None;
            for (i, ch) in text.char_indices() {
                let hits: Vec<_> = clusters.iter().filter(|(r, _)| r.start <= i && i < r.end).collect();
                assert_eq!(hits.len(), 1, "{ctx}: char {ch:?} at {i} in {} clusters", hits.len());
                let (r, li) = hits[0];
                assert!(r.end == i + ch.len_utf8() || !is_newline(ch), "{ctx}: newline {i} shares cluster {r:?}");
                if is_newline(ch) {
                    newline_line = Some(*li);
                } else if is_mark(ch) {
                    if let Some(nl) = newline_line {
                        assert!(*li > nl, "{ctx}: mark at {i} on the newline's line {nl}");
                    }
                }
            }
        }
    }
}

