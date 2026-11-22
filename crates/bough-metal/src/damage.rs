//! Damage planes (16x16 device-pixel tile bitmaps), a history ring for buffer-age repair,
//! and tile-plane -> rect-list extraction.

use crate::instance::*;

pub const TILE: i32 = 16;
pub const TX: usize = (W / TILE) as usize; // 120
pub const TY: usize = (H / TILE) as usize; // 90
pub const WORDS_PER_ROW: usize = 2; // 128 bits, tiles 120..127 are phantom
const _: () = assert!(TX <= WORDS_PER_ROW * 64);

#[derive(Clone)]
pub struct Plane {
    pub rows: [[u64; WORDS_PER_ROW]; TY],
    pub any: bool,
    pub full: bool,
    /// inclusive row range that may hold set bits (rows outside are stale/undefined)
    pub r0: u8,
    pub r1: u8,
}

impl Plane {
    pub const fn new() -> Self {
        Plane { rows: [[0; WORDS_PER_ROW]; TY], any: false, full: false, r0: 255, r1: 0 }
    }
    #[inline]
    pub fn clear(&mut self) {
        if self.any {
            for y in self.r0 as usize..=self.r1 as usize {
                self.rows[y] = [0; WORDS_PER_ROW];
            }
            self.any = false;
            self.full = false;
            self.r0 = 255;
            self.r1 = 0;
        }
    }
    /// Copy only the live row range.
    #[inline]
    pub fn copy_from(&mut self, o: &Plane) {
        self.any = o.any;
        self.full = o.full;
        self.r0 = o.r0;
        self.r1 = o.r1;
        if o.any {
            self.rows[o.r0 as usize..=o.r1 as usize].copy_from_slice(&o.rows[o.r0 as usize..=o.r1 as usize]);
        }
    }
    #[inline]
    pub fn add_rect(&mut self, r: &IRect) {
        let r = r.intersect(&IRect::FULL);
        if r.is_empty() {
            return;
        }
        self.any = true;
        let tx0 = (r.x0 / TILE) as u32;
        let tx1 = ((r.x1 - 1) / TILE) as u32; // inclusive
        let ty0 = (r.y0 / TILE) as usize;
        let ty1 = ((r.y1 - 1) / TILE) as usize;
        // row mask over 128 tiles as two words
        let mut m = [0u64; 2];
        for w in 0..2u32 {
            let lo = w * 64;
            let a = tx0.max(lo);
            let b = tx1.min(lo + 63);
            if a <= b {
                let n = b - a + 1;
                let bits = if n == 64 { !0u64 } else { ((1u64 << n) - 1) << (a - lo) };
                m[w as usize] = bits;
            }
        }
        self.r0 = self.r0.min(ty0 as u8);
        self.r1 = self.r1.max(ty1 as u8);
        for y in ty0..=ty1 {
            self.rows[y][0] |= m[0];
            self.rows[y][1] |= m[1];
        }
    }
    pub fn add_full(&mut self) {
        self.add_rect(&IRect::FULL);
        self.full = true;
    }
    #[inline]
    pub fn or_with(&mut self, o: &Plane) {
        if !o.any {
            return;
        }
        // rows outside [r0,r1] of `self` are stale only if self was never `any`; clear them lazily
        if !self.any {
            self.r0 = o.r0;
            self.r1 = o.r1;
            self.any = true;
            self.full = o.full;
            self.rows[o.r0 as usize..=o.r1 as usize].copy_from_slice(&o.rows[o.r0 as usize..=o.r1 as usize]);
            return;
        }
        let (n0, n1) = (self.r0.min(o.r0), self.r1.max(o.r1));
        // widen: rows newly inside self's range must start from zero
        for y in n0..self.r0.min(n1 + 1) {
            self.rows[y as usize] = [0; WORDS_PER_ROW];
        }
        for y in (self.r1 + 1).max(n0)..=n1 {
            self.rows[y as usize] = [0; WORDS_PER_ROW];
        }
        self.r0 = n0;
        self.r1 = n1;
        self.full |= o.full;
        for y in o.r0 as usize..=o.r1 as usize {
            self.rows[y][0] |= o.rows[y][0];
            self.rows[y][1] |= o.rows[y][1];
        }
    }

