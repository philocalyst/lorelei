//! The lowered runtime's CPU backend (Integration Card 3, decision D5): generic records composited in integers,
//! premultiplied, with antialiased rounded corners, borders, alpha, glyph coverage, colour bitmaps and images.
//! Nothing is forced opaque and nothing is skipped: a kind this backend does not know panics.
//!
//! Pixels are premultiplied RGBA8 packed with red in the low byte; an untouched canvas is transparent black.
//! Every operation is a pure function of the record, its rect and the clip, so a tile repainted alone equals
//! the same tile of a full repaint: that is what the retained == full redraw gate rests on.
use super::text::TextSys;
use bough_dataflow::draw::{Draws, Prims};
use bough_dataflow::expr::U;
use bough_dataflow::kinds::{FillArch, LabelArch, NumArch, OutlineArch};
use bough_dataflow::prim::{P_BOX, P_IMAGE, P_TEXT, Prim};
use bough_dataflow::store::{Backend, ColRef, Rect, Tiles};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Decoded images at the size they are drawn: premultiplied RGBA8.
#[derive(Default)]
pub struct Images {
    items: Vec<(u32, u32, Vec<u8>)>,
    by_key: HashMap<(usize, usize, u32, u32), u32>,
}
impl Images {
    /// The image `png` at `tw` x `th` device pixels: decoded and resampled the first time.
    pub fn put(&mut self, png: &'static [u8], src: (u32, u32), tw: u32, th: u32) -> u32 {
        *self
            .by_key
            .entry((png.as_ptr() as usize, png.len(), tw, th))
            .or_insert_with(|| {
                let mut full = vec![0u8; (src.0 * src.1 * 4) as usize];
                crate::ui::decode_premultiplied(png, &mut full);
                let mut out = vec![0u8; (tw * th * 4) as usize];
                crate::ui::resample(&full, src.0, src.1, &mut out, tw, th);
                self.items.push((tw, th, out));
                self.items.len() as u32 - 1
            })
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

pub struct Canvas {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u32>,
    pub clip: Rect,
    pub text: Rc<RefCell<TextSys>>,
    pub images: Rc<RefCell<Images>>,
    /// Records visited by paint passes (a count band for the gates).
    pub records: u64,
}

/// A typed archetype's colour (`0xRRGGBB`, opaque) as this backend's (red low, straight alpha).
#[inline(always)]
fn xcolor(c: u32) -> u32 {
    0xff00_0000 | (c & 0xff) << 16 | (c & 0xff00) | (c >> 16 & 0xff)
}
#[inline(always)]
fn div255(x: u32) -> u32 {
    let x = x + 128;
    (x + (x >> 8)) >> 8
}
/// Source over destination: `src` premultiplied RGBA.
#[inline(always)]
fn over(dst: u32, src: [u32; 4]) -> u32 {
    #[cfg(feature = "mutant_lower_alpha")]
    let src = if src[3] != 0 {
        [
            src[0] * 255 / src[3],
            src[1] * 255 / src[3],
            src[2] * 255 / src[3],
            255,
        ]
    } else {
        src
    };
    let ia = 255 - src[3];
    let ch = |s: u32, sh: u32| (s + div255((dst >> sh & 0xff) * ia)).min(255) << sh;
    ch(src[0], 0) | ch(src[1], 8) | ch(src[2], 16) | ch(src[3], 24)
}
/// A straight-alpha colour (red low) under coverage `cov` (0..=255) and opacity `alpha`, premultiplied.
#[inline(always)]
fn pm(color: u32, cov: u32, alpha: u32) -> [u32; 4] {
    let a = div255(div255((color >> 24) * cov) * alpha);
    [
        div255((color & 0xff) * a),
        div255((color >> 8 & 0xff) * a),
        div255((color >> 16 & 0xff) * a),
        a,
    ]
}

/// Coverage (0..=255) of pixel (x, y) by the rounded box `r` of radius `rad` (1/256 px): 255 away from the
/// corners, a 4 x 4 sample count inside a corner's square.
#[inline(always)]
fn rcov(r: Rect, rad: i32, x: i32, y: i32) -> u32 {
    if x < r.x0 || x >= r.x1 || y < r.y0 || y >= r.y1 {
        return 0;
    }
    if rad <= 0 {
        return 255;
    }
    // distances to the nearest vertical and horizontal edge, in 1/256 px, at the pixel's near corner
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
            // a sample outside the corner's square on either axis is inside the box
            let dx = if (cx == rad && qx < cx) || (cx != rad && qx > cx) {
                (qx - cx) as i64
            } else {
                0
            };
            let dy = if (cy == rad && qy < cy) || (cy != rad && qy > cy) {
                (qy - cy) as i64
            } else {
                0
            };
            n += (dx * dx + dy * dy <= rad as i64 * rad as i64) as u32;
        }
    }
    (n * 255 + 8) / 16
}

