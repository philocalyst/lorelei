//! Gate G1 of the incremental-operators card: every operator's output equals a recompute from scratch after
//! every step of a random edit sequence, and the positional journal it emitted, replayed onto a mirror of the
//! output as it was, names every position whose content is now another (so the deltas are right, not only
//! the state). `--features mutant_ops` and `--features mutant_lane` must fail this file.
use bough_dataflow::ops::{Count, Filter, Group, Join, Members, Sort, Sum, Total};
use bough_dataflow::rel::{Seq, Table};
use bough_dataflow::sink::{Edit, Journal};

fn rng(s: &mut u64) -> u64 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    *s
}

#[derive(Clone, Debug, PartialEq)]
struct Row {
    v: i64,
    g: u8,
}

/// A mirror of an operator's output that is told only what the journal says.
struct Mirror<T> {
    seen: u32,
    items: Vec<Option<T>>,
}
impl<T: Clone + PartialEq + core::fmt::Debug> Mirror<T> {
    fn new(j: &Journal, now: Vec<T>) -> Self {
        Mirror {
            seen: j.ver,
            items: now.into_iter().map(Some).collect(),
        }
    }
    /// Replay the journal's edits since the last check, then compare with the output as it is now: every
    /// position the journal did not name must hold what the mirror holds.
    fn check(&mut self, j: &Journal, now: &[T], what: &str) {
        let pending = j.ver.wrapping_sub(self.seen);
        if pending > 8 {
            // more than a journal keeps: a list re-describes its whole window
            self.items = vec![None; now.len()];
        } else {
            for k in (0..pending).rev() {
                match j.log[j.ver.wrapping_sub(1 + k) as usize & 7] {
                    Edit::Insert(at) => self.items.insert(at as usize, None),
                    Edit::Remove(at) => {
                        self.items.remove(at as usize);
                    }
                    Edit::Update(at) => self.items[at as usize] = None,
                    Edit::Shift(at) => {
                        self.items.truncate(at as usize);
                        self.items.resize(now.len().max(at as usize), None);
                    }
                }
            }
        }
        self.seen = j.ver;
        assert_eq!(
            self.items.len(),
            now.len(),
            "{what}: the journal's inserts and removes give another length"
        );
        for (i, (m, n)) in self.items.iter().zip(now).enumerate() {
            if let Some(m) = m {
                assert_eq!(
                    m, n,
                    "{what}: position {i} changed and the journal did not say so"
                );
            }
        }
        self.items = now.iter().cloned().map(Some).collect();
    }
}

fn pass(r: &Row, cut: i64) -> bool {
    r.v % 7 < cut
}

