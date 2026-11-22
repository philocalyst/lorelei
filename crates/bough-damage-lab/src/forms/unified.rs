//! `unified`: Unified adaptive damage formulation combining:
//! 1. Exact per-slot symmetric-difference write-time recording (`exact_slot`).
//! 2. Rigid-scroll copy-forward (`Out::copies`) from the last presented surface,
//!    slashing scroll repair area from ~85-100% down to the newly exposed strip
//!    plus non-rigid/overlaid hazards — mathematically exact across swapchain
//!    ring depths 1..=4.
//! 3. Front-to-back opaque cover culling: walks slots top-to-bottom against a
//!    remaining-damage tile plane and extinguishes tiles strictly covered by
//!    opaque `SOLID` primitives, skipping occluded background draws entirely.
//! 4. Optional effect-radius dilation (`dilate_px`) and format-C hardware-tile
//!    run emission (`unified32c`).

use super::*;
use crate::trace::SOLID;

const K: usize = 8;

/// Static classification of a scroll group computed at `reset`.
#[derive(Clone, Debug)]
struct GroupInfo {
    /// First and past-the-end slot indices belonging to this group.
    min_slot: usize,
    /// Per-slot membership flag (length = `sc.cov.len()`).
    in_group: Vec<bool>,
}

/// Per-frame recorded damage slot in the ring buffer.
#[derive(Clone)]
struct FrameSlot {
    /// Standard exact damage for this frame (used for older frames in the age window
    /// or whenever copy-forward is skipped).
    full: Plane,
    /// Reduced damage for this frame when `copies` are applied from frame `n - 1`.
    scroll: Plane,
    /// Copy-forward blits `(dst, dx, dy)` from the last presented surface (`n - 1`).
    copies: Vec<(R, i32, i32)>,
}

impl FrameSlot {
    fn new(w: i32, h: i32, sh: u32) -> Self {
        Self {
            full: Plane::new(w, h, sh),
            scroll: Plane::new(w, h, sh),
            copies: Vec::new(),
        }
    }
}

pub struct Unified {
    pub sh: u32,
    pub runs: bool,
    pub dilate_px: i32,
    w: i32,
    h: i32,
    ring_slots: Vec<FrameSlot>,
    acc: Plane,
    rem: Plane,
    ep: usize,
    groups: Vec<GroupInfo>,
    /// Slots not inside any group (checked as overlay/non-uniform-bg hazards during scroll).
    non_group_slots: Vec<u32>,
    /// Reusable scratch buffer for front-to-back visible slots.
    scratch_list: Vec<u32>,
    /// Reusable scratch plane for format-C runs.
    hw_runs: Plane,
    /// Reusable scratch list of eligible scroll-blit candidates.
    scratch_cand: Vec<(usize, R, i32, i32)>,
}

impl Unified {
    pub fn new(sh: u32) -> Self {
        Self {
            sh,
            runs: false,
            dilate_px: 0,
            w: 1,
            h: 1,
            ring_slots: Vec::new(),
            acc: Plane::new(1, 1, sh),
            rem: Plane::new(1, 1, sh),
            ep: 0,
            groups: Vec::new(),
            non_group_slots: Vec::new(),
            scratch_list: Vec::new(),
            hw_runs: Plane::new(1, 1, 5),
            scratch_cand: Vec::new(),
        }
    }

    #[inline]
    fn mark_dilated(p: &mut Plane, r: R, dilate: i32, vp: R) {
        if r.is_empty() {
            return;
        }
        let expanded = if dilate > 0 {
            R::new(r.x0 - dilate, r.y0 - dilate, r.x1 + dilate, r.y1 + dilate).clip(vp)
        } else {
            r.clip(vp)
        };
        if !expanded.is_empty() {
            p.mark(expanded);
        }
    }


