//! Collection operators: derived relations kept up to date from deltas.
//!
//! Each operator owns its output and a cursor into its input's [`Log`]. `sync` reads what the input changed
//! since the operator last ran and changes the output by that much: delta in, delta out. The output is ids (or
//! pairs, or groups) in an order lane, so a position is found and an insert costs one block; what the output
//! changed goes two ways: by id into the operator's own log (for an operator downstream) and by position into
//! a [`Journal`] (for the list that shows it). A list materialises only the rows it exposes, so an output of
//! a million ids costs a million words and the window's records.
//!
//! Nothing is subscribed. An operator is a value in the application's state; the handler that edited the
//! table, or the view's owner before a turn settles, calls `sync`. An operator that fell behind what a log
//! keeps recomputes from its input, which is also the definition every gate compares against.
//!
//! | operator | output | cost of one input change |
//! |---|---|---|
//! | [`Filter`] | ids that pass, ascending | one predicate, `O(log n)` to place, one block moved |
//! | [`Sort`] | ids by key | one key, `O(log n)` compares, one block moved |
//! | [`Group`] | one row per key with a count and a fold | one key, `O(log groups)`, a fold step |
//! | [`Total`] | one fold of everything | a fold step |
//! | [`Join`] | pairs with equal keys | the pairs that row takes part in |
//! | `index::Substring` | ids whose text holds the needle | see `crate::index` |
use crate::rel::{Change, Lane, Log, Table};
use crate::sink::{Edit, Journal};
use std::collections::HashMap;
use std::hash::Hash;

/// A set of ids with a log of its changes: what an operator reads.
pub trait Members {
    fn log(&self) -> &Log;
    fn contains(&self, id: u32) -> bool;
    /// Every member, ascending by id where the set has an order of its own.
    fn each(&self, f: &mut dyn FnMut(u32));
}
impl<T> Members for Table<T> {
    fn log(&self) -> &Log {
        &self.log
    }
    #[inline(always)]
    fn contains(&self, id: u32) -> bool {
        self.get(id).is_some()
    }
    fn each(&self, f: &mut dyn FnMut(u32)) {
        for id in self.ids() {
            f(id)
        }
    }
}

/// Work an operator has done since it was made: what the count gates read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    /// Rows whose predicate, key or text was evaluated.
    pub rows: u64,
    /// Times the output was recomputed from the whole input.
    pub rebuilds: u64,
}

#[inline(always)]
fn bit(m: &[u64], id: u32) -> bool {
    m.get(id as usize >> 6)
        .is_some_and(|w| w >> (id & 63) & 1 != 0)
}
#[inline(always)]
fn set_bit(m: &mut Vec<u64>, id: u32, on: bool) {
    let w = id as usize >> 6;
    if w >= m.len() {
        m.resize(w + 1, 0);
    }
    if on {
        m[w] |= 1 << (id & 63)
    } else {
        m[w] &= !(1 << (id & 63))
    }
}

// ---- filter ------------------------------------------------------------------------------------------------

/// The longest scratch list an operator keeps between changes (entries): what a change of a small result
/// allocates is reused, a larger list is freed after use.
const SCRATCH: usize = 4096;

