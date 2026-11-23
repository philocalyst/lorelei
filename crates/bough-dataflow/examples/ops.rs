//! The incremental-operators card, counted: instructions and cycles of the calling thread per step, bracket
//! subtracted, median of `REPS` steps (as `sheet`, `anim` and `text` count theirs).
//!
//!   REPS=64 cargo run --profile dist -p bough-dataflow --example ops            (every part)
//!   PART=select|lane|ops|search|corpus ...                                 (one part)
//!
//! Parts: `select` (a selection change and a pointer event on the grid fixture, declared against opaque forms,
//! at two exposures), `lane` (`Seq` against `Rel`), `ops` (one edit through each operator against its
//! recompute), `search` (the search fixture on the operator against the scanning one, key by key, through the
//! world), `corpus` (the substring operator alone on a repetitive and a unique corpus at three sizes, with the
//! index's bytes against the text's).
#[path = "../scenes/grid.rs"]
mod grid;
#[path = "../scenes/search.rs"]
mod search;
#[path = "../scenes/search_ops.rs"]
mod search_ops;
use bough_mem::pmu;
use bough_dataflow::index::Substring;
use bough_dataflow::ops::{Filter, Group, Join, Sort, Sum};
use bough_dataflow::prelude::*;
use bough_dataflow::rel::{Seq, Table};
use bough_dataflow::soft::Soft;
use std::hint::black_box;

struct Meter {
    bracket: [u64; 2],
    reps: usize,
}
impl Meter {
    fn new(reps: usize) -> Meter {
        let mut v: Vec<[u64; pmu::MAX]> = (0..64)
            .map(|_| {
                let a = pmu::sample();
                pmu::sample().since(&a).v
            })
            .collect();
        let mut bracket = [0u64; 2];
        for (i, b) in bracket.iter_mut().enumerate() {
            v.sort_by_key(|s| s[i]);
            *b = v[v.len() / 2][i];
        }
        Meter { bracket, reps }
    }
    /// One measured step: instructions and cycles.
    fn one(&self, f: impl FnOnce()) -> (u64, u64) {
        let a = pmu::sample();
        f();
        let d = pmu::sample().since(&a);
        (
            d.v[0].saturating_sub(self.bracket[0]),
            d.v[1].saturating_sub(self.bracket[1]),
        )
    }
    /// `step(k)` `reps` times; prints the median and the 95th percentile.
    fn phase(&self, part: &str, name: &str, mut step: impl FnMut(usize)) -> u64 {
        let mut v: Vec<(u64, u64)> = (0..self.reps).map(|k| self.one(|| step(k))).collect();
        v.sort();
        let (m, p95) = (v[v.len() / 2], v[(v.len() * 95 / 100).min(v.len() - 1)]);
        println!(
            "ops\t{part}\t{name}\t{}\t{}\t{}\t{}",
            self.reps, m.0, p95.0, m.1
        );
        m.0
    }
}
fn env(name: &str, d: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(d)
}
fn rng(s: &mut u64) -> u64 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    *s
}

fn select(m: &Meter) {
    use grid::*;
    fn run<const D: bool>(m: &Meter, h: i32) {
        let tag = if D { "declared" } else { "opaque" };
        let mut g = Grid::<D>::new(1_000_000, h);
        let mut w = World::<_, Soft>::new(W, h, NWORDS);
        w.mount(&g);
        w.tiles.clear();
        let (records, pages, _) = w.root().stats();
        let rows = (h / ROW_H) as u32;
        println!(
            "# select {tag} h={h}: {records} records in {pages} pages; {} keyed and {} pointer readers",
            rows * 8,
            rows * 8
        );
        // warm: two changes (the scan, then the index)
        for k in 0..2 {
            g.u.set::<Sel>(Grid::<D>::cell(k, 0));
            w.settle(&g);
            w.pointer_words(600 + k as i32, 100);
        }
        w.tiles.clear();
        m.phase("select", &format!("{tag} h={h} selection change"), |k| {
            g.u.set::<Sel>(Grid::<D>::cell(k as u32 * 7 % rows, k as u32 % 8));
            w.settle(&g);
            black_box(w.tiles.count());
            w.tiles.clear();
        });
        m.phase("select", &format!("{tag} h={h} pointer event"), |k| {
            w.pointer_words(700 + (k as i32 % 16) * 9, 100 + (k as i32 % 5) * 7);
            black_box(w.tiles.count());
            w.tiles.clear();
        });
        m.phase(
            "select",
            &format!("{tag} h={h} pointer event far from every reader"),
            |k| {
                w.pointer_words(-4000 - k as i32, -4000);
                w.tiles.clear();
            },
        );
        // the fallback, forced: a list pass before every change
        m.phase(
            "select",
            &format!("{tag} h={h} scroll 1 row then selection change"),
            |k| {
                g.scroll_by(if k % 2 == 0 { ROW_H } else { -ROW_H });
                g.u.set::<Sel>(Grid::<D>::cell(k as u32 % rows, 1));
                w.settle(&g);
                w.tiles.clear();
            },
        );
        m.phase("select", &format!("{tag} h={h} scroll 1 row"), |k| {
            g.scroll_by(if k % 2 == 0 { ROW_H } else { -ROW_H });
            w.settle(&g);
            w.tiles.clear();
        });
        m.phase("select", &format!("{tag} h={h} theme"), |_| {
            let t = g.u.get::<Theme>() ^ 1;
            g.u.set::<Theme>(t);
            w.settle(&g);
            w.tiles.clear();
        });
        if D {
            println!("# select declared h={h}: {:?}", w.fast_stats);
        }
    }
    for h in [2048, 16384] {
        run::<false>(m, h);
        run::<true>(m, h);
    }
}

