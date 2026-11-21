//! Relations: collections whose edits are their own derivative.
//!
//! * [`Lane`]: the order lane, a sequence in gapped blocks (an insert or a remove moves one block).
//! * [`Seq`]: a relation in a lane, with the journal a list reads (`sink::Journal`). The same surface as
//!   `sink::Rel`, which keeps its `Vec`: a list takes the journal of either.
//! * [`Table`]: rows under stable ids with a bounded log of changes ([`Log`]). The operators of `crate::ops`
//!   read a log from where they last stopped; a reader that fell behind what the log keeps recomputes.
//!
//! Nothing here is observed: an edit is a plain call that notes what it did, and whoever cares compares its own
//! cursor with the log's version when it next runs.
pub mod lane;
pub use lane::Lane;

use crate::sink::{Edit, Journal};
use std::collections::VecDeque;

/// A relation in an order lane. An edit is a plain call that also notes which row.
pub struct Seq<T> {
    items: Lane<T>,
    pub journal: Journal,
}
impl<T> Seq<T> {
    pub fn new(items: Vec<T>) -> Seq<T> {
        Seq {
            items: Lane::from_vec(items),
            journal: Journal::new(),
        }
    }
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    #[inline(always)]
    pub fn get(&self, i: usize) -> &T {
        self.items.get(i)
    }
    /// The items with no note taken: for state kept opaque, where the view is re-described and compared.
    pub fn items_mut(&mut self) -> &mut Lane<T> {
        &mut self.items
    }
    pub fn items(&self) -> &Lane<T> {
        &self.items
    }
    #[inline(always)]
    pub fn edit(&mut self, i: usize) -> &mut T {
        self.journal.push(Edit::Update(i as u32));
        self.items.get_mut(i)
    }
    pub fn insert(&mut self, i: usize, v: T) {
        self.journal.push(Edit::Insert(i as u32));
        self.items.insert(i, v)
    }
    pub fn remove(&mut self, i: usize) -> T {
        self.journal.push(Edit::Remove(i as u32));
        self.items.remove(i)
    }
    pub fn push(&mut self, v: T) {
        self.insert(self.len(), v)
    }
}

/// One change of a [`Table`] or of an operator's output, by row id.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Change {
    /// The id is now a member (a row was inserted, or began to pass a filter).
    Insert(u32),
    /// The id is no longer a member.
    Remove(u32),
    /// The id is still a member and its value may be another.
    Update(u32),
}

/// The last changes of a set of ids, numbered. A reader keeps the number it has read up to (`seen`); what it
/// missed is [`Log::since`]. The log keeps at most `cap` changes: a reader further behind is told so (`None`)
/// and recomputes from the source, which is then no dearer than replaying would have been.
pub struct Log {
    buf: VecDeque<Change>,
    /// The number of the change after the last one pushed.
    ver: u64,
    cap: usize,
}
impl Log {
    pub const DEFAULT_CAP: usize = 1024;
    pub fn new(cap: usize) -> Log {
        Log {
            buf: VecDeque::new(),
            ver: 0,
            cap: cap.max(1),
        }
    }
    #[inline(always)]
    pub fn ver(&self) -> u64 {
        self.ver
    }
    pub fn len(&self) -> usize {
        self.buf.len()
    }
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
    #[inline]
    pub fn push(&mut self, c: Change) {
        if self.buf.len() == self.cap {
            self.buf.pop_front();
        }
        self.buf.push_back(c);
        self.ver += 1;
    }
    /// Everything changed and no list of it is kept: every reader recomputes.
    pub fn reset(&mut self) {
        self.buf.clear();
        // skip a number, so a reader exactly at `ver` also sees that it missed something
        self.ver += 1;
    }
    /// The changes numbered `seen..ver`, oldest first; `None` if the log no longer keeps all of them.
    #[inline]
    pub fn since(&self, seen: u64) -> Option<impl Iterator<Item = Change> + '_> {
        let pending = self.ver.checked_sub(seen)?;
        if pending as usize > self.buf.len() {
            return None;
        }
        Some(
            self.buf
                .iter()
                .skip(self.buf.len() - pending as usize)
                .copied(),
        )
    }
}

