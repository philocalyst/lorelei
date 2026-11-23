//! The engine against a reference that repaints everything and diffs. Random scenes of solid rects (opaque ones
//! are covers), stacked and overlapping backdrop-blur layers and a scrolled group are changed frame after frame
//! (recolours, moves, layer moves and depth changes, a theme, inserts and removals that renumber, scrolls with
//! their seams); the engine is told what changed as a driver would tell it, and after `resolve` the old frame
//! with the engine's blits applied must equal the new frame everywhere outside the tiles the engine marked.
//! Without blur layers or scrolls the engine has no slack, so there its mask, skip counts and report are
//! computed independently and asserted equal.
mod common;
use bough_damage::*;
use common::*;
use std::collections::BTreeSet;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    /// Layers, covers, scrolls: the mask must cover every changed pixel.
    Full,
    /// Covers only, no layers, no scroll: the mask is predicted tile for tile.
    Exact,
}

/// What the test told the engine this frame, for the exact prediction and for the tight oracle.
struct Told {
    w: i32,
    h: i32,
    damage: Vec<(u32, Rect, bool)>,
    masks: BTreeSet<(u32, u32)>,
    /// What changed, as the engine's documented model takes it (a move is the hull when the windows touch, a
    /// theme tells every layer its whole backdrop may have changed).
    model: Vec<Piece>,
    /// What changed, as nearly as the driver knows it (a move is its two windows, a theme only its tiles).
    exact: Vec<Piece>,
    /// The scroll group moved this frame (its covers moved with it: what they hid is not what they hide).
    group_moved: bool,
}
impl Told {
    fn new(w: i32, h: i32) -> Told {
        Told { w, h, damage: Vec::new(), masks: BTreeSet::new(), model: Vec::new(), exact: Vec::new(), group_moved: false }
    }
    fn rebase(&mut self) {
        for d in &mut self.damage {
            d.0 = 0;
            d.2 = true;
        }
        for p in self.model.iter_mut().chain(self.exact.iter_mut()) {
            p.z = 0;
            p.nocull = true;
        }
    }
    fn both(&mut self, p: Piece) {
        self.model.push(p.clone());
        self.exact.push(p);
    }
    /// `e.damage(z, r)`, told.
    fn damage(&mut self, e: &mut DamageEngine, z: u32, r: Rect) {
        e.damage(z, r, Work::Flat);
        self.damage.push((z, r, false));
        self.both(Piece::rect(self.w, self.h, z, r, false));
    }
}

// ---- the tight oracle ----
//
// What must be repainted is computed from first principles, without any of the engine's code: the told changes
// are pixel sets at paint orders, carried up the scene's own items (opaque solids hide what is below them, a
// blur layer replaces its rect with the pyramid of what is below it), the pyramid's footprint being taken by
// brute force from the pyramid of the crate documentation (`common::pyramid`, held to the documentation's text by
// `tests/footprint.rs`). Two sets come out:
//
// * `exact` (E): what can differ for some content consistent with what the driver knows: covers hide pixel by
//   pixel, from the frame's layers as well as from its marks, a move is its two windows, a theme its tiles.
// * `model` (M): E widened by exactly the slack the engine documents, rule by rule (see `oracle`).
//
// Then E ⊆ M (the slack only adds) and, ungated, the engine's mask equals M tile for tile; gated, the mask is
// within M (the gate only narrows) and the pixel reference still holds.

/// A set of the window's pixels: each row is `words` 64-bit words, bit `x & 63` of word `x >> 6`.
#[derive(Clone)]
struct Bits {
    w: i32,
    h: i32,
    words: usize,
    v: Vec<u64>,
}
/// The bits of word `i` that fall in `x0 .. x1`.
fn span(i: usize, x0: i32, x1: i32) -> u64 {
    let (lo, hi) = ((x0 - 64 * i as i32).clamp(0, 64), (x1 - 64 * i as i32).clamp(0, 64));
    if hi <= lo {
        return 0;
    }
    let ones = |n: i32| if n >= 64 { !0u64 } else { (1u64 << n) - 1 };
    ones(hi) & !ones(lo)
}
impl Bits {
    fn new(w: i32, h: i32) -> Bits {
        let words = ((w + 63) / 64) as usize;
        Bits { w, h, words, v: vec![0; words * h as usize] }
    }
    fn rect(w: i32, h: i32, r: Rect) -> Bits {
        let mut b = Bits::new(w, h);
        b.set_rect(r, true);
        b
    }
    fn row(&self, y: i32) -> &[u64] {
        &self.v[y as usize * self.words..(y as usize + 1) * self.words]
    }
    fn row_mut(&mut self, y: i32) -> &mut [u64] {
        let n = self.words;
        &mut self.v[y as usize * n..(y as usize + 1) * n]
    }
    fn set_rect(&mut self, r: Rect, on: bool) {
        let r = r.and(Rect::new(0, 0, self.w, self.h));
        for y in r.y0..r.y1 {
            let row = self.row_mut(y);
            for (i, wd) in row.iter_mut().enumerate() {
                let m = span(i, r.x0, r.x1);
                *wd = if on { *wd | m } else { *wd & !m };
            }
        }
    }
    fn set(&mut self, x: i32, y: i32) {
        self.row_mut(y)[(x >> 6) as usize] |= 1 << (x & 63);
    }
    fn or(&mut self, o: &Bits) {
        for (a, b) in self.v.iter_mut().zip(&o.v) {
            *a |= *b;
        }
    }
    fn and(&mut self, o: &Bits) {
        for (a, b) in self.v.iter_mut().zip(&o.v) {
            *a &= *b;
        }
    }
    /// The bounding box of the set.
    fn hull(&self) -> Rect {
        let mut r = Rect::EMPTY;
        for y in 0..self.h {
            self.runs(y, &mut |a, b| r = r.hull(Rect::new(a, y, b + 1, y + 1)));
        }
        r
    }
    /// The tiles with a pixel in the set (a tile is half a word).
    fn tiles(&self) -> BTreeSet<(u32, u32)> {
        let mut t = BTreeSet::new();
        let tw = ((self.w + 31) / 32) as u32;
        for ty in 0..((self.h + 31) / 32) {
            let mut acc = vec![0u64; self.words];
            for y in ty * 32..(ty * 32 + 32).min(self.h) {
                for (a, b) in acc.iter_mut().zip(self.row(y)) {
                    *a |= *b;
                }
            }
            for tx in 0..tw {
                if (acc[(tx >> 1) as usize] >> (32 * (tx & 1))) as u32 != 0 {
                    t.insert((tx, ty as u32));
                }
            }
        }
        t
    }
    /// The maximal runs of set pixels of row `y`, inclusive.
    fn runs(&self, y: i32, f: &mut dyn FnMut(i32, i32)) {
        let row = self.row(y);
        let mut start: Option<i32> = None;
        for (i, &wd) in row.iter().enumerate() {
            let base = 64 * i as i32;
            let mut x = 0;
            while x < 64 {
                let rest = wd >> x;
                match start {
                    None => {
                        if rest == 0 {
                            break;
                        }
                        x += rest.trailing_zeros() as i32;
                        start = Some(base + x);
                    }
                    Some(s0) => {
                        let ones = (!rest).trailing_zeros() as i32;
                        if x + ones >= 64 {
                            break;
                        }
                        x += ones;
                        f(s0, base + x - 1);
                        start = None;
                    }
                }
            }
        }
        if let Some(s0) = start {
            f(s0, self.w - 1);
        }
    }
}