    /// Front-to-back opaque cover culling against `self.acc`, emitting surviving slots
    /// in exact back-to-front paint order into `out`.
    fn cull_front_to_back(&mut self, sc: &Scene, out: &mut Out) {
        let bb = self.acc.px_bbox();
        if bb.is_empty() {
            out.close(R::E);
            return;
        }

        // Copy `acc` into `rem`; we extinguish tiles strictly covered by opaque SOLID slots.
        self.rem.copy_from(&self.acc);
        self.scratch_list.clear();

        let tile = 1i32 << self.sh;
        let sh = self.sh;
        let cols = self.rem.cols;
        let rows = self.rem.rows;
        let mut q = N::from_r(bb);

        for pi in (0..sc.page.len()).rev() {
            if !sc.page[pi].meets(q) {
                continue;
            }
            let base = pi * 64;
            let page = &sc.cov[base..base + 64];
            let kinds = &sc.kind[base..base + 64];
            let mut cleared_any = false;
            for k in (0..64).rev() {
                let c = page[k];
                if !c.meets(q) {
                    continue;
                }
                let r = c.to_r();
                if !self.rem.test(r) {
                    continue;
                }
                let slot = (base + k) as u32;
                self.scratch_list.push(slot);

                if kinds[k] == SOLID {
                    // Strictly interior tiles completely covered by `r`:
                    // no slot painted earlier can ever contribute to any pixel in these tiles.
                    let tx0 = (r.x0 + tile - 1) >> sh;
                    let ty0 = (r.y0 + tile - 1) >> sh;
                    let tx1 = if r.x1 >= sc.w { cols } else { r.x1 >> sh };
                    let ty1 = if r.y1 >= sc.h { rows } else { r.y1 >> sh };
                    if tx0 < tx1 && ty0 < ty1 {
                        self.rem.clear_tiles(tx0, ty0, tx1, ty1);
                        cleared_any = true;
                    }
                }
            }
            if cleared_any {
                if self.rem.is_empty_scan() {
                    break;
                }
                q = N::from_r(self.rem.px_bbox());
            }
        }

        // Restore exact back-to-front paint order.
        if self.runs {
            // Format-C: emit (slot, damaged-hw-tile-run) pairs.
            self.hw_runs.clear();
            let hw_runs = &mut self.hw_runs;
            self.acc.emit(&mut |r| hw_runs.mark(r));
            for &slot in self.scratch_list.iter().rev() {
                out.list.push(slot);
                let c = sc.cov[slot as usize].to_r();
                let (tx0, ty0, tx1, ty1) = (
                    c.x0 >> 5,
                    c.y0 >> 5,
                    ((c.x1 - 1) >> 5) + 1,
                    ((c.y1 - 1) >> 5) + 1,
                );
                for ty in ty0..ty1 {
                    let mut tx = tx0;
                    while tx < tx1 {
                        if self.hw_runs.get(tx, ty) {
                            let a = tx;
                            while tx < tx1 && self.hw_runs.get(tx, ty) {
                                tx += 1;
                            }
                            let run_r = R::new(a << 5, ty << 5, tx << 5, (ty + 1) << 5).inter(c);
                            if !run_r.is_empty() {
                                out.runs.push((slot, run_r));
                            }
                        } else {
                            tx += 1;
                        }
                    }
                }
            }
        } else {
            out.list.extend(self.scratch_list.iter().rev().copied());
        }
        out.close(bb);
    }
}

impl Form for Unified {
    fn name(&self) -> String {
        format!("unified{}{}", 1 << self.sh, if self.runs { "c" } else { "" })
    }

    fn reset(&mut self, tr: &Trace, sc: &Scene, _ring: usize) {
        self.w = sc.w;
        self.h = sc.h;
        self.ring_slots = vec![FrameSlot::new(sc.w, sc.h, self.sh); K];
        self.acc = Plane::new(sc.w, sc.h, self.sh);
        self.rem = Plane::new(sc.w, sc.h, self.sh);
        self.hw_runs = Plane::new(sc.w, sc.h, 5);
        self.ep = 0;

        let nslots = sc.cov.len();
        let mut any_group = vec![false; nslots];
        self.groups.clear();
        for (gi, _gd) in tr.groups.iter().enumerate() {
            let mut in_group = vec![false; nslots];
            let mut min_slot = usize::MAX;
            for nd in &tr.nodes {
                if nd.group as usize == gi {
                    let s0 = nd.slot0 as usize;
                    let s1 = (nd.slot0 + nd.n) as usize;
                    for s in s0..s1.min(nslots) {
                        in_group[s] = true;
                        any_group[s] = true;
                        min_slot = min_slot.min(s);
                    }
                }
            }
            self.groups.push(GroupInfo {
                min_slot,
                in_group,
            });
        }
        self.non_group_slots.clear();
        for (s, &in_g) in any_group.iter().enumerate() {
            if !in_g {
                self.non_group_slots.push(s as u32);
            }
        }
    }

