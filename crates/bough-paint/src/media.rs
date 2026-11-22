//! Media: video frames as leased, immutable slots of one pool the kernel samples directly.
//!
//! A frame source (a decoder; here a deterministic synthetic one) never hands the raster a copy. It asks the
//! pool for a **lease** (a slot nobody shows), writes its NV12 planes there, and **publishes** it: the slot
//! becomes immutable until it is retired, and the slot's header offset is what the video's record carries
//! in its `slot` word. "A new frame arrived" is therefore one record word rewritten (`Scene::set`), which
//! damages the video's visible window and nothing else; the frame it replaced is retired after the frame
//! that stopped showing it. The pool is one page-aligned allocation, which the Metal executor wraps without
//! a copy: a decoder that writes into a lease has written into the GPU's buffer.
//!
//! Frame layout (words at `slot`): width, height, byte offset of the Y plane, byte offset of the
//! interleaved CbCr plane, Y stride, CbCr stride, sequence number, 0. Planes follow, each row padded to
//! its stride. BT.709, limited range.
use crate::ops::ops as x;
use crate::scene::{Arena, VM_PAGE};
use bough_kernel::Cmd;
use std::sync::atomic::{AtomicU64, Ordering};
use wgsl_rs::std::Vec4u;

pub const HDR: usize = 8;
pub(crate) const BGRA_BIT: u32 = 1 << 31;
const BGRA_MAGIC: u32 = u32::from_le_bytes(*b"BGRA");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Free,
    /// Being written by a source: not readable.
    Leased,
    /// Readable, immutable.
    Shown,
    /// Replaced: free after the next `reclaim` (the frame that stopped showing it has been drawn).
    Retired,
    /// A retained surface: readable, and written in place by its one owner between frames.
    Surface,
}
struct Slot {
    at: usize,
    words: usize,
    state: State,
}
/// A slot being written. Publishing consumes it.
pub struct Lease {
    slot: usize,
    pub at: u32,
    pub w: u32,
    pub h: u32,
}

pub struct Pool {
    identity: u64,
    arena: Arena,
    used: usize,
    slots: Vec<Slot>,
}
pub fn frame_words(w: u32, h: u32) -> usize {
    let (ys, cs) = (w.next_multiple_of(4), w.div_ceil(2) * 2);
    HDR + ((ys * h + cs.next_multiple_of(4) * h.div_ceil(2)) as usize).div_ceil(4)
}
impl Pool {
    pub fn new(words: usize) -> Pool {
        static NEXT_POOL: AtomicU64 = AtomicU64::new(1);
        Pool { identity: NEXT_POOL.fetch_add(1, Ordering::Relaxed), arena: Arena::new(words.max(VM_PAGE / 4)), used: 0, slots: Vec::new() }
    }
    pub fn identity(&self) -> u64 {
        self.identity
    }
    pub fn words(&self) -> &[u32] {
        self.arena.words()
    }
    pub fn arena(&self) -> &Arena {
        &self.arena
    }
    /// A slot for a `w` x `h` frame that no record shows. `None` when the pool is full (the source keeps its
    /// frame and tries after the next `reclaim`: a frame is dropped, never torn).
    pub fn lease(&mut self, w: u32, h: u32) -> Option<Lease> {
        assert!((2..=2047).contains(&w) && (2..=2047).contains(&h), "a video plane of {w} x {h}");
        let need = frame_words(w, h);
        let slot = self.allocate(need)?;
        let at = self.slots[slot].at;
        let (ys, cs) = (w.next_multiple_of(4), (w.div_ceil(2) * 2).next_multiple_of(4));
        let yoff = (at + HDR) as u32 * 4;
        let hdr = [w, h, yoff, yoff + ys * h, ys, cs, 0, 0];
        self.arena_mut()[at..at + HDR].copy_from_slice(&hdr);
        Some(Lease { slot, at: at as u32, w, h })
    }
    fn allocate(&mut self, need: usize) -> Option<usize> {
        let slot = match self.slots.iter().position(|s| s.state == State::Free && s.words == need) {
            Some(i) => i,
            None => {
                if self.used.checked_add(need)? > self.arena.words().len().min(BGRA_BIT as usize) {
                    return None;
                }
                self.slots.push(Slot { at: self.used, words: need, state: State::Free });
                self.used += need;
                self.slots.len() - 1
            }
        };
        self.slots[slot].state = State::Leased;
        Some(slot)
    }
    /// Publish device-ready premultiplied BGRA8 pixels. A rejected publication leaves `previous` live.
    pub fn publish_bgra(&mut self, width: u32, height: u32, pixels: &[u32], sequence: u32, previous: Option<u32>) -> Option<u32> {
        if !(1..=2047).contains(&width) || !(1..=2047).contains(&height) || pixels.len() != (width * height) as usize {
            return None;
        }
        if pixels.iter().any(|&p| {
            let a = p >> 24;
            (p & 255) > a || ((p >> 8) & 255) > a || ((p >> 16) & 255) > a
        }) {
            return None;
        }
        let slot = self.allocate(HDR + pixels.len())?;
        let at = self.slots[slot].at;
        let m = self.arena_mut();
        m[at..at + HDR].copy_from_slice(&[width, height, 0, 0, 0, 0, sequence, BGRA_MAGIC]);
        m[at + HDR..][..pixels.len()].copy_from_slice(pixels);
        self.slots[slot].state = State::Shown;
        self.retire(previous);
        Some(at as u32 | BGRA_BIT)
    }