impl Canvas {
    pub fn new(w: i32, h: i32, text: Rc<RefCell<TextSys>>, images: Rc<RefCell<Images>>) -> Canvas {
        Canvas {
            w,
            h,
            px: vec![0; (w.max(0) * h.max(0)) as usize],
            clip: Rect {
                x0: 0,
                y0: 0,
                x1: w,
                y1: h,
            },
            text,
            images,
            records: 0,
        }
    }
    fn cut(&self, r: Rect) -> Rect {
        r.and(self.clip).and(Rect {
            x0: 0,
            y0: 0,
            x1: self.w,
            y1: self.h,
        })
    }
    fn clear(&mut self, r: Rect) {
        let r = self.cut(r);
        if r.empty() {
            return;
        }
        for y in r.y0..r.y1 {
            let row = (y * self.w) as usize;
            self.px[row + r.x0 as usize..row + r.x1 as usize].fill(0);
        }
    }
    /// A box: fill inside the border, the border between the outer and the inner rounded shape.
    pub fn rbox(&mut self, r: Rect, cut: Rect, p: &Prim) {
        let c = self.cut(r.and(cut));
        if c.empty() {
            return;
        }
        let rad = p.radius as i32;
        let bw = p.border as i32;
        let inner = if bw > 0 {
            let i = (bw + 128) >> 8;
            Rect {
                x0: r.x0 + i,
                y0: r.y0 + i,
                x1: r.x1 - i,
                y1: r.y1 - i,
            }
        } else {
            r
        };
        let irad = if bw > 0 { (rad - bw).max(0) } else { rad };
        let opaque = p.color >> 24 == 255 && p.alpha == 255 && rad <= 0 && bw <= 0;
        for y in c.y0..c.y1 {
            let row = (y * self.w) as usize;
            if opaque {
                self.px[row + c.x0 as usize..row + c.x1 as usize].fill(p.color);
                continue;
            }
            for x in c.x0..c.x1 {
                let d = &mut self.px[row + x as usize];
                let co = rcov(r, rad, x, y);
                if bw > 0 {
                    let ci = if inner.empty() {
                        0
                    } else {
                        rcov(inner, irad, x, y)
                    }
                    .min(co);
                    if ci > 0 && p.color >> 24 != 0 {
                        *d = over(*d, pm(p.color, ci, p.alpha));
                    }
                    if co > ci {
                        *d = over(*d, pm(p.border_color, co - ci, p.alpha));
                    }
                } else if co > 0 && p.color >> 24 != 0 {
                    *d = over(*d, pm(p.color, co, p.alpha));
                }
            }
        }
    }
    fn line(&mut self, r: Rect, cut: Rect, p: &Prim) {
        let c = self.cut(r.and(cut));
        if c.empty() {
            return;
        }
        let text = self.text.clone();
        let text = text.borrow();
        // a record that outlived its run draws nothing (it is replaced by the description that released it)
        let Some(run) = text.get(p.a, p.b) else {
            return;
        };
        let (ox, oy) = (r.x0 + p.ox, r.y0 + p.oy);
        for g in &run.glyphs {
            let Some(b) = text.bitmap_ready(g.face, g.gid, run.size) else {
                continue;
            };
            let (gx, gy) = (ox + g.x + b.left, oy + g.y + b.top);
            let (x0, x1) = (gx.max(c.x0), (gx + b.w).min(c.x1));
            for y in gy.max(c.y0)..(gy + b.h).min(c.y1) {
                let row = (y * self.w) as usize;
                let src = ((y - gy) * b.w) as usize;
                for x in x0..x1 {
                    let d = &mut self.px[row + x as usize];
                    let i = src + (x - gx) as usize;
                    if b.color {
                        let s = &b.data[i * 4..i * 4 + 4];
                        let a = p.alpha * (p.color >> 24) / 255;
                        *d = over(
                            *d,
                            [
                                div255(s[0] as u32 * a),
                                div255(s[1] as u32 * a),
                                div255(s[2] as u32 * a),
                                div255(s[3] as u32 * a),
                            ],
                        );
                    } else if b.data[i] != 0 {
                        *d = over(*d, pm(p.color, b.data[i] as u32, p.alpha));
                    }
                }
            }
        }
    }
    fn image(&mut self, r: Rect, cut: Rect, p: &Prim) {
        let c = self.cut(r.and(cut));
        if c.empty() {
            return;
        }
        let images = self.images.clone();
        let images = images.borrow();
        let (iw, ih, data) = &images.items[p.a as usize];
        for y in c.y0..c.y1.min(r.y0 + *ih as i32) {
            let row = (y * self.w) as usize;
            for x in c.x0..c.x1.min(r.x0 + *iw as i32) {
                let i = (((y - r.y0) as u32 * iw + (x - r.x0) as u32) * 4) as usize;
                let s = &data[i..i + 4];
                let a = p.alpha;
                let d = &mut self.px[row + x as usize];
                *d = over(
                    *d,
                    [
                        div255(s[0] as u32 * a),
                        div255(s[1] as u32 * a),
                        div255(s[2] as u32 * a),
                        div255(s[3] as u32 * a),
                    ],
                );
            }
        }
    }
    /// Repaint the damaged tiles of `tiles` from `paint` (which paints everything, cut to `self.clip`). Marked
    /// tiles that are neighbours in a tile row are repainted as one span: `paint` walks the records once per
    /// span, not once per tile. (A span is a union of whole damaged tiles, so what is cleared and repainted is
    /// exactly the damage.)
    pub fn repair(&mut self, tiles: &Tiles, mut paint: impl FnMut(&mut Canvas)) {
        let whole = self.clip;
        let mut span: Option<Rect> = None;
        let mut flush = |c: &mut Canvas, t: Rect| {
            c.clip = t.and(whole);
            c.clear(t);
            paint(c);
        };
        tiles.each(|t| match span {
            Some(s) if s.y0 == t.y0 && s.x1 == t.x0 => span = Some(Rect { x1: t.x1, ..s }),
            Some(s) => {
                flush(self, s);
                span = Some(t);
            }
            None => span = Some(t),
        });
        if let Some(s) = span {
            flush(self, s);
        }
        self.clip = whole;
    }
    /// One record, as the stores' paint pass would draw it.
    pub fn one(&mut self, r: Rect, cut: Rect, p: &Prim) {
        self.records += 1;
        CanvasPrims(self).prim(r, cut, p)
    }
    /// The pixels as bytes (premultiplied RGBA).
    pub fn bytes(&self) -> Vec<u8> {
        self.px.iter().flat_map(|p| p.to_le_bytes()).collect()
    }
}

