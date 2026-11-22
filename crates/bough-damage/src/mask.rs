//! The tile mask: one bit per 32 x 32 tile under a coarse mask of super-tiles.
use crate::{Rect, SUPER_SHIFT, TILE, TILE_SHIFT};

/// A window's dirty tiles: one bit per 32 x 32 tile in 64-bit row words, and one bit per 256 x 256 super-tile
/// saying "some tile of this super-tile is set". Marking is O(1) per row word touched; iteration and clearing
/// visit only super-tiles that have a bit; the count is kept, so the repaired fraction is a division.
#[derive(Clone, Debug, Default)]
pub struct TileMask {
    w: i32,
    h: i32,
    tw: u32,
    th: u32,
    /// Words per tile row.
    wpr: usize,
    bits: Vec<u64>,
    sw: u32,
    sh: u32,
    /// Words per super-tile row.
    cpr: usize,
    coarse: Vec<u64>,
    count: u32,
}

#[inline]
fn range_mask(lo: u32, hi: u32) -> u64 {
    // bits lo..=hi of a word, 0 <= lo <= hi <= 63
    (u64::MAX >> (63 - (hi - lo))) << lo
}

impl TileMask {
    pub fn new(w: i32, h: i32) -> TileMask {
        let mut m = TileMask::default();
        m.resize(w, h);
        m
    }
    /// Take the size of a window of `w` x `h` pixels; every bit is cleared.
    pub fn resize(&mut self, w: i32, h: i32) {
        let (w, h) = (w.max(0), h.max(0));
        self.w = w;
        self.h = h;
        self.tw = ((w + TILE - 1) >> TILE_SHIFT) as u32;
        self.th = ((h + TILE - 1) >> TILE_SHIFT) as u32;
        self.wpr = (self.tw as usize).div_ceil(64);
        self.sw = self.tw.div_ceil(1 << SUPER_SHIFT);
        self.sh = self.th.div_ceil(1 << SUPER_SHIFT);
        self.cpr = (self.sw as usize).div_ceil(64);
        self.bits.clear();
        self.bits.resize(self.wpr * self.th as usize, 0);
        self.coarse.clear();
        self.coarse.resize(self.cpr * self.sh as usize, 0);
        self.count = 0;
    }
    pub fn size(&self) -> (i32, i32) {
        (self.w, self.h)
    }
    pub fn tiles_wide(&self) -> u32 {
        self.tw
    }
    pub fn tiles_high(&self) -> u32 {
        self.th
    }
    /// Tiles set.
    #[inline]
    pub fn count(&self) -> u32 {
        self.count
    }
    #[inline]
    pub fn total(&self) -> u32 {
        self.tw * self.th
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// The fraction of the window's tiles that are set, 0.0 ..= 1.0.
    pub fn fraction(&self) -> f32 {
        if self.total() == 0 { 0.0 } else { self.count as f32 / self.total() as f32 }
    }
    #[inline]
    pub fn test(&self, tx: u32, ty: u32) -> bool {
        tx < self.tw && ty < self.th && self.bits[ty as usize * self.wpr + (tx >> 6) as usize] >> (tx & 63) & 1 != 0
    }
    /// The pixel rect of tile (tx, ty), cut to the window.
    #[inline]
    pub fn tile_rect(&self, tx: u32, ty: u32) -> Rect {
        Rect {
            x0: (tx as i32) << TILE_SHIFT,
            y0: (ty as i32) << TILE_SHIFT,
            x1: (((tx + 1) as i32) << TILE_SHIFT).min(self.w),
            y1: (((ty + 1) as i32) << TILE_SHIFT).min(self.h),
        }
    }
    /// The inclusive tile range a pixel rect touches, or `None` if it misses the window.
    #[inline]
    pub fn tile_range(&self, r: Rect) -> Option<(u32, u32, u32, u32)> {
        let r = r.and(Rect { x0: 0, y0: 0, x1: self.w, y1: self.h });
        if r.is_empty() {
            return None;
        }
        Some((
            (r.x0 >> TILE_SHIFT) as u32,
            (r.y0 >> TILE_SHIFT) as u32,
            ((r.x1 - 1) >> TILE_SHIFT) as u32,
            ((r.y1 - 1) >> TILE_SHIFT) as u32,
        ))
    }
    /// Set one tile. Returns whether it was clear.
    #[inline]
    pub fn mark_tile(&mut self, tx: u32, ty: u32) -> bool {
        if tx >= self.tw || ty >= self.th {
            return false;
        }
        let w = &mut self.bits[ty as usize * self.wpr + (tx >> 6) as usize];
        let b = 1u64 << (tx & 63);
        if *w & b != 0 {
            return false;
        }
        *w |= b;
        self.count += 1;
        let (sx, sy) = (tx >> SUPER_SHIFT, ty >> SUPER_SHIFT);
        self.coarse[sy as usize * self.cpr + (sx >> 6) as usize] |= 1 << (sx & 63);
        true
    }
    /// Set every tile a pixel rect touches. Returns how many were clear.
    pub fn mark_rect(&mut self, r: Rect) -> u32 {
        let Some((tx0, ty0, tx1, ty1)) = self.tile_range(r) else {
            return 0;
        };
        let mut new = 0;
        for ty in ty0..=ty1 {
            let row = ty as usize * self.wpr;
            for wi in (tx0 >> 6)..=(tx1 >> 6) {
                let lo = if wi == tx0 >> 6 { tx0 & 63 } else { 0 };
                let hi = if wi == tx1 >> 6 { tx1 & 63 } else { 63 };
                let m = range_mask(lo, hi);
                let w = &mut self.bits[row + wi as usize];
                new += (m & !*w).count_ones();
                *w |= m;
            }
        }
        self.count += new;
        let (sx0, sx1) = (tx0 >> SUPER_SHIFT, tx1 >> SUPER_SHIFT);
        for sy in (ty0 >> SUPER_SHIFT)..=(ty1 >> SUPER_SHIFT) {
            let row = sy as usize * self.cpr;
            for wi in (sx0 >> 6)..=(sx1 >> 6) {
                let lo = if wi == sx0 >> 6 { sx0 & 63 } else { 0 };
                let hi = if wi == sx1 >> 6 { sx1 & 63 } else { 63 };
                self.coarse[row + wi as usize] |= range_mask(lo, hi);
            }
        }
        new
    }
    pub fn mark_all(&mut self) {
        self.mark_rect(Rect { x0: 0, y0: 0, x1: self.w, y1: self.h });
    }
    /// Set every tile `o` has set (same window size).
    pub fn or(&mut self, o: &TileMask) {
        assert!(self.w == o.w && self.h == o.h, "two masks of one window");
        if o.count == 0 {
            return;
        }
        let mut n = 0;
        for (a, b) in self.bits.iter_mut().zip(&o.bits) {
            *a |= *b;
            n += a.count_ones();
        }
        self.count = n;
        for (a, b) in self.coarse.iter_mut().zip(&o.coarse) {
            *a |= *b;
        }
    }
    /// Clear every bit, visiting only super-tiles that have one.
    pub fn clear(&mut self) {
        if self.count == 0 && self.coarse.iter().all(|w| *w == 0) {
            return;
        }
        for sy in 0..self.sh {
            for cw in 0..self.cpr {
                let mut c = std::mem::take(&mut self.coarse[sy as usize * self.cpr + cw]);
                while c != 0 {
                    let sx = (cw as u32) << 6 | c.trailing_zeros();
                    c &= c - 1;
                    let tx0 = sx << SUPER_SHIFT;
                    let tx1 = (tx0 + (1 << SUPER_SHIFT) - 1).min(self.tw - 1);
                    for ty in (sy << SUPER_SHIFT)..((sy + 1) << SUPER_SHIFT).min(self.th) {
                        // a super-tile is 8 tiles wide and 8 divides 64: it never straddles a word
                        self.bits[ty as usize * self.wpr + (tx0 >> 6) as usize] &= !range_mask(tx0 & 63, tx1 & 63);
                    }
                }
            }
        }
        self.count = 0;
    }
    /// Every set tile, row by row, left to right.
    pub fn each_tile(&self, f: &mut dyn FnMut(u32, u32)) {
        self.each_run(&mut |ty, tx0, tx1| {
            for tx in tx0..=tx1 {
                f(tx, ty)
            }
        });
    }
    /// Every maximal horizontal run of set tiles as a pixel rect cut to the window: what an executor repaints.
    pub fn each_span(&self, f: &mut dyn FnMut(Rect)) {
        self.each_run(&mut |ty, tx0, tx1| {
            f(Rect {
                x0: (tx0 as i32) << TILE_SHIFT,
                y0: (ty as i32) << TILE_SHIFT,
                x1: (((tx1 + 1) as i32) << TILE_SHIFT).min(self.w),
                y1: (((ty + 1) as i32) << TILE_SHIFT).min(self.h),
            })
        });
    }
    /// Runs `(tile row, first tile, last tile)`. Rows of super-tiles with no bit are never entered.
    fn each_run(&self, f: &mut dyn FnMut(u32, u32, u32)) {
        if self.count == 0 {
            return;
        }
        for sy in 0..self.sh {
            let crow = &self.coarse[sy as usize * self.cpr..(sy as usize + 1) * self.cpr];
            if crow.iter().all(|w| *w == 0) {
                continue;
            }
            for ty in (sy << SUPER_SHIFT)..((sy + 1) << SUPER_SHIFT).min(self.th) {
                let row = &self.bits[ty as usize * self.wpr..(ty as usize + 1) * self.wpr];
                let mut run: Option<(u32, u32)> = None;
                for (wi, word) in row.iter().enumerate() {
                    let mut w = *word;
                    while w != 0 {
                        let lo = w.trailing_zeros();
                        let len = (w >> lo).trailing_ones();
                        let (a, b) = ((wi as u32) << 6 | lo, ((wi as u32) << 6 | lo) + len - 1);
                        w = if lo + len >= 64 { 0 } else { w & !range_mask(lo, lo + len - 1) };
                        run = match run {
                            Some((s, e)) if e + 1 == a => Some((s, b)),
                            Some((s, e)) => {
                                f(ty, s, e);
                                Some((a, b))
                            }
                            None => Some((a, b)),
                        };
                    }
                }
                if let Some((s, e)) = run {
                    f(ty, s, e);
                }
            }
        }
    }
}
