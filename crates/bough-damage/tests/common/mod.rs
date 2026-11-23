//! What the engine's tests share: the pyramid of the crate documentation written out again from its text (the
//! oracle the footprints are held to), and a tiny reference scene of solid rects and backdrop-blur layers that
//! repaints everything.
#![allow(dead_code)]
use bough_damage::{BlurLayer, OpaqueCover, Rect};

pub fn lcg(s: &mut u64) -> u32 {
    *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    (*s >> 33) as u32
}
pub fn rnd(s: &mut u64, n: u32) -> u32 {
    lcg(s) % n.max(1)
}
pub fn range(s: &mut u64, lo: i32, hi: i32) -> i32 {
    lo + (lcg(s) % (hi - lo).max(1) as u32) as i32
}

/// The texels of one pyramid level: texel (x, y) for x in `x0 .. x0 + w`, y in `y0 .. y0 + h`.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub x0: i32,
    pub y0: i32,
    pub w: i32,
    pub h: i32,
    pub v: Vec<i128>,
}

impl Grid {
    pub fn new(x0: i32, y0: i32, w: i32, h: i32) -> Grid {
        Grid { x0, y0, w, h, v: vec![0; (w.max(0) * h.max(0)) as usize] }
    }
    pub fn of(r: Rect) -> Grid {
        Grid::new(r.x0, r.y0, r.x1 - r.x0, r.y1 - r.y0)
    }
    pub fn bounds(&self) -> Rect {
        Rect::new(self.x0, self.y0, self.x0 + self.w, self.y0 + self.h)
    }
    #[inline]
    pub fn idx(&self, x: i32, y: i32) -> usize {
        debug_assert!(self.bounds().has(x, y), "({x},{y}) outside {:?}", self.bounds());
        ((y - self.y0) * self.w + (x - self.x0)) as usize
    }
    #[inline]
    pub fn get(&self, x: i32, y: i32) -> i128 {
        self.v[self.idx(x, y)]
    }
    #[inline]
    pub fn set(&mut self, x: i32, y: i32, v: i128) {
        let i = self.idx(x, y);
        self.v[i] = v;
    }
    /// The edge rule of the crate documentation's raster: a read past the level's domain takes the edge texel.
    #[inline]
    pub fn at(&self, x: i32, y: i32) -> i128 {
        self.get(x.clamp(self.x0, self.x0 + self.w - 1), y.clamp(self.y0, self.y0 + self.h - 1))
    }
}

const W4: [i128; 4] = [1, 3, 3, 1];

/// Level `l + 1` from level `l`: texel `i` is `[1 3 3 1] / 8` over texels `2i-1 ..= 2i+2`, in both axes. The
/// domain of the next level is every texel `j >> 1` of this one (texel `j` of level `l` covers pixels
/// `[j << l, (j + 1) << l)`). `round`: divide by 64 rounding to nearest as the raster does; otherwise keep the
/// raw sums (every weight is positive, so a raised input raises every output that reads it).
pub fn down(g: &Grid, round: bool) -> Grid {
    let (x0, y0, x1, y1) = (g.x0 >> 1, g.y0 >> 1, (g.x0 + g.w - 1) >> 1, (g.y0 + g.h - 1) >> 1);
    let mut n = Grid::new(x0, y0, x1 - x0 + 1, y1 - y0 + 1);
    for j in y0..=y1 {
        for i in x0..=x1 {
            let mut s = 0;
            for (b, wy) in W4.iter().enumerate() {
                for (a, wx) in W4.iter().enumerate() {
                    s += wy * wx * g.at(2 * i - 1 + a as i32, 2 * j - 1 + b as i32);
                }
            }
            n.set(i, j, if round { (s + 32) >> 6 } else { s });
        }
    }
    n
}

/// Level `l` back from level `l + 1`, over the texels of `fine`'s domain: texel `2i` is `(t[i-1] + 3 t[i]) / 4`
/// and `2i+1` is `(3 t[i] + t[i+1]) / 4`, in both axes. With `only`, texels outside it are left 0.
pub fn up(c: &Grid, fine: Rect, only: Option<Rect>, round: bool) -> Grid {
    let mut n = Grid::of(fine);
    let r = only.map_or(fine, |o| o.and(fine));
    let tap = |i: i32| if i & 1 == 0 { ((i >> 1) - 1, 1i128, 3i128) } else { (i >> 1, 3, 1) };
    for j in r.y0..r.y1 {
        let (ya, wya, wyb) = tap(j);
        for i in r.x0..r.x1 {
            let (xa, wxa, wxb) = tap(i);
            let s = wya * (wxa * c.at(xa, ya) + wxb * c.at(xa + 1, ya)) + wyb * (wxa * c.at(xa, ya + 1) + wxb * c.at(xa + 1, ya + 1));
            n.set(i, j, if round { (s + 8) >> 4 } else { s });
        }
    }
    n
}

