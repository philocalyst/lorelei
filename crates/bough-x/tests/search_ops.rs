//! Gate G3 of the incremental-operators card: the substring operator's hits equal a naive scan after every
//! step of needle edits (append, backspace, middle edit, replacement, short and empty needles) interleaved
//! with dataset edits, on a repetitive corpus and on one of unique strings; the journal names every position
//! that changed; the index stays within a band of an exact one under churn. `--features mutant_tri` must fail.
#[path = "../scenes/search.rs"]
mod search;
#[path = "../scenes/search_ops.rs"]
mod search_ops;
use bough_x::index::{Route, Substring, Trigrams, holds};
use bough_x::prelude::*;
use bough_x::rel::Table;
use bough_x::sink::Edit;
use bough_x::soft::Soft;

fn rng(s: &mut u64) -> u64 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    *s
}
const ADJ: [&str; 8] = [
    "quick", "amber", "bright", "calm", "eager", "fluffy", "grand", "humble",
];
const COL: [&str; 6] = ["red", "green", "blue", "violet", "copper", "ivory"];
const NOUN: [&str; 8] = [
    "river", "office", "stone", "field", "harbour", "meadow", "lantern", "orchard",
];
fn repetitive(id: u32) -> String {
    format!(
        "{} {} {}",
        ADJ[(id * 7 % 8) as usize],
        COL[(id * 13 % 6) as usize],
        NOUN[(id * 17 % 8) as usize]
    )
}
fn unique(s: &mut u64) -> String {
    let n = 6 + rng(s) % 18;
    (0..n)
        .map(|_| (b'a' + (rng(s) % 6) as u8) as char)
        .collect::<String>()
        + "é"
}

fn text(r: &String) -> &str {
    r
}
fn naive(t: &Table<String>, needle: &str) -> Vec<u32> {
    t.iter()
        .filter(|(_, r)| r.contains(needle))
        .map(|(id, _)| id)
        .collect()
}

fn run(seed: u64, uniq: bool, n: u32, steps: u32) {
    let mut s = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    let mut t: Table<String> = Table::from_vec(
        (0..n)
            .map(|id| if uniq { unique(&mut s) } else { repetitive(id) })
            .collect(),
    );
    let mut q = Substring::new();
    q.sync(&t, text);
    assert_eq!(q.ids().collect::<Vec<_>>(), naive(&t, ""));
    let mut needle = String::new();
    let mut mirror: Vec<Option<u32>> = q.ids().map(Some).collect();
    let mut seen = q.journal().ver;
    let mut routes = [0u32; 5];
    for step in 0..steps {
        let r = rng(&mut s);
        match r % 12 {
            // append: a character of some row, so the needle often still matches
            0..=3 => {
                let ids: Vec<u32> = t.ids().collect();
                let row = t.row(ids[(r >> 8) as usize % ids.len()]);
                let cs: Vec<char> = row.chars().collect();
                if needle.is_empty() || r >> 30 & 1 == 0 {
                    needle.push(cs[(r >> 16) as usize % cs.len()]);
                } else if let Some(at) = row.find(&needle) {
                    // the character that follows a match: the needle goes on matching
                    if let Some(c) = row[at + needle.len()..].chars().next() {
                        needle.push(c)
                    }
                }
            }
            4..=5 => {
                needle.pop();
            }
            // a middle edit
            6 if needle.chars().count() >= 2 => {
                let cs: Vec<char> = needle.chars().collect();
                let at = (r >> 8) as usize % cs.len();
                needle = cs
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != at)
                    .map(|(_, c)| *c)
                    .collect();
            }
            // a replacement: a slice of some row
            7 => {
                let ids: Vec<u32> = t.ids().collect();
                let cs: Vec<char> = t.row(ids[(r >> 8) as usize % ids.len()]).chars().collect();
                let a = (r >> 20) as usize % cs.len();
                let b = (a + 1 + (r >> 28) as usize % 6).min(cs.len());
                needle = cs[a..b].iter().collect();
            }
            8 => needle.clear(),
            // dataset edits
            9 => {
                t.insert(if uniq {
                    unique(&mut s)
                } else {
                    repetitive((r >> 8) as u32)
                });
            }
            10 => {
                let ids: Vec<u32> = t.ids().collect();
                if ids.len() > 4 {
                    t.remove(ids[(r >> 8) as usize % ids.len()]);
                }
            }
            _ => {
                let ids: Vec<u32> = t.ids().collect();
                let id = ids[(r >> 8) as usize % ids.len()];
                *t.edit(id).unwrap() = if uniq {
                    unique(&mut s)
                } else {
                    repetitive((r >> 16) as u32)
                };
            }
        }
        if r >> 50 & 7 == 0 {
            // a dataset edit taken in with no change of the needle
            q.sync(&t, text);
        } else {
            q.set_needle(&t, text, &needle);
            routes[q.route as usize] += 1;
        }
        let want = naive(&t, core::str::from_utf8(q.needle()).unwrap());
        let got: Vec<u32> = q.ids().collect();
        assert_eq!(
            got, want,
            "seed {seed} unique {uniq} step {step} needle {needle:?} route {:?}",
            q.route
        );
        // the journal: every position it did not name still holds what it held
        let j = q.journal();
        let pending = j.ver.wrapping_sub(seen);
        if pending > 8 {
            mirror = vec![None; got.len()];
        } else {
            for k in (0..pending).rev() {
                match j.log[j.ver.wrapping_sub(1 + k) as usize & 7] {
                    Edit::Insert(at) => mirror.insert(at as usize, None),
                    Edit::Remove(at) => {
                        mirror.remove(at as usize);
                    }
                    Edit::Update(at) => mirror[at as usize] = None,
                    Edit::Shift(at) => {
                        mirror.truncate(at as usize);
                        mirror.resize(got.len().max(at as usize), None);
                    }
                }
            }
        }
        seen = j.ver;
        assert_eq!(mirror.len(), got.len(), "step {step}: journal length");
        for (i, (m, g)) in mirror.iter().zip(&got).enumerate() {
            assert!(
                m.is_none_or(|m| m == *g),
                "step {step}: position {i} changed unsaid"
            );
        }
        mirror = got.iter().copied().map(Some).collect();
    }
    // every route was taken: the gate covers each of them
    assert!(
        routes[Route::Narrow as usize] > 0
            && routes[Route::Kept as usize] > 0
            && routes[Route::Index as usize] > 0
            && routes[Route::Scan as usize] > 0,
        "routes {routes:?}"
    );
}