#[derive(Clone)]
struct Piece {
    z: u32,
    px: Bits,
    /// The backdrop changed here but no pixel needs repainting for it (a blit supplied them, or a mask did).
    virt: bool,
    /// Its order is not known: nothing may hide it.
    nocull: bool,
}
impl Piece {
    fn rect(w: i32, h: i32, z: u32, r: Rect, virt: bool) -> Piece {
        Piece { z, px: Bits::rect(w, h, r), virt, nocull: false }
    }
}

/// The write footprint of every pixel of a window dimension `n`, per pyramid depth, by brute force: an impulse
/// at `p` through the unrounded pyramid of the documentation (positive weights: the outputs that change are
/// exactly the nonzero ones), edges clamped as the raster clamps them. `[levels][p] = (lo, hi)`, inclusive. The
/// changed outputs are asserted to be one interval, and neighbours' intervals to meet, so that the footprint of a
/// run of pixels is the interval from its first's `lo` to its last's `hi`.
fn footprint_table(n: i32) -> std::sync::Arc<Vec<Vec<(i32, i32)>>> {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<i32, Arc<Vec<Vec<(i32, i32)>>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(t) = cache.lock().unwrap().get(&n) {
        return t.clone();
    }
    let mut t = Vec::new();
    for levels in 0..=4u8 {
        let mut row: Vec<(i32, i32)> = Vec::with_capacity(n as usize);
        for p in 0..n {
            let mut src = Grid::new(0, 0, n, 1);
            src.set(p, 0, 1);
            let out = pyramid(&src, levels, false, None).0;
            let xs: Vec<i32> = (0..n).filter(|x| out.get(*x, 0) != 0).collect();
            let (lo, hi) = (xs[0], *xs.last().unwrap());
            assert_eq!(xs.len() as i32, hi - lo + 1, "the footprint of one pixel is an interval");
            if let Some(&(plo, phi)) = row.last() {
                assert!(plo <= lo && phi <= hi && lo <= phi + 1, "footprints are monotone and meet");
            }
            row.push((lo, hi));
        }
        t.push(row);
    }
    let t = Arc::new(t);
    cache.lock().unwrap().insert(n, t.clone());
    t
}

/// Every output pixel some pixel of `s` writes through a pyramid of `levels`, cut to `r`. The pyramid is
/// separable with positive weights, so the 2-D footprint of a pixel is the product of its 1-D footprints, and
/// the footprint of a set is the union of its pixels'.
fn dilate(s: &Bits, levels: u8, r: Rect) -> Bits {
    let (w, h) = (s.w, s.h);
    let mut out = Bits::new(w, h);
    let r = r.and(Rect::new(0, 0, w, h));
    if r.is_empty() {
        return out;
    }
    let (fx, fy) = (footprint_table(w), footprint_table(h));
    let (fx, fy) = (&fx[levels as usize], &fy[levels as usize]);
    let mut row = Bits::new(w, 1);
    for sy in 0..h {
        let (lo_y, hi_y) = fy[sy as usize];
        if hi_y < r.y0 || lo_y >= r.y1 || s.row(sy).iter().all(|wd| *wd == 0) {
            continue;
        }
        // the row's footprint across, cut to the rect's columns
        row.v.fill(0);
        s.runs(sy, &mut |a, b| row.set_rect(Rect::new(fx[a as usize].0.max(r.x0), 0, (fx[b as usize].1 + 1).min(r.x1), 1), true));
        for y in lo_y.max(r.y0)..=hi_y.min(r.y1 - 1) {
            for (a, b) in out.row_mut(y).iter_mut().zip(&row.v) {
                *a |= *b;
            }
        }
    }
    out
}

