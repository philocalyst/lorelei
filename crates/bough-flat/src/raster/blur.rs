//! The backdrop blur: the window-anchored pyramid whose footprints the damage engine is exact for.
use super::{Surface, div255, rcov};
use bough_damage::Rect;

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
/// The pyramid is the one damage engine (`bough-damage`) documents: anchored to the window, `[1 3 3 1]` down, `[1 3]`/`[3 1]`
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
