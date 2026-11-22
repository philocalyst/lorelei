//! The flat core's reference raster: integer, premultiplied, safe, and pure.
//!
//! Every kernel here computes a pixel as a function of the record, the pixel's window coordinates and (for the
//! backdrop blur) the backdrop: never of the region being painted. That is what lets a tile repainted alone
//! equal the same tile of a full repaint, which is what the oracles test.
//!
//! Pixels are premultiplied RGBA8 with red in the low byte; colours handed in are straight alpha, red low (the
//! façade's `Color`). Boxes without effects are composited exactly as the lowered runtime's canvas does, so the
//! two can be compared pixel for pixel.
use super::effect::GradDir;
use super::expr::lerp_color;
use bough_damage::{Rect, ScrollBlit, shadow_reach};

/// A pixel buffer placed in the window: `px[0]` is the pixel at `org`.
#[derive(Clone, Debug, Default)]
pub struct Surface {
    pub w: i32,
    pub h: i32,
    pub org: (i32, i32),
    pub px: Vec<u32>,
}

impl Surface {
    pub fn new(w: i32, h: i32) -> Surface {
        Surface::at(Rect::new(0, 0, w.max(0), h.max(0)))
    }
    /// A buffer covering `r` of the window.
    pub fn at(r: Rect) -> Surface {
        let (w, h) = ((r.x1 - r.x0).max(0), (r.y1 - r.y0).max(0));
        Surface { w, h, org: (r.x0, r.y0), px: vec![0; (w * h) as usize] }
    }
    /// Become a zeroed buffer covering `r`, keeping the allocation when it is large enough.
    pub fn reset(&mut self, r: Rect) {
        let (w, h) = ((r.x1 - r.x0).max(0), (r.y1 - r.y0).max(0));
        self.w = w;
        self.h = h;
        self.org = (r.x0, r.y0);
        self.px.clear();
        self.px.resize((w * h) as usize, 0);
    }
    #[inline]
    pub fn bounds(&self) -> Rect {
        Rect { x0: self.org.0, y0: self.org.1, x1: self.org.0 + self.w, y1: self.org.1 + self.h }
    }
    /// The pixels of row `y` from `x0` to `x1` (window coordinates inside the bounds).
    #[inline]
    pub fn row_mut(&mut self, y: i32, x0: i32, x1: i32) -> &mut [u32] {
        let row = ((y - self.org.1) * self.w) as usize;
        &mut self.px[row + (x0 - self.org.0) as usize..row + (x1 - self.org.0) as usize]
    }
    #[inline]
    pub fn row(&self, y: i32, x0: i32, x1: i32) -> &[u32] {
        let row = ((y - self.org.1) * self.w) as usize;
        &self.px[row + (x0 - self.org.0) as usize..row + (x1 - self.org.0) as usize]
    }
    /// The pixel at (x, y), clamped into the bounds (the blur's edge rule).
    #[inline]
    pub fn clamped(&self, x: i32, y: i32) -> u32 {
        let x = x.clamp(self.org.0, self.org.0 + self.w - 1) - self.org.0;
        let y = y.clamp(self.org.1, self.org.1 + self.h - 1) - self.org.1;
        self.px[(y * self.w + x) as usize]
    }
    pub fn clear(&mut self, r: Rect) {
        let r = r.and(self.bounds());
        for y in r.y0..r.y1 {
            self.row_mut(y, r.x0, r.x1).fill(0);
        }
    }
    /// Move the pixels of the blit's clip by its offset (a `memmove` per row, in the order that never reads a row
    /// it has written).
    pub fn blit(&mut self, b: &ScrollBlit) {
        let dst = b.kept().and(self.bounds()).and(self.bounds().shift(b.dx, b.dy));
        if dst.is_empty() {
            return;
        }
        let n = (dst.x1 - dst.x0) as usize;
        let one = |s: &mut Surface, y: i32| {
            let from = ((y - b.dy - s.org.1) * s.w + (dst.x0 - b.dx - s.org.0)) as usize;
            let to = ((y - s.org.1) * s.w + (dst.x0 - s.org.0)) as usize;
            s.px.copy_within(from..from + n, to);
        };
        if b.dy > 0 {
            for y in (dst.y0..dst.y1).rev() {
                one(self, y);
            }
        } else {
            for y in dst.y0..dst.y1 {
                one(self, y);
            }
        }
    }
    /// The pixels as bytes (premultiplied RGBA).
    pub fn bytes(&self) -> Vec<u8> {
        self.px.iter().flat_map(|p| p.to_le_bytes()).collect()
    }
}

