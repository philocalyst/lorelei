//! `paint_backdrop_blur` against the pyramid exactly as `bough-damage`'s documentation defines it (written out
//! again here, per channel, with the raster's stated rounding: each level rounded to 8 bits), on small windows
//! that are not powers of two, every level, several opacities and rounded, clipped and off-window rects; and the
//! claims the damage engine rests on: the result does not depend on how much backdrop beyond the read rect the
//! executor kept, and a tile repaired alone equals the same tile of a full repaint.
use bough_damage::BlurLayer;
use bough_flat::Rect;
use bough_flat::raster::{BlurScratch, Surface, div255, paint_backdrop_blur, rcov};

fn noise(r: Rect, seed: u32) -> Surface {
    let mut s = Surface::at(r);
    let mut z = seed;
    for p in s.px.iter_mut() {
        z = z.wrapping_mul(1664525).wrapping_add(1013904223);
        *p = z;
    }
    s
}

/// One channel's pyramid over the whole window, clamped at the window's edges, rounded per level.
fn pyramid(ch: &[i64], w: i32, h: i32, levels: u8) -> Vec<i64> {
    // a level: texels x0..=x1, y0..=y1
    struct L {
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        v: Vec<i64>,
    }
    impl L {
        fn at(&self, x: i32, y: i32) -> i64 {
            let (x, y) = (x.clamp(self.x0, self.x1), y.clamp(self.y0, self.y1));
            self.v[((y - self.y0) * (self.x1 - self.x0 + 1) + (x - self.x0)) as usize]
        }
    }
    let wt = [1i64, 3, 3, 1];
    let mut stack = vec![L { x0: 0, y0: 0, x1: w - 1, y1: h - 1, v: ch.to_vec() }];
    for _ in 0..levels {
        let g = stack.last().unwrap();
        let (x0, y0, x1, y1) = (g.x0 >> 1, g.y0 >> 1, g.x1 >> 1, g.y1 >> 1);
        let mut v = Vec::new();
        for j in y0..=y1 {
            for i in x0..=x1 {
                let mut s = 0;
                for b in 0..4 {
                    for a in 0..4 {
                        s += wt[b] * wt[a] * g.at(2 * i - 1 + a as i32, 2 * j - 1 + b as i32);
                    }
                }
                v.push((s + 32) >> 6);
            }
        }
        stack.push(L { x0, y0, x1, y1, v });
    }
    let tap = |i: i32| if i & 1 == 0 { ((i >> 1) - 1, 1i64, 3i64) } else { (i >> 1, 3, 1) };
    let mut cur = stack.pop().unwrap();
    while let Some(f) = stack.pop() {
        let mut v = Vec::new();
        for j in f.y0..=f.y1 {
            let (ya, wya, wyb) = tap(j);
            for i in f.x0..=f.x1 {
                let (xa, wxa, wxb) = tap(i);
                let s = wya * (wxa * cur.at(xa, ya) + wxb * cur.at(xa + 1, ya)) + wyb * (wxa * cur.at(xa, ya + 1) + wxb * cur.at(xa + 1, ya + 1));
                v.push((s + 8) >> 4);
            }
        }
        cur = L { v, ..f };
    }
    cur.v
}

/// What the documentation says the layer paints: its blur under coverage `rcov * opacity`, over `dst`.
fn oracle(dst: &Surface, back: &Surface, r: Rect, radius: u32, levels: u8, opacity: u32) -> Surface {
    let (w, h) = (back.w, back.h);
    let mut out = dst.clone();
    if levels == 0 || opacity == 0 {
        return out;
    }
    let blurred: Vec<Vec<i64>> = (0..4).map(|k| pyramid(&back.px.iter().map(|p| (p >> (8 * k) & 0xff) as i64).collect::<Vec<_>>(), w, h, levels)).collect();
    for y in 0..h {
        for x in 0..w {
            let cov = div255(rcov(r, radius as i32, x, y) * opacity);
            if cov == 0 {
                continue;
            }
            let i = (y * w + x) as usize;
            let d = out.px[i];
            let mut p = 0;
            for (k, b) in blurred.iter().enumerate() {
                let sh = 8 * k as u32;
                p |= (div255(b[i] as u32 * cov) + div255((d >> sh & 0xff) * (255 - cov))).min(255) << sh;
            }
            out.px[i] = p;
        }
    }
    out
}

fn copy_of(s: &Surface, r: Rect) -> Surface {
    let mut c = Surface::at(r);
    for y in r.y0..r.y1 {
        c.row_mut(y, r.x0, r.x1).copy_from_slice(s.row(y, r.x0, r.x1));
    }
    c
}