#[test]
fn operators_equal_a_recompute_after_every_step() {
    for seed in 1..=12u64 {
        let mut s = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
        // a small log on odd seeds: batches overflow it and the operators must fall back
        let cap = if seed % 2 == 1 { 6 } else { 1024 };
        let mut t: Table<Row> = Table::from_vec(
            (0..(seed as i64 * 23) % 200)
                .map(|i| Row {
                    v: i * 37 % 101,
                    g: (i % 5) as u8,
                })
                .collect(),
        )
        .with_log(cap);
        let mut u: Table<Row> = Table::new().with_log(cap);
        let mut cut = 4i64;
        let (mut filt, mut sort, mut fsort) =
            (Filter::new(), Sort::<i64>::new(), Sort::<i64>::new());
        let (mut grp, mut tot, mut cnt, mut join) = (
            Group::<u8, Sum>::new(),
            Total::<Sum>::new(),
            Total::<Count>::new(),
            Join::<u8>::new(),
        );
        let sync_all = |t: &Table<Row>,
                        u: &Table<Row>,
                        cut: i64,
                        filt: &mut Filter,
                        sort: &mut Sort<i64>,
                        fsort: &mut Sort<i64>,
                        grp: &mut Group<u8, Sum>,
                        tot: &mut Total<Sum>,
                        cnt: &mut Total<Count>,
                        join: &mut Join<u8>| {
            filt.sync(t, |r| pass(r, cut));
            sort.sync(t, |id| t.row(id).v);
            // a chain: the filter's own log feeds a sort
            fsort.sync(&*filt, |id| -t.row(id).v);
            grp.sync(t, |id| (t.row(id).g, t.row(id).v));
            tot.sync(&*filt, |id| t.row(id).v);
            cnt.sync(t, |_| ());
            join.sync(t, u, |id| t.row(id).g, |id| u.row(id).g);
        };
        sync_all(
            &t, &u, cut, &mut filt, &mut sort, &mut fsort, &mut grp, &mut tot, &mut cnt, &mut join,
        );
        let mut m_filt = Mirror::new(&filt.journal, filt.ids().collect());
        let mut m_sort = Mirror::new(&sort.journal, sort.ids().collect());
        let mut m_fsort = Mirror::new(&fsort.journal, fsort.ids().collect());
        let mut m_grp = Mirror::new(&grp.journal, grp.rows().cloned().collect());
        let mut m_join = Mirror::new(&join.journal, join.pairs().collect());
        for step in 0..1500u32 {
            // one edit, or a batch (which a small log does not keep whole)
            let batch = if rng(&mut s) % 9 == 0 {
                2 + rng(&mut s) % 12
            } else {
                1
            };
            for _ in 0..batch {
                let r = rng(&mut s);
                let tab = if r >> 40 & 3 == 0 { &mut u } else { &mut t };
                let ids: Vec<u32> = tab.ids().collect();
                match r % 10 {
                    0..=3 => {
                        tab.insert(Row {
                            v: (r >> 8) as i64 % 101,
                            g: (r >> 20) as u8 % 5,
                        });
                    }
                    4..=5 if !ids.is_empty() => {
                        tab.remove(ids[(r >> 8) as usize % ids.len()]);
                    }
                    6..=8 if !ids.is_empty() => {
                        let id = ids[(r >> 8) as usize % ids.len()];
                        let row = tab.edit(id).unwrap();
                        match r >> 30 & 3 {
                            0 => row.v = (r >> 12) as i64 % 101,
                            1 => row.g = (r >> 12) as u8 % 5,
                            // an edit that changes nothing: noted all the same
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            // now and then the predicate itself changes
            match rng(&mut s) % 40 {
                0 => {
                    filt.sync(&t, |r| pass(r, cut));
                    cut = if cut > 1 { cut - 1 } else { 6 };
                    if cut < 6 {
                        filt.narrow(&t, |r| pass(r, cut));
                    } else {
                        filt.refilter(&t, |r| pass(r, cut), t.ids());
                    }
                }
                1 => {
                    // without a sync first: the candidates cannot be trusted and the filter recomputes
                    t.insert(Row {
                        v: (rng(&mut s) % 101) as i64,
                        g: 0,
                    });
                    cut = 1 + (rng(&mut s) % 6) as i64;
                    let cur: Vec<u32> = filt.ids().collect();
                    filt.refilter(&t, |r| pass(r, cut), cur.into_iter());
                }
                _ => {}
            }
            sync_all(
                &t, &u, cut, &mut filt, &mut sort, &mut fsort, &mut grp, &mut tot, &mut cnt,
                &mut join,
            );

            // ---- the recompute, written without the operators' code
            let want_filt: Vec<u32> = t
                .iter()
                .filter(|(_, r)| pass(r, cut))
                .map(|(id, _)| id)
                .collect();
            let mut want_sort: Vec<(i64, u32)> = t.iter().map(|(id, r)| (r.v, id)).collect();
            want_sort.sort();
            let mut want_fsort: Vec<(i64, u32)> =
                want_filt.iter().map(|&id| (-t.row(id).v, id)).collect();
            want_fsort.sort();
            let mut want_grp: Vec<(u8, u32, i64)> = Vec::new();
            for g in 0..5u8 {
                let rows: Vec<&Row> = t.iter().map(|(_, r)| r).filter(|r| r.g == g).collect();
                if !rows.is_empty() {
                    want_grp.push((g, rows.len() as u32, rows.iter().map(|r| r.v).sum()));
                }
            }
            let mut want_join: Vec<(u32, u32)> = Vec::new();
            for (a, ra) in t.iter() {
                for (b, rb) in u.iter() {
                    if ra.g == rb.g {
                        want_join.push((a, b));
                    }
                }
            }
            let at = format!("seed {seed} step {step}");
            let got: Vec<u32> = filt.ids().collect();
            assert_eq!(got, want_filt, "filter, {at}");
            assert_eq!(filt.len(), want_filt.len());
            for (i, &id) in want_filt.iter().enumerate().take(40) {
                assert_eq!(
                    (filt.get(i), filt.position(id)),
                    (id, Some(i)),
                    "filter by position, {at}"
                );
            }
            let mut n = 0;
            filt.each(&mut |id| {
                assert!(filt.contains(id));
                n += 1
            });
            assert_eq!(n, want_filt.len());
            assert_eq!(
                sort.ids().collect::<Vec<_>>(),
                want_sort.iter().map(|e| e.1).collect::<Vec<_>>(),
                "sort, {at}"
            );
            assert_eq!(
                fsort.ids().collect::<Vec<_>>(),
                want_fsort.iter().map(|e| e.1).collect::<Vec<_>>(),
                "sort of a filter, {at}"
            );
            if let Some(&(k, id)) = want_sort.first() {
                assert_eq!(
                    (*sort.key(0), sort.get(0), sort.position(id)),
                    (k, id, Some(0))
                );
            }
            let got_grp: Vec<(u8, u32, i64)> =
                grp.rows().map(|r| (r.key, r.count, r.acc)).collect();
            assert_eq!(got_grp, want_grp, "group, {at}");
            assert_eq!(
                tot.value(),
                want_filt.iter().map(|&id| t.row(id).v).sum::<i64>(),
                "total of a filter, {at}"
            );
            assert_eq!(
                (tot.count() as usize, cnt.count() as usize),
                (want_filt.len(), t.len()),
                "counts, {at}"
            );
            assert_eq!(join.pairs().collect::<Vec<_>>(), want_join, "join, {at}");

            // ---- the deltas
            m_filt.check(&filt.journal, &got, &format!("filter journal, {at}"));
            m_sort.check(
                &sort.journal,
                &sort.ids().collect::<Vec<_>>(),
                &format!("sort journal, {at}"),
            );
            m_fsort.check(
                &fsort.journal,
                &fsort.ids().collect::<Vec<_>>(),
                &format!("filter-sort journal, {at}"),
            );
            m_join.check(&join.journal, &want_join, &format!("join journal, {at}"));
            // a group row's content is its count and fold too: an update must be said
            m_grp.check(
                &grp.journal,
                &grp.rows().cloned().collect::<Vec<_>>(),
                &format!("group journal, {at}"),
            );
        }
        // the fallback was taken on the small log and never needed on the large one after the first build
        if cap == 6 {
            assert!(
                sort.work.rebuilds > 1,
                "a batch larger than the log must recompute"
            );
        } else {
            assert_eq!(
                sort.work.rebuilds, 1,
                "only the first build recomputes when the log keeps every batch"
            );
        }
    }
}

/// A sort's content at a position is its row: an update of a row that keeps its key keeps its position and is
/// said as an update, so the list patches one row.
#[test]
fn an_update_that_keeps_its_place_is_one_update() {
    let mut t = Table::from_vec((0..1000).map(|i| Row { v: i, g: 0 }).collect());
    let mut sort = Sort::<i64>::new();
    sort.sync(&t, |id| t.row(id).v);
    let seen = sort.journal.ver;
    t.edit(500).unwrap().g = 3;
    sort.sync(&t, |id| t.row(id).v);
    assert_eq!(sort.journal.ver.wrapping_sub(seen), 1);
    assert_eq!(
        sort.journal.since(seen),
        (u32::MAX, [500, 0, 0, 0, 0, 0, 0, 0], 1)
    );
    // and one that moves is a remove and an insert, at the two places
    let seen = sort.journal.ver;
    t.edit(500).unwrap().v = -1;
    sort.sync(&t, |id| t.row(id).v);
    assert_eq!(sort.journal.since(seen).0, 0);
    assert_eq!((sort.get(0), sort.work.rows), (500, 1000 + 2));
}

/// Growth (G6): operators hold what the live rows need, however long the churn.
#[test]
fn churn_leaves_nothing_behind() {
    let mut t: Table<Row> = Table::from_vec(
        (0..500)
            .map(|i| Row {
                v: i,
                g: (i % 5) as u8,
            })
            .collect(),
    );
    let mut u: Table<Row> = Table::from_vec(
        (0..20)
            .map(|i| Row {
                v: i,
                g: (i % 5) as u8,
            })
            .collect(),
    );
    let (mut filt, mut sort, mut grp, mut join) = (
        Filter::new(),
        Sort::<i64>::new(),
        Group::<u8, Sum>::new(),
        Join::<u8>::new(),
    );
    let mut s = 99u64;
    let mut sizes = Vec::new();
    for round in 0..1000 {
        for _ in 0..8 {
            let ids: Vec<u32> = t.ids().collect();
            t.remove(ids[rng(&mut s) as usize % ids.len()]);
            t.insert(Row {
                v: (rng(&mut s) % 1000) as i64,
                g: (rng(&mut s) % 5) as u8,
            });
            let uid: Vec<u32> = u.ids().collect();
            u.edit(uid[rng(&mut s) as usize % uid.len()]).unwrap().g = (rng(&mut s) % 7) as u8;
        }
        filt.sync(&t, |r| r.v % 2 == 0);
        sort.sync(&t, |id| t.row(id).v);
        grp.sync(&t, |id| (t.row(id).g, t.row(id).v));
        join.sync(&t, &u, |id| t.row(id).g, |id| u.row(id).g);
        if round == 9 || round == 999 {
            sizes.push((
                t.slots(),
                t.log.len(),
                sort.len(),
                sort.bytes(),
                grp.len(),
                join.index_keys(),
                filt.log.len(),
            ));
        }
    }
    let (a, b) = (sizes[0], sizes[1]);
    assert_eq!(
        (a.0, a.2, a.4),
        (b.0, b.2, b.4),
        "slots, sorted ids and groups follow the live rows"
    );
    assert!(b.1 <= 1024 && b.6 <= 1024, "logs are bounded");
    assert!(
        b.3 <= a.3 * 2,
        "the order lane's blocks stay within a band: {} then {}",
        a.3,
        b.3
    );
    assert!(
        b.5 <= 5 + 7,
        "the join's index holds only keys live rows have"
    );
}

/// A `Seq` is a `Rel` for a list: the same journal, the same answers.
#[test]
fn a_seq_and_a_rel_tell_a_list_the_same() {
    use bough_dataflow::sink::Rel;
    let mut s = 5u64;
    let (mut a, mut b) = (
        Seq::new((0..400u32).collect()),
        Rel::new((0..400u32).collect()),
    );
    for step in 0..3000u32 {
        let r = rng(&mut s);
        let n = a.len();
        match r % 3 {
            0 => {
                let at = (r >> 8) as usize % (n + 1);
                a.insert(at, step);
                b.insert(at, step);
            }
            1 if n > 0 => {
                let at = (r >> 8) as usize % n;
                assert_eq!(a.remove(at), b.remove(at));
            }
            _ if n > 0 => {
                let at = (r >> 8) as usize % n;
                *a.edit(at) = step;
                *b.edit(at) = step;
            }
            _ => {}
        }
        assert_eq!(
            (a.journal.ver, a.journal.log),
            (b.journal.ver, b.journal.log)
        );
        assert_eq!(a.len(), b.len());
    }
    assert!(a.items().iter().eq(b.items().iter()));
}