/// The whole pyramid over `src` (its bounds are the domain): down to `levels`, back up. Returns the output
/// (level 0; with `only`, just those pixels are computed) and every level of the way down (`[0]` is `src`).
pub fn pyramid(src: &Grid, levels: u8, round: bool, only: Option<Rect>) -> (Grid, Vec<Grid>) {
    let mut stack = vec![src.clone()];
    for _ in 0..levels {
        let d = down(stack.last().unwrap(), round);
        stack.push(d);
    }
    let mut cur = stack[levels as usize].clone();
    for l in (0..levels as usize).rev() {
        let fine = stack[l].bounds();
        cur = up(&cur, fine, if l == 0 { only } else { None }, round);
    }
    (cur, stack)
}

// ---- the reference scene ----

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// A rect of one value; `alpha` 0 ..= 255 (255: opaque, a cover).
    Solid { color: i64, alpha: i64 },
    /// A backdrop-blur layer: inside its rect, the pyramid of everything below it.
    Blur { levels: u8 },
}

#[derive(Clone, Copy, Debug)]
pub struct Item {
    pub id: u32,
    /// In the scroll group's coordinates if `group`, else the window's.
    pub rect: Rect,
    pub kind: Kind,
    pub group: bool,
}

/// Items in paint order (`z` is the index; item 0 is an opaque background over the whole window). The group
/// items are contiguous, drawn at `off` and cut to `clip`.
#[derive(Clone, Debug)]
pub struct Scene {
    pub w: i32,
    pub h: i32,
    pub items: Vec<Item>,
    pub clip: Rect,
    pub off: (i32, i32),
    pub next_id: u32,
}

impl Scene {
    pub fn window(&self) -> Rect {
        Rect::new(0, 0, self.w, self.h)
    }
    /// Where item `k` paints, in the window.
    pub fn win(&self, k: usize) -> Rect {
        let it = &self.items[k];
        let r = if it.group { it.rect.shift(self.off.0, self.off.1).and(self.clip) } else { it.rect };
        r.and(self.window())
    }
    /// The scroll group's paint orders.
    pub fn group_z(&self) -> Option<(u32, u32)> {
        let lo = self.items.iter().position(|i| i.group)?;
        let hi = self.items.iter().rposition(|i| i.group)?;
        Some((lo as u32, hi as u32))
    }
    pub fn covers(&self) -> Vec<OpaqueCover> {
        (0..self.items.len())
            .filter(|k| matches!(self.items[*k].kind, Kind::Solid { alpha: 255, .. }))
            .map(|k| OpaqueCover { z: k as u32, core_rect: self.win(k) })
            .collect()
    }
    pub fn layers(&self) -> Vec<BlurLayer> {
        (0..self.items.len())
            .filter_map(|k| match self.items[k].kind {
                Kind::Blur { levels } => Some(BlurLayer { id: self.items[k].id, z: k as u32, rect: self.win(k), radius: 1 << levels >> 1, max_level: levels }),
                _ => None,
            })
            .collect()
    }
    /// Everything painted from nothing. `backdrops` receives, per blur layer, what was below it (its id and the
    /// whole window as it stood).
    pub fn render(&self, mut backdrops: Option<&mut Vec<(u32, Grid)>>) -> Grid {
        let mut img = Grid::of(self.window());
        if let Some(b) = backdrops.as_deref_mut() {
            b.clear();
        }
        for k in 0..self.items.len() {
            let r = self.win(k);
            match self.items[k].kind {
                Kind::Solid { color, alpha } => {
                    for y in r.y0..r.y1 {
                        for x in r.x0..r.x1 {
                            let v = img.get(x, y);
                            let c = color as i128;
                            img.set(x, y, if alpha == 255 { c } else { (v * (255 - alpha as i128) + c * alpha as i128 + 127) / 255 });
                        }
                    }
                }
                Kind::Blur { levels } => {
                    if let Some(b) = backdrops.as_deref_mut() {
                        b.push((self.items[k].id, img.clone()));
                    }
                    if r.is_empty() {
                        continue;
                    }
                    let (out, _) = pyramid(&img, levels, true, Some(r));
                    for y in r.y0..r.y1 {
                        for x in r.x0..r.x1 {
                            img.set(x, y, out.get(x, y));
                        }
                    }
                }
            }
        }
        img
    }
}