/// The rows of a table that pass a predicate, as ids in ascending order.
pub struct Filter {
    member: Vec<u64>,
    out: Lane<u32>,
    seen: u64,
    pub log: Log,
    pub journal: Journal,
    pub work: Work,
    /// Scratch of `refilter` and `narrow` (the next output) and of `replace` (its changes), kept while small.
    next: Vec<u32>,
    changes: Vec<Change>,
}
impl Default for Filter {
    fn default() -> Self {
        Filter::new()
    }
}
impl Filter {
    pub fn new() -> Filter {
        Filter {
            member: Vec::new(),
            out: Lane::new(),
            seen: 0,
            log: Log::new(Log::DEFAULT_CAP),
            journal: Journal::new(),
            work: Work::default(),
            next: Vec::new(),
            changes: Vec::new(),
        }
    }
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.out.len()
    }
    pub fn is_empty(&self) -> bool {
        self.out.is_empty()
    }
    /// The id at position `i` of the output.
    #[inline(always)]
    pub fn get(&self, i: usize) -> u32 {
        *self.out.get(i)
    }
    pub fn ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.out.iter().copied()
    }
    /// The output's ids, in order, appended to `out`.
    pub fn ids_into(&self, out: &mut Vec<u32>) {
        self.out.copy_to(out)
    }
    /// The position of `id` in the output, if it is a member.
    pub fn position(&self, id: u32) -> Option<usize> {
        bit(&self.member, id).then(|| self.out.lower_bound(|&x| x < id))
    }
    pub fn bytes(&self) -> usize {
        self.out.bytes()
            + self.member.capacity() * 8
            + self.next.capacity() * 4
            + self.changes.capacity() * size_of::<Change>()
    }
    /// The version of the table's log this output is up to date with.
    pub fn seen_at(&self) -> u64 {
        self.seen
    }
    fn enter(&mut self, id: u32) {
        let pos = self.out.lower_bound(|&x| x < id);
        self.out.insert(pos, id);
        set_bit(&mut self.member, id, true);
        self.log.push(Change::Insert(id));
        self.journal.push(Edit::Insert(pos as u32));
    }
    fn leave(&mut self, id: u32) {
        let pos = self.out.lower_bound(|&x| x < id);
        debug_assert_eq!(*self.out.get(pos), id);
        self.out.remove(pos);
        set_bit(&mut self.member, id, false);
        self.log.push(Change::Remove(id));
        self.journal.push(Edit::Remove(pos as u32));
    }
    /// Bring the output up to date with the table's edits, under the predicate the output was built with.
    pub fn sync<T>(&mut self, t: &Table<T>, mut pred: impl FnMut(&T) -> bool) {
        let Some(changes) = t.log.since(self.seen) else {
            return self.rebuild(t, pred);
        };
        for c in changes {
            let id = match c {
                Change::Insert(id) => id,
                #[cfg(feature = "mutant_ops")]
                Change::Update(_) => continue,
                #[cfg(not(feature = "mutant_ops"))]
                Change::Update(id) => id,
                Change::Remove(id) => {
                    // the id may be live again (reused): what it holds now is said by a later change
                    if bit(&self.member, id) {
                        self.leave(id);
                    }
                    continue;
                }
            };
            self.work.rows += 1;
            let now = t.get(id).is_some_and(&mut pred);
            match (bit(&self.member, id), now) {
                (false, true) => self.enter(id),
                (true, false) => self.leave(id),
                (true, true) => {
                    self.log.push(Change::Update(id));
                    let pos = self.out.lower_bound(|&x| x < id);
                    self.journal.push(Edit::Update(pos as u32));
                }
                (false, false) => {}
            }
        }
        self.seen = t.log.ver();
    }
    /// The output from the whole table. What `sync` falls back to, and what it must equal.
    pub fn rebuild<T>(&mut self, t: &Table<T>, mut pred: impl FnMut(&T) -> bool) {
        self.work.rebuilds += 1;
        self.work.rows += t.len() as u64;
        // Said first: the output is rewritten while `pred` runs, so if it panics the output is one that
        // every reader, and the next `sync` or `refilter`, recomputes (`seen` names no version of the table).
        self.seen = u64::MAX;
        self.log.reset();
        self.journal.push(Edit::Shift(0));
        self.member.clear();
        self.member.reserve(t.slots().div_ceil(64));
        let member = &mut self.member;
        // the members go straight into the lane's blocks (no list of them is made first)
        self.out
            .assign_iter(t.iter().filter(|(_, r)| pred(r)).map(|(id, _)| {
                set_bit(member, id, true);
                id
            }));
        self.seen = t.log.ver();
    }
    /// The predicate changed. `candidates` is every id that may pass now, ascending (an index's answer, the
    /// current members when the new predicate implies the old, or the whole table). If the table has edits the output has
    /// not taken in (call `sync` first), the candidates are not trusted and the output is recomputed.
    pub fn refilter<T>(
        &mut self,
        t: &Table<T>,
        mut pred: impl FnMut(&T) -> bool,
        candidates: impl Iterator<Item = u32>,
    ) {
        if self.seen != t.log.ver() {
            // the table was edited since the output was last brought up to date: the candidates were chosen
            // without those rows, so the answer is taken from the whole table
            return self.rebuild(t, pred);
        }
        let mut rows = 0u64;
        let mut next = core::mem::take(&mut self.next);
        next.clear();
        next.extend(candidates.filter(|&id| {
            rows += 1;
            t.get(id).is_some_and(&mut pred)
        }));
        self.work.rows += rows;
        self.replace_from(&next);
        self.keep_scratch(next);
        self.seen = t.log.ver();
    }
    /// Narrow: the new predicate passes nothing the old one refused, so only members are asked. (What
    /// `refilter` with the members as the candidates does, without copying them first: a list of the
    /// members that pass is begun only at the first that fails, and none when all pass.)
    pub fn narrow<T>(&mut self, t: &Table<T>, mut pred: impl FnMut(&T) -> bool) {
        if self.seen != t.log.ver() {
            // as `refilter`: the members were chosen without the table's latest edits
            return self.rebuild(t, pred);
        }
        let mut next = core::mem::take(&mut self.next);
        next.clear();
        let mut all = true;
        for (i, &id) in self.out.iter().enumerate() {
            let pass = t.get(id).is_some_and(&mut pred);
            if all {
                if !pass {
                    all = false;
                    next.extend(self.out.iter().take(i).copied());
                }
            } else if pass {
                next.push(id);
            }
        }
        self.work.rows += self.out.len() as u64;
        if !all {
            self.replace_from(&next);
        }
        self.keep_scratch(next);
        self.seen = t.log.ver();
    }
    /// A scratch list is kept for the next change if it is small; a large one is given back (its allocation
    /// is amortised over the rows it held).
    #[inline]
    fn keep_scratch(&mut self, v: Vec<u32>) {
        if v.capacity() <= SCRATCH {
            self.next = v;
        }
    }
    /// The output becomes `next` (ascending ids): the difference goes to the log and the journal.
    pub fn replace(&mut self, next: Vec<u32>) {
        self.replace_from(&next)
    }
    /// [`Filter::replace`] from a borrowed list: the lane's blocks are rewritten in place.
    pub fn replace_from(&mut self, next: &[u32]) {
        debug_assert!(
            next.windows(2).all(|w| w[0] < w[1]),
            "ids ascending, each once"
        );
        // the first position that differs is the length of the common prefix
        let common = self.out.common_prefix(next);
        let (old_n, new_n) = (self.out.len(), next.len());
        if common == old_n && common == new_n {
            return;
        }
        if old_n.abs_diff(new_n) > Log::DEFAULT_CAP / 2 {
            // more changes than a log is asked to keep: whoever reads it recomputes, so none is listed
            self.member.clear();
            if next.last().is_some_and(|&l| l as usize + 1 == new_n) {
                // every id up to the last: whole words
                self.member.resize(new_n / 64, !0);
                if new_n % 64 != 0 {
                    self.member.push((1u64 << (new_n % 64)) - 1);
                }
            } else {
                for &id in next {
                    set_bit(&mut self.member, id, true);
                }
            }
            self.log.reset();
        } else {
            let mut changes = core::mem::take(&mut self.changes);
            changes.clear();
            let mut i = common;
            let mut old = self.out.from(common).copied().peekable();
            loop {
                match (old.peek().copied(), next.get(i).copied()) {
                    (None, None) => break,
                    (Some(a), Some(b)) if a == b => {
                        old.next();
                        i += 1;
                    }
                    (Some(a), b) if b.is_none_or(|b| a < b) => {
                        changes.push(Change::Remove(a));
                        old.next();
                    }
                    (_, Some(b)) => {
                        changes.push(Change::Insert(b));
                        i += 1;
                    }
                    (Some(_), None) => unreachable!(),
                }
            }
            drop(old);
            for &c in &changes {
                match c {
                    Change::Insert(id) => set_bit(&mut self.member, id, true),
                    Change::Remove(id) => set_bit(&mut self.member, id, false),
                    Change::Update(_) => {}
                }
            }
            if changes.len() > Log::DEFAULT_CAP / 2 {
                self.log.reset();
            } else {
                for &c in &changes {
                    self.log.push(c);
                }
            }
            if changes.capacity() <= SCRATCH {
                self.changes = changes;
            }
        }
        // `common` is still the first position that differs: the output is unchanged so far
        self.out.assign(next);
        self.journal.push(Edit::Shift(common as u32));
    }
}
impl Members for Filter {
    fn log(&self) -> &Log {
        &self.log
    }
    #[inline(always)]
    fn contains(&self, id: u32) -> bool {
        bit(&self.member, id)
    }
    fn each(&self, f: &mut dyn FnMut(u32)) {
        for &id in self.out.iter() {
            f(id)
        }
    }
}