    /// Dilate every set tile by `radius_px` pixels (for Gaussian shadows and toroidal backdrop blur hulls).
    pub fn dilate(&mut self, radius_px: i32) {
        if !self.any || radius_px <= 0 {
            return;
        }
        let rt = ((radius_px + TILE - 1) / TILE) as usize;
        if rt == 0 {
            return;
        }
        let mut h_rows = [[0u64; WORDS_PER_ROW]; TY];
        let r0 = self.r0 as usize;
        let r1 = self.r1 as usize;
        // Horizontal dilation across the two 64-bit words (120 valid columns).
        let valid_hi_mask: u64 = if TX >= 128 { !0u64 } else { (1u64 << (TX - 64)) - 1 };
        for y in r0..=r1 {
            let mut w0 = self.rows[y][0];
            let mut w1 = self.rows[y][1] & valid_hi_mask;
            for _ in 0..rt {
                let n0 = w0 | (w0 << 1) | (w0 >> 1) | ((w1 & 1) << 63);
                let n1 = (w1 | (w1 << 1) | (w1 >> 1) | (w0 >> 63)) & valid_hi_mask;
                w0 = n0;
                w1 = n1;
            }
            h_rows[y] = [w0, w1];
        }
        // Vertical dilation across rows.
        let nr0 = r0.saturating_sub(rt);
        let nr1 = (r1 + rt).min(TY - 1);
        for y in nr0..=nr1 {
            let mut o0 = 0u64;
            let mut o1 = 0u64;
            let sy0 = y.saturating_sub(rt).max(r0);
            let sy1 = (y + rt).min(r1);
            if sy0 <= sy1 {
                for sy in sy0..=sy1 {
                    o0 |= h_rows[sy][0];
                    o1 |= h_rows[sy][1];
                }
            }
            self.rows[y] = [o0, o1];
        }
        self.r0 = nr0 as u8;
        self.r1 = nr1 as u8;
    }

    /// Clear tiles strictly contained inside an opaque rect (front-to-back opaque cover culling).
    pub fn subtract_opaque_rect(&mut self, opaque: &IRect) {
        if !self.any {
            return;
        }
        let r = opaque.intersect(&IRect::FULL);
        if r.is_empty() {
            return;
        }
        let tx0 = ((r.x0 + TILE - 1) / TILE) as u32;
        let tx1 = (r.x1 / TILE) as u32; // exclusive
        let ty0 = ((r.y0 + TILE - 1) / TILE) as usize;
        let ty1 = (r.y1 / TILE) as usize; // exclusive
        if tx0 >= tx1 || ty0 >= ty1 {
            return;
        }
        let mut m = [0u64; 2];
        for w in 0..2u32 {
            let lo = w * 64;
            let a = tx0.max(lo);
            let b = (tx1 - 1).min(lo + 63);
            if a <= b {
                let n = b - a + 1;
                let bits = if n == 64 { !0u64 } else { ((1u64 << n) - 1) << (a - lo) };
                m[w as usize] = bits;
            }
        }
        let y_start = ty0.max(self.r0 as usize);
        let y_end = (ty1 - 1).min(self.r1 as usize);
        if y_start > y_end {
            return;
        }
        for y in y_start..=y_end {
            self.rows[y][0] &= !m[0];
            self.rows[y][1] &= !m[1];
        }
        // Tighten live row bounds.
        let mut nr0 = 255u8;
        let mut nr1 = 0u8;
        let mut any = false;
        for y in self.r0 as usize..=self.r1 as usize {
            if self.rows[y][0] != 0 || self.rows[y][1] != 0 {
                if !any {
                    nr0 = y as u8;
                    any = true;
                }
                nr1 = y as u8;
            }
        }
        self.any = any;
        self.full = false;
        self.r0 = nr0;
        self.r1 = nr1;
    }

