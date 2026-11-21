//! Layers: what a record that reads its backdrop owns.
//!
//! A backdrop record (`paint::Backdrop`) cannot be shaded from its own words: its pixels are a function of
//! everything beneath it over a neighbourhood. So it owns a **layer**: the flattened (8-bit) prefix of the
//! frame beneath it over its halo, and a pyramid of that prefix. A layer is retained and repaired, never
//! redrawn:
//!
//! * **Storage.** Each level is a *toroidal pixel window* in one pool: level-`l` pixel (x, y) lives at
//!   `base + (y mod ch) * cw + (x mod cw)`. Pixels are addressed by their place in the frame, not in the
//!   window, so when the record moves the pixels it still needs stay where they are and only the pixels
//!   that entered the window are new. Nothing is copied or shifted.
//! * **Damage.** A level is dirty by blocks that each cover one frame tile: at level `l` a block is
//!   `32 >> l` pixels a side (level 0's blocks are the frame's tiles; at the coarsest level a block is one
//!   pixel), so damage keeps the frame's grain at every level. A change of a record beneath the layer arrives as an event (a pixel window and a paint-order
//!   key); it dirties level-0 blocks; a dirty block dirties the blocks one level up whose [1 3 3 1] taps
//!   reach it (dilation in block space, computed from pixel ranges); the dirty blocks of the levels the
//!   record reads dirty the frame tiles whose bilinear taps reach them, which is output damage of the
//!   record and an event for the layers above it.
//! * **Repair** is the plan's stages for this layer, in order: prefix jobs (the tile program up to the
//!   record, stored into level 0), then one stage per level (dirty blocks downsampled from the level
//!   beneath). `Scene::prepare` runs them on the CPU, `metal::Gpu::staged` as dispatches.
//!
//! Idle: no events, no geometry change, nothing here runs.
use crate::ops::ops as x;
use bough_kernel::Cmd;
use wgsl_rs::std::{Vec4, Vec4i, Vec4u};

/// Levels a layer can have (0 ..= `BD_LMAX`).
pub const LEVELS: usize = x::BD_LMAX as usize + 1;
/// Words of a layer's row in the table the GPU reads: 4 of header, 4 per level.
pub const ROW: usize = 32;

/// One level's window.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Lvl {
    /// The level's pixels the layer needs (x0, y0, x1, y1), cut to the level's size.
    pub r: [i32; 4],
    /// The toroidal window's size in pixels and its first word in the pool.
    pub cw: u32,
    pub ch: u32,
    pub base: u32,
}
impl Lvl {
    pub fn empty(&self) -> bool {
        self.r[0] >= self.r[2] || self.r[1] >= self.r[3]
    }
}