fn lane(m: &Meter) {
    type Row = [f32; 10];
    for n in [10_000usize, 100_000, 1_000_000] {
        let rows: Vec<Row> = (0..n).map(|r| [r as f32; 10]).collect();
        let (mut rel, mut seq) = (Rel::new(rows.clone()), Seq::new(rows));
        m.phase(
            "lane",
            &format!("Rel n={n} insert + remove in the middle"),
            |k| {
                rel.insert(n / 2 + k, [0.0; 10]);
                black_box(rel.remove(n / 2));
            },
        );
        m.phase(
            "lane",
            &format!("Seq n={n} insert + remove in the middle"),
            |k| {
                seq.insert(n / 2 + k, [0.0; 10]);
                black_box(seq.remove(n / 2));
            },
        );
        m.phase("lane", &format!("Rel n={n} insert at the front"), |_| {
            rel.insert(0, [0.0; 10]);
        });
        m.phase("lane", &format!("Seq n={n} insert at the front"), |_| {
            seq.insert(0, [0.0; 10]);
        });
        // a window of 50 rows read in order, somewhere else each time (what a list pass reads)
        m.phase("lane", &format!("Rel n={n} read a window of 50"), |k| {
            let at = k * 7919 % (n - 50);
            let mut s = 0.0;
            for r in at..at + 50 {
                s += rel.get(r)[3];
            }
            black_box(s);
        });
        m.phase("lane", &format!("Seq n={n} read a window of 50"), |k| {
            let at = k * 7919 % (n - 50);
            let mut s = 0.0;
            for r in at..at + 50 {
                s += seq.get(r)[3];
            }
            black_box(s);
        });
        println!(
            "# lane n={n}: Seq holds {} bytes in {} blocks; Rel holds {}",
            seq.items().bytes(),
            seq.items().blocks(),
            rel.items().len() * size_of::<Row>()
        );
    }
}

struct Item {
    v: i64,
    g: u16,
}
fn ops(m: &Meter) {
    for n in [10_000usize, 100_000, 1_000_000] {
        let mut s = 42u64;
        let mut t: Table<Item> = Table::from_vec(
            (0..n)
                .map(|_| Item {
                    v: (rng(&mut s) % 1_000_000) as i64,
                    g: (rng(&mut s) % 200) as u16,
                })
                .collect(),
        );
        let mut u: Table<Item> =
            Table::from_vec((0..200).map(|i| Item { v: i, g: i as u16 }).collect());
        let (mut f, mut so, mut g, mut j) = (
            Filter::new(),
            Sort::<i64>::new(),
            Group::<u16, Sum>::new(),
            Join::<u16>::new(),
        );
        let full = m.one(|| f.rebuild(&t, |r| r.v % 3 == 0)).0;
        println!("ops\tops\tFilter n={n} recompute\t1\t{full}\t{full}\t0");
        let full = m.one(|| so.rebuild(&t, |id| t.row(id).v)).0;
        println!("ops\tops\tSort n={n} recompute\t1\t{full}\t{full}\t0");
        let full = m.one(|| g.rebuild(&t, |id| (t.row(id).g, t.row(id).v))).0;
        println!("ops\tops\tGroup n={n} recompute\t1\t{full}\t{full}\t0");
        // the join is of a tenth of the table, or its output is the table times a constant
        let small: Table<Item> = Table::from_vec(
            (0..n / 10)
                .map(|i| Item {
                    v: i as i64,
                    g: (i % 2000) as u16,
                })
                .collect(),
        );
        let mut small = small;
        let full = m
            .one(|| j.rebuild(&small, &u, |id| small.row(id).g, |id| u.row(id).g))
            .0;
        println!(
            "ops\tops\tJoin n={} x 200 recompute ({} pairs)\t1\t{full}\t{full}\t0",
            n / 10,
            j.len()
        );
        m.phase("ops", &format!("Filter n={n} one row edited"), |k| {
            let id = (k * 7919 % n) as u32;
            t.edit(id).unwrap().v += 1;
            f.sync(&t, |r| r.v % 3 == 0);
        });
        m.phase(
            "ops",
            &format!("Sort n={n} one row edited (it moves)"),
            |k| {
                let id = (k * 7919 % n) as u32;
                t.edit(id).unwrap().v = (k as i64 * 104_729) % 1_000_000;
                so.sync(&t, |id| t.row(id).v);
            },
        );
        m.phase(
            "ops",
            &format!("Group n={n} one row edited (another group)"),
            |k| {
                let id = (k * 7919 % n) as u32;
                t.edit(id).unwrap().g = (k % 200) as u16;
                g.sync(&t, |id| (t.row(id).g, t.row(id).v));
            },
        );
        m.phase(
            "ops",
            &format!("Filter n={n} a row inserted and one removed"),
            |k| {
                let id = t.insert(Item {
                    v: k as i64 * 3,
                    g: 0,
                });
                t.remove(id - 1 - (k as u32 % 50));
                f.sync(&t, |r| r.v % 3 == 0);
            },
        );
        m.phase(
            "ops",
            &format!("Join n={} x 200 one left row's key edited", n / 10),
            |k| {
                let id = (k * 7919 % (n / 10)) as u32;
                small.edit(id).unwrap().g = (k % 200) as u16;
                j.sync(&small, &u, |id| small.row(id).g, |id| u.row(id).g);
            },
        );
        u.edit(0).unwrap().v = 1;
        black_box((f.len(), so.len(), g.len(), j.len()));
    }
}