    /// A retained `width` x `height` premultiplied BGRA8 surface with pyramid levels 1 ..= `top` after it
    /// (`ops::mp_off`), all transparent. Its handle is an image's slot word (`paint::Image` reads level 0,
    /// `paint::Mip` the pyramid); its owner repaints it in place through [`Pool::surface_mut`], between
    /// frames, so a small change writes a few tiles and nothing is copied or re-published.
    pub fn surface(&mut self, width: u32, height: u32, top: u32) -> Option<u32> {
        if !(1..=2047).contains(&width) || !(1..=2047).contains(&height) || top > x::BD_LMAX {
            return None;
        }
        let words = x::mp_off(width as i32, height as i32, top + 1) as usize;
        let slot = self.allocate(HDR + words)?;
        let at = self.slots[slot].at;
        let m = self.arena_mut();
        m[at..at + HDR].copy_from_slice(&[width, height, top, 0, 0, 0, 0, BGRA_MAGIC]);
        m[at + HDR..at + HDR + words].fill(0);
        self.slots[slot].state = State::Surface;
        Some(at as u32 | BGRA_BIT)
    }
    /// A surface's words (level 0, then each pyramid level), for its owner. `None` for any other handle.
    pub fn surface_mut(&mut self, handle: u32) -> Option<&mut [u32]> {
        let at = (handle & !BGRA_BIT) as usize;
        let s = self.slots.iter().find(|s| s.at == at && s.state == State::Surface)?;
        let words = s.words - HDR;
        Some(&mut self.arena_mut()[at + HDR..at + HDR + words])
    }
    /// Give a surface back (after the frame that stopped showing it).
    pub fn release_surface(&mut self, handle: u32) {
        let at = (handle & !BGRA_BIT) as usize;
        if let Some(s) = self.slots.iter_mut().find(|s| s.at == at && s.state == State::Surface) {
            s.state = State::Retired;
        }
    }

    fn arena_mut(&mut self) -> &mut [u32] {
        // SAFETY: the arena's words are initialised and this pool owns them; `&mut self` is exclusive.
        unsafe { std::slice::from_raw_parts_mut(self.arena.ptr(), self.arena.words().len()) }
    }
    /// The leased frame's planes: (Y: `stride * h` bytes, CbCr: `stride * ceil(h / 2)` bytes, strides).
    pub fn planes(&mut self, l: &Lease) -> (&mut [u8], &mut [u8], usize, usize) {
        let (at, w, h) = (l.at as usize, l.w, l.h);
        let (ys, cs) = (w.next_multiple_of(4) as usize, (w.div_ceil(2) * 2).next_multiple_of(4) as usize);
        let words = &mut self.arena_mut()[at + HDR..at + frame_words(w, h)];
        // SAFETY: a `[u32]` viewed as its bytes (alignment 1, same length in bytes, any bits valid).
        let bytes = unsafe { std::slice::from_raw_parts_mut(words.as_mut_ptr() as *mut u8, words.len() * 4) };
        let (y, c) = bytes.split_at_mut(ys * h as usize);
        (y, &mut c[..cs * (h as usize).div_ceil(2)], ys, cs)
    }
    /// Make the leased frame readable, replacing `prev` (the slot word the record showed, if any), which is
    /// retired. Returns the new slot word for the record.
    pub fn publish(&mut self, l: Lease, seq: u32, prev: Option<u32>) -> u32 {
        self.arena_mut()[l.at as usize + 6] = seq;
        self.slots[l.slot].state = State::Shown;
        self.retire(prev);
        l.at
    }
    fn retire(&mut self, previous: Option<u32>) {
        if let Some(handle) = previous {
            let at = (handle & !BGRA_BIT) as usize;
            if let Some(s) = self.slots.iter_mut().find(|s| s.at == at && s.state == State::Shown) {
                s.state = State::Retired;
            }
        }
    }
    /// Call after a frame has been drawn (on every executor that reads this pool): frames retired before
    /// it are no longer read by anything and become leasable.
    pub fn reclaim(&mut self) {
        for s in self.slots.iter_mut() {
            if s.state == State::Retired {
                s.state = State::Free;
            }
        }
    }
    pub fn stats(&self) -> (usize, usize) {
        (self.slots.len(), self.used)
    }
}