#[test]
fn hits_equal_a_scan_on_a_repetitive_corpus() {
    for seed in 1..=4 {
        run(seed, false, 600, 2500);
    }
}
#[test]
fn hits_equal_a_scan_on_unique_strings() {
    for seed in 5..=8 {
        run(seed, true, 400, 2500);
    }
}

#[test]
fn holds_is_contains() {
    let mut s = 3u64;
    for _ in 0..20_000 {
        let h: Vec<u8> = (0..rng(&mut s) % 12)
            .map(|_| b'a' + (rng(&mut s) % 3) as u8)
            .collect();
        let n: Vec<u8> = (0..rng(&mut s) % 5)
            .map(|_| b'a' + (rng(&mut s) % 3) as u8)
            .collect();
        let want = n.is_empty() || h.windows(n.len().max(1)).any(|w| w == &n[..]);
        assert_eq!(holds(&h, &n), want, "{h:?} {n:?}");
    }
}

/// The candidates are a superset of the rows that hold the needle, whatever was edited (the index's contract),
/// and the index's size stays within a band of an exact index of the same rows (G6).
#[test]
fn the_index_is_a_superset_and_stays_in_its_band() {
    let mut s = 11u64;
    let mut rows: Vec<String> = (0..2000).map(|_| unique(&mut s)).collect();
    let mut tri = Trigrams::new();
    tri.rebuild(
        rows.iter()
            .enumerate()
            .map(|(i, r)| (i as u32, r.as_bytes())),
    );
    let exact0 = tri.postings();
    let mut sizes = Vec::new();
    for round in 0..1000 {
        for _ in 0..20 {
            let id = rng(&mut s) as usize % rows.len();
            rows[id] = unique(&mut s);
            if tri.edit(id as u32, rows[id].as_bytes()) {
                tri.rebuild(
                    rows.iter()
                        .enumerate()
                        .map(|(i, r)| (i as u32, r.as_bytes())),
                );
            }
        }
        if round == 9 || round == 999 {
            sizes.push((tri.postings(), tri.bytes()));
        }
        if round % 50 == 0 {
            for _ in 0..40 {
                let r = &rows[rng(&mut s) as usize % rows.len()];
                let a = rng(&mut s) as usize % (r.len() - 4);
                let needle = &r.as_bytes()[a..a + 3 + rng(&mut s) as usize % 2];
                let c = tri.candidates(needle).unwrap();
                assert!(c.windows(2).all(|w| w[0] < w[1]), "ascending, each once");
                for (id, row) in rows.iter().enumerate() {
                    if holds(row.as_bytes(), needle) {
                        assert!(
                            c.binary_search(&(id as u32)).is_ok(),
                            "row {id} holds the needle and is no candidate"
                        );
                    }
                }
            }
        }
    }
    assert!(tri.rebuilds > 2, "churn must have rebuilt the index");
    for (p, _) in &sizes {
        assert!(
            *p as f64 <= exact0 as f64 * 1.4,
            "postings {p} against an exact {exact0}"
        );
    }
    assert!(tri.candidates(b"zzz").unwrap().is_empty() && tri.candidates(b"ab").is_none());
}