#[inline(always)]
pub fn div255(x: u32) -> u32 {
    let x = x + 128;
    (x + (x >> 8)) >> 8
}
/// Source over destination: `src` premultiplied RGBA.
#[inline(always)]
pub fn over(dst: u32, src: [u32; 4]) -> u32 {
    let ia = 255 - src[3];
    let ch = |s: u32, sh: u32| (s + div255((dst >> sh & 0xff) * ia)).min(255) << sh;
    ch(src[0], 0) | ch(src[1], 8) | ch(src[2], 16) | ch(src[3], 24)
}
/// A straight-alpha colour under coverage `cov` (0..=255) and opacity `alpha`, premultiplied.
#[inline(always)]
pub fn pm(color: u32, cov: u32, alpha: u32) -> [u32; 4] {
    let a = div255(div255((color >> 24) * cov) * alpha);
    [div255((color & 0xff) * a), div255((color >> 8 & 0xff) * a), div255((color >> 16 & 0xff) * a), a]
}

/// Coverage (0..=255) of pixel (x, y) by the rounded box `r` of radius `rad` (1/256 px): 255 away from the
/// corners, a 4 x 4 sample count inside a corner's square.
#[inline(always)]
pub fn rcov(r: Rect, rad: i32, x: i32, y: i32) -> u32 {
    if x < r.x0 || x >= r.x1 || y < r.y0 || y >= r.y1 {
        return 0;
    }
    if rad <= 0 {
        return 255;
    }
    let rad = rad.min((r.x1 - r.x0) * 128).min((r.y1 - r.y0) * 128);
    let (w, h) = ((r.x1 - r.x0) * 256, (r.y1 - r.y0) * 256);
    let (px, py) = ((x - r.x0) * 256, (y - r.y0) * 256);
    let cx = if px < rad {
        rad
    } else if px + 256 > w - rad {
        w - rad
    } else {
        return 255;
    };
    let cy = if py < rad {
        rad
    } else if py + 256 > h - rad {
        h - rad
    } else {
        return 255;
    };
    let mut n = 0u32;
    for sy in 0..4 {
        for sx in 0..4 {
            let (qx, qy) = (px + 32 + sx * 64, py + 32 + sy * 64);
            let dx = if (cx == rad && qx < cx) || (cx != rad && qx > cx) { (qx - cx) as i64 } else { 0 };
            let dy = if (cy == rad && qy < cy) || (cy != rad && qy > cy) { (qy - cy) as i64 } else { 0 };
            n += (dx * dx + dy * dy <= rad as i64 * rad as i64) as u32;
        }
    }
    (n * 255 + 8) / 16
}

/// Signed distance (1/256 px, negative inside) from the centre of pixel (x, y) to the rounded box `r` of
/// radius `rad` (1/256 px).
#[inline]
pub fn sdist(r: Rect, rad: i64, x: i32, y: i32) -> i64 {
    let (hw, hh) = ((r.x1 - r.x0) as i64 * 128, (r.y1 - r.y0) as i64 * 128);
    let rad = rad.clamp(0, hw.min(hh));
    let (cx, cy) = (r.x0 as i64 * 256 + hw, r.y0 as i64 * 256 + hh);
    let (px, py) = (x as i64 * 256 + 128, y as i64 * 256 + 128);
    let (qx, qy) = ((px - cx).abs() - (hw - rad), (py - cy).abs() - (hh - rad));
    let (ox, oy) = (qx.max(0) as u64, qy.max(0) as u64);
    (ox * ox + oy * oy).isqrt() as i64 + qx.max(qy).min(0) - rad
}

