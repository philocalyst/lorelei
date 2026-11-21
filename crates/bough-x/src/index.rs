//! A substring index over the text of a table's rows, and the search operator on it.
//!
//! [`Trigrams`] maps every three consecutive bytes that occur in some row's text to the ids of rows that hold
//! them (ascending). It is a *superset* index: a posting says "this row may hold this trigram". A query takes
//! the shortest posting list among the needle's trigrams as its candidates and verifies each against the row's
//! text, so an answer is exact whatever the postings say, and three things follow:
//!
//! * an edited row is only added to the lists of the trigrams it now holds; its stale postings cost a
//!   verification each until the index is rebuilt, which happens when a quarter of it is stale (so an edit is
//!   amortised `O(text)`, and the index's size stays within a band of the exact one);
//! * a removed row's postings fail verification (no such row), and a reused id inherits them harmlessly;
//! * a needle under three bytes has no trigram: it is answered by narrowing the last result or by a scan.
//!
//! [`Substring`] is the operator: the ids whose text holds the needle, ascending, with a journal for the list
//! that shows them. Per change of the needle it asks the cheapest of: the current hits (when the new needle
//! contains the old one: nothing can enter), a result it still holds for that needle (a backspace), the
//! index's candidates, the whole table. Its cost is the candidates it verifies, not the dataset.
use crate::ops::{Filter, Members, Work};
use crate::rel::{Change, Log, Table};
use crate::sink::Journal;
use std::collections::HashMap;

/// A posting list: ascending ids, in blocks once it is long so an insert moves one block.
enum Post {
    Small(Vec<u32>),
    Big(Vec<Vec<u32>>),
}
const SMALL: usize = 256;
const BLOCK: usize = 512;
impl Post {
    fn len(&self) -> usize {
        match self {
            Post::Small(v) => v.len(),
            Post::Big(b) => b.iter().map(|v| v.len()).sum(),
        }
    }
    fn bytes(&self) -> usize {
        match self {
            Post::Small(v) => v.capacity() * 4,
            Post::Big(b) => {
                b.capacity() * size_of::<Vec<u32>>()
                    + b.iter().map(|v| v.capacity() * 4).sum::<usize>()
            }
        }
    }
    /// Add `id`; false if it was there.
    fn add(&mut self, id: u32) -> bool {
        match self {
            Post::Small(v) => {
                let at = v.partition_point(|&x| x < id);
                if v.get(at) == Some(&id) {
                    return false;
                }
                v.insert(at, id);
                if v.len() > SMALL {
                    let hi = v.split_off(v.len() / 2);
                    *self = Post::Big(vec![core::mem::take(v), hi]);
                }
                true
            }
            Post::Big(b) => {
                // the last block whose first id is at most `id` (the first block otherwise)
                let k = b.partition_point(|v| v[0] <= id).saturating_sub(1);
                let v = &mut b[k];
                let at = v.partition_point(|&x| x < id);
                if v.get(at) == Some(&id) {
                    return false;
                }
                v.insert(at, id);
                if v.len() > BLOCK {
                    let hi = v.split_off(v.len() / 2);
                    b.insert(k + 1, hi);
                }
                true
            }
        }
    }
    fn each(&self, f: &mut dyn FnMut(u32)) {
        match self {
            Post::Small(v) => v.iter().for_each(|&x| f(x)),
            Post::Big(b) => b.iter().flatten().for_each(|&x| f(x)),
        }
    }
}