    /// Shift tile damage inside `clip` by `(dx, dy)` pixels and mark the newly exposed strip
    /// `clip \ (clip + (dx, dy))` as dirty (rigid-scroll copy-forward damage propagation).
    pub fn shift_rect(&mut self, clip: &IRect, dx: i32, dy: i32) {
        let clip = clip.intersect(&IRect::FULL);
        if clip.is_empty() || (dx == 0 && dy == 0) {
            return;
        }
        let shifted = IRect::new(clip.x0 + dx, clip.y0 + dy, clip.x1 + dx, clip.y1 + dy);
        let dst = clip.intersect(&shifted);
        if dst.is_empty() {
            self.add_rect(&clip);
            return;
        }
        // Snapshot existing set bits inside `clip` and remap each set tile rect shifted by (dx, dy).
        let mut shifted_rects: Vec<IRect> = Vec::new();
        if self.any {
            let tx0 = (clip.x0 / TILE) as usize;
            let tx1 = ((clip.x1 - 1) / TILE) as usize;
            let ty0 = ((clip.y0 / TILE) as usize).max(self.r0 as usize);
            let ty1 = (((clip.y1 - 1) / TILE) as usize).min(self.r1 as usize);
            if ty0 <= ty1 {
                for ty in ty0..=ty1 {
                    for tx in tx0..=tx1 {
                        let w = tx >> 6;
                        let bit = tx & 63;
                        if (self.rows[ty][w] >> bit) & 1 != 0 {
                            let tr = IRect::new(
                                tx as i32 * TILE + dx,
                                ty as i32 * TILE + dy,
                                (tx as i32 + 1) * TILE + dx,
                                (ty as i32 + 1) * TILE + dy,
                            ).intersect(&dst);
                            if !tr.is_empty() {
                                shifted_rects.push(tr);
                            }
                        }
                    }
                }
            }
            // Clear tiles strictly inside `dst` before re-marking shifted damage.
            self.subtract_opaque_rect(&dst);
            for r in shifted_rects {
                self.add_rect(&r);
            }
        }
        // Mark the newly exposed strip(s): `clip \ dst`.
        if dst.y0 > clip.y0 {
            self.add_rect(&IRect::new(clip.x0, clip.y0, clip.x1, dst.y0));
        }
        if dst.y1 < clip.y1 {
            self.add_rect(&IRect::new(clip.x0, dst.y1, clip.x1, clip.y1));
        }
        let my0 = clip.y0.max(dst.y0);
        let my1 = clip.y1.min(dst.y1);
        if dst.x0 > clip.x0 {
            self.add_rect(&IRect::new(clip.x0, my0, dst.x0, my1));
        }
        if dst.x1 < clip.x1 {
            self.add_rect(&IRect::new(dst.x1, my0, clip.x1, my1));
        }
    }
}

pub const HIST: usize = 16;

/// Ring of the last HIST epochs' damage planes. Epoch e lives in slot e % HIST. Epoch 0 is "never drawn".
pub struct History {
    pub planes: Vec<Plane>,
    pub epoch: u64,
}

impl History {
    pub fn new() -> Self {
        History { planes: vec![Plane::new(); HIST], epoch: 0 }
    }
    /// Commit `acc` as the next epoch's damage and clear it.
    pub fn push(&mut self, acc: &mut Plane) -> u64 {
        self.epoch += 1;
        self.planes[(self.epoch % HIST as u64) as usize].copy_from(acc);
        acc.clear();
        self.epoch
    }
    /// Union of damage for epochs (since, epoch]. `None` = too old / never drawn -> full repaint.
    pub fn repair(&self, since: Option<u64>, out: &mut Plane) -> bool {
        out.clear();
        let Some(since) = since else { return false };
        if self.epoch - since >= HIST as u64 {
            return false;
        }
        for e in since + 1..=self.epoch {
            out.or_with(&self.planes[(e % HIST as u64) as usize]);
        }
        true
    }
}

/// A repair region extracted from a plane.
#[derive(Default)]
pub struct Region {
    pub full: bool,
    pub bbox: IRect,
    pub rects: Vec<IRect>,
}