/// The soft edge every blurred shape uses: 255 from `reach` pixels inside the edge, 0 from `reach` pixels
/// outside (never beyond: this is the bound the damage hull is computed from), a smoothstep between. A reach
/// of 0 is a crisp edge.
#[inline]
pub fn soft(d: i64, reach_px: i32) -> u32 {
    let hw = (reach_px as i64 * 256).max(128);
    if d >= hw {
        return 0;
    }
    if d <= -hw {
        return 255;
    }
    // t in 0..=256, then t^2 (3 - 2t)
    let t = (hw - d) * 256 / (2 * hw);
    ((t * t * (768 - 2 * t)) >> 16).min(256) as u32 * 255 / 256
}

/// What a box is painted with. Lengths are 1/256 px unless said otherwise.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoxPaint {
    pub fill: u32,
    pub border_color: u32,
    pub border_w: u32,
    pub radius: u32,
    /// 0 ..= 255.
    pub opacity: u32,
    /// From `fill` to `grad_to`.
    pub grad_dir: GradDir,
    pub grad_to: u32,
    /// Blur of the box itself, pixels (0: crisp, with a border if it has one).
    pub elem_blur: i32,
}

#[inline]
fn grad_at(p: &BoxPaint, r: Rect, x: i32, y: i32) -> u32 {
    match p.grad_dir {
        GradDir::Down => lerp_color(p.fill, p.grad_to, ((y - r.y0) * 256 / (r.y1 - r.y0).max(1)) as u32),
        GradDir::Right => lerp_color(p.fill, p.grad_to, ((x - r.x0) * 256 / (r.x1 - r.x0).max(1)) as u32),
        GradDir::None => p.fill,
    }
}

/// A box: fill inside the border, the border between the outer and the inner rounded shape. `cut` is the part of
/// the window being painted (already inside the surface and the record's clip).
pub fn paint_box(s: &mut Surface, cut: Rect, r: Rect, p: &BoxPaint) {
    if p.opacity == 0 {
        return;
    }
    if p.elem_blur > 0 {
        let c = r.expand(p.elem_blur).and(cut);
        for y in c.y0..c.y1 {
            for x in c.x0..c.x1 {
                let cov = soft(sdist(r, p.radius as i64, x, y), p.elem_blur);
                if cov != 0 {
                    let col = grad_at(p, r, x, y);
                    let d = &mut s.row_mut(y, x, x + 1)[0];
                    *d = over(*d, pm(col, cov, p.opacity));
                }
            }
        }
        return;
    }
    let c = r.and(cut);
    if c.is_empty() {
        return;
    }
    let (rad, bw) = (p.radius as i32, p.border_w as i32);
    let inner = if bw > 0 {
        let i = (bw + 128) >> 8;
        Rect { x0: r.x0 + i, y0: r.y0 + i, x1: r.x1 - i, y1: r.y1 - i }
    } else {
        r
    };
    let irad = if bw > 0 { (rad - bw).max(0) } else { rad };
    let solid = p.grad_dir == GradDir::None;
    let opaque = solid && p.fill >> 24 == 255 && p.opacity == 255 && rad <= 0 && bw <= 0;
    // Away from the corners' squares a borderless solid box covers every pixel whole: those runs are one
    // source colour, composited (or stored) without asking the coverage. The same pixels as the general path.
    let flat = solid && bw <= 0;
    let crad = rad.min((r.x1 - r.x0) * 128).min((r.y1 - r.y0) * 128).max(0);
    let cpx = (crad + 255) >> 8;
    let whole = pm(p.fill, 255, p.opacity);
    let stored = p.fill >> 24 == 255 && p.opacity == 255;
    for y in c.y0..c.y1 {
        let row = s.row_mut(y, c.x0, c.x1);
        if opaque {
            row.fill(p.fill);
            continue;
        }
        // the run of this row that is outside the corners' squares
        let (fx0, fx1) = if !flat {
            (c.x0, c.x0)
        } else if (y - r.y0) * 256 >= crad && (y - r.y0 + 1) * 256 <= (r.y1 - r.y0) * 256 - crad {
            (c.x0, c.x1)
        } else {
            ((r.x0 + cpx).clamp(c.x0, c.x1), (r.x1 - cpx).clamp(c.x0, c.x1))
        };
        if fx1 > fx0 && p.fill >> 24 != 0 {
            let run = &mut row[(fx0 - c.x0) as usize..(fx1 - c.x0) as usize];
            if stored {
                run.fill(p.fill);
            } else {
                for d in run {
                    *d = over(*d, whole);
                }
            }
        }
        for (k, d) in row.iter_mut().enumerate() {
            let x = c.x0 + k as i32;
            if x >= fx0 && x < fx1 {
                continue;
            }
            let co = rcov(r, rad, x, y);
            let fill = if solid { p.fill } else { grad_at(p, r, x, y) };
            if bw > 0 {
                let ci = if inner.is_empty() { 0 } else { rcov(inner, irad, x, y) }.min(co);
                if ci > 0 && fill >> 24 != 0 {
                    *d = over(*d, pm(fill, ci, p.opacity));
                }
                if co > ci {
                    *d = over(*d, pm(p.border_color, co - ci, p.opacity));
                }
            } else if co > 0 && fill >> 24 != 0 {
                *d = over(*d, pm(fill, co, p.opacity));
            }
        }
    }
}