#[inline(always)]
fn tri(b: &[u8]) -> u32 {
    (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32
}

/// The trigram index. See the module.
pub struct Trigrams {
    post: HashMap<u32, Post>,
    /// Postings held, and postings an exact index of the indexed texts would hold as of the last rebuild plus
    /// the rows added since: their difference bounds the stale ones.
    held: usize,
    fresh: usize,
    /// Postings added by edits since the last rebuild (each may have left a stale one behind).
    churn: usize,
    pub rebuilds: u64,
}
impl Default for Trigrams {
    fn default() -> Self {
        Trigrams::new()
    }
}
impl Trigrams {
    pub fn new() -> Trigrams {
        Trigrams {
            post: HashMap::new(),
            held: 0,
            fresh: 0,
            churn: 0,
            rebuilds: 0,
        }
    }
    /// Postings held (stale ones included).
    pub fn postings(&self) -> usize {
        self.held
    }
    pub fn trigrams(&self) -> usize {
        self.post.len()
    }
    /// Bytes the index holds: the lists and the map's table.
    pub fn bytes(&self) -> usize {
        self.post.values().map(Post::bytes).sum::<usize>()
            + self.post.capacity() * (size_of::<u32>() + size_of::<Post>() + 1)
    }
    /// Row `id` holds `text` (now): it joins the list of every trigram of the text. Returns postings added.
    pub fn add(&mut self, id: u32, text: &[u8]) -> usize {
        let mut n = 0;
        for w in text.windows(3) {
            n += self
                .post
                .entry(tri(w))
                .or_insert_with(|| Post::Small(Vec::new()))
                .add(id) as usize;
        }
        self.held += n;
        n
    }
    /// The index of exactly these rows: every stale posting is gone.
    pub fn rebuild<'a>(&mut self, rows: impl Iterator<Item = (u32, &'a [u8])>) {
        self.rebuilds += 1;
        // by trigram, ids ascending: lists are built by appending
        let mut lists: HashMap<u32, Vec<u32>> = HashMap::new();
        for (id, text) in rows {
            for w in text.windows(3) {
                let v = lists.entry(tri(w)).or_default();
                if v.last() != Some(&id) {
                    v.push(id);
                }
            }
        }
        self.held = 0;
        self.post.clear();
        for (t, mut v) in lists {
            // a trigram that occurs twice in one text with another between was pushed twice, apart
            v.sort_unstable();
            v.dedup();
            self.held += v.len();
            let p = if v.len() <= SMALL {
                v.shrink_to_fit();
                Post::Small(v)
            } else {
                Post::Big(v.chunks(BLOCK * 3 / 4).map(|c| c.to_vec()).collect())
            };
            self.post.insert(t, p);
        }
        self.post.shrink_to_fit();
        self.fresh = self.held;
        self.churn = 0;
    }
    /// A row was edited or inserted: note its postings, and say whether the index has gone stale enough to
    /// rebuild (a quarter of what an exact one held, and at least a few hundred).
    pub fn edit(&mut self, id: u32, text: &[u8]) -> bool {
        self.churn += self.add(id, text);
        self.churn > 256 && self.churn * 4 > self.fresh
    }
    /// The ids that may hold `needle`, ascending: the shortest list among its trigrams. `None` for a needle
    /// with no trigram (under three bytes); an empty list when some trigram of it occurs nowhere.
    pub fn candidates(&self, needle: &[u8]) -> Option<Vec<u32>> {
        if needle.len() < 3 {
            return None;
        }
        let mut best: Option<&Post> = None;
        for w in needle.windows(3) {
            match self.post.get(&tri(w)) {
                None => return Some(Vec::new()),
                Some(p) => {
                    if best.is_none_or(|b| p.len() < b.len()) {
                        best = Some(p)
                    }
                }
            }
        }
        let p = best.expect("a needle of three bytes has a trigram");
        let mut out = Vec::with_capacity(p.len());
        p.each(&mut |id| out.push(id));
        Some(out)
    }
    /// The length of the list [`Trigrams::candidates`] would return, without building it.
    pub fn estimate(&self, needle: &[u8]) -> Option<usize> {
        if needle.len() < 3 {
            return None;
        }
        let mut best = usize::MAX;
        for w in needle.windows(3) {
            best = best.min(self.post.get(&tri(w)).map_or(0, Post::len));
        }
        Some(best)
    }
}

/// Whether `hay` holds `needle` as consecutive bytes. An empty needle is in everything.
#[inline]
pub fn holds(hay: &[u8], needle: &[u8]) -> bool {
    let n = needle.len();
    if n == 0 {
        return true;
    }
    if n > hay.len() {
        return false;
    }
    if n == 1 {
        return hay.contains(&needle[0]);
    }
    let (first, rest) = (needle[0], &needle[1..]);
    let last = hay.len() - n;
    let mut i = 0;
    while i <= last {
        if hay[i] == first && &hay[i + 1..i + n] == rest {
            return true;
        }
        i += 1;
    }
    false
}

/// A result the operator still holds for a needle it was asked before (a backspace returns to it).
struct Kept {
    needle: Vec<u8>,
    hits: Vec<u32>,
}

/// How the last change of the needle was answered: what the count gates and the measurements read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// Nothing to do (the same needle).
    Same,
    /// The current hits were verified (the new needle contains the old).
    Narrow,
    /// A kept result for this needle was taken as it was.
    Kept,
    /// The index's candidates were verified.
    Index,
    /// Every row was verified.
    Scan,
}