// ---- sort --------------------------------------------------------------------------------------------------

/// The members of a set ordered by a key (ties by id).
pub struct Sort<K> {
    out: Lane<(K, u32)>,
    keys: Vec<Option<K>>,
    seen: u64,
    pub journal: Journal,
    pub work: Work,
}
impl<K: Ord + Clone> Default for Sort<K> {
    fn default() -> Self {
        Sort::new()
    }
}
impl<K: Ord + Clone> Sort<K> {
    pub fn new() -> Sort<K> {
        Sort {
            out: Lane::new(),
            keys: Vec::new(),
            seen: 0,
            journal: Journal::new(),
            work: Work::default(),
        }
    }
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.out.len()
    }
    pub fn is_empty(&self) -> bool {
        self.out.is_empty()
    }
    /// The id at position `i`.
    #[inline(always)]
    pub fn get(&self, i: usize) -> u32 {
        self.out.get(i).1
    }
    pub fn key(&self, i: usize) -> &K {
        &self.out.get(i).0
    }
    pub fn ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.out.iter().map(|e| e.1)
    }
    /// Where `id` is in the order, if it is a member.
    pub fn position(&self, id: u32) -> Option<usize> {
        let k = self.keys.get(id as usize)?.as_ref()?;
        Some(self.out.lower_bound(|e| (&e.0, e.1) < (k, id)))
    }
    pub fn bytes(&self) -> usize {
        self.out.bytes() + self.keys.capacity() * size_of::<Option<K>>()
    }
    fn take(&mut self, id: u32) -> Option<usize> {
        let k = self.keys.get_mut(id as usize)?.take()?;
        let pos = self.out.lower_bound(|e| (&e.0, e.1) < (&k, id));
        debug_assert!(self.out.get(pos).1 == id);
        self.out.remove(pos);
        Some(pos)
    }
    fn put(&mut self, id: u32, k: K) -> usize {
        let pos = self.out.lower_bound(|e| (&e.0, e.1) < (&k, id));
        if id as usize >= self.keys.len() {
            self.keys.resize(id as usize + 1, None);
        }
        self.keys[id as usize] = Some(k.clone());
        self.out.insert(pos, (k, id));
        pos
    }
    /// Bring the order up to date with what `src` changed. `key` gives a member's key.
    pub fn sync<S: Members + ?Sized>(&mut self, src: &S, mut key: impl FnMut(u32) -> K) {
        let Some(changes) = src.log().since(self.seen) else {
            return self.rebuild(src, key);
        };
        for c in changes {
            let (Change::Insert(id) | Change::Update(id) | Change::Remove(id)) = c;
            if matches!(c, Change::Remove(_)) || !src.contains(id) {
                if let Some(pos) = self.take(id) {
                    self.journal.push(Edit::Remove(pos as u32));
                }
                continue;
            }
            self.work.rows += 1;
            let k = key(id);
            if self
                .keys
                .get(id as usize)
                .is_some_and(|o| o.as_ref() == Some(&k))
            {
                // the same place: its content may be another
                let pos = self.out.lower_bound(|e| (&e.0, e.1) < (&k, id));
                self.journal.push(Edit::Update(pos as u32));
                continue;
            }
            if let Some(pos) = self.take(id) {
                self.journal.push(Edit::Remove(pos as u32));
            }
            let pos = self.put(id, k);
            self.journal.push(Edit::Insert(pos as u32));
        }
        self.seen = src.log().ver();
    }
    pub fn rebuild<S: Members + ?Sized>(&mut self, src: &S, mut key: impl FnMut(u32) -> K) {
        self.work.rebuilds += 1;
        let mut v: Vec<(K, u32)> = Vec::new();
        src.each(&mut |id| v.push((key(id), id)));
        self.work.rows += v.len() as u64;
        v.sort();
        self.keys.clear();
        for (k, id) in &v {
            if *id as usize >= self.keys.len() {
                self.keys.resize(*id as usize + 1, None);
            }
            self.keys[*id as usize] = Some(k.clone());
        }
        self.out = Lane::from_vec(v);
        self.seen = src.log().ver();
        self.journal.push(Edit::Shift(0));
    }
}