#[inline(always)]
fn byte(m: &[u32], i: u32) -> u32 {
    let w = m[(i as usize >> 2).min(m.len() - 1)];
    (w >> ((i & 3) * 8)) & 255
}
/// The frame a video command names, sampled at frame pixel (x, y): bilinear luma and chroma, BT.709.
/// Every index is clamped to the pool, so a stale or forged slot word reads pool words, never beyond.
#[inline(always)]
pub fn sample(m: &[u32], c: Cmd, px: i32, py: i32) -> Vec4u {
    if c.par.z & BGRA_BIT != 0 {
        return sample_bgra(m, c, px, py);
    }
    let at = (c.par.z as usize).min(m.len() - HDR);
    let (yoff, coff, ys, cs) = (m[at + 2], m[at + 3], m[at + 4], m[at + 5]);
    let (vw, vh) = (c.pm.y as i32, c.pm.z as i32);
    let t = x::vd_pos(c, px, py);
    let (x0, x1, y0, y1) = (x::vd_ix(t.x, 0, vw), x::vd_ix(t.x, 1, vw), x::vd_ix(t.y, 0, vh), x::vd_ix(t.y, 1, vh));
    let ya = |ix: u32, iy: u32| byte(m, yoff.wrapping_add(iy.wrapping_mul(ys)).wrapping_add(ix));
    let y = x::vd_bil(ya(x0, y0), ya(x1, y0), ya(x0, y1), ya(x1, y1), t.x, t.y);
    let (cw, ch) = ((vw + 1) >> 1, (vh + 1) >> 1);
    let (ux, uy) = ((t.x - 128) >> 1, (t.y - 128) >> 1);
    let (u0, u1, v0, v1) = (x::vd_ix(ux, 0, cw), x::vd_ix(ux, 1, cw), x::vd_ix(uy, 0, ch), x::vd_ix(uy, 1, ch));
    let ca = |ix: u32, iy: u32, k: u32| byte(m, coff.wrapping_add(iy.wrapping_mul(cs)).wrapping_add(2 * ix + k));
    let cb = x::vd_bil(ca(u0, v0, 0), ca(u1, v0, 0), ca(u0, v1, 0), ca(u1, v1, 0), ux, uy);
    let cr = x::vd_bil(ca(u0, v0, 1), ca(u1, v0, 1), ca(u0, v1, 1), ca(u1, v1, 1), ux, uy);
    x::vd_rgb(y, cb, cr)
}

fn sample_bgra(m: &[u32], c: Cmd, px: i32, py: i32) -> Vec4u {
    let clear = Vec4u { x: 0, y: 0, z: 0, w: 0 };
    let at = (c.par.z & !BGRA_BIT) as usize;
    let Some(header) = m.get(at..at + HDR) else { return clear };
    let (width, height) = (c.pm.y, c.pm.z);
    if header[7] != BGRA_MAGIC || header[0] != width || header[1] != height {
        return clear;
    }
    let Some(pixels) = m.get(at + HDR..at + HDR + (width * height) as usize) else { return clear };
    let t = x::vd_pos(c, px, py);
    let (x0, x1) = (x::vd_ix(t.x, 0, width as i32), x::vd_ix(t.x, 1, width as i32));
    let (y0, y1) = (x::vd_ix(t.y, 0, height as i32), x::vd_ix(t.y, 1, height as i32));
    let pixel = |x, y| pixels[(y * width + x) as usize];
    let mut colour = x::bd_bil(pixel(x0, y0), pixel(x1, y0), pixel(x0, y1), pixel(x1, y1), (t.x & 255) as u32, (t.y & 255) as u32);
    if cfg!(feature = "mutant_bgra_channels") {
        std::mem::swap(&mut colour.x, &mut colour.z);
    }
    colour
}