/// The tight oracle: `(model, exact)` as tile sets (see the section comment).
///
/// The slack rules M adds to E, each the engine's documented behaviour:
///
/// * S1, tiles: a tile is repainted whole if any pixel of it must be (lib.rs: "Tiles are 32 x 32 device
///   pixels"; `resolve`: "mark the tiles").
/// * S2, culling is per cover and per whole tile (lib.rs, opaque covers: "not marked on a tile wholly inside a
///   cover with `cover.z > z`"): a tile hidden only by two covers together, or partly hidden, is repainted.
/// * S3, covers never keep damage from a layer (lib.rs, backdrop-blur layers: "Damage below a layer (culled by a
///   cover or not: the layer may sit between) is carried through the pyramid's exact tap footprint"): a layer's
///   backdrop change is everything told below it, hidden or not.
/// * S4, a move is one rect when the windows touch (`damage_move`: "as one rect when they touch"; touching is
///   within a tile, `old.expand(TILE).hits(new)`): the gap between them is repainted.
/// * S5, a theme mask tells every layer its whole backdrop may have changed (`damage_mask`).
///
/// With `gated` (each layer's real backdrop diff, by id; `None` for a layer with no old backdrop, which the gate
/// takes as all changed), the engine may narrow each part of a backdrop it asks about to a rect within it
/// (`resolve`: "`Some(rect)` (a part of it, or all of it)"). Every part it asks about is within the layer's
/// backdrop change B, so what passes the gate is within B cut to the hull of the diff inside B: M takes that.
///
/// What the engine claims exact is not widened: the footprint itself (lib.rs: "the pyramid the footprints are
/// exact for"), the cascade (a layer's changed output is the next layer's changed backdrop), the scroll's strip
/// (`scroll`: "damaged only the strip the blit cannot supply"), the seams (`seam`: "where it is and where the
/// blit carried its pixels") and the culling rule itself. Rebased damage (`rebase_orders`) has order 0 and is
/// never culled in both sets: that is the driver's knowledge, not the engine's slack.
fn oracle(sc: &Scene, told: &Told, gated: Option<&dyn Fn(u32) -> Option<Bits>>) -> (BTreeSet<(u32, u32)>, BTreeSet<(u32, u32)>) {
    let (w, h) = (sc.w, sc.h);
    let covers = sc.covers();
    // ---- M ----
    let mut layers = sc.layers();
    layers.sort_by_key(|l| (l.z, l.id));
    let mut outs: Vec<(u32, Bits)> = Vec::new();
    for l in &layers {
        let mut b = Bits::new(w, h);
        for p in told.model.iter().filter(|p| p.z < l.z) {
            b.or(&p.px);
        }
        for (_, o) in outs.iter().filter(|o| o.0 < l.z) {
            b.or(o);
        }
        if let Some(Some(diff)) = gated.map(|g| g(l.id)) {
            let mut d = b.clone();
            d.and(&diff);
            let hull = d.hull();
            b.and(&Bits::rect(w, h, hull));
        }
        outs.push((l.z, dilate(&b, l.max_level, l.rect)));
    }
    let mut model = told.masks.clone();
    let marks = told.model.iter().filter(|p| !p.virt).map(|p| (p.z, &p.px, p.nocull)).chain(outs.iter().map(|(z, o)| (*z, o, false)));
    for (z, px, nocull) in marks {
        for t in px.tiles() {
            if nocull || cover_over(&covers, t.0, t.1, w, h).is_none_or(|cz| cz <= z) {
                model.insert(t);
            }
        }
    }
    // ---- E ----
    // `v`: the composite after item k may differ from what the retained frame (blits applied) holds; `c`: what
    // the layers above read may differ. `vn`/`cn`: the same for changes of unknown order, which nothing hides.
    let (mut v, mut c, mut vn, mut cn) = (Bits::new(w, h), Bits::new(w, h), Bits::new(w, h), Bits::new(w, h));
    let top = told.exact.iter().map(|p| p.z as usize + 1).max().unwrap_or(0).max(sc.items.len());
    for k in 0..top {
        if let Some(it) = sc.items.get(k) {
            let r = sc.win(k);
            match it.kind {
                Kind::Solid { alpha: 255, .. } => {
                    v.set_rect(r, false);
                    // a cover that moved with the content hid other pixels before the blit
                    if !(told.group_moved && it.group) {
                        c.set_rect(r, false);
                    }
                }
                Kind::Blur { levels } => {
                    let mut read = c.clone();
                    read.or(&cn);
                    let o = dilate(&read, levels, r);
                    v.set_rect(r, false);
                    v.or(&o);
                    c.set_rect(r, false);
                    c.or(&o);
                }
                Kind::Solid { .. } => {}
            }
        }
        for p in told.exact.iter().filter(|p| p.z as usize == k) {
            let (vv, cc) = if p.nocull { (&mut vn, &mut cn) } else { (&mut v, &mut c) };
            cc.or(&p.px);
            if !p.virt {
                vv.or(&p.px);
            }
        }
    }
    v.or(&vn);
    let mut exact = v.tiles();
    exact.extend(told.masks.iter().copied());
    (model, exact)
}

fn tiles_of(r: Rect, w: i32, h: i32) -> Vec<(u32, u32)> {
    let r = r.and(Rect::new(0, 0, w, h));
    if r.is_empty() {
        return Vec::new();
    }
    let mut v = Vec::new();
    for ty in r.y0 >> 5..=(r.y1 - 1) >> 5 {
        for tx in r.x0 >> 5..=(r.x1 - 1) >> 5 {
            v.push((tx as u32, ty as u32));
        }
    }
    v
}

/// The paint order above which a tile is hidden: the highest cover whose core holds the whole tile (a tile the
/// window's edge cuts is whole when the core reaches that edge), written out from the documentation.
fn cover_over(covers: &[OpaqueCover], tx: u32, ty: u32, w: i32, h: i32) -> Option<u32> {
    let t = Rect::new((tx as i32) * 32, (ty as i32) * 32, ((tx as i32 + 1) * 32).min(w), ((ty as i32 + 1) * 32).min(h));
    covers.iter().filter(|c| c.core_rect.and(Rect::new(0, 0, w, h)).contains(t)).map(|c| c.z).max()
}

fn random_rect(s: &mut u64, w: i32, h: i32) -> Rect {
    let x = range(s, -30, w + 10);
    let y = range(s, -30, h + 10);
    Rect::xywh(x, y, 4 + rnd(s, (w / 2) as u32) as i32, 4 + rnd(s, (h / 2) as u32) as i32)
}

fn random_solid(s: &mut u64) -> Kind {
    let alpha = if rnd(s, 3) == 0 { 255 } else { [40, 128, 200, 255][rnd(s, 4) as usize] };
    Kind::Solid { color: rnd(s, 1 << 20) as i64, alpha }
}