/// An outer shadow. Lengths in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shadow {
    pub color: u32,
    pub dx: i32,
    pub dy: i32,
    pub blur: i32,
    pub spread: i32,
}
impl Shadow {
    pub fn is_some(&self) -> bool {
        self.color >> 24 != 0
    }
    /// The box the shadow can touch for a record of box `r`.
    pub fn window(&self, r: Rect) -> Rect {
        r.shift(self.dx, self.dy).expand((self.spread + shadow_reach(self.blur)).max(0))
    }
}

/// The shadow of a box of `radius` (1/256 px) at `r`, under the box's opacity.
pub fn paint_shadow(s: &mut Surface, cut: Rect, r: Rect, radius: u32, sh: &Shadow, opacity: u32) {
    if !sh.is_some() || opacity == 0 {
        return;
    }
    let body = r.shift(sh.dx, sh.dy).expand(sh.spread);
    if body.is_empty() {
        return;
    }
    let reach = shadow_reach(sh.blur);
    let c = body.expand(reach).and(cut);
    let rad = (radius as i64 + sh.spread as i64 * 256).max(0);
    for y in c.y0..c.y1 {
        for x in c.x0..c.x1 {
            let cov = soft(sdist(body, rad, x, y), reach);
            if cov != 0 {
                let d = &mut s.row_mut(y, x, x + 1)[0];
                *d = over(*d, pm(sh.color, cov, opacity));
            }
        }
    }
}

mod blur;
mod face;
pub use blur::{BlurScratch, paint_backdrop_blur};
pub use face::{cell, paint_text, text_width};

#[cfg(test)]
mod tests {
    use super::*;
    use bough_damage::BlurLayer;

    fn noise(w: i32, h: i32, seed: u32) -> Surface {
        let mut s = Surface::new(w, h);
        let mut z = seed;
        for p in s.px.iter_mut() {
            z = z.wrapping_mul(1664525).wrapping_add(1013904223);
            *p = z | 0xff00_0000;
        }
        s
    }

    #[test]
    fn a_blit_moves_the_kept_part_in_every_direction() {
        for (dx, dy) in [(0, 5), (0, -5), (7, 0), (-7, 0), (3, -4), (-2, 6)] {
            let src = noise(40, 30, 3);
            let mut s = src.clone();
            let b = ScrollBlit { clip_rect: Rect::new(4, 3, 36, 28), dx, dy };
            s.blit(&b);
            let kept = b.kept();
            for y in 0..30 {
                for x in 0..40 {
                    let want = if kept.has(x, y) { src.clamped(x - dx, y - dy) } else { src.clamped(x, y) };
                    assert_eq!(s.clamped(x, y), want, "({x},{y}) by ({dx},{dy})");
                }
            }
        }
    }