/// A surface a command reads, opened once (its header checked, its levels found): what a tile's rows, or a
/// single pixel, sample. A stale or forged slot word opens nothing and reads transparent.
#[derive(Clone, Copy)]
pub struct Surf<'a> {
    pixels: &'a [u32],
    w: i32,
    h: i32,
    /// The finer level read, and the weight (Q15) of the next.
    lvl: u32,
    frac: u32,
}
impl<'a> Surf<'a> {
    /// The retained surface a mip command names (`ops::mk_mip`), with the pyramid levels its blur reads.
    #[inline(always)]
    pub fn open(m: &'a [u32], c: Cmd) -> Option<Surf<'a>> {
        let at = (c.par.z & !BGRA_BIT) as usize;
        let header = m.get(at..at + HDR)?;
        let (w, h) = (c.pm.y as i32, c.pm.z as i32);
        if header[7] != BGRA_MAGIC || header[0] != w as u32 || header[1] != h as u32 {
            return None;
        }
        let top = header[2].min(x::BD_LMAX);
        let pixels = m.get(at + HDR..at + HDR + x::mp_off(w, h, top + 1) as usize)?;
        let lv = x::mp_lvl(c, top);
        Some(Surf { pixels, w, h, lvl: lv.x as u32, frac: lv.y as u32 })
    }
    /// The image a video command names when its slot is a BGRA one: level 0 alone (`sample_bgra`).
    #[inline(always)]
    pub fn open_plain(m: &'a [u32], c: Cmd) -> Option<Surf<'a>> {
        let at = (c.par.z & !BGRA_BIT) as usize;
        let header = m.get(at..at + HDR)?;
        let (w, h) = (c.pm.y, c.pm.z);
        if header[7] != BGRA_MAGIC || header[0] != w || header[1] != h {
            return None;
        }
        let pixels = m.get(at + HDR..at + HDR + (w * h) as usize)?;
        Some(Surf { pixels, w: w as i32, h: h as i32, lvl: 0, frac: 0 })
    }
    /// The read is of level 0 alone.
    #[inline(always)]
    pub fn sharp(&self) -> bool {
        self.lvl == 0 && self.frac == 0
    }
    /// Row `y` of level 0.
    #[inline(always)]
    pub fn row(&self, y: u32) -> &'a [u32] {
        &self.pixels[(y * self.w as u32) as usize..((y + 1) * self.w as u32) as usize]
    }
    pub fn size(&self) -> (i32, i32) {
        (self.w, self.h)
    }
    /// One level under bilinear weights at the level-0 texel position `t` (Q8).
    #[inline(always)]
    fn level(&self, k: u32, t: wgsl_rs::std::Vec2i) -> Vec4u {
        let (dw, dh) = (x::bd_dim(self.w as u32, k), x::bd_dim(self.h as u32, k));
        let level = &self.pixels[x::mp_off(self.w, self.h, k) as usize..];
        let (tx, ty) = (x::mp_pos(t.x, k), x::mp_pos(t.y, k));
        let (x0, x1, y0, y1) = (x::vd_ix(tx, 0, dw), x::vd_ix(tx, 1, dw), x::vd_ix(ty, 0, dh), x::vd_ix(ty, 1, dh));
        let px = |x: u32, y: u32| level[(y * dw as u32 + x) as usize];
        x::bd_bil(px(x0, y0), px(x1, y0), px(x0, y1), px(x1, y1), (tx & 255) as u32, (ty & 255) as u32)
    }
    /// The surface at the level-0 texel position `t` (`ops::vd_pos`), through the levels the command reads.
    #[inline(always)]
    pub fn at(&self, t: wgsl_rs::std::Vec2i) -> Vec4u {
        let v = self.level(self.lvl, t);
        if self.frac == 0 {
            return v;
        }
        x::x_mixv(v, self.level(self.lvl + 1, t), self.frac)
    }
}
/// One level of an opened surface, found once.
#[derive(Clone, Copy)]
struct Lv<'a> {
    px: &'a [u32],
    dw: i32,
    dh: i32,
    k: u32,
}
impl Lv<'_> {
    #[inline(always)]
    fn at(&self, tx: i32, ty: i32) -> [u32; 4] {
        let (tx, ty) = (x::mp_pos(tx, self.k), x::mp_pos(ty, self.k));
        let (x0, x1, y0, y1) = (x::vd_ix(tx, 0, self.dw), x::vd_ix(tx, 1, self.dw), x::vd_ix(ty, 0, self.dh), x::vd_ix(ty, 1, self.dh));
        let (r0, r1) = ((y0 * self.dw as u32) as usize, (y1 * self.dw as u32) as usize);
        bough_kernel::simd::bil_bgra([self.px[r0 + x0 as usize], self.px[r0 + x1 as usize], self.px[r1 + x0 as usize], self.px[r1 + x1 as usize]], (tx & 255) as u32, (ty & 255) as u32)
    }
}
/// [`Surf::at`] for a tile's rows: the levels' places found once, the taps weighed in vector lanes. The
/// same integers (the tile kernel is held to the plain form by the specification).
#[derive(Clone, Copy)]
pub struct Rows<'a> {
    a: Lv<'a>,
    b: Lv<'a>,
    frac: u32,
}
impl<'a> Surf<'a> {
    #[inline(always)]
    fn lv(&self, k: u32) -> Lv<'a> {
        Lv { px: &self.pixels[x::mp_off(self.w, self.h, k) as usize..], dw: x::bd_dim(self.w as u32, k), dh: x::bd_dim(self.h as u32, k), k }
    }
    #[inline(always)]
    pub fn rows(&self) -> Rows<'a> {
        let a = self.lv(self.lvl);
        Rows { a, b: if self.frac == 0 { a } else { self.lv(self.lvl + 1) }, frac: self.frac }
    }
}
impl Rows<'_> {
    /// The surface at the level-0 texel position (tx, ty) (Q8), as Q15 channels.
    #[inline(always)]
    pub fn at(&self, tx: i32, ty: i32) -> [u32; 4] {
        let v = self.a.at(tx, ty);
        if self.frac == 0 {
            return v;
        }
        bough_kernel::simd::mix15(v, self.b.at(tx, ty), self.frac)
    }
}
/// The retained surface a mip command names, read at frame pixel (px, py) through its pyramid: the level
/// the command's blur asks for and, by its weight, the next. Transparent for a stale or forged slot word.
#[inline(always)]
pub fn sample_mip(m: &[u32], c: Cmd, px: i32, py: i32) -> Vec4u {
    match Surf::open(m, c) {
        Some(s) => s.at(x::vd_pos(c, px, py)),
        None => Vec4u { x: 0, y: 0, z: 0, w: 0 },
    }
}

