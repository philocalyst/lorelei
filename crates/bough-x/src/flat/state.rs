//! The state block: every word a description can read, in one vector, written through typed handles.
//!
//! A [`Slot<T>`] is an index and the brand of the block that issued it: using it on another block is a checked
//! refusal ([`WrongBlock`]), not a read of someone else's word. Writing a word that differs sets its dirty bit;
//! [`StateBlock::drain_changed`] visits the dirty words once per turn, reports the ones that differ from what the
//! last turn saw (a word written away and back is not a change) and clears the bits. There is no `unsafe` here.
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU32, Ordering};

/// A state word's index in its block.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct SlotId(pub u16);

/// A typed handle to one word of one block.
pub struct Slot<T> {
    id: SlotId,
    brand: u32,
    _t: PhantomData<fn() -> T>,
}
impl<T> Clone for Slot<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Slot<T> {}
impl<T> std::fmt::Debug for Slot<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Slot({}@{})", self.id.0, self.brand)
    }
}
impl<T> Slot<T> {
    pub fn id(self) -> SlotId {
        self.id
    }
}

/// What a state word can hold: any value that is one 32-bit word.
pub trait SlotValue: Copy {
    fn to_word(self) -> u32;
    fn from_word(w: u32) -> Self;
}
impl SlotValue for u32 {
    fn to_word(self) -> u32 {
        self
    }
    fn from_word(w: u32) -> u32 {
        w
    }
}
impl SlotValue for i32 {
    fn to_word(self) -> u32 {
        self as u32
    }
    fn from_word(w: u32) -> i32 {
        w as i32
    }
}
impl SlotValue for bool {
    fn to_word(self) -> u32 {
        self as u32
    }
    fn from_word(w: u32) -> bool {
        w != 0
    }
}
impl SlotValue for f32 {
    fn to_word(self) -> u32 {
        self.to_bits()
    }
    fn from_word(w: u32) -> f32 {
        f32::from_bits(w)
    }
}

/// A handle was used on a block that did not issue it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WrongBlock;

/// A string of the side table: its text lives beside the words, and a generation word says when it changed.
#[derive(Clone, Copy, Debug)]
pub struct StrSlot {
    pub(crate) generation: Slot<u32>,
    pub(crate) index: u32,
}
impl StrSlot {
    pub fn id(self) -> SlotId {
        self.generation.id
    }
}

static BRANDS: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Debug)]
pub struct StateBlock {
    brand: u32,
    words: Vec<u32>,
    /// The words as the last drained turn saw them.
    prev: Vec<u32>,
    dirty_bits: Vec<u64>,
    any_dirty: bool,
    strings: Vec<String>,
}

impl Default for StateBlock {
    fn default() -> Self {
        Self::new()
    }
}