// ---- group and total ---------------------------------------------------------------------------------------

/// A fold that can take a value back: what lets an aggregate follow removes and updates without a rescan.
/// (A minimum or a maximum cannot; read those off the ends of a [`Sort`].)
pub trait Agg {
    type In: Clone + PartialEq;
    type Acc: Default + Clone + PartialEq;
    fn add(acc: &mut Self::Acc, v: &Self::In);
    fn sub(acc: &mut Self::Acc, v: &Self::In);
}
/// The sum of `i64` values.
pub struct Sum;
impl Agg for Sum {
    type In = i64;
    type Acc = i64;
    #[inline(always)]
    fn add(acc: &mut i64, v: &i64) {
        *acc = acc.wrapping_add(*v)
    }
    #[inline(always)]
    fn sub(acc: &mut i64, v: &i64) {
        *acc = acc.wrapping_sub(*v)
    }
}
/// Nothing but the count (which every group keeps anyway).
pub struct Count;
impl Agg for Count {
    type In = ();
    type Acc = ();
    #[inline(always)]
    fn add(_: &mut (), _: &()) {}
    #[inline(always)]
    fn sub(_: &mut (), _: &()) {}
}

/// One row of a [`Group`]'s output.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupRow<G, Acc> {
    pub key: G,
    pub count: u32,
    pub acc: Acc,
}