fn search_part(m: &Meter, n: usize) {
    use search::{FIELD, H, NWORDS, W};
    macro_rules! drive {
        ($tag:expr, $v:expr) => {{
            let mut v = $v;
            let mut world = World::<_, Soft>::new(W, H, NWORDS);
            world.mount(&v);
            world.click(&mut v, FIELD.x0 + 20, FIELD.y0 + 10);
            world.settle(&v);
            world.tiles.clear();
            let tag: &str = $tag;
            // key by key: every character of a query, then every backspace back to nothing
            for q in ["amber river", "ivory"] {
                for (i, c) in q.chars().enumerate() {
                    let c = c.to_string();
                    let (ins, _) = m.one(|| {
                        world.text(&mut v, &c);
                        world.settle(&v);
                    });
                    world.tiles.clear();
                    println!("ops\tsearch\t{tag} n={n} type {:?} (needle {:?}, {} hits)\t1\t{ins}\t{ins}\t0", c, &q[..=i], v.hits.len());
                }
                for i in (0..q.len()).rev() {
                    let (ins, _) = m.one(|| {
                        world.key_down(&mut v, Key::Backspace, Mods::default());
                        world.settle(&v);
                    });
                    world.tiles.clear();
                    println!("ops\tsearch\t{tag} n={n} backspace (needle {:?}, {} hits)\t1\t{ins}\t{ins}\t0", &q[..i], v.hits.len());
                }
            }
            // text's two phases, as it runs them
            for t in ["quick ", "river"] {
                world.text(&mut v, t);
                world.settle(&v);
            }
            world.tiles.clear();
            m.phase("search", &format!("{tag} n={n} type a character / backspace at 'quick river'"), |k| {
                if k % 2 == 0 {
                    world.text(&mut v, ["s", "e", "a", "o"][(k / 2) % 4]);
                } else {
                    world.key_down(&mut v, Key::Backspace, Mods::default());
                }
                world.settle(&v);
                world.tiles.clear();
            });
            // a middle edit: the caret goes left of "river" and a character is typed and deleted there
            for _ in 0..5 {
                world.key_down(&mut v, Key::Left, Mods::default());
            }
            m.phase("search", &format!("{tag} n={n} middle edit: type / backspace inside the needle"), |k| {
                if k % 2 == 0 {
                    world.text(&mut v, "x");
                } else {
                    world.key_down(&mut v, Key::Backspace, Mods::default());
                }
                world.settle(&v);
                world.tiles.clear();
            });
            // a dataset edit under a selective needle: one item relabelled
            m.phase("search", &format!("{tag} n={n} dataset edit under 'quick river'"), |k| {
                v.relabel((k * 7919 % n) as u32, search::label(k as u32 * 31));
                world.settle(&v);
                world.tiles.clear();
            });
            for _ in 0..40 {
                world.key_down(&mut v, Key::Backspace, Mods::default());
            }
            world.settle(&v);
            world.tiles.clear();
            m.phase("search", &format!("{tag} n={n} narrow by 'z' to nothing, widen by a backspace to everything"), |k| {
                if k % 2 == 0 {
                    world.text(&mut v, "z");
                } else {
                    world.key_down(&mut v, Key::Backspace, Mods::default());
                }
                world.settle(&v);
                world.tiles.clear();
            });
            v
        }};
    }
    drive!("scan", search::Search::new(n));
    let v = drive!("operator", search_ops::Search::new(n));
    let text: usize = v.all.iter().map(|(_, it)| it.label.len()).sum();
    println!(
        "# search n={n}: text {text} bytes; index {} bytes ({} postings, {} trigrams); result and kept {} bytes; index/text {:.2}",
        v.hits.index().bytes(),
        v.hits.index().postings(),
        v.hits.index().trigrams(),
        v.hits.bytes() - v.hits.index().bytes(),
        v.hits.index().bytes() as f64 / text as f64
    );
}