    #[test]
    fn a_soft_edge_is_zero_from_its_reach_out_and_a_shadow_stays_inside_its_window() {
        assert_eq!(soft(0, 0), 127);
        assert_eq!((soft(128, 0), soft(-128, 0)), (0, 255));
        assert_eq!((soft(10 * 256, 10), soft(-10 * 256, 10)), (0, 255));
        assert!(soft(10 * 256 - 1, 10) <= 1 && soft(0, 10) == 127);
        let sh = Shadow { color: 0xff00_0000, dx: 3, dy: -2, blur: 6, spread: 2 };
        let r = Rect::new(30, 30, 50, 44);
        let win = sh.window(r);
        let mut s = Surface::new(90, 80);
        let all = s.bounds();
        paint_shadow(&mut s, all, r, 4 * 256, &sh, 255);
        let mut touched = Rect::EMPTY;
        for y in 0..80 {
            for x in 0..90 {
                if s.clamped(x, y) != 0 {
                    assert!(win.has(x, y), "({x},{y}) is outside {win:?}");
                    touched = touched.hull(Rect::xywh(x, y, 1, 1));
                }
            }
        }
        // and the window is not slack by more than the smoothstep's last faint pixel
        assert!(touched.expand(2).contains(win), "{touched:?} against {win:?}");
    }

    #[test]
    fn a_blurred_region_equals_the_same_region_of_the_whole_blur() {
        let (w, h) = if cfg!(miri) { (40, 36) } else { (150, 130) };
        let glass = Rect::new(w / 4 + 1, h / 4, w - 9, h - 7);
        for levels in 1..=if cfg!(miri) { 2u8 } else { 4 } {
            let layer = BlurLayer { id: 0, z: 1, rect: glass, radius: 1 << levels, max_level: levels };
            let read = layer.expanded_read_rect().and(Rect::new(0, 0, w, h));
            let below = noise(w, h, 11 + levels as u32);
            let mut back = Surface::at(read);
            for y in read.y0..read.y1 {
                back.row_mut(y, read.x0, read.x1).copy_from_slice(below.row(y, read.x0, read.x1));
            }
            let mut sc = BlurScratch::default();
            let mut whole = below.clone();
            let all = whole.bounds();
            paint_backdrop_blur(&mut whole, all, glass, 6 * 256, levels, 255, &back, &mut sc);
            assert_ne!(whole.px, below.px);
            // any sub-rectangle painted alone gives the same pixels
            let mut z = 5u32;
            let mut rnd = |n: i32| {
                z = z.wrapping_mul(1664525).wrapping_add(1013904223);
                (z >> 8) as i32 % n
            };
            for _ in 0..if cfg!(miri) { 2 } else { 12 } {
                let (x, y) = (rnd(w), rnd(h));
                let part = Rect::xywh(x, y, 1 + rnd(40), 1 + rnd(40)).and(Rect::new(0, 0, w, h));
                let mut alone = below.clone();
                paint_backdrop_blur(&mut alone, part, glass, 6 * 256, levels, 255, &back, &mut sc);
                for yy in 0..h {
                    for xx in 0..w {
                        let want = if part.has(xx, yy) { whole.clamped(xx, yy) } else { below.clamped(xx, yy) };
                        assert_eq!(alone.clamped(xx, yy), want, "level {levels} part {part:?} at ({xx},{yy})");
                    }
                }
            }
            // the footprint: a backdrop pixel changed changes the output only inside `output_of`, and a backdrop
            // pixel outside the read rect cannot be seen at all (it is not in the buffer)
            for _ in 0..if cfg!(miri) { 1 } else { 6 } {
                let (x, y) = (read.x0 + rnd(read.x1 - read.x0), read.y0 + rnd(read.y1 - read.y0));
                let mut back2 = back.clone();
                back2.row_mut(y, x, x + 1)[0] ^= 0x00ff_ffff;
                let mut other = below.clone();
                paint_backdrop_blur(&mut other, all, glass, 6 * 256, levels, 255, &back2, &mut sc);
                let dirty = layer.output_of(Rect::xywh(x, y, 1, 1));
                for yy in 0..h {
                    for xx in 0..w {
                        if other.clamped(xx, yy) != whole.clamped(xx, yy) {
                            assert!(dirty.has(xx, yy), "level {levels}: backdrop ({x},{y}) reached ({xx},{yy}) outside {dirty:?}");
                        }
                    }
                }
            }
        }
    }