/// The members of a set grouped by a key: one output row per key that has a member, in key order, with the
/// members' count and a fold of their values.
pub struct Group<G, A: Agg> {
    groups: Lane<GroupRow<G, A::Acc>>,
    of: Vec<Option<(G, A::In)>>,
    seen: u64,
    pub journal: Journal,
    pub work: Work,
}
impl<G: Ord + Clone, A: Agg> Default for Group<G, A> {
    fn default() -> Self {
        Group::new()
    }
}
impl<G: Ord + Clone, A: Agg> Group<G, A> {
    pub fn new() -> Group<G, A> {
        Group {
            groups: Lane::new(),
            of: Vec::new(),
            seen: 0,
            journal: Journal::new(),
            work: Work::default(),
        }
    }
    pub fn len(&self) -> usize {
        self.groups.len()
    }
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
    pub fn get(&self, i: usize) -> &GroupRow<G, A::Acc> {
        self.groups.get(i)
    }
    pub fn rows(&self) -> impl Iterator<Item = &GroupRow<G, A::Acc>> + '_ {
        self.groups.iter()
    }
    /// The row of group `g`, if it has a member.
    pub fn of(&self, g: &G) -> Option<&GroupRow<G, A::Acc>> {
        let pos = self.groups.lower_bound(|e| e.key < *g);
        (pos < self.groups.len() && self.groups.get(pos).key == *g).then(|| self.groups.get(pos))
    }
    fn retract(&mut self, id: u32) {
        let Some((g, v)) = self.of.get_mut(id as usize).and_then(|o| o.take()) else {
            return;
        };
        let pos = self.groups.lower_bound(|e| e.key < g);
        let row = self.groups.get_mut(pos);
        debug_assert!(row.key == g && row.count > 0);
        row.count -= 1;
        A::sub(&mut row.acc, &v);
        if row.count == 0 {
            self.groups.remove(pos);
            self.journal.push(Edit::Remove(pos as u32));
        } else {
            self.journal.push(Edit::Update(pos as u32));
        }
    }
    fn add(&mut self, id: u32, g: G, v: A::In) {
        let pos = self.groups.lower_bound(|e| e.key < g);
        if pos < self.groups.len() && self.groups.get(pos).key == g {
            let row = self.groups.get_mut(pos);
            row.count += 1;
            A::add(&mut row.acc, &v);
            self.journal.push(Edit::Update(pos as u32));
        } else {
            let mut acc = A::Acc::default();
            A::add(&mut acc, &v);
            self.groups.insert(
                pos,
                GroupRow {
                    key: g.clone(),
                    count: 1,
                    acc,
                },
            );
            self.journal.push(Edit::Insert(pos as u32));
        }
        if id as usize >= self.of.len() {
            self.of.resize(id as usize + 1, None);
        }
        self.of[id as usize] = Some((g, v));
    }
    /// Bring the groups up to date with what `src` changed. `f` gives a member's group and value.
    pub fn sync<S: Members + ?Sized>(&mut self, src: &S, mut f: impl FnMut(u32) -> (G, A::In)) {
        let Some(changes) = src.log().since(self.seen) else {
            return self.rebuild(src, f);
        };
        for c in changes {
            let (Change::Insert(id) | Change::Update(id) | Change::Remove(id)) = c;
            if matches!(c, Change::Remove(_)) || !src.contains(id) {
                self.retract(id);
                continue;
            }
            self.work.rows += 1;
            let (g, v) = f(id);
            if self
                .of
                .get(id as usize)
                .is_some_and(|o| o.as_ref().is_some_and(|(g0, v0)| *g0 == g && *v0 == v))
            {
                continue;
            }
            self.retract(id);
            self.add(id, g, v);
        }
        self.seen = src.log().ver();
    }
    pub fn rebuild<S: Members + ?Sized>(&mut self, src: &S, mut f: impl FnMut(u32) -> (G, A::In)) {
        self.work.rebuilds += 1;
        self.groups.clear();
        self.of.clear();
        let mut all: Vec<(G, u32, A::In)> = Vec::new();
        src.each(&mut |id| {
            let (g, v) = f(id);
            all.push((g, id, v));
        });
        self.work.rows += all.len() as u64;
        all.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
        let mut rows: Vec<GroupRow<G, A::Acc>> = Vec::new();
        for (g, id, v) in all {
            match rows.last_mut() {
                Some(r) if r.key == g => {
                    r.count += 1;
                    A::add(&mut r.acc, &v);
                }
                _ => {
                    let mut acc = A::Acc::default();
                    A::add(&mut acc, &v);
                    rows.push(GroupRow {
                        key: g.clone(),
                        count: 1,
                        acc,
                    });
                }
            }
            if id as usize >= self.of.len() {
                self.of.resize(id as usize + 1, None);
            }
            self.of[id as usize] = Some((g, v));
        }
        self.groups = Lane::from_vec(rows);
        self.seen = src.log().ver();
        self.journal.push(Edit::Shift(0));
    }
}