fn scene(s: &mut u64, w: i32, h: i32, mode: Mode) -> Scene {
    let mut sc = Scene { w, h, items: Vec::new(), clip: Rect::EMPTY, off: (0, 0), next_id: 1 };
    let push = |sc: &mut Scene, rect: Rect, kind: Kind, group: bool| {
        let id = sc.next_id;
        sc.next_id += 1;
        sc.items.push(Item { id, rect, kind, group });
    };
    let all = sc.window();
    push(&mut sc, all, Kind::Solid { color: rnd(s, 1 << 20) as i64, alpha: 255 }, false);
    for _ in 0..2 + rnd(s, 4) {
        let r = random_rect(s, w, h);
        push(&mut sc, r, random_solid(s), false);
    }
    if mode == Mode::Full {
        // a scrolled group: a clip and contiguous content in its own coordinates
        sc.clip = Rect::new(range(s, -10, w / 3), range(s, -10, h / 3), range(s, w / 2, w + 10), range(s, h / 2, h + 10));
        for _ in 0..2 + rnd(s, 4) {
            let r = Rect::xywh(range(s, -20, w), range(s, -40, 2 * h), 6 + rnd(s, (w / 2) as u32) as i32, 6 + rnd(s, 50) as i32);
            push(&mut sc, r, random_solid(s), true);
        }
    }
    for _ in 0..2 + rnd(s, 4) {
        let r = random_rect(s, w, h);
        push(&mut sc, r, random_solid(s), false);
        if mode == Mode::Full && rnd(s, 2) == 0 {
            // stacked glass: layers over layers, overlapping
            let g = random_rect(s, w, h);
            push(&mut sc, g, Kind::Blur { levels: rnd(s, 5) as u8 }, false);
            if rnd(s, 2) == 0 {
                // an opaque cap over the glass's edge: what is hidden under it is still the glass's backdrop
                let (cx, cy) = (if rnd(s, 2) == 0 { g.x0 } else { g.x1 }, range(s, g.y0, g.y1.max(g.y0 + 1)));
                let cap = Rect::xywh(cx - range(s, 32, 80), cy - range(s, 32, 80), range(s, 64, 120), range(s, 64, 120));
                push(&mut sc, cap, Kind::Solid { color: rnd(s, 1 << 20) as i64, alpha: 255 }, false);
            }
        }
    }
    sc
}

/// Where an insert may go: above the background and not between two items of the group.
fn insert_slots(sc: &Scene) -> Vec<usize> {
    (1..=sc.items.len()).filter(|&k| !(sc.items[k - 1].group && sc.items.get(k).is_some_and(|i| i.group))).collect()
}
/// What a removal may take: not the background, not the group.
fn remove_slots(sc: &Scene) -> Vec<usize> {
    (1..sc.items.len()).filter(|&k| !sc.items[k].group).collect()
}

fn sync(e: &mut DamageEngine, sc: &Scene) {
    e.covers_clear();
    for c in sc.covers() {
        e.cover_push(c);
    }
    e.covers_commit();
    e.layers_clear();
    for l in sc.layers() {
        e.layer_push(l);
    }
    e.layers_commit();
}