/// Pixels a side of a block of level `l` (one frame tile's worth).
#[inline(always)]
pub fn bshift(l: u32) -> u32 {
    5 - l.min(5)
}
/// A set of blocks of one level (bits over the block range of its window).
#[derive(Clone, Default, Debug)]
pub struct Blocks {
    /// log2 of the block's side in the level's pixels.
    s: u32,
    x0: i32,
    y0: i32,
    w: i32,
    h: i32,
    bits: Vec<u64>,
    pub n: usize,
}
impl Blocks {
    /// Empty, over the blocks that `r` (pixels) touches.
    pub fn over(&mut self, r: [i32; 4], level: u32) {
        let e = r[0] >= r[2] || r[1] >= r[3];
        let s = bshift(level);
        self.s = s;
        let m = (1 << s) - 1;
        (self.x0, self.y0) = (r[0] >> s, r[1] >> s);
        (self.w, self.h) = if e { (0, 0) } else { (((r[2] + m) >> s) - self.x0, ((r[3] + m) >> s) - self.y0) };
        self.bits.clear();
        self.bits.resize(((self.w * self.h) as usize).div_ceil(64), 0);
        self.n = 0;
    }
    #[inline]
    pub fn set(&mut self, bx: i32, by: i32) {
        let (ix, iy) = (bx - self.x0, by - self.y0);
        if ix < 0 || iy < 0 || ix >= self.w || iy >= self.h {
            return;
        }
        let i = (iy * self.w + ix) as usize;
        self.n += (self.bits[i / 64] >> (i % 64) & 1 == 0) as usize;
        self.bits[i / 64] |= 1 << (i % 64);
    }
    /// Every block a pixel rect touches.
    pub fn add_px(&mut self, r: [i32; 4]) {
        if r[0] >= r[2] || r[1] >= r[3] {
            return;
        }
        let (s, m) = (self.s, (1 << self.s) - 1);
        for by in (r[1] >> s).max(self.y0)..((r[3] + m) >> s).min(self.y0 + self.h) {
            for bx in (r[0] >> s).max(self.x0)..((r[2] + m) >> s).min(self.x0 + self.w) {
                self.set(bx, by);
            }
        }
    }
    pub fn all(&mut self) {
        for by in self.y0..self.y0 + self.h {
            for bx in self.x0..self.x0 + self.w {
                self.set(bx, by);
            }
        }
    }
    pub fn each(&self, mut f: impl FnMut(i32, i32)) {
        for (k, &wd) in self.bits.iter().enumerate() {
            let mut m = wd;
            while m != 0 {
                let i = (k * 64) as i32 + m.trailing_zeros() as i32;
                m &= m - 1;
                f(self.x0 + i % self.w, self.y0 + i / self.w);
            }
        }
    }
}

fn cut(a: [i32; 4], b: [i32; 4]) -> [i32; 4] {
    [a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3])]
}
fn arr(v: Vec4i) -> [i32; 4] {
    [v.x, v.y, v.z, v.w]
}

/// A layer: the record that owns it and its levels.
#[derive(Clone, Debug, Default)]
pub struct Layer {
    /// The owning record's paint-order key.
    pub z: u32,
    /// The finer level read, the weight of the next (Q15), the coarsest level kept.
    pub lvl: u32,
    pub frac: u32,
    pub top: u32,
    /// The record's ink window (frame pixels).
    pub win: [i32; 4],
    pub lv: [Lvl; LEVELS],
    /// Its storage holds nothing yet (new, re-allocated, or the pool was repacked).
    pub fresh: bool,
    /// The pixels of each level that entered its window since the last frame (at most four strips).
    entered: [Vec<[i32; 4]>; LEVELS],
}

/// What one layer has to repair this frame.
#[derive(Clone, Debug, Default)]
pub struct Work {
    pub lid: u32,
    /// Frame tiles whose prefix is recomputed (the tile program up to the record) and stored in level 0.
    pub prefix: Vec<u32>,
    /// Per level 1 ..= top: the blocks (bx, by) to downsample from the level beneath.
    pub blocks: Vec<(u32, Vec<(i32, i32)>)>,
}
/// A frame's layer stages, in paint order.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub work: Vec<Work>,
    /// Counters: prefix tiles, pyramid blocks and pyramid pixels repaired.
    pub prefix_tiles: u32,
    pub blocks: u32,
    pub px: u64,
}

/// The scene's layers and their pool.
#[derive(Default)]
pub struct Layers {
    /// In paint order of their records.
    pub list: Vec<Layer>,
    /// By paint-order key: the record's layer (`u32::MAX`: none). Empty when the scene has no layer.
    pub of: Vec<u32>,
    /// Words in use, and words of the live layers (the rest is what moved layers left behind).
    pub used: usize,
    pub live: usize,
    /// The CPU executor's storage.
    pub pool: Vec<u32>,
    /// Bumped when the table changed (the GPU re-uploads it).
    pub epoch: u64,
    dirty: [Blocks; LEVELS],
}
pub const NONE: u32 = u32::MAX;