    /// The general per-pixel path of a box, written out again: what `paint_box` must equal wherever it takes a
    /// shortcut (and what the lowered runtime's canvas computes).
    fn reference_box(s: &mut Surface, cut: Rect, r: Rect, p: &BoxPaint) {
        let c = r.and(cut);
        let (rad, bw) = (p.radius as i32, p.border_w as i32);
        let i = (bw + 128) >> 8;
        let inner = if bw > 0 { Rect::new(r.x0 + i, r.y0 + i, r.x1 - i, r.y1 - i) } else { r };
        let irad = if bw > 0 { (rad - bw).max(0) } else { rad };
        for y in c.y0..c.y1 {
            for x in c.x0..c.x1 {
                let d = &mut s.row_mut(y, x, x + 1)[0];
                let co = rcov(r, rad, x, y);
                if bw > 0 {
                    let ci = if inner.is_empty() { 0 } else { rcov(inner, irad, x, y) }.min(co);
                    if ci > 0 && p.fill >> 24 != 0 {
                        *d = over(*d, pm(p.fill, ci, p.opacity));
                    }
                    if co > ci {
                        *d = over(*d, pm(p.border_color, co - ci, p.opacity));
                    }
                } else if co > 0 && p.fill >> 24 != 0 {
                    *d = over(*d, pm(p.fill, co, p.opacity));
                }
            }
        }
    }

    #[test]
    fn a_box_painted_by_runs_equals_the_box_painted_pixel_by_pixel() {
        let mut z = 9u32;
        let mut rnd = |n: u32| {
            z = z.wrapping_mul(1664525).wrapping_add(1013904223);
            (z >> 8) % n
        };
        for case in 0..if cfg!(miri) { 12 } else { 400 } {
            let r = Rect::xywh(rnd(30) as i32 - 5, rnd(30) as i32 - 5, 1 + rnd(50) as i32, 1 + rnd(40) as i32);
            let p = BoxPaint {
                fill: [0xff40_80c0, 0x8040_80c0, 0x0000_0000, 0xffff_ffff][rnd(4) as usize],
                border_color: 0xc010_2030,
                border_w: [0, 0, 256, 600][rnd(4) as usize],
                radius: [0, 0, 300, 1536, 4000, 100_000][rnd(6) as usize],
                opacity: [255, 255, 90][rnd(3) as usize],
                ..BoxPaint::default()
            };
            let cut = Rect::xywh(rnd(20) as i32, rnd(20) as i32, 1 + rnd(50) as i32, 1 + rnd(50) as i32);
            let base = noise(64, 56, case);
            let (mut a, mut b) = (base.clone(), base.clone());
            let cut = cut.and(a.bounds());
            paint_box(&mut a, cut, r, &p);
            reference_box(&mut b, cut, r, &p);
            assert_eq!(a.px, b.px, "case {case}: {r:?} {p:?} cut {cut:?}");
        }
    }

    #[test]
    fn text_is_cut_to_its_box() {
        let mut s = Surface::new(80, 20);
        let r = Rect::new(2, 2, 40, 18);
        let all = s.bounds();
        paint_text(&mut s, all, r, "Ab#9 zz?", 2, 0xff20_4060, 255);
        let mut n = 0;
        for y in 0..20 {
            for x in 0..80 {
                if s.clamped(x, y) != 0 {
                    assert!(r.has(x, y));
                    n += 1;
                }
            }
        }
        assert!(n > 40);
        assert_eq!(text_width("abc", 2), 36);
    }
}