/// One frame of random changes, told to the engine as they are made. Returns whether it scrolled.
fn change(e: &mut DamageEngine, sc: &mut Scene, s: &mut u64, told: &mut Told, mode: Mode) -> bool {
    let (w, h) = (sc.w, sc.h);
    let start = sc.clone();
    let mut scrolled = false;
    // a scroll comes first in its frame (what a driver settles first): the group's later changes are then told
    // at the new offset, where the blit carried their old pixels
    if let (Some((zlo, zhi)), true) = (sc.group_z(), rnd(s, 3) == 0) {
        let (dx, dy) = match rnd(s, 6) {
            0 => (range(s, -60, 60), 0),
            1 => (0, if rnd(s, 2) == 0 { 400 } else { -7 }),
            _ => (range(s, -12, 12), range(s, -45, 45)),
        };
        if (dx, dy) != (0, 0) {
            sc.off = (sc.off.0 + dx, sc.off.1 + dy);
            // what the blit can supply, pixel by pixel: a pixel of the clip whose source is in the clip too
            let clip = sc.clip.and(sc.window());
            let mut strip = Bits::new(w, h);
            let mut kept = 0;
            for y in clip.y0..clip.y1 {
                for x in clip.x0..clip.x1 {
                    if clip.has(x - dx, y - dy) {
                        kept += 1;
                    } else {
                        strip.set(x, y);
                    }
                }
            }
            told.group_moved = !clip.is_empty();
            let blit = e.scroll(zlo, zhi, sc.clip, dx, dy);
            assert_eq!(blit.is_some(), kept > 0, "a blit exactly when some pixel of the clip keeps its source");
            if !clip.is_empty() {
                // the layers above, and the ones that move with the content, read a backdrop that moved
                told.both(Piece::rect(w, h, zlo.saturating_sub(1), clip, true));
                // the strip the blit cannot supply (all of the clip for a jump), at the content's top order
                told.both(Piece { z: zhi, px: strip, virt: false, nocull: false });
            }
            if let Some(b) = blit {
                assert_eq!(b.clip_rect, clip);
                // everything that shows inside the clip without moving with it: below (but the opaque background,
                // a uniform fill whose shift is itself), the glass and everything above
                for k in 1..sc.items.len() {
                    if !sc.items[k].group && start.win(k).hits(b.clip_rect) {
                        e.seam(&b, k as u32, start.win(k));
                        // where it is and where the blit carried its pixels
                        let here = start.win(k).and(clip);
                        let there = here.shift(dx, dy).and(clip);
                        told.both(Piece::rect(w, h, k as u32, here, false));
                        told.both(Piece::rect(w, h, k as u32, there, false));
                    }
                }
                scrolled = true;
            }
        }
    }
    for _ in 0..1 + rnd(s, 3) {
        let n = sc.items.len();
        match rnd(s, 9) {
            // recolour (an opaque rect may stop being a cover, a translucent one become one)
            0 | 1 => {
                let k = rnd(s, n as u32) as usize;
                if let Kind::Solid { .. } = sc.items[k].kind {
                    sc.items[k].kind = if k == 0 { Kind::Solid { color: rnd(s, 1 << 20) as i64, alpha: 255 } } else { random_solid(s) };
                    let win = sc.win(k);
                    told.damage(e, k as u32, win);
                }
            }
            // move or resize anything but the background
            2 | 3 | 4 if n > 1 => {
                let k = 1 + rnd(s, n as u32 - 1) as usize;
                let old = sc.win(k);
                let it = &mut sc.items[k];
                it.rect = match rnd(s, 3) {
                    0 => it.rect.shift(range(s, -9, 9), range(s, -9, 9)),
                    1 => Rect { x1: it.rect.x1 + range(s, -6, 12), ..it.rect },
                    _ => random_rect(s, w, h),
                };
                let new = sc.win(k);
                if mode == Mode::Exact {
                    told.damage(e, k as u32, old);
                    told.damage(e, k as u32, new);
                } else {
                    e.damage_move(k as u32, old, new, Work::Flat);
                    // E: both windows; M (S4): their hull when they are within a tile of each other
                    told.exact.push(Piece::rect(w, h, k as u32, old, false));
                    told.exact.push(Piece::rect(w, h, k as u32, new, false));
                    if !old.is_empty() && !new.is_empty() && old.expand(32).hits(new) {
                        told.model.push(Piece::rect(w, h, k as u32, old.hull(new), false));
                    } else {
                        told.model.push(Piece::rect(w, h, k as u32, old, false));
                        told.model.push(Piece::rect(w, h, k as u32, new, false));
                    }
                }
            }
            // a layer goes deeper or shallower
            5 => {
                let k = rnd(s, n as u32) as usize;
                if let Kind::Blur { .. } = sc.items[k].kind {
                    sc.items[k].kind = Kind::Blur { levels: rnd(s, 5) as u8 };
                    let win = sc.win(k);
                    told.damage(e, k as u32, win);
                }
            }
            // a theme: some rects recoloured, told as a mask of the tiles they touch
            6 => {
                let mut m = TileMask::new(w, h);
                let mut px = Bits::new(w, h);
                for k in 0..n {
                    if matches!(sc.items[k].kind, Kind::Solid { .. }) && rnd(s, 2) == 0 {
                        let Kind::Solid { alpha, .. } = sc.items[k].kind else { unreachable!() };
                        sc.items[k].kind = Kind::Solid { color: rnd(s, 1 << 20) as i64, alpha };
                        m.mark_rect(sc.win(k));
                        for t in tiles_of(sc.win(k), w, h) {
                            told.masks.insert(t);
                            px.set_rect(Rect::xywh(t.0 as i32 * 32, t.1 as i32 * 32, 32, 32), true);
                        }
                    }
                }
                e.damage_mask(&m);
                if !m.is_empty() {
                    // E: the tiles' pixels changed, at an order the mask does not say; M (S5): every layer's
                    // whole backdrop, at order 0
                    told.exact.push(Piece { z: 0, px, virt: true, nocull: true });
                    told.model.push(Piece::rect(w, h, 0, sc.window(), true));
                }
            }
            // insert a rect (or glass) outside the group: everything above is renumbered
            7 => {
                let slots = insert_slots(sc);
                let at = slots[rnd(s, slots.len() as u32) as usize];
                e.rebase_orders();
                told.rebase();
                let kind = if mode == Mode::Full && rnd(s, 3) == 0 { Kind::Blur { levels: rnd(s, 5) as u8 } } else { random_solid(s) };
                let id = sc.next_id;
                sc.next_id += 1;
                sc.items.insert(at, Item { id, rect: random_rect(s, w, h), kind, group: false });
                let win = sc.win(at);
                told.damage(e, at as u32, win);
            }
            // remove one: its window is damaged at the order just below where it was
            _ => {
                let slots = remove_slots(sc);
                if slots.is_empty() {
                    continue;
                }
                let k = slots[rnd(s, slots.len() as u32) as usize];
                e.rebase_orders();
                told.rebase();
                let win = sc.win(k);
                sc.items.remove(k);
                told.damage(e, k as u32 - 1, win);
            }
        }
    }
    scrolled
}

fn masked(e: &DamageEngine) -> BTreeSet<(u32, u32)> {
    let mut m = BTreeSet::new();
    e.mask().each_tile(&mut |tx, ty| {
        m.insert((tx, ty));
    });
    m
}

/// What a run went through, so a test can say its scenes reached every path.
#[derive(Default, Debug)]
struct Seen {
    marked: u64,
    total: u64,
    blits: u32,
    blocks: u32,
    gated: u32,
    skips: u32,
    rebases: u32,
    /// Ungated: tiles of E, and tiles M (= the mask) holds beyond them.
    exact: u64,
    slack: u64,
}