/// Rows under stable ids. An id is a slot: it names the same row from its insert to its remove and is reused
/// afterwards, so the table's memory follows its largest population. Every edit is noted in the log.
pub struct Table<T> {
    rows: Vec<Option<T>>,
    free: Vec<u32>,
    live: usize,
    pub log: Log,
}
impl<T> Default for Table<T> {
    fn default() -> Self {
        Table::new()
    }
}
impl<T> Table<T> {
    pub fn new() -> Table<T> {
        Table {
            rows: Vec::new(),
            free: Vec::new(),
            live: 0,
            log: Log::new(Log::DEFAULT_CAP),
        }
    }
    /// A table of `items`, with ids `0..items.len()` in order. Nothing is logged: a reader at version 0 of an
    /// empty log recomputes (its cursor is behind), which is the first build.
    pub fn from_vec(items: Vec<T>) -> Table<T> {
        let live = items.len();
        let mut t = Table {
            rows: items.into_iter().map(Some).collect(),
            free: Vec::new(),
            live,
            log: Log::new(Log::DEFAULT_CAP),
        };
        t.log.reset();
        t
    }
    /// The log keeps `cap` changes (a batch edit larger than this makes every reader recompute).
    pub fn with_log(mut self, cap: usize) -> Table<T> {
        self.log.cap = cap.max(1);
        self
    }
    /// Live rows.
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.live
    }
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }
    /// One more than the largest id ever live: what a table indexed by id is sized to.
    #[inline(always)]
    pub fn slots(&self) -> usize {
        self.rows.len()
    }
    #[inline(always)]
    pub fn get(&self, id: u32) -> Option<&T> {
        self.rows.get(id as usize).and_then(|r| r.as_ref())
    }
    /// The row `id`, which is live.
    #[inline(always)]
    pub fn row(&self, id: u32) -> &T {
        self.rows[id as usize].as_ref().expect("a live row")
    }
    pub fn insert(&mut self, v: T) -> u32 {
        let id = match self.free.pop() {
            Some(id) => {
                self.rows[id as usize] = Some(v);
                id
            }
            None => {
                assert!(
                    self.rows.len() < u32::MAX as usize,
                    "a table holds under 2^32 rows"
                );
                self.rows.push(Some(v));
                self.rows.len() as u32 - 1
            }
        };
        self.live += 1;
        self.log.push(Change::Insert(id));
        id
    }
    pub fn remove(&mut self, id: u32) -> Option<T> {
        let v = self.rows.get_mut(id as usize)?.take()?;
        self.free.push(id);
        self.live -= 1;
        self.log.push(Change::Remove(id));
        Some(v)
    }
    /// The row `id` to change in place, noted as changed. `None` if no such row is live.
    pub fn edit(&mut self, id: u32) -> Option<&mut T> {
        let r = self.rows.get_mut(id as usize)?.as_mut()?;
        self.log.push(Change::Update(id));
        Some(r)
    }
    /// The live ids in ascending order.
    pub fn ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.rows
            .iter()
            .enumerate()
            .filter_map(|(i, r)| r.as_ref().map(|_| i as u32))
    }
    /// The live rows with their ids, ascending.
    pub fn iter(&self) -> impl Iterator<Item = (u32, &T)> + '_ {
        self.rows
            .iter()
            .enumerate()
            .filter_map(|(i, r)| r.as_ref().map(|r| (i as u32, r)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_log_tells_a_reader_that_fell_behind() {
        let mut l = Log::new(4);
        let seen = l.ver();
        for i in 0..4 {
            l.push(Change::Insert(i));
        }
        assert_eq!(l.since(seen).unwrap().count(), 4);
        l.push(Change::Remove(0));
        assert!(l.since(seen).is_none(), "five changes, four kept");
        assert_eq!(
            l.since(seen + 1).unwrap().collect::<Vec<_>>().last(),
            Some(&Change::Remove(0))
        );
        let at = l.ver();
        assert_eq!(l.since(at).unwrap().count(), 0);
        l.reset();
        assert!(l.since(at).is_none());
        assert!(l.since(l.ver() + 1).is_none(), "a cursor from another log");
    }
    #[test]
    fn a_table_reuses_ids_and_counts_live_rows() {
        let mut t = Table::new();
        let a = t.insert("a");
        let b = t.insert("b");
        assert_eq!(t.remove(a), Some("a"));
        assert_eq!(t.remove(a), None);
        let c = t.insert("c");
        assert_eq!((c, t.len(), t.slots()), (a, 2, 2));
        *t.edit(b).unwrap() = "B";
        assert_eq!(t.iter().collect::<Vec<_>>(), vec![(0, &"c"), (1, &"B")]);
        assert!(t.edit(9).is_none());
    }
    #[test]
    fn a_seq_notes_its_edits() {
        let mut s = Seq::new((0..300u32).collect());
        let seen = s.journal.ver;
        s.insert(150, 7);
        *s.edit(3) = 9;
        assert_eq!(s.remove(0), 0);
        let (from, upd, n) = s.journal.since(seen);
        assert_eq!((from, &upd[..n]), (0, &[3u32][..]));
        assert_eq!((s.len(), *s.get(2), *s.get(149)), (300, 9, 7));
    }
}