impl Layers {
    pub fn any(&self) -> bool {
        !self.list.is_empty()
    }
    #[inline]
    pub fn lid(&self, z: u32) -> u32 {
        self.of.get(z as usize).copied().unwrap_or(NONE)
    }
    /// Declare the record with key `z` a layer owner (at mount; keys arrive in paint order).
    pub fn mount(&mut self, z: u32) {
        if self.of.len() <= z as usize {
            self.of.resize(z as usize + 1, NONE);
        }
        self.of[z as usize] = self.list.len() as u32;
        self.list.push(Layer { z, fresh: true, ..Default::default() });
        self.epoch += 1;
    }
    /// Bring layer `lid` to its record's backdrop command under the current block: new level windows; the
    /// storage is kept when the windows still fit (a move), re-allocated otherwise.
    pub fn regeo(&mut self, lid: u32, c: Option<Cmd>, fw: u32, fh: u32) {
        let l = &mut self.list[lid as usize];
        let Some(c) = c else { return };
        let (lvl, frac, top) = (c.rad.y as u32, c.rad.z as u32, x::bd_top(c));
        let win = arr(c.win);
        let mut lv = l.lv;
        let mut fits = !l.fresh && top == l.top;
        for k in 0..LEVELS {
            let dims = [0, 0, x::bd_dim(fw, k as u32), x::bd_dim(fh, k as u32)];
            let r = cut(arr(x::bd_rect(c, k as u32)), dims);
            lv[k].r = if r[0] < r[2] && r[1] < r[3] { r } else { [0; 4] };
            fits &= (lv[k].r[2] - lv[k].r[0]) as u32 <= lv[k].cw && (lv[k].r[3] - lv[k].r[1]) as u32 <= lv[k].ch;
        }
        if (l.lvl, l.frac, l.win) == (lvl, frac, win) && lv == l.lv && !l.fresh {
            return;
        }
        self.epoch += 1;
        if fits {
            for k in 0..LEVELS {
                let (o, n) = (l.lv[k].r, lv[k].r);
                // the pixels of the new window outside the old one: up to four strips
                if n != o && n[0] < n[2] {
                    if o[0] >= o[2] || cut(o, n)[0] >= cut(o, n)[2] || cut(o, n)[1] >= cut(o, n)[3] {
                        l.entered[k].push(n);
                    } else {
                        let i = cut(o, n);
                        for s in [[n[0], n[1], n[2], i[1]], [n[0], i[3], n[2], n[3]], [n[0], i[1], i[0], i[3]], [i[2], i[1], n[2], i[3]]] {
                            if s[0] < s[2] && s[1] < s[3] {
                                l.entered[k].push(s);
                            }
                        }
                    }
                }
            }
        } else {
            // new storage: a slack of two pixels per side so that a move's rounding does not re-allocate
            let old: usize = l.lv.iter().map(|v| (v.cw * v.ch) as usize).sum();
            self.live -= old.min(self.live);
            for k in 0..LEVELS {
                let (w, h) = ((lv[k].r[2] - lv[k].r[0]) as u32, (lv[k].r[3] - lv[k].r[1]) as u32);
                (lv[k].cw, lv[k].ch) = if w == 0 || k as u32 > top { (1, 1) } else { (w + 4, h + 4) };
                lv[k].base = self.used as u32;
                self.used += (lv[k].cw * lv[k].ch) as usize;
                self.live += (lv[k].cw * lv[k].ch) as usize;
            }
            l.fresh = true;
            l.entered.iter_mut().for_each(Vec::clear);
        }
        (l.lvl, l.frac, l.top, l.win, l.lv) = (lvl, frac, top, win, lv);
    }
    /// The pool has more than twice the live words: give every layer new storage from zero (they are all
    /// fresh afterwards). Returns whether it did.
    pub fn repack(&mut self) -> bool {
        if self.used <= 2 * self.live + (1 << 16) {
            return false;
        }
        (self.used, self.live) = (0, 0);
        for l in self.list.iter_mut() {
            for v in l.lv.iter_mut() {
                (v.cw, v.ch) = if v.empty() { (1, 1) } else { ((v.r[2] - v.r[0]) as u32 + 4, (v.r[3] - v.r[1]) as u32 + 4) };
                v.base = self.used as u32;
                self.used += (v.cw * v.ch) as usize;
            }
            l.fresh = true;
            l.entered.iter_mut().for_each(Vec::clear);
        }
        self.live = self.used;
        self.epoch += 1;
        true
    }
    /// The table as the GPU reads it: per layer `ROW` words.
    pub fn table(&self, out: &mut Vec<u32>) {
        out.clear();
        for l in &self.list {
            out.extend([l.z, l.lvl, l.frac, l.top]);
            for v in &l.lv {
                let p = |a: i32, b: i32| (a as u16 as u32) | (b as u16 as u32) << 16;
                out.extend([p(v.r[0], v.r[1]), p(v.r[2], v.r[3]), v.cw | v.ch << 16, v.base]);
            }
            out.resize(out.len().div_ceil(ROW) * ROW, 0);
        }
    }