fn run(seed: u64, w: i32, h: i32, mode: Mode, frames: u32, gated: bool) -> Seen {
    let mut s = seed;
    let mut sc = scene(&mut s, w, h, mode);
    let mut e = DamageEngine::new(w, h);
    sync(&mut e, &sc);
    let mut backs: Vec<(u32, Grid)> = Vec::new();
    let mut old = sc.render(Some(&mut backs));
    let mut seen = Seen::default();
    for f in 0..frames {
        e.begin_frame();
        let mut told = Told::new(w, h);
        let scrolled = change(&mut e, &mut sc, &mut s, &mut told, mode);
        sync(&mut e, &sc);
        covers_are_front_to_back(&e, &sc.covers());
        let mut new_backs = Vec::new();
        let new = sc.render(Some(&mut new_backs));
        let mut asked = 0u32;
        if gated {
            // the gate a retaining executor runs: where the layer's backdrop came out different
            e.resolve(&mut |l, r| {
                asked += 1;
                assert!(l.expanded_read_rect().and(Rect::new(0, 0, w, h)).contains(r), "asked outside the read rect");
                let nb = &new_backs.iter().find(|b| b.0 == l.id).expect("a layer of the scene").1;
                let Some(ob) = backs.iter().find(|b| b.0 == l.id).map(|b| &b.1) else {
                    return Some(r);
                };
                let mut d = Rect::EMPTY;
                for y in r.y0..r.y1 {
                    for x in r.x0..r.x1 {
                        if nb.get(x, y) != ob.get(x, y) {
                            d = d.hull(Rect::xywh(x, y, 1, 1));
                        }
                    }
                }
                (!d.is_empty()).then_some(d)
            });
        } else {
            e.resolve_ungated();
        }
        // the oracle: the old frame, blitted, equals the new one wherever no tile is marked
        let mut got = old.clone();
        for b in e.blits() {
            let k = b.kept();
            for y in k.y0..k.y1 {
                for x in k.x0..k.x1 {
                    got.set(x, y, old.get(x - b.dx, y - b.dy));
                }
            }
        }
        let m = masked(&e);
        for y in 0..h {
            for x in 0..w {
                if got.get(x, y) != new.get(x, y) && !m.contains(&((x >> 5) as u32, (y >> 5) as u32)) {
                    panic!("seed {seed} frame {f} ({mode:?}{}): pixel ({x},{y}) changed in an unmarked tile; blits {:?}, layers {:?}", if gated { ", gated" } else { "" }, e.blits(), sc.layers());
                }
            }
        }
        // the tight oracle: the slack only widens; ungated the mask is the documented model tile for tile, gated
        // it is within it
        let diff = |id: u32| -> Option<Bits> {
            let nb = &new_backs.iter().find(|b| b.0 == id)?.1;
            let ob = &backs.iter().find(|b| b.0 == id)?.1;
            let mut d = Bits::new(w, h);
            for y in 0..h {
                for x in 0..w {
                    if nb.get(x, y) != ob.get(x, y) {
                        d.set(x, y);
                    }
                }
            }
            Some(d)
        };
        let (model, exact) = oracle(&sc, &told, if gated { Some(&diff) } else { None });
        if gated {
            // the gate knows the content: E (for any content) is no bound here, the pixel reference above is
            assert!(m.is_subset(&model), "seed {seed} frame {f} (gated): marked beyond the model: {:?}", m.difference(&model).collect::<Vec<_>>());
        } else {
            assert!(exact.is_subset(&model), "seed {seed} frame {f}: E has tiles M lacks: {:?}", exact.difference(&model).collect::<Vec<_>>());
            assert_eq!(m, model, "seed {seed} frame {f} ({mode:?}): the mask is not the documented model; extra {:?}, missing {:?}; layers {:?}", m.difference(&model).collect::<Vec<_>>(), model.difference(&m).collect::<Vec<_>>(), sc.layers());
            seen.slack += (model.len() - exact.len()) as u64;
            seen.exact += exact.len() as u64;
        }
        // the report says what the engine holds
        let r = e.report();
        let mut again = DamageReport { tiles: Vec::with_capacity(1), ..Default::default() };
        e.report_into(&mut again);
        assert_eq!(again, r);
        let mut rows: Vec<(u16, u16)> = m.iter().map(|(x, y)| (*x as u16, *y as u16)).collect();
        rows.sort_by_key(|(x, y)| (*y, *x));
        assert_eq!(r.tiles, rows);
        assert_eq!((r.dirty_tiles, r.total_tiles), (m.len() as u32, e.mask().tiles_wide() * e.mask().tiles_high()));
        assert_eq!(r.scroll_blits, e.blits());
        assert_eq!(r.blur_blocks, e.blur_blocks());
        assert_eq!(r.covers as usize, sc.covers().iter().filter(|c| !c.core_rect.and(sc.window()).is_empty()).count());
        assert_eq!(r.layers as usize, sc.layers().len());
        assert!(!gated || r.blur_gated <= asked);
        assert!((r.repair_percent - 100.0 * m.len() as f32 / r.total_tiles as f32).abs() < 1e-3);
        // the blocks come a whole pyramid at a time, level 0 being the changed backdrop rect
        let layers = sc.layers();
        let mut k = 0;
        while k < r.blur_blocks.len() {
            let b0 = r.blur_blocks[k];
            let l = layers.iter().find(|l| l.id == b0.layer).unwrap();
            assert_eq!(b0.level, 0);
            let lv0 = Rect::new(b0.x0, b0.y0, b0.x1 + 1, b0.y1 + 1);
            assert!(l.expanded_read_rect().contains(lv0) && !lv0.is_empty());
            for lv in 0..=l.max_level {
                let b = r.blur_blocks[k + lv as usize];
                assert_eq!((b.layer, b.level), (l.id, lv));
                let (wx, wy) = (blur_write_level(b0.x0, b0.x1, lv), blur_write_level(b0.y0, b0.y1, lv));
                assert_eq!((b.x0, b.x1, b.y0, b.y1), (wx.0, wx.1, wy.0, wy.1));
            }
            k += l.max_level as usize + 1;
        }
        assert_eq!(r.work.blur_blocks as usize, r.blur_blocks.len());
        if mode == Mode::Exact {
            assert!(!scrolled && e.blits().is_empty() && r.blur_blocks.is_empty());
            let covers = sc.covers();
            let (mut want, mut culled, mut skips) = (told.masks.clone(), BTreeSet::new(), 0u32);
            for &(z, rect, nocull) in &told.damage {
                for t in tiles_of(rect, w, h) {
                    match cover_over(&covers, t.0, t.1, w, h) {
                        Some(cz) if cz > z && !nocull => {
                            skips += 1;
                            culled.insert(t);
                        }
                        _ => {
                            want.insert(t);
                        }
                    }
                }
            }
            assert_eq!(m, want, "seed {seed} frame {f}: the mask without layers or scrolls");
            assert_eq!(e.occlusion_skips(), skips, "seed {seed} frame {f}: skips");
            assert_eq!(e.occluded_tiles_skipped() as usize, culled.difference(&want).count(), "seed {seed} frame {f}: occluded tiles");
            assert_eq!((r.occlusion_skips, r.occluded_tiles_skipped), (skips, culled.difference(&want).count() as u32));
        }
        seen.marked += m.len() as u64;
        seen.total += r.total_tiles as u64;
        seen.blits += r.scroll_blits.len() as u32;
        seen.blocks += r.blur_blocks.len() as u32;
        seen.gated += r.blur_gated;
        seen.skips += r.occlusion_skips;
        seen.rebases += told.damage.iter().any(|d| d.2) as u32;
        old = new;
        backs = new_backs;
    }
    seen
}