/// One fold of every member: a [`Group`] of one group.
pub struct Total<A: Agg>(Group<(), A>);
impl<A: Agg> Default for Total<A> {
    fn default() -> Self {
        Total(Group::new())
    }
}
impl<A: Agg> Total<A> {
    pub fn new() -> Total<A> {
        Total::default()
    }
    pub fn sync<S: Members + ?Sized>(&mut self, src: &S, mut f: impl FnMut(u32) -> A::In) {
        self.0.sync(src, |id| ((), f(id)))
    }
    pub fn count(&self) -> u32 {
        self.0.of(&()).map_or(0, |r| r.count)
    }
    pub fn value(&self) -> A::Acc {
        self.0
            .of(&())
            .map_or_else(A::Acc::default, |r| r.acc.clone())
    }
    pub fn work(&self) -> Work {
        self.0.work
    }
}

// ---- join --------------------------------------------------------------------------------------------------

/// The pairs (left id, right id) whose keys are equal, in (left, right) order.
pub struct Join<K> {
    lkey: Vec<Option<K>>,
    rkey: Vec<Option<K>>,
    lidx: HashMap<K, Vec<u32>>,
    ridx: HashMap<K, Vec<u32>>,
    out: Lane<(u32, u32)>,
    lseen: u64,
    rseen: u64,
    pub journal: Journal,
    pub work: Work,
    /// Lists of keys that lost their last row, for keys that gain their first (a few small ones).
    spare: Vec<Vec<u32>>,
}
/// The most key lists a [`Join`] keeps for reuse, and the most ids such a list may have room for.
const SPARE_KEYS: usize = 8;
const SPARE_KEY_ROOM: usize = 16;
impl<K: Hash + Eq + Clone> Default for Join<K> {
    fn default() -> Self {
        Join::new()
    }
}
impl<K: Hash + Eq + Clone> Join<K> {
    pub fn new() -> Join<K> {
        Join {
            lkey: Vec::new(),
            rkey: Vec::new(),
            lidx: HashMap::new(),
            ridx: HashMap::new(),
            out: Lane::new(),
            lseen: 0,
            rseen: 0,
            journal: Journal::new(),
            work: Work::default(),
            spare: Vec::new(),
        }
    }
    pub fn len(&self) -> usize {
        self.out.len()
    }
    pub fn is_empty(&self) -> bool {
        self.out.is_empty()
    }
    pub fn get(&self, i: usize) -> (u32, u32) {
        *self.out.get(i)
    }
    pub fn pairs(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.out.iter().copied()
    }
    /// Keys either side holds an index entry for: bounded by the keys live rows have.
    pub fn index_keys(&self) -> usize {
        self.lidx.len() + self.ridx.len()
    }
    /// The pair `p` enters (`on`) or leaves the output. (A function of the fields it changes, so a caller may
    /// hold the other side's index while it calls it.)
    fn pair(out: &mut Lane<(u32, u32)>, journal: &mut Journal, p: (u32, u32), on: bool) {
        let pos = out.lower_bound(|e| *e < p);
        if on {
            out.insert(pos, p);
            journal.push(Edit::Insert(pos as u32));
        } else {
            debug_assert!(*out.get(pos) == p);
            out.remove(pos);
            journal.push(Edit::Remove(pos as u32));
        }
    }
    /// One side's row `id` changed: `new` is its key now (`None`: it is gone). The other side's rows of a key
    /// are read in its index where they are (not copied out first), and the list of a key that loses its
    /// last row is kept for the next key that gains its first: moving a row between keys allocates nothing.
    fn side(&mut self, left: bool, id: u32, new: Option<K>) {
        let Join {
            lkey,
            rkey,
            lidx,
            ridx,
            out,
            journal,
            spare,
            ..
        } = self;
        let (keys, own, other) = if left {
            (lkey, lidx, &*ridx)
        } else {
            (rkey, ridx, &*lidx)
        };
        let rows = |k: &K| other.get(k).map_or(&[][..], Vec::as_slice);
        let p = |o: u32| if left { (id, o) } else { (o, id) };
        if id as usize >= keys.len() {
            keys.resize(id as usize + 1, None);
        }
        let old = keys[id as usize].take();
        if old == new {
            keys[id as usize] = old;
            if let Some(k) = &keys[id as usize] {
                // the same pairs: the rows that show them may show something else
                for &o in rows(k) {
                    let pos = out.lower_bound(|e| *e < p(o));
                    journal.push(Edit::Update(pos as u32));
                }
            }
            return;
        }
        if let Some(k) = old {
            if let Some(v) = own.get_mut(&k) {
                v.retain(|&x| x != id);
                if v.is_empty()
                    && let Some(v) = own.remove(&k)
                    && spare.len() < SPARE_KEYS
                    && v.capacity() <= SPARE_KEY_ROOM
                {
                    spare.push(v);
                }
            }
            for &o in rows(&k) {
                Self::pair(out, journal, p(o), false);
            }
        }
        if let Some(k) = new {
            keys[id as usize] = Some(k.clone());
            own.entry(k.clone())
                .or_insert_with(|| spare.pop().unwrap_or_default())
                .push(id);
            for &o in rows(&k) {
                Self::pair(out, journal, p(o), true);
            }
        }
    }
    /// Bring the pairs up to date with what either side changed.
    pub fn sync<L: Members + ?Sized, R: Members + ?Sized>(
        &mut self,
        l: &L,
        r: &R,
        mut lk: impl FnMut(u32) -> K,
        mut rk: impl FnMut(u32) -> K,
    ) {
        if l.log().since(self.lseen).is_none() || r.log().since(self.rseen).is_none() {
            return self.rebuild(l, r, lk, rk);
        }
        // the logs are the inputs' (not this operator's): read where they are
        for c in l.log().since(self.lseen).expect("checked") {
            let (Change::Insert(id) | Change::Update(id) | Change::Remove(id)) = c;
            let live = !matches!(c, Change::Remove(_)) && l.contains(id);
            self.work.rows += live as u64;
            let k = live.then(|| lk(id));
            self.side(true, id, k);
        }
        for c in r.log().since(self.rseen).expect("checked") {
            let (Change::Insert(id) | Change::Update(id) | Change::Remove(id)) = c;
            let live = !matches!(c, Change::Remove(_)) && r.contains(id);
            self.work.rows += live as u64;
            let k = live.then(|| rk(id));
            self.side(false, id, k);
        }
        (self.lseen, self.rseen) = (l.log().ver(), r.log().ver());
    }
    pub fn rebuild<L: Members + ?Sized, R: Members + ?Sized>(
        &mut self,
        l: &L,
        r: &R,
        mut lk: impl FnMut(u32) -> K,
        mut rk: impl FnMut(u32) -> K,
    ) {
        self.work.rebuilds += 1;
        self.lkey.clear();
        self.rkey.clear();
        self.lidx.clear();
        self.ridx.clear();
        let put = |keys: &mut Vec<Option<K>>, idx: &mut HashMap<K, Vec<u32>>, id: u32, k: K| {
            if id as usize >= keys.len() {
                keys.resize(id as usize + 1, None);
            }
            keys[id as usize] = Some(k.clone());
            idx.entry(k).or_default().push(id);
        };
        let (mut n, mut lids) = (0u64, Vec::new());
        l.each(&mut |id| lids.push(id));
        for id in lids {
            n += 1;
            put(&mut self.lkey, &mut self.lidx, id, lk(id));
        }
        let mut rids = Vec::new();
        r.each(&mut |id| rids.push(id));
        for id in rids {
            n += 1;
            put(&mut self.rkey, &mut self.ridx, id, rk(id));
        }
        self.work.rows += n;
        let mut pairs: Vec<(u32, u32)> = Vec::new();
        for (k, ls) in &self.lidx {
            if let Some(rs) = self.ridx.get(k) {
                for &a in ls {
                    for &b in rs {
                        pairs.push((a, b));
                    }
                }
            }
        }
        pairs.sort_unstable();
        self.out = Lane::from_vec(pairs);
        (self.lseen, self.rseen) = (l.log().ver(), r.log().ver());
        self.journal.push(Edit::Shift(0));
    }
}
