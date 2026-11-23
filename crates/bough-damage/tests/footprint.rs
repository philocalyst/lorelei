//! The backdrop-blur footprints held to the pyramid of the crate documentation, run: perturb backdrop pixels of a
//! window-anchored, edge-clamped pyramid and see which outputs (and which texels of each level) change. Every
//! weight of the pyramid is positive and the oracle keeps raw sums, so a raised input raises exactly the outputs
//! that read it: the truth is exact, and the footprints are claimed exact, so they are asserted equal to it (cut
//! to the domain), not merely to contain it.
mod common;
use bough_damage::{BlurLayer, MAX_BLUR_LEVEL, Rect, blur_level, blur_read, blur_write};
use common::*;

/// `lo ..= hi` cut to `d0 .. d1` (half-open), or `None`.
fn cut(lo: i32, hi: i32, d0: i32, d1: i32) -> Option<(i32, i32)> {
    let (a, b) = (lo.max(d0), hi.min(d1 - 1));
    (a <= b).then_some((a, b))
}

/// The indices at which two rows differ, as one interval (asserting it is one).
fn changed(a: &Grid, b: &Grid) -> Option<(i32, i32)> {
    assert_eq!((a.x0, a.w), (b.x0, b.w));
    let xs: Vec<i32> = (a.x0..a.x0 + a.w).filter(|x| a.get(*x, 0) != b.get(*x, 0)).collect();
    let (&f, &l) = (xs.first()?, xs.last()?);
    assert_eq!(xs.len() as i32, l - f + 1, "a change of one pixel reached a set with a hole: {xs:?}");
    Some((f, l))
}

fn row(d0: i32, n: i32, seed: u64) -> Grid {
    let mut g = Grid::new(d0, 0, n, 1);
    let mut s = seed;
    for v in g.v.iter_mut() {
        *v = rnd(&mut s, 1000) as i128;
    }
    g
}

/// One dimension, every level, domains anchored at negative, odd and zero coordinates with sizes that are not
/// powers of two, every pixel of the domain perturbed (so the domain's edges, where reads clamp, are swept too).
#[test]
fn the_one_dimensional_footprints_are_exactly_what_the_pyramid_reads_and_writes() {
    for levels in 0..=MAX_BLUR_LEVEL {
        let n = (7 << levels) + 13;
        for d0 in [0, -(3 << levels) - 5, 7, -1, 1 - n] {
            let d1 = d0 + n;
            let src = row(d0, n, 11 + levels as u64 * 7 + d0.unsigned_abs() as u64);
            let (base, base_stack) = pyramid(&src, levels, false, None);
            // the write footprint of every single pixel, and of every level on the way down
            let mut write: Vec<(i32, i32)> = Vec::new();
            for p in d0..d1 {
                let mut s = src.clone();
                s.set(p, 0, s.get(p, 0) + 1);
                let (out, stack) = pyramid(&s, levels, false, None);
                let truth = changed(&base, &out).expect("a pixel of the domain is always read by itself or a neighbour");
                let (lo, hi) = blur_write(p, p, levels, None);
                assert_eq!(cut(lo, hi, d0, d1), Some(truth), "levels {levels}, domain {d0}..{d1}: pixel {p}");
                // the blocks: the dirty texels of each level on the way down
                let mut blocks = [(0, 0); MAX_BLUR_LEVEL as usize + 1];
                blur_write(p, p, levels, Some(&mut blocks));
                for l in 0..=levels as usize {
                    let (a, b) = (&base_stack[l], &stack[l]);
                    let dirty = changed(a, b);
                    assert_eq!(cut(blocks[l].0, blocks[l].1, a.x0, a.x0 + a.w), dirty, "levels {levels}, domain {d0}..{d1}: pixel {p}, block of level {l}");
                }
                write.push(truth);
            }
            // a range writes the union of its pixels' footprints (one interval: the footprints are monotone)
            let mut sd = 5u64 + levels as u64;
            for _ in 0..40 {
                let a = range(&mut sd, d0, d1);
                let b = range(&mut sd, a, d1);
                let (lo, hi) = blur_write(a, b, levels, None);
                let union = (write[(a - d0) as usize].0, write[(b - d0) as usize].1);
                assert_eq!(cut(lo, hi, d0, d1), Some(union), "levels {levels}: range {a}..={b}");
                // and perturbing the whole range at once changes exactly that
                let mut s = src.clone();
                for p in a..=b {
                    s.set(p, 0, s.get(p, 0) + 1 + (p & 3) as i128);
                }
                assert_eq!(changed(&base, &pyramid(&s, levels, false, None).0), Some(union));
            }
            // the read footprint of an output range: every pixel whose write footprint meets it, nothing else
            for _ in 0..60 {
                let a = range(&mut sd, d0, d1);
                let b = if lcg(&mut sd) % 3 == 0 { a } else { range(&mut sd, a, d1) };
                let readers: Vec<i32> = (d0..d1).filter(|p| {
                    let (lo, hi) = write[(p - d0) as usize];
                    lo <= b && hi >= a
                }).collect();
                let truth = (readers[0], *readers.last().unwrap());
                assert_eq!(readers.len() as i32, truth.1 - truth.0 + 1);
                let (lo, hi) = blur_read(a, b, levels);
                assert_eq!(cut(lo, hi, d0, d1), Some(truth), "levels {levels}, domain {d0}..{d1}: outputs {a}..={b}");
            }
        }
    }
}