/// `covers()` is the covers pushed since `covers_clear` whose core meets the window, cut to it, front to back
/// (`covers_commit`: "Sort the covers front to back"; `covers`: "The covers, front to back"): `z` never rises.
fn covers_are_front_to_back(e: &DamageEngine, pushed: &[OpaqueCover]) {
    let got = e.covers();
    assert!(got.windows(2).all(|p| p[0].z >= p[1].z), "covers() not front to back: {:?}", got.iter().map(|c| c.z).collect::<Vec<_>>());
    let win = e.window();
    let key = |c: &OpaqueCover| (c.z, c.core_rect.x0, c.core_rect.y0, c.core_rect.x1, c.core_rect.y1);
    let mut want: Vec<_> = pushed.iter().map(|c| OpaqueCover { z: c.z, core_rect: c.core_rect.and(win) }).filter(|c| !c.core_rect.is_empty()).map(|c| key(&c)).collect();
    let mut have: Vec<_> = got.iter().map(key).collect();
    want.sort();
    have.sort();
    assert_eq!(have, want);
}

/// The block of level `lv` of the way down for backdrop texels `lo ..= hi`.
fn blur_write_level(lo: i32, hi: i32, lv: u8) -> (i32, i32) {
    let mut b = [(0, 0); MAX_BLUR_LEVEL as usize + 1];
    blur_write(lo, hi, lv, Some(&mut b));
    b[lv as usize]
}

#[test]
fn random_scenes_with_layers_covers_and_scrolls_are_repaired_wherever_they_changed() {
    // windows that are not whole tiles, and one wider than a super-tile
    let mut all = Seen::default();
    for (seed, w, h, gated) in [(1u64, 150, 110, false), (2, 150, 110, true), (3, 200, 96, false), (4, 97, 131, true), (5, 300, 70, false), (6, 300, 70, true), (7, 128, 128, false), (8, 128, 128, true), (9, 290, 270, true), (10, 64, 300, false)] {
        for k in 0..8 {
            let s = run(seed * 1000 + k, w, h, Mode::Full, 30, gated);
            all.marked += s.marked;
            all.total += s.total;
            all.blits += s.blits;
            all.blocks += s.blocks;
            all.gated += s.gated;
            all.skips += s.skips;
            all.rebases += s.rebases;
            all.exact += s.exact;
            all.slack += s.slack;
        }
    }
    eprintln!("full mode: {} tiles marked of {}; ungated, {} tiles of E and {} of documented slack", all.marked, all.total, all.exact, all.slack);
    // the scenes reached every path, and the test does not pass because everything is marked every frame
    assert!(all.blits > 50 && all.blocks > 500 && all.gated > 50 && all.skips > 100 && all.rebases > 50, "{all:?}");
    assert!((all.marked as f64) < 0.8 * all.total as f64, "{all:?}");
}

#[test]
fn without_layers_or_scrolls_the_mask_skips_and_report_are_exact() {
    for (seed, w, h) in [(11u64, 150, 110), (12, 300, 290), (13, 97, 64), (14, 600, 40), (15, 64, 600)] {
        for k in 0..10 {
            let s = run(seed * 1000 + k, w, h, Mode::Exact, 40, false);
            assert!(s.marked > 0);
        }
    }
}

#[test]
fn a_moved_record_is_damaged_where_it_was_and_is_as_one_rect_only_when_they_touch() {
    let mut e = DamageEngine::new(640, 480);
    let mut s = 9u64;
    for _ in 0..400 {
        let a = random_rect(&mut s, 640, 480);
        let b = if rnd(&mut s, 2) == 0 { a.shift(range(&mut s, -40, 40), range(&mut s, -40, 40)) } else { random_rect(&mut s, 640, 480) };
        e.begin_frame();
        e.damage_move(1, a, b, Work::Flat);
        e.resolve_ungated();
        let got = masked(&e);
        let mut want: BTreeSet<(u32, u32)> = tiles_of(a, 640, 480).into_iter().chain(tiles_of(b, 640, 480)).collect();
        assert!(want.is_subset(&got));
        if !a.is_empty() && !b.is_empty() && a.expand(32).hits(b) {
            want.extend(tiles_of(a.hull(b), 640, 480));
        }
        assert_eq!(got, want, "{a:?} -> {b:?}");
    }
}

/// The tile mask against a naive bitmap: single marks, rect marks, unions and clears, on windows wider than a
/// word of tiles and taller than a super-tile, frame after frame (a clear that misses a super-tile leaves
/// tiles set; a coarse bit that is not set hides tiles from the iteration).
#[test]
fn a_tile_mask_equals_a_naive_bitmap_under_every_kind_of_mark() {
    let mut s = 3u64;
    for (w, h) in [(2100, 900), (300, 2200), (33, 33), (4096 + 7, 300)] {
        let mut m = TileMask::new(w, h);
        let mut o = TileMask::new(w, h);
        let (tw, th) = (m.tiles_wide(), m.tiles_high());
        assert_eq!((tw, th), (((w + 31) / 32) as u32, ((h + 31) / 32) as u32));
        let mut naive = BTreeSet::new();
        for round in 0..200 {
            match rnd(&mut s, 7) {
                0 | 1 => {
                    let (tx, ty) = (rnd(&mut s, tw + 2), rnd(&mut s, th + 2));
                    let fresh = tx < tw && ty < th && !naive.contains(&(tx, ty));
                    assert_eq!(m.mark_tile(tx, ty), fresh);
                    if tx < tw && ty < th {
                        naive.insert((tx, ty));
                    }
                }
                2 | 3 => {
                    let r = Rect::xywh(range(&mut s, -100, w + 50), range(&mut s, -100, h + 50), rnd(&mut s, 900) as i32, rnd(&mut s, 500) as i32);
                    let t = tiles_of(r, w, h);
                    let fresh = t.iter().filter(|t| !naive.contains(t)).count() as u32;
                    assert_eq!(m.mark_rect(r), fresh);
                    naive.extend(t);
                }
                4 => {
                    o.clear();
                    let mut on = BTreeSet::new();
                    for _ in 0..rnd(&mut s, 20) {
                        let (tx, ty) = (rnd(&mut s, tw), rnd(&mut s, th));
                        o.mark_tile(tx, ty);
                        on.insert((tx, ty));
                    }
                    m.or(&o);
                    naive.extend(on);
                }
                5 if round % 3 == 0 => {
                    m.clear();
                    naive.clear();
                }
                _ => {}
            }
            assert_eq!(m.count() as usize, naive.len());
            let mut seen = Vec::new();
            m.each_tile(&mut |tx, ty| seen.push((tx, ty)));
            let mut want: Vec<(u32, u32)> = naive.iter().copied().collect();
            want.sort_by_key(|(x, y)| (*y, *x));
            assert_eq!(seen, want, "{w}x{h} round {round}");
            let mut area = 0;
            m.each_span(&mut |r| {
                for t in tiles_of(r, w, h) {
                    assert!(naive.contains(&t));
                }
                area += r.area();
            });
            assert_eq!(area, naive.iter().map(|(x, y)| m.tile_rect(*x, *y).area()).sum::<i64>());
            for _ in 0..20 {
                let (tx, ty) = (rnd(&mut s, tw + 1), rnd(&mut s, th + 1));
                assert_eq!(m.test(tx, ty), naive.contains(&(tx, ty)));
            }
            assert!((m.fraction() - naive.len() as f32 / (tw * th) as f32).abs() < 1e-6);
        }
        m.mark_all();
        assert_eq!(m.count(), tw * th);
        m.resize(w / 2, h / 2);
        assert!(m.is_empty());
    }
}