struct CanvasPrims<'a>(&'a mut Canvas);
impl Prims for CanvasPrims<'_> {
    fn solid(&mut self, r: Rect, c: u32) {
        self.0.rbox(
            r,
            r,
            &Prim {
                kind: P_BOX,
                color: xcolor(c),
                alpha: 255,
                ..Prim::default()
            },
        )
    }
    fn text(&mut self, _: Rect, _: Rect, _: &[u8], _: bough_dataflow::draw::Align, _: u32) {
        panic!("the lowered backend draws text as shaped runs (a generic record), not as bytes")
    }
    fn rbox(&mut self, r: Rect, cut: Rect, radius: i32, turn: u32, c: u32) {
        assert!(turn == 0, "the lowered backend does not turn boxes");
        self.0.rbox(
            r,
            cut,
            &Prim {
                kind: P_BOX,
                color: xcolor(c),
                radius: (radius.max(0) as u32) << 8,
                alpha: 255,
                ..Prim::default()
            },
        )
    }
    fn prim(&mut self, r: Rect, cut: Rect, p: &Prim) {
        if p.alpha == 0 {
            return;
        }
        match p.kind {
            P_BOX => self.0.rbox(r, cut, p),
            P_TEXT => self.0.line(r, cut, p),
            P_IMAGE => self.0.image(r, cut, p),
            k => panic!("the lowered backend does not know record kind {k}"),
        }
    }
}

impl Backend for Canvas {
    fn draws<A: Draws>(&mut self, col: ColRef<'_, A>, u: &U) {
        let (org, clip) = (col.space.origin(u), col.space.clip);
        col.each(|r, a| {
            let r = r.shift(org);
            // a record wholly outside what is being repainted is not entered
            if !r.hits(self.clip) {
                return;
            }
            self.records += 1;
            a.draw(r, clip, u, &mut CanvasPrims(self))
        });
    }
    fn fill<A: FillArch>(&mut self, col: ColRef<'_, A>, u: &U) {
        let (org, clip) = (col.space.origin(u), col.space.clip);
        col.each(|r, a| {
            self.records += 1;
            self.rbox(
                r.shift(org),
                clip,
                &Prim {
                    kind: P_BOX,
                    color: xcolor(a.color(u).0),
                    alpha: 255,
                    ..Prim::default()
                },
            )
        });
    }
    fn outline<A: OutlineArch>(&mut self, col: ColRef<'_, A>, u: &U) {
        let (org, clip) = (col.space.origin(u), col.space.clip);
        col.each(|r, a| {
            self.records += 1;
            let r = r.shift(a.offset(u)).shift(org);
            self.rbox(
                r,
                clip,
                &Prim {
                    kind: P_BOX,
                    border: (a.width(u).max(1) as u32) << 8,
                    border_color: xcolor(a.color(u).0),
                    alpha: 255,
                    ..Prim::default()
                },
            )
        });
    }
    fn num<A: NumArch>(&mut self, _: ColRef<'_, A>, _: &U) {
        panic!("the lowered backend draws text as shaped runs: describe numbers through `text(..)`")
    }
    fn label<A: LabelArch>(&mut self, _: ColRef<'_, A>, _: &U) {
        panic!("the lowered backend draws text as shaped runs: describe labels through `text(..)`")
    }
}