/// Away from every edge the footprints are symmetric about the pixel and of the documented width: a pixel is
/// read by the outputs it writes, and the read of one output is `blur_write` of it (the relation is its own
/// transpose).
#[test]
fn the_read_footprint_is_the_transpose_of_the_write_footprint() {
    for levels in 0..=MAX_BLUR_LEVEL {
        for p in -300..300 {
            let (wl, wh) = blur_write(p, p, levels, None);
            assert_eq!(blur_read(p, p, levels), (wl, wh), "levels {levels} pixel {p}");
            for q in [wl - 1, wl, wh, wh + 1] {
                let (rl, rh) = blur_read(q, q, levels);
                assert_eq!(rl <= p && p <= rh, q >= wl && q <= wh, "levels {levels}: {q} reads {p}?");
            }
            assert!(wl <= p && p <= wh && (levels == 0) == (wl == p && wh == p));
            // a pyramid of `l` levels reaches `2 << l` pixels to each side, give or take the texel grid
            if levels > 0 {
                let reach = 2 << levels;
                assert!((p - wl - reach).abs() <= 1 << levels && (wh - p - reach).abs() <= 1 << levels, "levels {levels} pixel {p}: {wl}..={wh}");
            }
        }
    }
}

#[test]
fn the_level_of_a_radius_is_its_bit_length_up_to_the_deepest_level() {
    for r in -5..=4096 {
        let l = blur_level(r);
        if r <= 0 {
            assert_eq!(l, 0);
        } else if l < MAX_BLUR_LEVEL {
            assert!(1 << (l - 1) <= r && r < 1 << l, "radius {r} -> level {l}");
        } else {
            assert!(r >= 1 << (MAX_BLUR_LEVEL - 1));
        }
    }
    assert_eq!((blur_level(1), blur_level(2), blur_level(3), blur_level(8), blur_level(1 << 20)), (1, 2, 2, 4, MAX_BLUR_LEVEL));
}

fn window(w: i32, h: i32, seed: u64) -> Grid {
    let mut g = Grid::new(0, 0, w, h);
    let mut s = seed;
    for v in g.v.iter_mut() {
        *v = rnd(&mut s, 1000) as i128;
    }
    g
}