#[test]
fn the_backdrop_blur_is_the_documented_pyramid_bit_for_bit() {
    let mut sc = BlurScratch::default();
    for (w, h) in [(97, 75), (64, 64), (130, 41)] {
        let win = Rect::new(0, 0, w, h);
        let rects = [Rect::new(w / 4, h / 5, w - 9, h - 7), Rect::new(-13, -6, w / 2, h / 2 + 3), Rect::new(w / 2, 3, w + 40, h - 1), Rect::new(0, 0, w, h), Rect::new(11, 9, 12, 30)];
        for levels in 0..=6u8 {
            let back = noise(win, 7 + levels as u32 + w as u32);
            for (k, &r) in rects.iter().enumerate() {
                for (radius, opacity) in [(0u32, 255u32), (6 * 256, 255), (12 * 256 + 100, 128), (3 * 256, 1), (0, 0)] {
                    let dst = noise(win, 99 + k as u32);
                    let want = oracle(&dst, &back, r, radius, levels, opacity);
                    let mut got = dst.clone();
                    paint_backdrop_blur(&mut got, win, r, radius, levels, opacity, &back, &mut sc);
                    if let Some(i) = got.px.iter().zip(&want.px).position(|(a, b)| a != b) {
                        panic!("{w}x{h} levels {levels} rect {r:?} radius {radius} opacity {opacity}: ({},{}) is {:08x}, the pyramid says {:08x}", i as i32 % w, i as i32 / w, got.px[i], want.px[i]);
                    }
                    // what the executor keeps is the read rect cut to the window: the same pixels
                    let layer = BlurLayer { id: 0, z: 1, rect: r, radius: 0, max_level: levels };
                    let read = layer.expanded_read_rect().and(win);
                    if !read.is_empty() {
                        let mut kept = dst.clone();
                        paint_backdrop_blur(&mut kept, win, r, radius, levels, opacity, &copy_of(&back, read), &mut sc);
                        assert_eq!(kept.px, want.px, "{w}x{h} levels {levels} rect {r:?}: a backdrop cut to the read rect {read:?}");
                    }
                }
            }
        }
    }
}

/// What the damage engine culls with: `opaque_core` of a rounded box (radius rounded up to whole pixels, as the
/// flat world asks it) holds only pixels the raster covers whole.
#[test]
fn an_opaque_core_holds_only_pixels_the_raster_covers_whole() {
    let mut z = 1u32;
    let mut rnd = |n: u32| {
        z = z.wrapping_mul(1664525).wrapping_add(1013904223);
        (z >> 8) % n
    };
    for _ in 0..3000 {
        let r = Rect::xywh(rnd(20) as i32 - 10, rnd(20) as i32 - 10, 1 + rnd(90) as i32, 1 + rnd(90) as i32);
        let rad = [0, 1, 100, 256, 700, 1536, 4000, 12_000, 100_000][rnd(9) as usize] + rnd(200);
        let core = bough_damage::opaque_core(r, ((rad + 255) >> 8) as i32);
        assert!(r.contains(core));
        for y in core.y0..core.y1 {
            for x in core.x0..core.x1 {
                assert_eq!(rcov(r, rad as i32, x, y), 255, "{r:?} radius {rad}/256: ({x},{y}) is in the core {core:?}");
            }
        }
    }
}

#[test]
fn a_tile_repaired_alone_equals_that_tile_of_the_full_repaint() {
    let mut sc = BlurScratch::default();
    let (w, h) = (150, 110);
    let win = Rect::new(0, 0, w, h);
    for levels in 1..=6u8 {
        for r in [Rect::new(20, 13, 133, 97), Rect::new(-40, 50, 70, 150)] {
            let layer = BlurLayer { id: 0, z: 1, rect: r, radius: 0, max_level: levels };
            let back = copy_of(&noise(win, 3 * levels as u32), layer.expanded_read_rect().and(win));
            let dst = noise(win, 5);
            let mut full = dst.clone();
            paint_backdrop_blur(&mut full, win, r, 10 * 256, levels, 200, &back, &mut sc);
            for ty in 0..(h + 31) / 32 {
                for tx in 0..(w + 31) / 32 {
                    let tile = Rect::new(tx * 32, ty * 32, (tx * 32 + 32).min(w), (ty * 32 + 32).min(h));
                    let mut alone = dst.clone();
                    // the executor clears and repaints the tile; here the tile's own previous pixels stand in
                    paint_backdrop_blur(&mut alone, tile, r, 10 * 256, levels, 200, &back, &mut sc);
                    for y in 0..h {
                        for x in 0..w {
                            let i = (y * w + x) as usize;
                            let want = if tile.has(x, y) { full.px[i] } else { dst.px[i] };
                            assert_eq!(alone.px[i], want, "levels {levels} rect {r:?} tile ({tx},{ty}) at ({x},{y})");
                        }
                    }
                }
            }
        }
    }
}
