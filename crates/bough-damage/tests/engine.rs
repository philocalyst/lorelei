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

/// What the test told the engine this frame, for the exact prediction.
#[derive(Default)]
struct Told {
    damage: Vec<(u32, Rect, bool)>,
    masks: BTreeSet<(u32, u32)>,
}
impl Told {
    fn rebase(&mut self) {
        for d in &mut self.damage {
            d.0 = 0;
            d.2 = true;
        }
    }
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
            if let Some(b) = e.scroll(zlo, zhi, sc.clip, dx, dy) {
                assert_eq!(b.clip_rect, sc.clip.and(sc.window()));
                // everything that shows inside the clip without moving with it: below (but the opaque background,
                // a uniform fill whose shift is itself), the glass and everything above
                for k in 1..sc.items.len() {
                    if !sc.items[k].group && start.win(k).hits(b.clip_rect) {
                        e.seam(&b, k as u32, start.win(k));
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
                    e.damage(k as u32, win, Work::Flat);
                    told.damage.push((k as u32, win, false));
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
                    e.damage(k as u32, old, Work::Flat);
                    e.damage(k as u32, new, Work::Flat);
                    told.damage.push((k as u32, old, false));
                    told.damage.push((k as u32, new, false));
                } else {
                    e.damage_move(k as u32, old, new, Work::Flat);
                }
            }
            // a layer goes deeper or shallower
            5 => {
                let k = rnd(s, n as u32) as usize;
                if let Kind::Blur { .. } = sc.items[k].kind {
                    sc.items[k].kind = Kind::Blur { levels: rnd(s, 5) as u8 };
                    e.damage(k as u32, sc.win(k), Work::Flat);
                }
            }
            // a theme: some rects recoloured, told as a mask of the tiles they touch
            6 => {
                let mut m = TileMask::new(w, h);
                for k in 0..n {
                    if matches!(sc.items[k].kind, Kind::Solid { .. }) && rnd(s, 2) == 0 {
                        let Kind::Solid { alpha, .. } = sc.items[k].kind else { unreachable!() };
                        sc.items[k].kind = Kind::Solid { color: rnd(s, 1 << 20) as i64, alpha };
                        m.mark_rect(sc.win(k));
                        told.masks.extend(tiles_of(sc.win(k), w, h));
                    }
                }
                e.damage_mask(&m);
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
                e.damage(at as u32, win, Work::Flat);
                told.damage.push((at as u32, win, false));
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
                e.damage(k as u32 - 1, win, Work::Flat);
                told.damage.push((k as u32 - 1, win, false));
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
        let mut told = Told::default();
        let scrolled = change(&mut e, &mut sc, &mut s, &mut told, mode);
        sync(&mut e, &sc);
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
        }
    }
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

#[test]
#[should_panic(expected = "one resolve per frame")]
fn a_frame_resolves_once() {
    let mut e = DamageEngine::new(64, 64);
    e.begin_frame();
    e.resolve_ungated();
    e.resolve_ungated();
}