impl StateBlock {
    pub fn new() -> StateBlock {
        StateBlock { brand: BRANDS.fetch_add(1, Ordering::Relaxed), words: Vec::new(), prev: Vec::new(), dirty_bits: Vec::new(), any_dirty: false, strings: Vec::new() }
    }
    /// A copy that is its own block: handles of `self` are refused by it unless taken through [`StateBlock::adopt`].
    pub fn rebrand(&mut self) {
        self.brand = BRANDS.fetch_add(1, Ordering::Relaxed);
    }
    pub fn brand(&self) -> u32 {
        self.brand
    }
    pub fn len(&self) -> usize {
        self.words.len()
    }
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }
    /// A new word holding `init`. Panics past 65,535 words (a slot id is 16 bits).
    pub fn alloc<T: SlotValue>(&mut self, init: T) -> Slot<T> {
        let i = self.words.len();
        assert!(i < u16::MAX as usize, "a state block holds at most 65,535 words");
        self.words.push(init.to_word());
        self.prev.push(init.to_word());
        if self.dirty_bits.len() * 64 <= i {
            self.dirty_bits.push(0);
        }
        Slot { id: SlotId(i as u16), brand: self.brand, _t: PhantomData }
    }
    pub fn alloc_str(&mut self, init: &str) -> StrSlot {
        let generation = self.alloc(0u32);
        self.strings.push(init.to_string());
        StrSlot { generation, index: self.strings.len() as u32 - 1 }
    }
    #[inline]
    fn check<T>(&self, s: Slot<T>) -> Result<usize, WrongBlock> {
        if s.brand == self.brand && (s.id.0 as usize) < self.words.len() { Ok(s.id.0 as usize) } else { Err(WrongBlock) }
    }
    /// Read a word. A handle of another block is refused.
    #[inline]
    pub fn try_get<T: SlotValue>(&self, s: Slot<T>) -> Result<T, WrongBlock> {
        Ok(T::from_word(self.words[self.check(s)?]))
    }
    #[inline]
    #[track_caller]
    pub fn get<T: SlotValue>(&self, s: Slot<T>) -> T {
        self.try_get(s).expect("a slot of this state block")
    }
    /// Write a word. Returns whether it changed.
    #[inline]
    pub fn try_set<T: SlotValue>(&mut self, s: Slot<T>, v: T) -> Result<bool, WrongBlock> {
        let i = self.check(s)?;
        Ok(self.set_word(SlotId(i as u16), v.to_word()))
    }
    #[inline]
    #[track_caller]
    pub fn set<T: SlotValue>(&mut self, s: Slot<T>, v: T) -> bool {
        self.try_set(s, v).expect("a slot of this state block")
    }
    /// Replace a string. Allocates only if the new text is longer than any the slot has held.
    #[track_caller]
    pub fn set_str(&mut self, s: StrSlot, v: &str) -> bool {
        let g = self.get(s.generation);
        let t = &mut self.strings[s.index as usize];
        if t == v {
            return false;
        }
        t.clear();
        t.push_str(v);
        self.set(s.generation, g.wrapping_add(1))
    }
    pub fn str(&self, s: StrSlot) -> &str {
        &self.strings[s.index as usize]
    }
    pub(crate) fn str_at(&self, index: u32) -> &str {
        &self.strings[index as usize]
    }
    /// The word at `id` (0 past the end: an expression naming a word that does not exist reads nothing).
    #[inline]
    pub fn word(&self, id: SlotId) -> u32 {
        self.words.get(id.0 as usize).copied().unwrap_or(0)
    }
    /// Write the word at `id` (untyped: the driver's own words). Returns whether it changed.
    #[inline]
    pub fn set_word(&mut self, id: SlotId, w: u32) -> bool {
        let i = id.0 as usize;
        if self.words[i] == w {
            return false;
        }
        self.words[i] = w;
        self.dirty_bits[i >> 6] |= 1 << (i & 63);
        self.any_dirty = true;
        true
    }
    #[inline]
    pub fn any_dirty(&self) -> bool {
        self.any_dirty
    }
    /// Visit every word that differs from what the last drain saw as `(slot, old, new)`, and forget the dirt.
    pub fn drain_changed(&mut self, f: &mut dyn FnMut(SlotId, u32, u32)) {
        if !self.any_dirty {
            return;
        }
        self.any_dirty = false;
        for wi in 0..self.dirty_bits.len() {
            let mut bits = std::mem::take(&mut self.dirty_bits[wi]);
            while bits != 0 {
                let i = wi << 6 | bits.trailing_zeros() as usize;
                bits &= bits - 1;
                let (old, new) = (self.prev[i], self.words[i]);
                if old != new {
                    self.prev[i] = new;
                    f(SlotId(i as u16), old, new);
                }
            }
        }
    }
    /// Take every word as seen (a fresh mount has no changes to report).
    pub fn settle_all(&mut self) {
        self.prev.copy_from_slice(&self.words);
        self.dirty_bits.fill(0);
        self.any_dirty = false;
    }
}

