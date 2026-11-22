//! The engine itself.
use crate::*;

/// Negative controls: each bit breaks one rule, and the oracle that rests on the rule must fail with it
/// (`bough-flat`, `tests/mutants.rs`). Compiled only with the `mutants` feature.
pub mod mutant {
    /// A cover culls damage whatever its paint order (the `cover.z > z` test is dropped).
    pub const OCCLUSION_ANY_Z: u32 = 1 << 0;
    /// A record that moved is damaged only where it is now.
    pub const NO_OLD_WIN: u32 = 1 << 1;
    /// Damage under a blur layer reaches the layer undilated (the pyramid footprint is dropped).
    pub const NO_BLUR_DILATE: u32 = 1 << 2;
    /// A scroll blit's uncovered edge strip is not marked.
    pub const NO_SCROLL_STRIP: u32 = 1 << 3;
    /// (flat core) the last target of every dependency list is skipped.
    pub const CSR_DROP_LAST: u32 = 1 << 4;
    /// (flat core) effect extents are left out of a record's window.
    pub const NO_EFFECT_HULL: u32 = 1 << 5;
    /// Fixed records over a scrolled port get no seam damage.
    pub const NO_SCROLL_SEAM: u32 = 1 << 6;
    /// Damage hidden under a cover does not reach a blur layer that sits between them.
    pub const CULL_BEFORE_BLUR: u32 = 1 << 7;
}

#[derive(Clone, Copy, Debug)]
struct Dmg {
    z: u32,
    rect: Rect,
    /// Carried to blur layers but not marked: the pixels were supplied another way (a blit) or marked as a mask.
    virt: bool,
    /// Its order is no longer known (the driver renumbered): no cover may cull it.
    nocull: bool,
}

/// See the module documentation.
#[derive(Clone, Debug, Default)]
pub struct DamageEngine {
    w: i32,
    h: i32,
    mask: TileMask,
    /// Tiles some damage was culled on this frame.
    culled: TileMask,
    direct: Vec<Dmg>,
    layer_out: Vec<Dmg>,
    covers: Vec<OpaqueCover>,
    /// Per tile: 1 + the highest `z` of a cover whose core holds the whole tile (0: none).
    cover_z: Vec<u32>,
    layers: Vec<BlurLayer>,
    blits: Vec<ScrollBlit>,
    blur_blocks: Vec<BlurBlock>,
    work: WorkEstimate,
    occlusion_skips: u32,
    blur_gated: u32,
    resolved: bool,
    chooser: Chooser,
    choice: (ExecutorChoice, &'static str),
    #[cfg(feature = "mutants")]
    pub mutants: u32,
}

impl DamageEngine {
    pub fn new(w: i32, h: i32) -> DamageEngine {
        let mut e = DamageEngine::default();
        e.resize(w, h);
        e
    }
    /// Take a window size. Covers and layers are dropped (the driver hands them over again).
    pub fn resize(&mut self, w: i32, h: i32) {
        self.w = w.max(0);
        self.h = h.max(0);
        self.mask.resize(w, h);
        self.culled.resize(w, h);
        self.cover_z.clear();
        self.cover_z.resize(self.mask.total() as usize, 0);
        self.covers.clear();
        self.layers.clear();
        self.begin_frame();
    }
    #[inline]
    pub fn mutant(&self, _bit: u32) -> bool {
        #[cfg(feature = "mutants")]
        {
            self.mutants & _bit != 0
        }
        #[cfg(not(feature = "mutants"))]
        {
            false
        }
    }
    pub fn window(&self) -> Rect {
        Rect { x0: 0, y0: 0, x1: self.w, y1: self.h }
    }

    // ---- the scene's occluders and readers ----

    pub fn covers_clear(&mut self) {
        self.covers.clear();
    }
    pub fn cover_push(&mut self, c: OpaqueCover) {
        let core = c.core_rect.and(self.window());
        // a core that holds no whole tile culls nothing
        if !core.is_empty() {
            self.covers.push(OpaqueCover { z: c.z, core_rect: core });
        }
    }
    /// Sort the covers front to back and rebuild the per-tile cover order.
    pub fn covers_commit(&mut self) {
        self.covers.sort_unstable_by(|a, b| b.z.cmp(&a.z));
        self.cover_z.fill(0);
        let (tw, w, h) = (self.mask.tiles_wide() as usize, self.w, self.h);
        for c in &self.covers {
            let r = c.core_rect;
            // whole tiles inside the core; a tile cut by the window's edge is whole if the core reaches the edge
            let tx0 = (r.x0 + TILE - 1) >> TILE_SHIFT;
            let ty0 = (r.y0 + TILE - 1) >> TILE_SHIFT;
            let tx1 = if r.x1 >= w { (w + TILE - 1) >> TILE_SHIFT } else { r.x1 >> TILE_SHIFT };
            let ty1 = if r.y1 >= h { (h + TILE - 1) >> TILE_SHIFT } else { r.y1 >> TILE_SHIFT };
            for ty in ty0..ty1 {
                for tx in tx0..tx1 {
                    let cz = &mut self.cover_z[ty as usize * tw + tx as usize];
                    *cz = (*cz).max(c.z + 1);
                }
            }
        }
    }
    /// The covers, front to back.
    pub fn covers(&self) -> &[OpaqueCover] {
        &self.covers
    }
    pub fn layers_clear(&mut self) {
        self.layers.clear();
    }
    pub fn layer_push(&mut self, l: BlurLayer) {
        self.layers.push(BlurLayer { max_level: l.max_level.min(MAX_BLUR_LEVEL), ..l });
    }
    /// Sort the layers back to front.
    pub fn layers_commit(&mut self) {
        self.layers.sort_unstable_by_key(|l| (l.z, l.id));
    }
    /// The blur layers, back to front.
    pub fn layers(&self) -> &[BlurLayer] {
        &self.layers
    }