/// Two dimensions, a window that is not a power of two: a layer's read rect is exactly the backdrop its output
/// is a function of (cut to the window), and `output_of` is exactly the part of the output a changed backdrop
/// rect changes, for layers inside the window, across its edges and partly off it.
#[test]
fn a_layers_read_rect_and_output_are_exact_in_two_dimensions() {
    let (w, h) = (75, 53);
    let win = Rect::new(0, 0, w, h);
    let rects = [Rect::new(20, 14, 47, 31), Rect::new(-9, 3, 18, 22), Rect::new(60, 40, 90, 70), Rect::new(33, 0, 34, 53), Rect::new(0, 0, 75, 53), Rect::new(5, 50, 70, 53)];
    for levels in 0..=3u8 {
        let src = window(w, h, 3 + levels as u64);
        for (ri, &rect) in rects.iter().enumerate() {
            let layer = BlurLayer { id: 1, z: 1, rect, radius: 0, max_level: levels };
            let r = rect.and(win);
            let base = pyramid(&src, levels, false, Some(r)).0;
            let diff = |s: &Grid| -> Vec<(i32, i32)> {
                let out = pyramid(s, levels, false, Some(r)).0;
                let mut v = Vec::new();
                for y in r.y0..r.y1 {
                    for x in r.x0..r.x1 {
                        if out.get(x, y) != base.get(x, y) {
                            v.push((x, y));
                        }
                    }
                }
                v
            };
            let poke = |x: i32, y: i32| {
                let mut s = src.clone();
                s.set(x, y, s.get(x, y) + 1);
                !diff(&s).is_empty()
            };
            // the read rect: every pixel on its inner edges is read, none just outside it is
            let read = layer.expanded_read_rect().and(win);
            assert!(!read.is_empty() && read.contains(r));
            let (xm, ym) = ((read.x0 + read.x1) / 2, (read.y0 + read.y1) / 2);
            for x in [read.x0, xm, read.x1 - 1] {
                for y in [read.y0, ym, read.y1 - 1] {
                    assert!(poke(x, y), "levels {levels} rect {ri}: ({x},{y}) inside the read rect {read:?} is not read");
                }
            }
            for (x, y) in [(read.x0 - 1, ym), (read.x1, ym), (xm, read.y0 - 1), (xm, read.y1), (read.x0 - 1, read.y0 - 1), (read.x1, read.y1)] {
                if win.has(x, y) {
                    assert!(!poke(x, y), "levels {levels} rect {ri}: ({x},{y}) outside the read rect {read:?} is read");
                }
            }
            // the output of a changed rect: exactly `output_of`, a whole rect
            let mut sd = 17 + ri as u64;
            for _ in 0..8 {
                let x0 = range(&mut sd, read.x0 - 4, read.x1 + 2).clamp(0, w - 1);
                let y0 = range(&mut sd, read.y0 - 4, read.y1 + 2).clamp(0, h - 1);
                let c = Rect::new(x0, y0, x0 + 1 + rnd(&mut sd, 9) as i32, y0 + 1 + rnd(&mut sd, 6) as i32).and(win);
                let mut s = src.clone();
                for y in c.y0..c.y1 {
                    for x in c.x0..c.x1 {
                        s.set(x, y, s.get(x, y) + 2);
                    }
                }
                let got = diff(&s);
                let want = layer.output_of(c).and(win);
                assert_eq!(got.len() as i64, want.area(), "levels {levels} rect {ri}: {c:?} changed {} outputs, output_of says {want:?}", got.len());
                assert!(got.iter().all(|(x, y)| want.has(*x, *y)), "levels {levels} rect {ri}: {c:?} reached outside {want:?}");
            }
        }
    }
}

#[test]
fn an_empty_layer_or_change_has_no_footprint_and_level_zero_is_the_identity() {
    let layer = BlurLayer { id: 1, z: 1, rect: Rect::new(10, 10, 40, 40), radius: 8, max_level: 0 };
    assert_eq!(layer.expanded_read_rect(), layer.rect);
    assert_eq!(layer.output_of(Rect::new(0, 0, 20, 20)), Rect::new(10, 10, 20, 20));
    assert_eq!(layer.output_of(Rect::new(0, 0, 5, 5)), Rect::EMPTY);
    let deep = BlurLayer { max_level: 4, ..layer };
    assert_eq!(deep.output_of(Rect::EMPTY), Rect::EMPTY);
    assert_eq!(BlurLayer { rect: Rect::EMPTY, ..deep }.expanded_read_rect(), Rect::EMPTY);
    // a change far from the layer reaches none of it, one just past the read rect neither
    let read = deep.expanded_read_rect();
    assert!(deep.output_of(Rect::new(read.x1, 20, read.x1 + 3, 22)).is_empty());
    assert!(!deep.output_of(Rect::new(read.x1 - 1, 20, read.x1 + 3, 22)).is_empty());
}