impl Region {
    pub fn full() -> Self {
        Region { full: true, bbox: IRect::FULL, rects: vec![IRect::FULL] }
    }
    pub fn set_full(&mut self) {
        self.full = true;
        self.bbox = IRect::FULL;
        self.rects.clear();
        self.rects.push(IRect::FULL);
    }
    /// Extract tile-aligned rects (horizontal runs merged vertically) from a plane. No allocation.
    pub fn from_plane(&mut self, p: &Plane) {
        const CAP: usize = 64;
        self.full = false;
        self.rects.clear();
        let mut bb = IRect::EMPTY;
        let mut overflow = false;
        // (tile x0, tile x1, start row)
        let mut prev = [(0u32, 0u32, 0i32); CAP];
        let mut np = 0usize;
        let mut cur = [(0u32, 0u32, 0i32); CAP];
        if !p.any {
            self.bbox = IRect::EMPTY;
            return;
        }
        for ty in p.r0 as usize..=p.r1 as usize + 1 {
            let mut nc = 0usize;
            if ty <= p.r1 as usize {
                for w in 0..WORDS_PER_ROW {
                    let mut bits = p.rows[ty][w];
                    while bits != 0 {
                        let s = bits.trailing_zeros();
                        let run = (bits >> s).trailing_ones();
                        let (a, b) = (w as u32 * 64 + s, w as u32 * 64 + s + run);
                        bits &= if s + run >= 64 { 0 } else { !(((1u64 << run) - 1) << s) };
                        if nc > 0 && cur[nc - 1].1 == a {
                            cur[nc - 1].1 = b; // run crosses the word boundary
                        } else if nc < CAP {
                            cur[nc] = (a, b, ty as i32);
                            nc += 1;
                        } else {
                            overflow = true;
                        }
                    }
                }
            }
            // runs identical to an open run continue it; open runs not continued are emitted
            let mut matched = [false; CAP];
            for c in cur.iter_mut().take(nc) {
                for pi in 0..np {
                    if !matched[pi] && prev[pi].0 == c.0 && prev[pi].1 == c.1 {
                        matched[pi] = true;
                        c.2 = prev[pi].2;
                        break;
                    }
                }
            }
            for pi in 0..np {
                if !matched[pi] {
                    let pr = prev[pi];
                    let r = IRect::new(pr.0 as i32 * TILE, pr.2 * TILE, pr.1 as i32 * TILE, ty as i32 * TILE);
                    self.rects.push(r);
                    bb = bb.union(&r);
                }
            }
            prev = cur;
            np = nc;
        }
        self.bbox = bb.intersect(&IRect::FULL);
        if overflow || self.rects.len() > MAX_RECTS {
            if overflow {
                bb = IRect::EMPTY;
                for ty in p.r0 as usize..=p.r1 as usize {
                    for w in 0..WORDS_PER_ROW {
                        let b = p.rows[ty][w];
                        if b != 0 {
                            let x0 = (w as u32 * 64 + b.trailing_zeros()) as i32 * TILE;
                            let x1 = (w as u32 * 64 + 64 - b.leading_zeros()) as i32 * TILE;
                            bb = bb.union(&IRect::new(x0, ty as i32 * TILE, x1, ty as i32 * TILE + TILE));
                        }
                    }
                }
                self.bbox = bb.intersect(&IRect::FULL);
            }
            self.rects.clear();
            self.rects.push(self.bbox);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plane_dilate_expands_by_tile_radius() {
        let mut p = Plane::new();
        p.add_rect(&IRect::new(64, 64, 80, 80)); // tile (4, 4)
        p.dilate(16); // +1 tile in every direction -> [3..=5] x [3..=5]
        assert!(p.any);
        assert_eq!(p.r0, 3);
        assert_eq!(p.r1, 5);
        for y in 3..=5 {
            assert_eq!(p.rows[y][0], 0b111 << 3);
        }
    }

    #[test]
    fn plane_subtract_opaque_clears_interior_tiles() {
        let mut p = Plane::new();
        p.add_rect(&IRect::new(0, 0, 128, 128));
        p.subtract_opaque_rect(&IRect::new(16, 16, 112, 112));
        assert!(p.any);
        // Interior tile (2, 2) must be cleared; border tile (0, 0) remains set.
        assert_eq!((p.rows[2][0] >> 2) & 1, 0);
        assert_eq!(p.rows[0][0] & 1, 1);
    }

    #[test]
    fn plane_shift_rect_marks_only_exposed_strip() {
        let mut p = Plane::new();
        let clip = IRect::new(0, 0, 320, 320);
        p.shift_rect(&clip, 0, -16); // scroll down 16 px -> bottom 16 px exposed
        assert!(p.any);
        let mut reg = Region::default();
        reg.from_plane(&p);
        assert_eq!(reg.bbox, IRect::new(0, 304, 320, 320));
    }
}