/// The state words something reads, folded into 256 bits: exact for blocks of up to 256 words, a superset
/// (never a miss) beyond. The exact lists are the world's CSR index; this is the cheap first question.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DepMask(pub [u64; 4]);

impl DepMask {
    pub const EMPTY: DepMask = DepMask([0; 4]);
    #[inline]
    pub fn add(&mut self, s: SlotId) {
        let b = s.0 as usize & 255;
        self.0[b >> 6] |= 1 << (b & 63);
    }
    #[inline]
    pub fn has(&self, s: SlotId) -> bool {
        let b = s.0 as usize & 255;
        self.0[b >> 6] >> (b & 63) & 1 != 0
    }
    #[inline]
    pub fn or(&mut self, o: &DepMask) {
        for k in 0..4 {
            self.0[k] |= o.0[k];
        }
    }
    #[inline]
    pub fn hits(&self, o: &DepMask) -> bool {
        (0..4).any(|k| self.0[k] & o.0[k] != 0)
    }
    pub fn is_empty(&self) -> bool {
        self.0 == [0; 4]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slot_of_another_block_is_refused() {
        let (mut a, mut b) = (StateBlock::new(), StateBlock::new());
        let x = a.alloc(7u32);
        let _y = b.alloc(9u32);
        assert_eq!(a.get(x), 7);
        assert_eq!(b.try_get(x), Err(WrongBlock));
        assert_eq!(b.try_set(x, 1), Err(WrongBlock));
        assert_eq!(b.word(SlotId(0)), 9, "the refused write wrote nothing");
        // a clone is the same block until it is rebranded
        let mut c = a.clone();
        assert_eq!(c.get(x), 7);
        c.rebrand();
        assert_eq!(c.try_get(x), Err(WrongBlock));
    }

    #[test]
    fn a_drain_reports_net_changes_once() {
        let mut s = StateBlock::new();
        let a = s.alloc(1i32);
        let b = s.alloc(false);
        let c = s.alloc(2.5f32);
        for _ in 0..70 {
            s.alloc(0u32);
        }
        let far = s.alloc(5u32);
        assert!(!s.any_dirty());
        assert!(s.set(a, -4));
        assert!(!s.set(a, -4));
        assert!(s.set(b, true));
        assert!(s.set(b, false), "written back: dirty, but no net change");
        assert!(s.set(far, 6));
        let mut seen = Vec::new();
        s.drain_changed(&mut |id, old, new| seen.push((id.0, old, new)));
        assert_eq!(seen, vec![(0, 1, (-4i32) as u32), (73, 5, 6)]);
        seen.clear();
        s.drain_changed(&mut |id, old, new| seen.push((id.0, old, new)));
        assert!(seen.is_empty());
        assert_eq!((s.get(a), s.get(b), s.get(c)), (-4, false, 2.5));
    }

    #[test]
    fn a_string_changes_through_its_generation_word() {
        let mut s = StateBlock::new();
        let t = s.alloc_str("one");
        assert!(!s.set_str(t, "one"));
        assert!(s.set_str(t, "two"));
        assert_eq!(s.str(t), "two");
        let mut n = 0;
        s.drain_changed(&mut |id, _, _| {
            assert_eq!(id, t.id());
            n += 1
        });
        assert_eq!(n, 1);
    }

    #[test]
    fn a_dep_mask_is_exact_under_256_words_and_never_misses_beyond() {
        let mut m = DepMask::EMPTY;
        m.add(SlotId(3));
        m.add(SlotId(200));
        m.add(SlotId(300));
        assert!(m.has(SlotId(3)) && m.has(SlotId(200)) && m.has(SlotId(300)));
        assert!(!m.has(SlotId(4)) && !m.has(SlotId(201)));
        assert!(m.has(SlotId(44)), "300 folds onto 44: a superset, not a miss");
        let mut o = DepMask::EMPTY;
        o.add(SlotId(200));
        assert!(m.hits(&o));
    }
}
