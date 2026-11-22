//! The flat core's reference raster: integer, premultiplied, safe, and pure.
//!
//! Every kernel here computes a pixel as a function of the record, the pixel's window coordinates and (for the
//! backdrop blur) the backdrop: never of the region being painted. That is what lets a tile repainted alone
//! equal the same tile of a full repaint, which is what the oracles test.
//!
//! Pixels are premultiplied RGBA8 with red in the low byte; colours handed in are straight alpha, red low (the
//! façade's `Color`). Boxes without effects are composited exactly as the lowered runtime's canvas does, so the
//! two can be compared pixel for pixel.
use crate::damage_engine::{Rect, ScrollBlit, shadow_reach};

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
    /// 0: solid; 1: `fill` at the top to `grad_to` at the bottom; 2: left to right.
    pub grad_dir: u32,
    pub grad_to: u32,
    /// Blur of the box itself, pixels (0: crisp, with a border if it has one).
    pub elem_blur: i32,
}

#[inline]
fn grad_at(p: &BoxPaint, r: Rect, x: i32, y: i32) -> u32 {
    match p.grad_dir {
        1 => super::expr::lerp_color(p.fill, p.grad_to, ((y - r.y0) * 256 / (r.y1 - r.y0).max(1)) as u32),
        2 => super::expr::lerp_color(p.fill, p.grad_to, ((x - r.x0) * 256 / (r.x1 - r.x0).max(1)) as u32),
        _ => p.fill,
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
    let solid = p.grad_dir == 0;
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

// ---- the built-in face: 5 x 7 cells, for the core's own tests and examples ----------------------------------

const GLYPHS: [(u8, [u8; 7]); 43] = [
    (b'0', [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E]),
    (b'1', [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E]),
    (b'2', [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F]),
    (b'3', [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E]),
    (b'4', [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02]),
    (b'5', [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E]),
    (b'6', [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E]),
    (b'7', [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08]),
    (b'8', [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E]),
    (b'9', [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C]),
    (b'A', [0x0E, 0x11, 0x11, 0x11, 0x1F, 0x11, 0x11]),
    (b'B', [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E]),
    (b'C', [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E]),
    (b'D', [0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C]),
    (b'E', [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F]),
    (b'F', [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10]),
    (b'G', [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F]),
    (b'H', [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11]),
    (b'I', [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E]),
    (b'J', [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C]),
    (b'K', [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11]),
    (b'L', [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F]),
    (b'M', [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11]),
    (b'N', [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11]),
    (b'O', [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E]),
    (b'P', [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10]),
    (b'Q', [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D]),
    (b'R', [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11]),
    (b'S', [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E]),
    (b'T', [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04]),
    (b'U', [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E]),
    (b'V', [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04]),
    (b'W', [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A]),
    (b'X', [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11]),
    (b'Y', [0x11, 0x11, 0x11, 0x0A, 0x04, 0x04, 0x04]),
    (b'Z', [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F]),
    (b'#', [0x0A, 0x0A, 0x1F, 0x0A, 0x1F, 0x0A, 0x0A]),
    (b'.', [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C]),
    (b':', [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00]),
    (b'-', [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00]),
    (b'+', [0x00, 0x04, 0x04, 0x1F, 0x04, 0x04, 0x00]),
    (b'%', [0x18, 0x19, 0x02, 0x04, 0x08, 0x13, 0x03]),
    (b'?', [0x1F, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1F]),
];

fn glyph(c: u8) -> [u8; 7] {
    let c = c.to_ascii_uppercase();
    if c == b' ' {
        return [0; 7];
    }
    GLYPHS.iter().find(|g| g.0 == c).map_or(GLYPHS[42].1, |g| g.1)
}

/// The advance of one cell at `scale`, and the line height.
pub const fn cell(scale: u32) -> (i32, i32) {
    (6 * scale as i32, 8 * scale as i32)
}
pub fn text_width(text: &str, scale: u32) -> i32 {
    text.len() as i32 * cell(scale).0
}

/// A line of the built-in face from the top-left corner of `r`, cut to `r`.
pub fn paint_text(s: &mut Surface, cut: Rect, r: Rect, text: &str, scale: u32, color: u32, opacity: u32) {
    let c = r.and(cut);
    if c.is_empty() || opacity == 0 || color >> 24 == 0 {
        return;
    }
    let sc = scale.max(1) as i32;
    let src = pm(color, 255, opacity);
    for (k, ch) in text.bytes().enumerate() {
        let gx = r.x0 + k as i32 * 6 * sc;
        if gx >= c.x1 {
            break;
        }
        if gx + 5 * sc <= c.x0 {
            continue;
        }
        let g = glyph(ch);
        for y in c.y0.max(r.y0)..c.y1.min(r.y0 + 7 * sc) {
            let bits = g[((y - r.y0) / sc) as usize];
            if bits == 0 {
                continue;
            }
            for x in c.x0.max(gx)..c.x1.min(gx + 5 * sc) {
                if bits >> (4 - (x - gx) / sc) & 1 != 0 {
                    let d = &mut s.row_mut(y, x, x + 1)[0];
                    *d = over(*d, src);
                }
            }
        }
    }
}

// ---- the backdrop blur ---------------------------------------------------------------------------------------

/// Buffers of the blur pyramid, kept between frames.
#[derive(Clone, Debug, Default)]
pub struct BlurScratch {
    a: Vec<u32>,
    b: Vec<u32>,
}

#[derive(Clone, Copy)]
struct Grid {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}
impl Grid {
    fn w(&self) -> usize {
        (self.x1 - self.x0 + 1).max(0) as usize
    }
    fn h(&self) -> usize {
        (self.y1 - self.y0 + 1).max(0) as usize
    }
    fn cut(self, d: Grid) -> Grid {
        Grid { x0: self.x0.max(d.x0), y0: self.y0.max(d.y0), x1: self.x1.min(d.x1), y1: self.y1.min(d.y1) }
    }
}

#[inline(always)]
fn mix4(t: [u32; 4], w: [u32; 4], acc: &mut [u32; 4], wy: u32) {
    for (k, a) in acc.iter_mut().enumerate() {
        let sh = k as u32 * 8;
        *a += wy * (w[0] * (t[0] >> sh & 0xff) + w[1] * (t[1] >> sh & 0xff) + w[2] * (t[2] >> sh & 0xff) + w[3] * (t[3] >> sh & 0xff));
    }
}
#[inline(always)]
fn pack(acc: [u32; 4], round: u32, shift: u32) -> u32 {
    ((acc[0] + round) >> shift) | ((acc[1] + round) >> shift) << 8 | ((acc[2] + round) >> shift) << 16 | ((acc[3] + round) >> shift) << 24
}

/// Replace the backdrop inside the rounded box `r` by its blur through a pyramid of `levels`, for the pixels of
/// `cut`. `backdrop` holds what is below the layer over (at least) the layer's read rect cut to the window; its
/// bounds are the pyramid's domain (reads past them take the edge texel).
///
/// The pyramid is the one `damage_engine` documents: anchored to the window, `[1 3 3 1]` down, `[1 3]`/`[3 1]`
/// up, each level rounded to 8 bits.
pub fn paint_backdrop_blur(s: &mut Surface, cut: Rect, r: Rect, radius: u32, levels: u8, opacity: u32, backdrop: &Surface, sc: &mut BlurScratch) {
    let out = r.and(cut).and(backdrop.bounds());
    if out.is_empty() || levels == 0 || opacity == 0 {
        return;
    }
    let l = levels as usize;
    let dom0 = backdrop.bounds();
    let dom = |lv: usize| Grid { x0: dom0.x0 >> lv, y0: dom0.y0 >> lv, x1: (dom0.x1 - 1) >> lv, y1: (dom0.y1 - 1) >> lv };
    // need[lv]: the texels of level lv the way up reads
    let mut need = [Grid { x0: out.x0, y0: out.y0, x1: out.x1 - 1, y1: out.y1 - 1 }; 8];
    for lv in 0..l {
        let n = need[lv];
        need[lv + 1] = Grid { x0: (n.x0 - 1) >> 1, y0: (n.y0 - 1) >> 1, x1: (n.x1 + 1) >> 1, y1: (n.y1 + 1) >> 1 };
    }
    // down[lv]: the texels of level lv the way down computes, for lv 1 ..= l
    let mut down = [need[l].cut(dom(l)); 8];
    for lv in (1..l).rev() {
        let d = down[lv + 1];
        down[lv] = Grid { x0: 2 * d.x0 - 1, y0: 2 * d.y0 - 1, x1: 2 * d.x1 + 2, y1: 2 * d.y1 + 2 }.cut(dom(lv));
    }
    let (mut cur, mut nxt) = (std::mem::take(&mut sc.a), std::mem::take(&mut sc.b));
    const W: [u32; 4] = [1, 3, 3, 1];
    // level 1 from the backdrop, then each level from the one before
    let mut have = Grid { x0: 0, y0: 0, x1: -1, y1: -1 };
    for lv in 1..=l {
        let g = down[lv];
        let src_dom = dom(lv - 1);
        nxt.clear();
        nxt.resize(g.w() * g.h(), 0);
        for j in g.y0..=g.y1 {
            for i in g.x0..=g.x1 {
                let mut acc = [0u32; 4];
                for (ty, wy) in W.iter().enumerate() {
                    let yy = 2 * j - 1 + ty as i32;
                    let mut t = [0u32; 4];
                    for (tx, tt) in t.iter_mut().enumerate() {
                        let xx = 2 * i - 1 + tx as i32;
                        *tt = if lv == 1 {
                            backdrop.clamped(xx, yy)
                        } else {
                            let (cx, cy) = (xx.clamp(src_dom.x0, src_dom.x1).clamp(have.x0, have.x1), yy.clamp(src_dom.y0, src_dom.y1).clamp(have.y0, have.y1));
                            cur[(cy - have.y0) as usize * have.w() + (cx - have.x0) as usize]
                        };
                    }
                    mix4(t, W, &mut acc, *wy);
                }
                nxt[(j - g.y0) as usize * g.w() + (i - g.x0) as usize] = pack(acc, 32, 6);
            }
        }
        std::mem::swap(&mut cur, &mut nxt);
        have = g;
    }
    // back up: level lv from level lv + 1, for lv = l - 1 .. 1 into the scratch, and level 0 into the surface
    for lv in (0..l).rev() {
        let g = if lv == 0 { need[0] } else { need[lv].cut(dom(lv)) };
        let src_dom = dom(lv + 1);
        if lv != 0 {
            nxt.clear();
            nxt.resize(g.w() * g.h(), 0);
        }
        for j in g.y0..=g.y1 {
            for i in g.x0..=g.x1 {
                let at = |cur: &Vec<u32>, x: i32, y: i32| {
                    let (cx, cy) = (x.clamp(src_dom.x0, src_dom.x1).clamp(have.x0, have.x1), y.clamp(src_dom.y0, src_dom.y1).clamp(have.y0, have.y1));
                    cur[(cy - have.y0) as usize * have.w() + (cx - have.x0) as usize]
                };
                // even texel 2k reads (k-1, k) with weights (1, 3); odd texel 2k+1 reads (k, k+1) with (3, 1)
                let (xa, wxa, wxb) = if i & 1 == 0 { ((i >> 1) - 1, 1, 3) } else { (i >> 1, 3, 1) };
                let (ya, wya, wyb) = if j & 1 == 0 { ((j >> 1) - 1, 1, 3) } else { (j >> 1, 3, 1) };
                let mut acc = [0u32; 4];
                mix4([at(&cur, xa, ya), at(&cur, xa + 1, ya), 0, 0], [wxa, wxb, 0, 0], &mut acc, wya);
                mix4([at(&cur, xa, ya + 1), at(&cur, xa + 1, ya + 1), 0, 0], [wxa, wxb, 0, 0], &mut acc, wyb);
                let v = pack(acc, 8, 4);
                if lv != 0 {
                    nxt[(j - g.y0) as usize * g.w() + (i - g.x0) as usize] = v;
                } else {
                    let cov = div255(rcov(r, radius as i32, i, j) * opacity);
                    if cov != 0 {
                        let d = &mut s.row_mut(j, i, i + 1)[0];
                        // dst = blurred * cov + dst * (1 - cov), per premultiplied channel
                        let ia = 255 - cov;
                        let ch = |sh: u32| (div255((v >> sh & 0xff) * cov) + div255((*d >> sh & 0xff) * ia)).min(255) << sh;
                        *d = ch(0) | ch(8) | ch(16) | ch(24);
                    }
                }
            }
        }
        if lv != 0 {
            std::mem::swap(&mut cur, &mut nxt);
            have = g;
        }
    }
    sc.a = cur;
    sc.b = nxt;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::damage_engine::BlurLayer;

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