    /// Turn this frame's events into layer `lid`'s work, and its consequences for what is above it:
    /// `out(rect)` receives the frame-pixel rects of the record's ink whose blurred value changed.
    /// `events`: (window, key) of every record change this frame, those of lower layers' consequences
    /// included (the caller appends what `out` reports before the next layer).
    pub fn schedule(&mut self, lid: u32, events: &[([i32; 4], u32)], tx: usize, plan: &mut Plan, mut out: impl FnMut([i32; 4])) {
        let l = &mut self.list[lid as usize];
        let d = &mut self.dirty;
        let top = l.top as usize;
        for k in 0..=top {
            d[k].over(l.lv[k].r, k as u32);
        }
        if l.lv[0].empty() {
            l.fresh = false;
            l.entered.iter_mut().for_each(Vec::clear);
            return;
        }
        if l.fresh {
            for b in d[..=top].iter_mut() {
                b.all();
            }
        } else {
            for k in 0..=top {
                for s in l.entered[k].drain(..) {
                    d[k].add_px(s);
                }
            }
            if !cfg!(feature = "mutant_layer_event") {
                for &(w, z) in events {
                    if z < l.z {
                        d[0].add_px(cut(w, l.lv[0].r));
                    }
                }
            }
        }
        l.fresh = false;
        l.entered.iter_mut().for_each(Vec::clear);
        // a dirty block reaches the blocks one level up whose taps touch it: pixel p of a level is read by
        // pixels ceil((p - 2) / 2) ..= floor((p + 1) / 2) of the next
        for k in 1..=top {
            let (lo, hi) = d.split_at_mut(k);
            let (src, dst) = (&lo[k - 1], &mut hi[0]);
            let (bs, ds) = (bshift(k as u32 - 1), bshift(k as u32));
            src.each(|bx, by| {
                let span = |b: i32| if cfg!(feature = "mutant_layer_dilate") { ((b << bs) >> 1, (((b + 1) << bs) - 1) >> 1) } else { (((b << bs) - 1) >> 1, ((b + 1) << bs) >> 1) };
                let ((x0, x1), (y0, y1)) = (span(bx), span(by));
                for py in y0 >> ds..=y1 >> ds {
                    for px in x0 >> ds..=x1 >> ds {
                        dst.set(px, py);
                    }
                }
            });
        }
        if d[..=top].iter().all(|b| b.n == 0) {
            return;
        }
        let mut w = Work { lid, ..Default::default() };
        d[0].each(|bx, by| w.prefix.push(by as u32 * tx as u32 + bx as u32));
        plan.prefix_tiles += w.prefix.len() as u32;
        for k in 1..=top {
            if d[k].n != 0 {
                let mut v = Vec::with_capacity(d[k].n);
                let r = l.lv[k].r;
                let bs = bshift(k as u32);
                d[k].each(|bx, by| {
                    v.push((bx, by));
                    let c = cut([bx << bs, by << bs, (bx + 1) << bs, (by + 1) << bs], r);
                    plan.px += ((c[2] - c[0]) * (c[3] - c[1])) as u64;
                });
                plan.blocks += v.len() as u32;
                w.blocks.push((k as u32, v));
            }
        }
        // the frame pixels whose read (taps i, i + 1 at each level read) reaches a dirty block
        for k in [l.lvl as usize, top] {
            let s = k as u32;
            let bs = bshift(s);
            let lo = |b: i32| (((((b << bs) - 1) << (s + 1)) + (1 << s) - 1) + 1) >> 1;
            let hi = |b: i32| (((((b + 1) << bs) << (s + 1)) + (1 << s) - 1) + 1) >> 1;
            d[k].each(|bx, by| {
                let r = cut([lo(bx), lo(by), hi(bx), hi(by)], l.win);
                if r[0] < r[2] && r[1] < r[3] {
                    out(r);
                }
            });
            if l.lvl as usize == top {
                break;
            }
        }
        plan.work.push(w);
    }
}