#[test]
fn a_scroll_blit_keeps_what_both_the_clip_and_its_shift_hold() {
    let mut s = 4u64;
    for _ in 0..500 {
        let clip = Rect::xywh(range(&mut s, -20, 100), range(&mut s, -20, 100), rnd(&mut s, 200) as i32, rnd(&mut s, 200) as i32);
        let (dx, dy) = (range(&mut s, -120, 120), range(&mut s, -120, 120));
        let b = ScrollBlit { clip_rect: clip, dx, dy };
        let k = b.kept();
        for (x, y) in [(k.x0, k.y0), (k.x1 - 1, k.y1 - 1)] {
            if !k.is_empty() {
                // a kept pixel and its source are both in the clip
                assert!(clip.has(x, y) && clip.has(x - dx, y - dy));
            }
        }
        // and every such pixel is kept
        for _ in 0..20 {
            let (x, y) = (range(&mut s, clip.x0 - 5, clip.x1 + 5), range(&mut s, clip.y0 - 5, clip.y1 + 5));
            assert_eq!(k.has(x, y), clip.has(x, y) && clip.has(x - dx, y - dy), "{b:?} at ({x},{y})");
        }
    }
}

#[test]
fn a_resize_drops_covers_and_layers_and_a_layer_is_never_deeper_than_the_pyramid() {
    let mut e = DamageEngine::new(300, 200);
    e.cover_push(OpaqueCover { z: 4, core_rect: Rect::new(0, 0, 300, 200) });
    e.cover_push(OpaqueCover { z: 5, core_rect: Rect::new(400, 0, 500, 10) });
    e.covers_commit();
    e.layer_push(BlurLayer { id: 1, z: 2, rect: Rect::new(10, 10, 50, 50), radius: 999, max_level: 40 });
    e.layers_commit();
    assert_eq!((e.covers().len(), e.layers()[0].max_level), (1, MAX_BLUR_LEVEL), "a core off the window culls nothing");
    e.begin_frame();
    assert!(!e.pending());
    e.damage(1, Rect::new(-50, -50, -1, -1), Work::Flat);
    assert!(!e.pending(), "damage off the window is no damage");
    e.damage(1, Rect::new(5, 5, 9, 9), Work::Flat);
    assert!(e.pending());
    e.resolve_ungated();
    assert_eq!(e.mask().count(), 0, "under the cover");
    e.resize(700, 70);
    assert!(e.covers().is_empty() && e.layers().is_empty() && !e.pending());
    assert_eq!((e.window(), e.mask().tiles_wide(), e.mask().tiles_high()), (Rect::new(0, 0, 700, 70), 22, 3));
    e.damage(1, Rect::new(5, 5, 9, 9), Work::Flat);
    e.resolve_ungated();
    assert_eq!(e.mask().count(), 1);
}

/// The order of `covers()` and `layers()` is part of the API (the doc of each names it): covers front to back,
/// layers back to front (ties by id), whatever order they were pushed in, after every commit.
#[test]
fn covers_come_front_to_back_and_layers_back_to_front() {
    let mut s = 21u64;
    let mut e = DamageEngine::new(300, 200);
    for round in 0..200 {
        let mut pushed = Vec::new();
        if round % 3 != 2 {
            e.covers_clear();
        } else {
            // pushed onto the committed ones: the next commit sorts all of them
            pushed.extend_from_slice(e.covers());
        }
        for _ in 0..rnd(&mut s, 12) {
            let c = OpaqueCover { z: rnd(&mut s, 6), core_rect: random_rect(&mut s, 300, 200).shift(range(&mut s, -200, 200), 0) };
            e.cover_push(c);
            pushed.push(c);
        }
        e.covers_commit();
        covers_are_front_to_back(&e, &pushed);
        e.layers_clear();
        let mut want = Vec::new();
        for id in 0..rnd(&mut s, 8) {
            let l = BlurLayer { id: 100 - id, z: rnd(&mut s, 4), rect: random_rect(&mut s, 300, 200), radius: 3, max_level: rnd(&mut s, 9) as u8 };
            e.layer_push(l);
            want.push(BlurLayer { max_level: l.max_level.min(MAX_BLUR_LEVEL), ..l });
        }
        e.layers_commit();
        want.sort_by_key(|l| (l.z, l.id));
        assert_eq!(e.layers(), &want[..]);
    }
    // the order is what callers read: the front-most cover first
    e.covers_clear();
    for z in [3, 9, 1, 9, 5] {
        e.cover_push(OpaqueCover { z, core_rect: Rect::new(0, 0, 64, 64) });
    }
    e.covers_commit();
    assert_eq!(e.covers().iter().map(|c| c.z).collect::<Vec<_>>(), vec![9, 9, 5, 3, 1]);
}

#[test]
#[should_panic(expected = "one resolve per frame")]
fn a_frame_resolves_once() {
    let mut e = DamageEngine::new(64, 64);
    e.begin_frame();
    e.resolve_ungated();
    e.resolve_ungated();
}