/// The search fixture on the operator paints what the scanning fixture paints, after the same keys, wheel
/// steps and dataset edits; and its retained frame is its full redraw and a fresh mount's.
#[test]
fn the_operator_fixture_paints_what_the_scanning_fixture_paints() {
    use search::{FIELD, H, NWORDS, W};
    let n = 3000;
    let (mut a, mut b) = (search::Search::new(n), search_ops::Search::new(n));
    let (mut wa, mut wb) = (
        World::<_, Soft>::new(W, H, NWORDS),
        World::<_, Soft>::new(W, H, NWORDS),
    );
    wa.mount(&a);
    wb.mount(&b);
    wa.click(&mut a, FIELD.x0 + 20, FIELD.y0 + 10);
    wb.click(&mut b, FIELD.x0 + 20, FIELD.y0 + 10);
    let mut retained = Soft::new(W, H);
    let mut s = 0xabcdu64;
    let keys = [
        "a", "m", "b", "e", "r", " ", "i", "v", "o", "q", "u", "z", "re", "st",
    ];
    for step in 0..500 {
        let r = rng(&mut s);
        match r % 10 {
            0..=3 => {
                let k = keys[(r >> 8) as usize % keys.len()];
                wa.text(&mut a, k);
                wb.text(&mut b, k);
            }
            4..=5 => {
                wa.key_down(&mut a, Key::Backspace, Mods::default());
                wb.key_down(&mut b, Key::Backspace, Mods::default());
            }
            6 => {
                let k = [Key::Left, Key::Right, Key::Home, Key::End][(r >> 8) as usize % 4];
                wa.key_down(&mut a, k, Mods::default());
                wb.key_down(&mut b, k, Mods::default());
            }
            7 => {
                let (id, label) = (
                    (r >> 8) as u32 % n as u32,
                    search::label((r >> 30) as u32 % 4000),
                );
                a.relabel(id, label.clone());
                b.relabel(id, label);
            }
            _ => {
                let dy = [40.0, -40.0, 400.0][(r >> 8) as usize % 3];
                wa.wheel(&mut a, 300, 300, 0.0, dy);
                wb.wheel(&mut b, 300, 300, 0.0, dy);
            }
        }
        wa.settle(&a);
        wb.settle(&b);
        assert_eq!(
            a.hits.items(),
            &b.hits.ids().collect::<Vec<_>>()[..],
            "step {step}: hits"
        );
        retained.repair(&wb.tiles, |s| wb.paint(s));
        wa.tiles.clear();
        wb.tiles.clear();
        let (mut fa, mut fb) = (Soft::new(W, H), Soft::new(W, H));
        wa.paint(&mut fa);
        wb.paint(&mut fb);
        assert!(
            fa.px == fb.px,
            "step {step}: the two fixtures paint different pixels"
        );
        assert!(retained.px == fb.px, "step {step}: retained != full redraw");
        if step % 50 == 0 {
            let mut w2 = World::<_, Soft>::new(W, H, NWORDS);
            w2.mount(&b);
            let mut f = Soft::new(W, H);
            w2.paint(&mut f);
            // focus is the world's: compare the body, below the header
            let from = (search::HEADER * W) as usize;
            assert!(
                f.px[from..] == fb.px[from..],
                "step {step}: retained != fresh mount"
            );
        }
    }
}

/// A backspace returns to a result the operator kept, across two typing sessions (the kept results are
/// bounded by the table's size in ids, oldest dropped first).
#[test]
fn a_backspace_is_answered_from_a_kept_result() {
    let n = 20_000u32;
    let t: Table<String> = Table::from_vec((0..n).map(search::label).collect());
    let mut q = Substring::new();
    let mut routes = Vec::new();
    let mut ask = |q: &mut Substring, s: &str| {
        q.set_needle(&t, text, s);
        assert_eq!(q.ids().collect::<Vec<_>>(), naive(&t, s), "needle {s:?}");
        routes.push((s.to_string(), q.route, q.kept_ids()));
    };
    for word in ["amber river", "ivory"] {
        for i in 1..=word.len() {
            ask(&mut q, &word[..i]);
        }
        for i in (0..word.len()).rev() {
            ask(&mut q, &word[..i]);
        }
    }
    for (s, r, kept) in &routes {
        assert!(*kept <= n as usize, "kept ids {kept} over the table's {n}");
        let _ = (s, r);
    }
    let back: Vec<_> = routes.iter().rev().take(5).rev().collect();
    // back through "ivor", "ivo", "iv": kept; "i" and "" are over half the table and are scanned for
    assert_eq!(
        back.iter().map(|b| b.1).collect::<Vec<_>>(),
        [
            Route::Kept,
            Route::Kept,
            Route::Kept,
            Route::Scan,
            Route::Scan
        ]
    );
}