/// The rows of a table whose text holds a needle, ascending by id. See the module.
pub struct Substring {
    tri: Trigrams,
    hits: Filter,
    needle: Vec<u8>,
    kept: Vec<Kept>,
    /// Ids the kept results may hold in all: at most the table's slots, so they cost at most what one full
    /// result does.
    budget: usize,
    built: bool,
    pub route: Route,
    /// Rows verified by the last change of the needle or the last sync.
    pub verified: u64,
}
impl Default for Substring {
    fn default() -> Self {
        Substring::new()
    }
}
impl Substring {
    pub fn new() -> Substring {
        Substring {
            tri: Trigrams::new(),
            hits: Filter::new(),
            needle: Vec::new(),
            kept: Vec::new(),
            budget: 0,
            built: false,
            route: Route::Same,
            verified: 0,
        }
    }
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.hits.len()
    }
    pub fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }
    /// The id of the row at position `i` of the result.
    #[inline(always)]
    pub fn get(&self, i: usize) -> u32 {
        self.hits.get(i)
    }
    pub fn ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.hits.ids()
    }
    /// What a list of the result reads its edits from.
    pub fn journal(&self) -> &Journal {
        &self.hits.journal
    }
    pub fn needle(&self) -> &[u8] {
        &self.needle
    }
    pub fn index(&self) -> &Trigrams {
        &self.tri
    }
    pub fn work(&self) -> Work {
        self.hits.work
    }
    /// Bytes held beyond the table: the index, the result, the kept results.
    pub fn bytes(&self) -> usize {
        self.tri.bytes()
            + self.hits.bytes()
            + self
                .kept
                .iter()
                .map(|k| k.hits.capacity() * 4 + k.needle.capacity())
                .sum::<usize>()
    }
    /// Ids held in kept results.
    pub fn kept_ids(&self) -> usize {
        self.kept.iter().map(|k| k.hits.len()).sum()
    }
    fn build<T>(&mut self, t: &Table<T>, text: &impl Fn(&T) -> &str) {
        self.tri
            .rebuild(t.iter().map(|(id, r)| (id, text(r).as_bytes())));
        self.built = true;
    }
    /// Bring the index and the result up to date with the table's edits.
    pub fn sync<T>(&mut self, t: &Table<T>, text: impl Fn(&T) -> &str) {
        let before = self.hits.work.rows;
        if !self.built {
            self.build(t, &text);
        } else {
            // the index first: it reads the same log from the same cursor the result holds
            let mut stale = false;
            match t.log.since(self.seen()) {
                None => stale = true,
                Some(changes) => {
                    let mut any = false;
                    for c in changes {
                        any = true;
                        if let Change::Insert(id) | Change::Update(id) = c
                            && let Some(r) = t.get(id)
                        {
                            stale |= self.tri.edit(id, text(r).as_bytes());
                        }
                    }
                    if any {
                        // a kept result is of the table as it was
                        self.kept.clear();
                    }
                }
            }
            if stale {
                self.kept.clear();
                self.build(t, &text);
            }
        }
        let needle = core::mem::take(&mut self.needle);
        self.hits.sync(t, |r| holds(text(r).as_bytes(), &needle));
        self.needle = needle;
        self.budget = t.slots();
        self.verified = self.hits.work.rows - before;
    }
    fn seen(&self) -> u64 {
        self.hits.seen_at()
    }
    /// The needle is now `needle`. The table's pending edits are taken in first.
    pub fn set_needle<T>(&mut self, t: &Table<T>, text: impl Fn(&T) -> &str, needle: &str) {
        self.sync(t, &text);
        let before = self.hits.work.rows;
        let new = needle.as_bytes();
        if new == &self.needle[..] {
            self.route = Route::Same;
            self.verified = 0;
            return;
        }
        // what is shown now may be wanted again (the needle one backspace from the next)
        self.keep();
        let pred = |r: &T| {
            #[cfg(feature = "mutant_tri")]
            return true;
            #[cfg(not(feature = "mutant_tri"))]
            holds(text(r).as_bytes(), new)
        };
        if let Some(k) = self.kept.iter().position(|k| k.needle == new) {
            let k = self.kept.remove(k);
            self.hits.replace(k.hits);
            self.route = Route::Kept;
        } else {
            let narrow = holds(new, &self.needle).then(|| self.hits.len());
            let index = self.tri.estimate(new);
            if let Some(n) = narrow
                && index.is_none_or(|i| n <= i)
            {
                self.hits.narrow(t, pred);
                self.route = Route::Narrow;
            } else if index.is_some() {
                let c = self.tri.candidates(new).expect("estimated");
                self.hits.refilter(t, pred, c.into_iter());
                self.route = Route::Index;
            } else if new.is_empty() {
                // everything holds nothing
                self.hits.replace(t.ids().collect());
                self.route = Route::Scan;
            } else {
                self.hits.refilter(t, pred, t.ids());
                self.route = Route::Scan;
            }
        }
        self.needle.clear();
        self.needle.extend_from_slice(new);
        self.verified = self.hits.work.rows - before;
    }
    /// Keep the current result under the current needle, within the budget (a result of more than half the
    /// table is not kept: it narrows little and costs most).
    fn keep(&mut self) {
        let n = self.hits.len();
        if n > self.budget / 2 || self.kept.iter().any(|k| k.needle == self.needle) {
            return;
        }
        self.kept.push(Kept {
            needle: self.needle.clone(),
            hits: self.hits.ids().collect(),
        });
        while self.kept.len() > 8 || self.kept_ids() > self.budget {
            // the largest costs the most and narrows the least
            let big = (0..self.kept.len())
                .max_by_key(|&i| self.kept[i].hits.len())
                .expect("not empty");
            self.kept.remove(big);
        }
    }
}
impl Members for Substring {
    fn log(&self) -> &Log {
        &self.hits.log
    }
    fn contains(&self, id: u32) -> bool {
        self.hits.contains(id)
    }
    fn each(&self, f: &mut dyn FnMut(u32)) {
        self.hits.each(f)
    }
}