    // ---- a frame ----

    pub fn begin_frame(&mut self) {
        self.mask.clear();
        self.culled.clear();
        self.direct.clear();
        self.layer_out.clear();
        self.blits.clear();
        self.blur_blocks.clear();
        self.work = WorkEstimate::default();
        self.occlusion_skips = 0;
        self.blur_gated = 0;
        self.resolved = false;
        self.choice = (ExecutorChoice::Idle, "nothing damaged");
    }
    /// Whether anything was reported since `begin_frame`.
    pub fn pending(&self) -> bool {
        !self.direct.is_empty() || !self.blits.is_empty() || !self.mask.is_empty()
    }
    /// A record at order `z` changed inside `rect` (its effect-expanded window, cut to its clip).
    pub fn damage(&mut self, z: u32, rect: Rect, work: Work) {
        let rect = rect.and(self.window());
        if rect.is_empty() {
            return;
        }
        self.work.affected_records += 1;
        let t = ((rect.area() + 1023) / 1024) as u32;
        match work {
            Work::Flat => {}
            Work::Shadow => self.work.shadow_tiles += t,
            Work::Gradient => self.work.grad_tiles += t,
        }
        self.direct.push(Dmg { z, rect, virt: false, nocull: false });
    }
    /// A record at order `z` was in `old` and is in `new` (windows cut to their clips): both are damaged, as one
    /// rect when they touch and as two when they are apart (a jump across the window does not repaint the gap).
    pub fn damage_move(&mut self, z: u32, old: Rect, new: Rect, work: Work) {
        let old = if self.mutant(mutant::NO_OLD_WIN) { Rect::EMPTY } else { old };
        if old == new || old.is_empty() || new.is_empty() || old.expand(TILE).hits(new) {
            self.damage(z, old.hull(new), work);
        } else {
            self.damage(z, old, work);
            self.damage(z, new, work);
        }
    }
    /// The backdrop changed inside `rect` at order `z` in a way the driver repairs itself: carried to the blur
    /// layers above, not marked.
    pub fn damage_virtual(&mut self, z: u32, rect: Rect) {
        let rect = rect.and(self.window());
        if !rect.is_empty() {
            self.direct.push(Dmg { z, rect, virt: true, nocull: false });
        }
    }
    /// Mark the tiles of `m` (a set the driver maintains, such as "every tile a themed record touches"), and
    /// tell every blur layer its whole backdrop may have changed.
    pub fn damage_mask(&mut self, m: &TileMask) {
        self.mask.or(m);
        if !m.is_empty() {
            let w = self.window();
            self.direct.push(Dmg { z: 0, rect: w, virt: true, nocull: false });
        }
    }
    /// The driver is about to renumber its paint orders (records were added or removed) with damage already
    /// reported this frame: that damage keeps its rect and forgets its order. It reaches every blur layer and
    /// no cover culls it.
    pub fn rebase_orders(&mut self) {
        for d in &mut self.direct {
            d.z = 0;
            d.nocull = true;
        }
    }
    pub fn note_fanout(&mut self, n: u32) {
        self.work.fanout += n;
    }
    /// The content of `clip_rect`, painted at orders `z_lo ..= z_hi`, moved rigidly by (`dx`, `dy`). Returns the
    /// blit (recorded for the executor) if one is worth it, having damaged only the strip the blit cannot
    /// supply; otherwise `None`, having damaged the whole clip. Report what does not move with the content
    /// through [`DamageEngine::seam`].
    ///
    /// The strip is damaged at `z_hi`: a cover that moved with the content must not cull it (the retained
    /// pixels under that cover are not the cover's yet), a fixed cover above all of the content may.
    pub fn scroll(&mut self, z_lo: u32, z_hi: u32, clip_rect: Rect, dx: i32, dy: i32) -> Option<ScrollBlit> {
        let clip = clip_rect.and(self.window());
        if clip.is_empty() || (dx == 0 && dy == 0) {
            return None;
        }
        let b = ScrollBlit { clip_rect: clip, dx, dy };
        let kept = b.kept();
        if kept.is_empty() {
            self.damage(z_hi, clip, Work::Flat);
            self.damage_virtual(z_lo.saturating_sub(1), clip);
            return None;
        }
        // the layers above (and the ones that moved with the content) read a backdrop that moved everywhere
        self.damage_virtual(z_lo.saturating_sub(1), clip);
        if !self.mutant(mutant::NO_SCROLL_STRIP) {
            // clip minus kept: a horizontal strip over the full width, and a vertical one beside what was kept
            if dy > 0 {
                self.damage(z_hi, Rect { y1: kept.y0, ..clip }, Work::Flat);
            } else if dy < 0 {
                self.damage(z_hi, Rect { y0: kept.y1, ..clip }, Work::Flat);
            }
            if dx > 0 {
                self.damage(z_hi, Rect { x0: clip.x0, x1: kept.x0, y0: kept.y0, y1: kept.y1 }, Work::Flat);
            } else if dx < 0 {
                self.damage(z_hi, Rect { x0: kept.x1, x1: clip.x1, y0: kept.y0, y1: kept.y1 }, Work::Flat);
            }
        }
        self.blits.push(b);
        self.work.scroll_blits += 1;
        Some(b)
    }
    /// Something at order `z` whose window is `rect` shows inside a blitted clip without moving with it (a fixed
    /// header, a sticky cell, glass, what shows through from below): it is damaged where it is and where the
    /// blit carried its pixels. (Both at its own order: under a cover that moved with the content the blit
    /// supplied the cover's pixels, and a fixed cover above it reports its own seam.)
    ///
    /// The one thing that needs no seam is an opaque uniform fill below the content whose core holds the whole
    /// clip: a shift of it is itself. The driver decides that; the engine is told only what does need one.
    pub fn seam(&mut self, blit: &ScrollBlit, z: u32, rect: Rect) {
        if self.mutant(mutant::NO_SCROLL_SEAM) {
            return;
        }
        let here = rect.and(blit.clip_rect);
        if here.is_empty() {
            return;
        }
        let there = here.shift(blit.dx, blit.dy).and(blit.clip_rect);
        self.damage(z, here, Work::Flat);
        self.damage(z, there, Work::Flat);
    }