    fn record(&mut self, f: &Frame, sc: &Scene) {
        self.ep += 1;
        let slot_idx = self.ep % K;
        let vp = sc.viewport;
        let dilate = self.dilate_px;

        // First compute standard exact damage for the age-ring fallback.
        {
            let fs = &mut self.ring_slots[slot_idx];
            fs.full.clear();
            fs.scroll.clear();
            fs.copies.clear();
            exact_nodes_group_bounds(f, &mut |r| Self::mark_dilated(&mut fs.full, r, dilate, vp));
        }

        // Check if any scroll groups qualify for rigid copy-forward blitting.
        if f.groups.is_empty() {
            return;
        }

        // Collect eligible non-overlapping rigid scroll blits (zero per-frame heap allocation).
        self.scratch_cand.clear();
        for (gi, g) in f.groups.iter().enumerate() {
            if (g.dx == 0 && g.dy == 0) || g.clip.is_empty() {
                continue;
            }
            let clip = g.clip.clip(vp);
            if clip.is_empty() {
                continue;
            }
            let dst = clip.inter(clip.tr(g.dx, g.dy));
            if dst.is_empty() || dst.area() * 3 < clip.area() {
                continue;
            }
            if self.scratch_cand.iter().any(|&(other_i, _, _, _)| {
                f.groups[other_i].clip.clip(vp).meets(clip)
            }) {
                continue;
            }
            self.scratch_cand.push((gi, dst, g.dx, g.dy));
        }

        if self.scratch_cand.is_empty() {
            return;
        }

        let multi_group = self.groups.len() > 1;
        let fs = &mut self.ring_slots[slot_idx];
        let scroll_p = &mut fs.scroll;
        let copies_out = &mut fs.copies;

        // Copy-forward only replaces pixels inside `dst`; all node events still apply.
        for e in &f.nodes {
            for k in e.at as usize..(e.at + e.n) as usize {
                let o = &f.olds[k];
                let n = &f.news[k];
                exact_slot(o, n, &mut |r| Self::mark_dilated(scroll_p, r, dilate, vp));
                if !o.cov.is_empty() {
                    for &(_, dst, dx, dy) in &self.scratch_cand {
                        let moved = o.cov.tr(dx, dy).clip(dst);
                        if !moved.is_empty() {
                            Self::mark_dilated(scroll_p, moved, dilate, vp);
                        }
                    }
                }
            }
        }

        // Non-blitted groups fall back to exact events.
        for (gi, g) in f.groups.iter().enumerate() {
            if !self.scratch_cand.iter().any(|&(ci, _, _, _)| ci == gi) {
                for j in g.at as usize..(g.at + g.n) as usize {
                    exact_slot(&f.gold[j], &f.gnew[j], &mut |r| {
                        Self::mark_dilated(scroll_p, r, dilate, vp)
                    });
                }
            }
        }

        for &(gi, dst, dx, dy) in &self.scratch_cand {
            let g = &f.groups[gi];
            let clip = g.clip.clip(vp);

            // 1. Newly exposed strip `clip \ dst`.
            clip.sub(dst, &mut |r| Self::mark_dilated(scroll_p, r, dilate, vp));

            // 2. Non-rigid slots inside the group.
            for j in g.at as usize..(g.at + g.n) as usize {
                let o = &f.gold[j];
                let n = &f.gnew[j];
                let rigid = o.kind == n.kind
                    && o.key == n.key
                    && o.rad == n.rad
                    && n.ink == o.ink.tr(dx, dy)
                    && o.cov.tr(dx, dy).clip(dst) == n.cov.clip(dst);
                if !rigid {
                    if !o.cov.is_empty() {
                        let moved = o.cov.tr(dx, dy).clip(dst);
                        if !moved.is_empty() {
                            Self::mark_dilated(scroll_p, moved, dilate, vp);
                        }
                    }
                    if !n.cov.is_empty() {
                        Self::mark_dilated(scroll_p, n.cov, dilate, vp);
                    }
                }
            }

            // 3. Static hazards outside the group overlapping `clip`.
            // Fast path when there is at most one group: scan ONLY `non_group_slots` instead of all
            // 26,000+ group slots!
            let ginfo = if g.n > 0 {
                let sample = f.gslots[g.at as usize] as usize;
                self.groups.iter().find(|info| sample < info.in_group.len() && info.in_group[sample])
            } else {
                None
            };
            let min_s = ginfo.map(|info| info.min_slot).unwrap_or(0);
            let clip_n = N::from_r(clip);
            let mut check_slot = |s: usize| {
                let c = sc.cov[s];
                if !c.meets(clip_n) {
                    return;
                }
                let r = c.to_r();
                if s < min_s && r.contains(clip) {
                    if sc.kind[s] == SOLID {
                        return;
                    }
                    if sc.kind[s] == ROUND {
                        let rad = 32i32.min(r.x1 - r.x0).min(r.y1 - r.y0);
                        for c_rect in [
                            R::new(r.x0, r.y0, r.x0 + rad, r.y0 + rad),
                            R::new(r.x1 - rad, r.y0, r.x1, r.y0 + rad),
                            R::new(r.x0, r.y1 - rad, r.x0 + rad, r.y1),
                            R::new(r.x1 - rad, r.y1 - rad, r.x1, r.y1),
                        ] {
                            let in_clip = c_rect.clip(clip);
                            if !in_clip.is_empty() {
                                Self::mark_dilated(scroll_p, in_clip, dilate, vp);
                            }
                            let moved = c_rect.tr(dx, dy).clip(dst);
                            if !moved.is_empty() {
                                Self::mark_dilated(scroll_p, moved, dilate, vp);
                            }
                        }
                        return;
                    }
                }
                let in_clip = r.clip(clip);
                if !in_clip.is_empty() {
                    Self::mark_dilated(scroll_p, in_clip, dilate, vp);
                }
                let moved = r.tr(dx, dy).clip(dst);
                if !moved.is_empty() {
                    Self::mark_dilated(scroll_p, moved, dilate, vp);
                }
            };

            if !multi_group {
                for &su in &self.non_group_slots {
                    check_slot(su as usize);
                }
            } else {
                for (pi, pg) in sc.page.iter().enumerate() {
                    if !pg.meets(clip_n) {
                        continue;
                    }
                    let base = pi * 64;
                    for k in 0..64 {
                        let s = base + k;
                        if let Some(info) = ginfo {
                            if s < info.in_group.len() && info.in_group[s] {
                                continue;
                            }
                        }
                        check_slot(s);
                    }
                }
            }

            copies_out.push((dst, dx, dy));
        }
    }

