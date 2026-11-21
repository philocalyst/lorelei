//! The order lane: a sequence in gapped blocks, so that an insert or a remove moves the items of one block
//! and no others.
//!
//! Items live in blocks of at most [`CAP`]. A Fenwick tree over the blocks' lengths turns a position into a
//! block in `O(log blocks)` and takes an insert or a remove as one point update. A block that fills splits in
//! two; a block that empties is dropped; either rebuilds the tree (`O(blocks)`, once per `CAP / 2` inserts into
//! the same place, so amortised a few instructions per edit). A read of the position after the last one read is
//! answered from a cursor without the tree: a list's window is read in order.
//!
//! This is round 6's gapped directory applied to relations. It replaces a `Vec` whose insert moves a suffix.
use core::cell::Cell;

/// Items per block. 128 items of 4 to 40 bytes are 0.5 to 5 KiB: an insert moves on average half of that.
pub const CAP: usize = 128;

pub struct Lane<T> {
    blocks: Vec<Vec<T>>,
    /// Fenwick tree over `blocks[i].len()`, 1-based.
    fen: Vec<u32>,
    len: usize,
    /// The block last read and the position of its first item: a hint, checked before use.
    cursor: Cell<(u32, u32)>,
}

impl<T> Default for Lane<T> {
    fn default() -> Self {
        Lane::new()
    }
}