    /// Carry the damage through the blur layers and mark the tiles. `gate` is asked, for a layer and a part of
    /// its backdrop that may have changed, what did change: `Some(rect)` (a part of it, or all of it) to
    /// propagate, `None` if the repainted backdrop is pixel for pixel what it was, which halts the propagation
    /// there. An executor that keeps each layer's backdrop repaints it inside this call.
    pub fn resolve(&mut self, gate: &mut dyn FnMut(&BlurLayer, Rect) -> Option<Rect>) {
        assert!(!self.resolved, "one resolve per frame");
        self.resolved = true;
        self.direct.sort_unstable_by_key(|d| d.z);
        let tw = self.mask.tiles_wide() as usize;
        for li in 0..self.layers.len() {
            let l = self.layers[li];
            let read = l.expanded_read_rect().and(self.window());
            if read.is_empty() {
                continue;
            }
            let (nd, nl) = (self.direct.len(), self.layer_out.len());
            for k in 0..nd + nl {
                let d = if k < nd { self.direct[k] } else { self.layer_out[k - nd] };
                if k < nd && d.z >= l.z {
                    // sorted: the rest of the direct damage is at or above the layer; go on to the layers' own
                    continue;
                }
                let c = d.rect.and(read);
                if c.is_empty() {
                    continue;
                }
                if self.mutant(mutant::CULL_BEFORE_BLUR) && !d.virt && !d.nocull {
                    if let Some((tx0, ty0, tx1, ty1)) = self.mask.tile_range(c) {
                        let all = (ty0..=ty1).all(|ty| (tx0..=tx1).all(|tx| self.cover_z[ty as usize * tw + tx as usize] as u64 > d.z as u64 + 1));
                        if all {
                            continue;
                        }
                    }
                }
                let Some(changed) = gate(&l, c) else {
                    self.blur_gated += 1;
                    continue;
                };
                let changed = changed.and(c);
                if changed.is_empty() {
                    self.blur_gated += 1;
                    continue;
                }
                let out = if self.mutant(mutant::NO_BLUR_DILATE) { changed.and(l.rect) } else { l.output_of(changed) };
                // the dirty blocks of every level, for the executor that keeps the pyramid and for the report
                let (mut bx, mut by) = ([(0, 0); MAX_BLUR_LEVEL as usize + 1], [(0, 0); MAX_BLUR_LEVEL as usize + 1]);
                blur_write(changed.x0, changed.x1 - 1, l.max_level, Some(&mut bx));
                blur_write(changed.y0, changed.y1 - 1, l.max_level, Some(&mut by));
                for lv in 0..=l.max_level as usize {
                    self.blur_blocks.push(BlurBlock { layer: l.id, level: lv as u8, x0: bx[lv].0, y0: by[lv].0, x1: bx[lv].1, y1: by[lv].1 });
                    let texels = (bx[lv].1 - bx[lv].0 + 1) as i64 * (by[lv].1 - by[lv].0 + 1) as i64;
                    self.work.blur_tiles += ((texels + 1023) / 1024) as u32;
                }
                self.work.blur_blocks += l.max_level as u32 + 1;
                if !out.is_empty() {
                    self.layer_out.push(Dmg { z: l.z, rect: out, virt: false, nocull: false });
                }
            }
        }
        // mark, culling what an opaque cover above hides
        let any_cover = !self.covers.is_empty();
        let any_z = self.mutant(mutant::OCCLUSION_ANY_Z);
        for k in 0..self.direct.len() + self.layer_out.len() {
            let d = if k < self.direct.len() { self.direct[k] } else { self.layer_out[k - self.direct.len()] };
            if d.virt {
                continue;
            }
            if !any_cover || d.nocull {
                self.mask.mark_rect(d.rect);
                continue;
            }
            let Some((tx0, ty0, tx1, ty1)) = self.mask.tile_range(d.rect) else {
                continue;
            };
            for ty in ty0..=ty1 {
                for tx in tx0..=tx1 {
                    let cz = self.cover_z[ty as usize * tw + tx as usize];
                    if cz as u64 > d.z as u64 + 1 || (any_z && cz != 0) {
                        self.occlusion_skips += 1;
                        self.culled.mark_tile(tx, ty);
                    } else {
                        self.mask.mark_tile(tx, ty);
                    }
                }
            }
        }
        self.work.dirty_tiles = self.mask.count();
        self.choice = self.chooser.choose(&self.work);
    }
    /// [`DamageEngine::resolve`] with no gate: every backdrop that may have changed is taken to have changed.
    pub fn resolve_ungated(&mut self) {
        self.resolve(&mut |_, r| Some(r));
    }