    fn repair(&mut self, _surf: usize, age: u32, sc: &Scene, out: &mut Out) {
        self.acc.clear();
        let cur_idx = self.ep % K;
        let use_copy = !self.ring_slots[cur_idx].copies.is_empty();

        if use_copy {
            // 1. Accumulate older frames (1..age) for everything outside the copy destinations.
            for k in 1..age as usize {
                let idx = self.ep.wrapping_sub(k) % K;
                self.acc.or_from(&self.ring_slots[idx].full);
            }
            // 2. Copy-forward overwrites `dst` with exact frame `n - 1` pixels shifted by `(dx, dy)`.
            //    Therefore older-frame damage on tiles STRICTLY INSIDE `dst` is completely superseded!
            let tile = 1i32 << self.sh;
            let sh = self.sh;
            let cols = self.acc.cols;
            let rows = self.acc.rows;
            for &(dst, dx, dy) in &self.ring_slots[cur_idx].copies {
                out.copies.push((dst, dx, dy));
                let tx0 = (dst.x0 + tile - 1) >> sh;
                let ty0 = (dst.y0 + tile - 1) >> sh;
                let tx1 = if dst.x1 >= self.w { cols } else { dst.x1 >> sh };
                let ty1 = if dst.y1 >= self.h { rows } else { dst.y1 >> sh };
                if tx0 < tx1 && ty0 < ty1 {
                    self.acc.clear_tiles(tx0, ty0, tx1, ty1);
                }
            }
            self.acc.recompute_bb();
            // 3. OR in frame `n`'s reduced scroll damage (exposed strips + non-rigid + hazards + nodes).
            self.acc.or_from(&self.ring_slots[cur_idx].scroll);
        } else {
            for k in 0..age as usize {
                let idx = self.ep.wrapping_sub(k) % K;
                self.acc.or_from(&self.ring_slots[idx].full);
            }
        }

        self.cull_front_to_back(sc, out);
    }

    fn region(&self, emit: &mut dyn FnMut(R)) {
        self.acc.emit(emit);
    }

    fn mem_bytes(&self) -> usize {
        self.ring_slots
            .iter()
            .map(|s| s.full.bytes() + s.scroll.bytes())
            .sum::<usize>()
            + self.acc.bytes()
            + self.rem.bytes()
    }
}