/// A deterministic frame source: moving colour bars and a bouncing box, a function of (seed, sequence).
/// It stands where a decoder would: lease, write the planes, publish.
pub struct Synth {
    pub w: u32,
    pub h: u32,
    pub seed: u32,
    pub seq: u32,
    /// The slot word its record shows.
    pub shown: Option<u32>,
}
impl Synth {
    pub fn new(w: u32, h: u32, seed: u32) -> Synth {
        Synth { w, h, seed, seq: 0, shown: None }
    }
    /// Produce the next frame into the pool. Returns the record's new slot word, or `None` if no slot was
    /// free (the frame is skipped).
    pub fn next(&mut self, pool: &mut Pool) -> Option<u32> {
        let l = pool.lease(self.w, self.h)?;
        let (w, h, seq, seed) = (self.w as usize, self.h as usize, self.seq, self.seed);
        let (y, c, ys, cs) = pool.planes(&l);
        let (bx, by) = ((seq as usize * 3 + seed as usize * 7) % w.max(1), (seq as usize * 2 + seed as usize * 5) % h.max(1));
        for j in 0..h {
            for i in 0..w {
                let bar = ((i + seq as usize * 2) * 8 / w + seed as usize) % 8;
                let inbox = i.abs_diff(bx) < w / 8 && j.abs_diff(by) < h / 8;
                y[j * ys + i] = if inbox { 235 } else { (40 + bar * 24 + (j * 16 / h)) as u8 };
            }
        }
        for j in 0..h.div_ceil(2) {
            for i in 0..w.div_ceil(2) {
                let bar = ((2 * i + seq as usize * 2) * 8 / w + seed as usize) % 8;
                c[j * cs + 2 * i] = (64 + bar * 20) as u8;
                c[j * cs + 2 * i + 1] = (200 - bar * 18 + (j * 2 * 12 / h)) as u8;
            }
        }
        self.seq += 1;
        let at = pool.publish(l, seq, self.shown);
        self.shown = Some(at);
        Some(at)
    }
}