    // ---- what a frame came to ----

    /// The tiles to repaint (after `resolve`).
    pub fn mask(&self) -> &TileMask {
        &self.mask
    }
    /// The blits to apply, in order, before repainting.
    pub fn blits(&self) -> &[ScrollBlit] {
        &self.blits
    }
    pub fn blur_blocks(&self) -> &[BlurBlock] {
        &self.blur_blocks
    }
    pub fn work(&self) -> &WorkEstimate {
        &self.work
    }
    pub fn choice(&self) -> ExecutorChoice {
        self.choice.0
    }
    pub fn reason(&self) -> &'static str {
        self.choice.1
    }
    /// (damage, tile) pairs a cover culled this frame.
    pub fn occlusion_skips(&self) -> u32 {
        self.occlusion_skips
    }
    /// Distinct tiles some damage wanted and none got.
    pub fn occluded_tiles_skipped(&self) -> u32 {
        let mut n = 0;
        self.culled.each_tile(&mut |tx, ty| n += !self.mask.test(tx, ty) as u32);
        n
    }
    pub fn blur_gated(&self) -> u32 {
        self.blur_gated
    }
    pub fn repair_percent(&self) -> f32 {
        self.mask.fraction() * 100.0
    }
    /// Fill `r` with this frame (its vectors are reused).
    pub fn report_into(&self, r: &mut DamageReport) {
        r.width = self.w;
        r.height = self.h;
        r.tile = TILE;
        r.tiles.clear();
        self.mask.each_tile(&mut |tx, ty| r.tiles.push((tx as u16, ty as u16)));
        r.dirty_tiles = self.mask.count();
        r.total_tiles = self.mask.total();
        r.repair_percent = self.repair_percent();
        r.occluded_tiles_skipped = self.occluded_tiles_skipped();
        r.occlusion_skips = self.occlusion_skips;
        r.blur_blocks.clear();
        r.blur_blocks.extend_from_slice(&self.blur_blocks);
        r.blur_gated = self.blur_gated;
        r.scroll_blits.clear();
        r.scroll_blits.extend_from_slice(&self.blits);
        r.covers = self.covers.len() as u32;
        r.layers = self.layers.len() as u32;
        r.work = self.work;
        r.cost = self.work.cost();
        r.choice = self.choice.0;
        r.reason = self.choice.1;
    }
    pub fn report(&self) -> DamageReport {
        let mut r = DamageReport::default();
        self.report_into(&mut r);
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lcg(s: &mut u64) -> u32 {
        *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (*s >> 33) as u32
    }

    #[test]
    fn a_mask_equals_a_naive_bitmap_under_random_marks() {
        let (w, h) = if cfg!(miri) { (300, 200) } else { (2600, 1500) };
        let mut m = TileMask::new(w, h);
        let (tw, th) = (m.tiles_wide(), m.tiles_high());
        let mut naive = vec![false; (tw * th) as usize];
        let mut s = 7u64;
        for round in 0..if cfg!(miri) { 6 } else { 60 } {
            let x = (lcg(&mut s) % (w as u32 + 200)) as i32 - 100;
            let y = (lcg(&mut s) % (h as u32 + 200)) as i32 - 100;
            let r = Rect::xywh(x, y, (lcg(&mut s) % 700) as i32, (lcg(&mut s) % 300) as i32);
            let mut new = 0;
            let c = r.and(Rect::new(0, 0, w, h));
            if !c.is_empty() {
                for ty in c.y0 >> 5..=(c.y1 - 1) >> 5 {
                    for tx in c.x0 >> 5..=(c.x1 - 1) >> 5 {
                        let b = &mut naive[(ty as u32 * tw + tx as u32) as usize];
                        new += !*b as u32;
                        *b = true;
                    }
                }
            }
            assert_eq!(m.mark_rect(r), new);
            assert_eq!(m.count(), naive.iter().filter(|b| **b).count() as u32);
            let mut seen = Vec::new();
            m.each_tile(&mut |tx, ty| seen.push((tx, ty)));
            let want: Vec<_> = (0..th).flat_map(|ty| (0..tw).map(move |tx| (tx, ty))).filter(|(tx, ty)| naive[(ty * tw + tx) as usize]).collect();
            assert_eq!(seen, want);
            // spans cover exactly the set tiles, each a maximal run
            let mut area = 0i64;
            let mut last: Option<Rect> = None;
            m.each_span(&mut |r| {
                area += r.area();
                if let Some(p) = last {
                    assert!(!(p.y0 == r.y0 && p.x1 == r.x0), "two spans that touch are one span");
                }
                last = Some(r);
            });
            let want_area: i64 = want.iter().map(|(tx, ty)| m.tile_rect(*tx, *ty).area()).sum();
            assert_eq!(area, want_area);
            if round % 20 == 19 {
                m.clear();
                naive.fill(false);
                assert!(m.is_empty());
                let mut n = 0;
                m.each_tile(&mut |_, _| n += 1);
                assert_eq!(n, 0);
            }
        }
    }

    #[test]
    fn a_mask_wider_than_a_word_keeps_runs_across_words() {
        let mut m = TileMask::new(64 * 32 * 3, 64);
        m.mark_rect(Rect::new(60 * 32, 0, 130 * 32, 32));
        let mut runs = Vec::new();
        m.each_span(&mut |r| runs.push(r));
        assert_eq!(runs, vec![Rect::new(60 * 32, 0, 130 * 32, 32)]);
        assert_eq!(m.count(), 70);
        assert!(m.test(63, 0) && m.test(64, 0) && m.test(129, 0) && !m.test(130, 0) && !m.test(64, 1));
    }

    /// The pyramid of the module documentation, in one dimension over i64 (no rounding: the footprint is what is
    /// tested), with clamped reads at the ends of the domain.
    fn pyramid(src: &[i64], levels: u8) -> Vec<i64> {
        fn down(t: &[i64], lo: i32) -> (Vec<i64>, i32) {
            // t[k] is level-l texel lo + k; the next level covers every texel that has a tap inside
            let hi = lo + t.len() as i32 - 1;
            let (a, b) = (lo >> 1, hi >> 1);
            let at = |j: i32| t[(j.clamp(lo, hi) - lo) as usize];
            ((a..=b).map(|i| at(2 * i - 1) + 3 * at(2 * i) + 3 * at(2 * i + 1) + at(2 * i + 2)).collect(), a)
        }
        fn up(t: &[i64], lo: i32, out_lo: i32, out_hi: i32) -> Vec<i64> {
            let hi = lo + t.len() as i32 - 1;
            let at = |i: i32| t[(i.clamp(lo, hi) - lo) as usize];
            (out_lo..=out_hi).map(|j| if j & 1 == 0 { at((j >> 1) - 1) + 3 * at(j >> 1) } else { 3 * at(j >> 1) + at((j >> 1) + 1) }).collect()
        }
        let mut stack = vec![(src.to_vec(), 0i32)];
        for _ in 0..levels {
            let (t, lo) = stack.last().unwrap();
            let d = down(t, *lo);
            stack.push(d);
        }
        let (mut cur, mut lo) = stack.pop().unwrap();
        while let Some((t, tlo)) = stack.pop() {
            cur = up(&cur, lo, tlo, tlo + t.len() as i32 - 1);
            lo = tlo;
        }
        cur
    }

    #[test]
    fn the_blur_footprint_is_exact_for_the_pyramid() {
        let n = if cfg!(miri) { 40 } else { 96 };
        for levels in 0..=if cfg!(miri) { 2 } else { 4u8 } {
            let base: Vec<i64> = (0..n).map(|i| (i * 37 % 101) as i64).collect();
            let want = pyramid(&base, levels);
            // away from the clamped ends, so the footprint is the taps' alone
            let margin = 6 << levels;
            for p in (margin..n - margin).step_by(if cfg!(miri) { 7 } else { 1 }) {
                let mut src = base.clone();
                src[p as usize] += 1 << 20;
                let got = pyramid(&src, levels);
                let changed: Vec<i32> = (0..n).filter(|i| got[*i as usize] != want[*i as usize]).collect();
                let (lo, hi) = blur_write(p, p, levels, None);
                assert!(changed.iter().all(|c| *c >= lo && *c <= hi), "level {levels}: pixel {p} reached outside {lo}..={hi}: {changed:?}");
                // exact: both ends are reached
                assert_eq!((changed[0], *changed.last().unwrap()), (lo, hi), "level {levels} pixel {p}");
                // and the read interval is the transpose
                for c in [lo, hi] {
                    let (rl, rh) = blur_read(c, c, levels);
                    assert!(rl <= p && p <= rh);
                }
            }
        }
    }

    #[test]
    fn a_shadow_window_holds_offset_spread_and_one_and_a_half_blurs() {
        let e = EffectExtents { shadow: true, shadow_dx: 4, shadow_dy: -6, shadow_blur: 10, shadow_spread: 3, blur: 0 };
        let r = Rect::new(100, 100, 200, 150);
        assert_eq!(e.window(r), Rect::new(100 + 4 - 18, 100 - 6 - 18, 200 + 4 + 18, 150 + 18 - 6).hull(r));
        assert_eq!(EffectExtents { blur: 5, ..Default::default() }.window(r), r.expand(5));
        assert_eq!(EffectExtents::default().window(r), r);
    }

    fn engine() -> DamageEngine {
        DamageEngine::new(640, 480)
    }

    #[test]
    fn damage_under_a_cover_is_culled_and_counted_and_damage_over_it_is_not() {
        let mut e = engine();
        e.cover_push(OpaqueCover { z: 10, core_rect: Rect::new(64, 64, 320, 320) });
        e.covers_commit();
        e.begin_frame();
        // wholly under the cover
        e.damage(3, Rect::new(100, 100, 200, 200), Work::Flat);
        e.resolve_ungated();
        assert_eq!(e.mask().count(), 0);
        assert_eq!(e.occluded_tiles_skipped(), 16);
        assert_eq!(e.choice(), ExecutorChoice::Idle);
        // above it
        e.begin_frame();
        e.damage(11, Rect::new(100, 100, 200, 200), Work::Flat);
        e.resolve_ungated();
        assert_eq!((e.mask().count(), e.occlusion_skips()), (16, 0));
        // the cover's own change is not culled by itself
        e.begin_frame();
        e.damage(10, Rect::new(64, 64, 320, 320), Work::Flat);
        e.resolve_ungated();
        assert_eq!(e.mask().count(), 64);
        // straddling the edge: only the tiles inside the core are culled
        e.begin_frame();
        e.damage(3, Rect::new(32, 96, 128, 128), Work::Flat);
        e.resolve_ungated();
        assert_eq!((e.mask().count(), e.occluded_tiles_skipped()), (1, 2));
    }

    #[test]
    fn a_core_that_is_not_tile_aligned_culls_only_whole_tiles() {
        let mut e = engine();
        e.cover_push(OpaqueCover { z: 5, core_rect: Rect::new(70, 70, 200, 200) });
        e.covers_commit();
        e.begin_frame();
        e.damage(1, Rect::new(64, 64, 224, 224), Work::Flat);
        e.resolve_ungated();
        // tiles 3..=5 (96..192) are whole inside 70..200
        assert_eq!(e.occluded_tiles_skipped(), 9);
        assert_eq!(e.mask().count(), 25 - 9);
    }

    #[test]
    fn a_rounded_cover_has_a_smaller_core() {
        let r = Rect::new(0, 0, 200, 200);
        assert_eq!(opaque_core(r, 0), r);
        let c = opaque_core(r, 40);
        // the corner circle's 45 degree point is at 40 * (1 - 0.7071) = 11.7 from each side
        assert!(c.x0 >= 12 && c.x0 <= 14 && c.x1 == 200 - c.x0);
        assert!(opaque_core(Rect::new(0, 0, 4, 4), 100).is_empty());
    }

    #[test]
    fn damage_under_a_blur_layer_reaches_the_layers_output_dilated_and_cascades() {
        let mut e = engine();
        let glass = BlurLayer { id: 1, z: 10, rect: Rect::new(200, 100, 400, 300), radius: 8, max_level: 3 };
        let upper = BlurLayer { id: 2, z: 20, rect: Rect::new(380, 100, 600, 300), radius: 4, max_level: 2 };
        e.layer_push(upper);
        e.layer_push(glass);
        e.layers_commit();
        assert_eq!(e.layers()[0].id, 1);
        // a pixel left of the glass, inside its read rect
        let read = glass.expanded_read_rect();
        assert!(read.x0 < 200 - 8 && read.x0 > 200 - 4 * 8 - 8);
        e.begin_frame();
        let d = Rect::new(190, 150, 194, 154);
        e.damage(2, d, Work::Flat);
        e.resolve_ungated();
        let out = glass.output_of(d);
        assert!(!out.is_empty() && out.x0 == 200 && out.x1 > 200);
        // the glass's dirty output tiles are marked although the damage itself is outside the glass
        assert!(e.mask().test(200 / 32, 150 / 32));
        assert_eq!(e.blur_blocks().iter().filter(|b| b.layer == 1).count(), 4);
        // far from the upper layer: it is not reached
        assert_eq!(e.blur_blocks().iter().filter(|b| b.layer == 2).count(), 0);

        // damage near the glass's right edge cascades: glass output -> the upper layer's read rect
        e.begin_frame();
        let d = Rect::new(370, 200, 372, 202);
        e.damage(2, d, Work::Flat);
        e.resolve_ungated();
        assert!(e.blur_blocks().iter().any(|b| b.layer == 2), "the glass's output is the upper layer's backdrop");
        // damage above the glass does not go through it
        e.begin_frame();
        e.damage(15, Rect::new(250, 150, 260, 160), Work::Flat);
        e.resolve_ungated();
        assert!(e.blur_blocks().iter().all(|b| b.layer != 1));
    }

    #[test]
    fn a_gate_that_finds_the_backdrop_unchanged_halts_the_propagation() {
        let mut e = engine();
        e.layer_push(BlurLayer { id: 1, z: 10, rect: Rect::new(200, 100, 400, 300), radius: 8, max_level: 3 });
        e.layers_commit();
        e.begin_frame();
        e.damage(2, Rect::new(186, 150, 196, 160), Work::Flat);
        let mut asked = 0;
        e.resolve(&mut |l, r| {
            asked += 1;
            assert_eq!(l.id, 1);
            assert!(l.expanded_read_rect().contains(r));
            None
        });
        // only the damage's own two tiles; nothing of the glass
        assert_eq!((asked, e.blur_gated(), e.mask().count()), (1, 1, 2));
        assert!(e.blur_blocks().is_empty());
        // a gate that narrows the change narrows the output
        e.begin_frame();
        e.damage(2, Rect::new(180, 100, 191, 300), Work::Flat);
        e.resolve(&mut |_, r| Some(Rect { y1: r.y0 + 1, ..r }));
        let rows: std::collections::BTreeSet<u32> = {
            let mut s = std::collections::BTreeSet::new();
            e.mask().each_tile(&mut |tx, ty| {
                if tx >= 200 / 32 {
                    s.insert(ty);
                }
            });
            s
        };
        assert!(rows.iter().all(|ty| *ty <= (100 + 40) / 32), "{rows:?}");
    }

    #[test]
    fn damage_hidden_by_a_cover_still_reaches_a_blur_layer_between_them() {
        let mut e = engine();
        // z 2: damage; z 10: glass over 200..400; z 30: an opaque cover over the damage only
        e.layer_push(BlurLayer { id: 1, z: 10, rect: Rect::new(200, 100, 400, 300), radius: 8, max_level: 3 });
        e.layers_commit();
        e.cover_push(OpaqueCover { z: 30, core_rect: Rect::new(160, 128, 192, 192) });
        e.covers_commit();
        e.begin_frame();
        e.damage(2, Rect::new(164, 132, 188, 188), Work::Flat);
        e.resolve_ungated();
        // the damage's own tile is under the cover: skipped
        assert!(!e.mask().test(5, 4) && e.occluded_tiles_skipped() >= 1);
        // the glass beside the cover reads it: its output is dirty
        assert!(e.mask().test(200 / 32, 160 / 32));
    }

    #[test]
    fn a_scroll_blits_and_marks_only_the_uncovered_strip_and_the_seams() {
        let mut e = DamageEngine::new(1280, 800);
        let clip = Rect::new(0, 64, 1280, 800);
        e.begin_frame();
        let b = e.scroll(5, 9, clip, 0, -40).expect("a blit");
        assert_eq!(b.kept(), Rect::new(0, 64, 1280, 760));
        // a fixed badge over the list
        e.seam(&b, 50, Rect::new(1100, 100, 1200, 140));
        e.resolve_ungated();
        assert_eq!(e.blits(), &[b]);
        let pct = e.repair_percent();
        assert!(pct > 1.0 && pct < 10.0, "{pct}");
        // strip rows: 760..800 -> tile rows 23, 24; the seam where it is (rows 3, 4) and where it was carried (2, 3)
        let mut rows = std::collections::BTreeSet::new();
        e.mask().each_tile(&mut |_, ty| {
            rows.insert(ty);
        });
        assert_eq!(rows.into_iter().collect::<Vec<_>>(), vec![2, 3, 4, 23, 24]);
        // a jump of a whole port is a repaint, not a blit
        e.begin_frame();
        assert!(e.scroll(5, 9, clip, 0, 900).is_none());
        e.resolve_ungated();
        assert_eq!(e.mask().count(), 40 * 23);
        // both axes
        e.begin_frame();
        let b = e.scroll(5, 9, Rect::new(100, 100, 500, 500), 10, 20).unwrap();
        assert_eq!(b.kept(), Rect::new(110, 120, 500, 500));
        e.resolve_ungated();
        assert!(e.mask().test(3, 3) && e.mask().test(15, 3) && e.mask().test(3, 15) && !e.mask().test(8, 8));
    }

    #[test]
    fn a_scroll_under_glass_dirties_the_glass() {
        let mut e = engine();
        e.layer_push(BlurLayer { id: 9, z: 40, rect: Rect::new(0, 0, 640, 48), radius: 8, max_level: 3 });
        e.layers_commit();
        e.begin_frame();
        e.scroll(5, 9, Rect::new(0, 0, 640, 480), 0, -8);
        e.resolve_ungated();
        assert!(e.mask().test(0, 0) && e.mask().test(19, 1));
    }

    #[test]
    fn the_chooser_is_idle_sparse_on_the_cpu_and_broad_on_the_gpu_with_hysteresis_per_signature() {
        let mut e = DamageEngine::new(1920, 1088);
        e.begin_frame();
        e.resolve_ungated();
        assert_eq!(e.choice(), ExecutorChoice::Idle);
        // two rows of a hover
        e.begin_frame();
        e.damage(1, Rect::new(0, 100, 600, 124), Work::Flat);
        e.damage(1, Rect::new(0, 124, 600, 148), Work::Flat);
        e.resolve_ungated();
        assert_eq!(e.choice(), ExecutorChoice::Cpu);
        // a theme switch: everything
        e.begin_frame();
        e.damage(0, Rect::new(0, 0, 1920, 1088), Work::Flat);
        e.resolve_ungated();
        assert_eq!(e.choice(), ExecutorChoice::Gpu);
        // the hover after it is still the CPU's although the last flat frame went to the GPU: it is far under half
        e.begin_frame();
        e.damage(1, Rect::new(0, 100, 600, 124), Work::Flat);
        e.resolve_ungated();
        assert_eq!(e.choice(), ExecutorChoice::Cpu);
        // hysteresis: 400 tiles is 1600 + 1, under the threshold: CPU from below, GPU once it has been there
        let mid = Rect::new(0, 0, 20 * 32, 20 * 32);
        e.begin_frame();
        e.damage(1, mid, Work::Flat);
        e.resolve_ungated();
        assert_eq!(e.choice(), ExecutorChoice::Cpu);
        e.begin_frame();
        e.damage(0, Rect::new(0, 0, 1920, 1088), Work::Flat);
        e.resolve_ungated();
        e.begin_frame();
        e.damage(1, mid, Work::Flat);
        e.resolve_ungated();
        assert_eq!((e.choice(), e.reason()), (ExecutorChoice::Gpu, "broad work, held on the GPU (hysteresis)"));
        // a shadow frame has its own signature: the flat signature's history does not touch it
        e.begin_frame();
        e.damage(1, Rect::new(0, 0, 200, 200), Work::Shadow);
        e.resolve_ungated();
        assert_eq!(e.choice(), ExecutorChoice::Cpu);
        // blur blocks are dear: a small damage under a large deep glass goes to the GPU
        e.layer_push(BlurLayer { id: 1, z: 10, rect: Rect::new(0, 0, 1920, 1088), radius: 32, max_level: 5 });
        e.layers_commit();
        e.begin_frame();
        e.damage(1, Rect::new(0, 0, 1920, 1088), Work::Flat);
        e.resolve_ungated();
        assert_eq!(e.choice(), ExecutorChoice::Gpu);
    }

    #[test]
    fn a_report_names_the_tiles_the_skips_the_blocks_the_blits_and_the_verdict() {
        let mut e = engine();
        e.cover_push(OpaqueCover { z: 10, core_rect: Rect::new(0, 0, 64, 64) });
        e.covers_commit();
        e.layer_push(BlurLayer { id: 3, z: 20, rect: Rect::new(320, 0, 640, 100), radius: 4, max_level: 2 });
        e.layers_commit();
        e.begin_frame();
        e.damage(1, Rect::new(0, 0, 100, 40), Work::Gradient);
        e.damage(1, Rect::new(330, 10, 340, 20), Work::Flat);
        e.scroll(2, 4, Rect::new(0, 200, 640, 480), 0, 16);
        e.note_fanout(7);
        e.resolve_ungated();
        let r = e.report();
        assert_eq!(r.dirty_tiles as usize, r.tiles.len());
        assert_eq!(r.occluded_tiles_skipped, 4);
        assert!(r.tiles.contains(&(2, 0)) && !r.tiles.contains(&(0, 0)));
        assert_eq!(r.scroll_blits.len(), 1);
        assert!(r.blur_blocks.iter().any(|b| b.layer == 3 && b.level == 2));
        assert_eq!((r.work.fanout, r.covers, r.layers), (7, 1, 1));
        assert!(r.work.grad_tiles >= 4 && r.choice == ExecutorChoice::Cpu);
        assert!((r.repair_percent - r.dirty_tiles as f32 * 100.0 / r.total_tiles as f32).abs() < 1e-3);
        let j = r.to_json();
        assert!(j.starts_with('{') && j.ends_with('}') && j.contains("\"choice\":\"Cpu\"") && j.contains("\"scroll_blits\":[{\"clip\":[0,200,640,480],\"dx\":0,\"dy\":16}]"));
        // a second report into the same value reuses it
        let mut again = DamageReport::default();
        e.report_into(&mut again);
        assert_eq!(again, r);
    }

    #[test]
    fn damage_reported_before_a_renumbering_is_neither_culled_nor_kept_from_any_layer() {
        let mut e = engine();
        e.cover_push(OpaqueCover { z: 10, core_rect: Rect::new(64, 64, 320, 320) });
        e.covers_commit();
        e.layer_push(BlurLayer { id: 1, z: 4, rect: Rect::new(400, 100, 500, 200), radius: 4, max_level: 2 });
        e.layers_commit();
        e.begin_frame();
        // above the cover and above the layer, as the orders stood
        e.damage(12, Rect::new(100, 100, 120, 120), Work::Flat);
        e.damage(12, Rect::new(410, 110, 420, 120), Work::Flat);
        e.rebase_orders();
        e.resolve_ungated();
        assert!(e.mask().test(3, 3) && e.occlusion_skips() == 0);
        assert!(!e.blur_blocks().is_empty());
    }

    #[test]
    fn a_moved_record_is_damaged_where_it_was_and_where_it_is() {
        let mut e = engine();
        e.begin_frame();
        e.damage_move(1, Rect::new(0, 0, 20, 20), Rect::new(610, 450, 630, 470), Work::Flat);
        e.resolve_ungated();
        assert!(e.mask().test(0, 0) && e.mask().test(19, 14) && e.mask().count() == 2);
        e.begin_frame();
        e.damage_move(1, Rect::new(0, 0, 20, 20), Rect::new(10, 0, 30, 20), Work::Flat);
        e.resolve_ungated();
        assert_eq!(e.mask().count(), 1);
    }
}