fn corpus(m: &Meter) {
    fn text(r: &String) -> &str {
        r
    }
    for unique in [false, true] {
        for n in [10_000usize, 100_000, 1_000_000] {
            let mut s = 7u64;
            let rows: Vec<String> = (0..n as u32)
                .map(|id| {
                    if unique {
                        // 12 to 28 letters of 20: nearly every string is its own
                        (0..12 + rng(&mut s) % 17)
                            .map(|_| (b'a' + (rng(&mut s) % 20) as u8) as char)
                            .collect()
                    } else {
                        search::label(id)
                    }
                })
                .collect();
            // a needle that some row holds: six characters of row 17
            let needle: String = rows[17].chars().skip(2).take(6).collect();
            let bytes: usize = rows.iter().map(|r| r.len()).sum();
            let mut t = Table::from_vec(rows);
            let mut q = Substring::new();
            let build = m.one(|| q.sync(&t, text)).0;
            let tag = if unique { "unique" } else { "repetitive" };
            println!("ops\tcorpus\t{tag} n={n} build the index\t1\t{build}\t{build}\t0");
            let scan = m
                .one(|| {
                    black_box(t.iter().filter(|(_, r)| r.contains(&needle)).count());
                })
                .0;
            println!(
                "ops\tcorpus\t{tag} n={n} a scan with String::contains ({:?})\t1\t{scan}\t{scan}\t0",
                needle
            );
            for len in 1..=needle.len() {
                let (ins, _) = m.one(|| q.set_needle(&t, text, &needle[..len]));
                println!(
                    "ops\tcorpus\t{tag} n={n} needle {:?}: {:?}, {} verified, {} hits\t1\t{ins}\t{ins}\t0",
                    &needle[..len],
                    q.route,
                    q.verified,
                    q.len()
                );
            }
            for len in (0..needle.len()).rev() {
                let (ins, _) = m.one(|| q.set_needle(&t, text, &needle[..len]));
                println!(
                    "ops\tcorpus\t{tag} n={n} back to {:?}: {:?}, {} verified, {} hits\t1\t{ins}\t{ins}\t0",
                    &needle[..len],
                    q.route,
                    q.verified,
                    q.len()
                );
            }
            // a replacement: a needle with nothing in common with the last
            q.set_needle(&t, text, &needle);
            let other: String = t.row(4242 % n as u32).chars().skip(1).take(5).collect();
            let (ins, _) = m.one(|| q.set_needle(&t, text, &other));
            println!(
                "ops\tcorpus\t{tag} n={n} replaced by {:?}: {:?}, {} verified, {} hits\t1\t{ins}\t{ins}\t0",
                other,
                q.route,
                q.verified,
                q.len()
            );
            q.set_needle(&t, text, &needle);
            m.phase(
                "corpus",
                &format!("{tag} n={n} dataset edit under the needle"),
                |k| {
                    let id = (k * 7919 % n) as u32;
                    let mut r = t.row(id).clone();
                    r.push('q');
                    *t.edit(id).unwrap() = r;
                    q.sync(&t, text);
                },
            );
            println!(
                "# corpus {tag} n={n}: text {bytes} bytes; index {} bytes ({} postings, {} trigrams); index/text {:.2}",
                q.index().bytes(),
                q.index().postings(),
                q.index().trigrams(),
                q.index().bytes() as f64 / bytes as f64
            );
        }
    }
}

fn main() {
    let m = Meter::new(env("REPS", 64));
    let part = std::env::var("PART").unwrap_or_default();
    println!("scene\tpart\tphase\tsteps\tinstr_p50\tinstr_p95\tcycles_p50");
    if part.is_empty() || part == "select" {
        select(&m);
    }
    if part.is_empty() || part == "lane" {
        lane(&m);
    }
    if part.is_empty() || part == "ops" {
        ops(&m);
    }
    if part.is_empty() || part == "search" {
        search_part(&m, env("ROWS", 100_000));
    }
    if part.is_empty() || part == "corpus" {
        corpus(&m);
    }
}