impl<T> Lane<T> {
    pub const fn new() -> Lane<T> {
        Lane {
            blocks: Vec::new(),
            fen: Vec::new(),
            len: 0,
            cursor: Cell::new((0, 0)),
        }
    }
    /// The items of `v`, in blocks three quarters full (room for inserts before the first split).
    pub fn from_vec(v: Vec<T>) -> Lane<T> {
        let mut l = Lane::new();
        l.len = v.len();
        let fill = CAP * 3 / 4;
        let mut it = v.into_iter();
        loop {
            let b: Vec<T> = it.by_ref().take(fill).collect();
            if b.is_empty() {
                break;
            }
            let mut blk = Vec::with_capacity(CAP);
            blk.extend(b);
            l.blocks.push(blk);
        }
        l.refen();
        l
    }
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len
    }
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn blocks(&self) -> usize {
        self.blocks.len()
    }
    /// Bytes held: the blocks' capacities, the block table and the tree.
    pub fn bytes(&self) -> usize {
        self.blocks
            .iter()
            .map(|b| b.capacity() * size_of::<T>())
            .sum::<usize>()
            + self.blocks.capacity() * size_of::<Vec<T>>()
            + self.fen.capacity() * 4
    }
    #[cold]
    #[inline(never)]
    fn refen(&mut self) {
        let n = self.blocks.len();
        self.fen.clear();
        self.fen.resize(n + 1, 0);
        for i in 0..n {
            self.fen[i + 1] += self.blocks[i].len() as u32;
            let j = (i + 1) + ((i + 1) & (i + 1).wrapping_neg());
            if j <= n {
                self.fen[j] += self.fen[i + 1];
            }
        }
        self.cursor.set((0, 0));
    }
    #[inline(always)]
    fn fen_add(&mut self, b: usize, d: i32) {
        let mut i = b + 1;
        while i < self.fen.len() {
            self.fen[i] = (self.fen[i] as i32 + d) as u32;
            i += i & i.wrapping_neg();
        }
    }
    /// The block that holds position `at` and the position of that block's first item. `at == len` names the
    /// end of the last block.
    #[inline]
    fn locate(&self, at: usize) -> (usize, usize) {
        debug_assert!(at <= self.len);
        let (cb, cs) = self.cursor.get();
        let (cb, cs) = (cb as usize, cs as usize);
        if cb < self.blocks.len() && at >= cs {
            let n = self.blocks[cb].len();
            if at < cs + n {
                return (cb, cs);
            }
            if cb + 1 < self.blocks.len() && at < cs + n + self.blocks[cb + 1].len() {
                self.cursor.set((cb as u32 + 1, (cs + n) as u32));
                return (cb + 1, cs + n);
            }
        }
        // Fenwick descent: the last block whose prefix sum is at most `at`.
        let n = self.blocks.len();
        let (mut pos, mut rem) = (0usize, at as u32);
        let mut step = if n == 0 {
            0
        } else {
            1usize << (usize::BITS - 1 - n.leading_zeros())
        };
        while step != 0 {
            let nx = pos + step;
            if nx <= n && self.fen[nx] <= rem {
                pos = nx;
                rem -= self.fen[nx];
            }
            step >>= 1;
        }
        // `pos` blocks lie wholly before `at`; `at == len` lands past the last block
        let (b, start) = if pos == n && n > 0 {
            (n - 1, at - rem as usize - self.blocks[n - 1].len() + 0)
        } else {
            (pos, at - rem as usize)
        };
        self.cursor.set((b as u32, start as u32));
        (b, start)
    }
    #[inline]
    pub fn get(&self, i: usize) -> &T {
        assert!(i < self.len, "position {i} of a lane of {}", self.len);
        let (b, s) = self.locate(i);
        &self.blocks[b][i - s]
    }
    #[inline]
    pub fn get_mut(&mut self, i: usize) -> &mut T {
        assert!(i < self.len, "position {i} of a lane of {}", self.len);
        let (b, s) = self.locate(i);
        &mut self.blocks[b][i - s]
    }
    pub fn push(&mut self, v: T) {
        self.insert(self.len, v)
    }
    pub fn insert(&mut self, at: usize, v: T) {
        assert!(at <= self.len, "insert at {at} into a lane of {}", self.len);
        if self.blocks.is_empty() {
            let mut b = Vec::with_capacity(CAP);
            b.push(v);
            self.blocks.push(b);
            self.len = 1;
            self.refen();
            return;
        }
        let (b, s) = self.locate(at);
        self.len += 1;
        if self.blocks[b].len() < CAP {
            self.blocks[b].insert(at - s, v);
            self.fen_add(b, 1);
            return;
        }
        // the block is full: its upper half becomes a block of its own
        let mut hi = Vec::with_capacity(CAP);
        hi.extend(self.blocks[b].drain(CAP / 2..));
        let k = at - s;
        if k <= CAP / 2 {
            self.blocks[b].insert(k, v)
        } else {
            hi.insert(k - CAP / 2, v)
        }
        self.blocks.insert(b + 1, hi);
        #[cfg(not(feature = "mutant_lane"))]
        self.refen();
    }
    pub fn remove(&mut self, at: usize) -> T {
        assert!(at < self.len, "remove at {at} from a lane of {}", self.len);
        let (b, s) = self.locate(at);
        let v = self.blocks[b].remove(at - s);
        self.len -= 1;
        if self.blocks[b].is_empty() {
            self.blocks.remove(b);
            self.refen();
        } else {
            self.fen_add(b, -1);
        }
        v
    }
    pub fn clear(&mut self) {
        self.blocks.clear();
        self.len = 0;
        self.refen();
    }
    pub fn iter(&self) -> impl Iterator<Item = &T> + '_ {
        self.blocks.iter().flat_map(|b| b.iter())
    }
    /// The items from position `lo` on.
    pub fn from(&self, lo: usize) -> impl Iterator<Item = &T> + '_ {
        let (b, s) = if lo >= self.len {
            (self.blocks.len(), 0)
        } else {
            self.locate(lo)
        };
        let skip = if b < self.blocks.len() { lo - s } else { 0 };
        self.blocks[b.min(self.blocks.len())..]
            .iter()
            .flat_map(|b| b.iter())
            .skip(skip)
    }
    /// For a lane kept in order: the first position whose item is not `less`. `less` is true for a prefix.
    pub fn lower_bound(&self, mut less: impl FnMut(&T) -> bool) -> usize {
        // the first block whose last item is not less holds the answer
        let (mut lo, mut hi) = (0usize, self.blocks.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            if less(self.blocks[mid].last().expect("no block is empty")) {
                lo = mid + 1
            } else {
                hi = mid
            }
        }
        if lo == self.blocks.len() {
            return self.len;
        }
        let start = self.start_of(lo);
        let blk = &self.blocks[lo];
        start + blk.partition_point(|t| less(t))
    }
    /// The position of block `b`'s first item (a prefix sum).
    fn start_of(&self, b: usize) -> usize {
        let (mut i, mut s) = (b, 0u32);
        while i > 0 {
            s += self.fen[i];
            i -= i & i.wrapping_neg();
        }
        s as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rng(s: &mut u64) -> u64 {
        *s ^= *s << 13;
        *s ^= *s >> 7;
        *s ^= *s << 17;
        *s
    }
    #[test]
    fn a_lane_is_a_vec_under_random_edits() {
        for seed in 1..6u64 {
            let mut s = seed * 0x9e37_79b9_7f4a_7c15;
            let init: Vec<u32> = (0..(seed as u32 * 97)).collect();
            let (mut l, mut v) = (Lane::from_vec(init.clone()), init);
            for step in 0..6000u32 {
                let r = rng(&mut s);
                match r % 10 {
                    0..=4 => {
                        // bias towards one place so blocks split, and towards the ends
                        let at = match r >> 8 & 3 {
                            0 => 0,
                            1 => v.len(),
                            2 => v.len() / 2,
                            _ => (r >> 16) as usize % (v.len() + 1),
                        };
                        l.insert(at, step);
                        v.insert(at, step);
                    }
                    5..=7 if !v.is_empty() => {
                        let at = (r >> 16) as usize % v.len();
                        assert_eq!(l.remove(at), v.remove(at));
                    }
                    _ if !v.is_empty() => {
                        let at = (r >> 16) as usize % v.len();
                        assert_eq!(*l.get(at), v[at], "seed {seed} step {step} at {at}");
                        // in order from there, through the cursor
                        for k in at..(at + 300).min(v.len()) {
                            assert_eq!(*l.get(k), v[k]);
                        }
                    }
                    _ => {}
                }
                assert_eq!(l.len(), v.len());
            }
            assert!(l.iter().copied().eq(v.iter().copied()));
            for lo in [0, 1, v.len() / 3, v.len().saturating_sub(1), v.len()] {
                assert!(l.from(lo).copied().eq(v[lo..].iter().copied()), "from {lo}");
            }
            // drain to nothing through removes at both ends
            while !v.is_empty() {
                let at = if v.len() % 2 == 0 { 0 } else { v.len() - 1 };
                assert_eq!(l.remove(at), v.remove(at));
            }
            assert!(l.is_empty() && l.blocks() == 0);
        }
    }
    #[test]
    fn lower_bound_is_partition_point() {
        let mut l = Lane::new();
        let mut v: Vec<u32> = Vec::new();
        let mut s = 7u64;
        for _ in 0..5000 {
            let x = (rng(&mut s) % 3000) as u32;
            let at = l.lower_bound(|&t| t < x);
            assert_eq!(at, v.partition_point(|&t| t < x));
            l.insert(at, x);
            v.insert(at, x);
        }
        for x in [0u32, 1, 1500, 2999, 3000, u32::MAX] {
            assert_eq!(l.lower_bound(|&t| t < x), v.partition_point(|&t| t < x));
        }
    }
}