/// Level-`k` pixel (x, y) of a layer (inside the level and the layer's window).
#[inline(always)]
pub fn at(v: &Lvl, x: i32, y: i32) -> usize {
    x::bd_addr(x, y, v.cw, v.ch, v.base) as usize
}
/// A bilinear read of one level at frame pixel (x, y).
#[inline(always)]
fn read(pool: &[u32], l: &Layer, k: u32, fw: u32, fh: u32, x: i32, y: i32) -> Vec4u {
    let v = &l.lv[k as usize];
    let (dw, dh) = (x::bd_dim(fw, k), x::bd_dim(fh, k));
    let (i, j) = (x::bd_i(x, k), x::bd_i(y, k));
    let (x0, x1, y0, y1) = (i.clamp(0, dw - 1), (i + 1).clamp(0, dw - 1), j.clamp(0, dh - 1), (j + 1).clamp(0, dh - 1));
    x::bd_bil(pool[at(v, x0, y0)], pool[at(v, x1, y0)], pool[at(v, x0, y1)], pool[at(v, x1, y1)], x::bd_f(x, k), x::bd_f(y, k))
}
/// The blurred prefix a backdrop command reads at frame pixel (x, y) from its layer.
#[inline(always)]
pub fn sample(pool: &[u32], l: &Layer, fw: u32, fh: u32, c: Cmd, x: i32, y: i32) -> Vec4u {
    let a = read(pool, l, c.rad.y as u32, fw, fh, x, y);
    if c.rad.z == 0 {
        return a;
    }
    bough_kernel::k::mix4u(a, read(pool, l, c.rad.y as u32 + 1, fw, fh, x, y), c.rad.z as u32)
}
/// Downsample block (bx, by) of level `k` (`32 >> k` pixels a side) of a layer from level `k - 1` (the pixels inside its window).
pub fn down(pool: &mut [u32], l: &Layer, k: u32, fw: u32, fh: u32, bx: i32, by: i32) {
    let (dst, src) = (l.lv[k as usize], l.lv[k as usize - 1]);
    let (sw, sh) = (x::bd_dim(fw, k - 1), x::bd_dim(fh, k - 1));
    let bs = bshift(k);
    let r = cut([bx << bs, by << bs, (bx + 1) << bs, (by + 1) << bs], dst.r);
    for y in r[1]..r[3] {
        for xx in r[0]..r[2] {
            let mut rows = [Vec4 { x: 0, y: 0, z: 0, w: 0 }; 4];
            for (j, row) in rows.iter_mut().enumerate() {
                let sy = (2 * y - 1 + j as i32).clamp(0, sh - 1);
                let t = |i: i32| pool[at(&src, (2 * xx - 1 + i).clamp(0, sw - 1), sy)];
                *row = x::dn_row(t(0), t(1), t(2), t(3));
            }
            let v = x::dn_fin(rows[0], rows[1], rows[2], rows[3]);
            pool[at(&dst, xx, y)] = if cfg!(feature = "mutant_layer_weights") { v ^ 1 } else { v };
        }
    }
}
