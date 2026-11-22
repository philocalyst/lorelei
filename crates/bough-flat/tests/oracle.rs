//! The flat core's pixel gates: after every operation the retained surface equals a full redraw of the stores
//! and (animations at rest) the same state mounted from nothing.
mod common;
use bough_flat::store::ROOT;
use bough_flat::*;
use common::*;

fn ok(s: &mut Scene, what: &str) -> FrameInfo {
    match s.w.check(&mut NoExt) {
        Ok(i) => i,
        Err(e) => panic!("after {what}: {e}"),
    }
}

#[test]
fn a_mount_paints_something_and_holds() {
    let mut s = Scene::new(20);
    let i = ok(&mut s, "mount");
    assert_eq!(i.dirty_tiles, i.total_tiles);
    assert_eq!(i.choice, ExecutorChoice::Gpu, "a whole window is broad work");
    // a second frame with nothing changed is idle
    let i = ok(&mut s, "nothing");
    assert!(i.idle && i.choice == ExecutorChoice::Idle);
    // the scene is not blank and not one colour
    let px = s.w.pixels();
    let mut distinct = std::collections::BTreeSet::new();
    for p in px.iter().step_by(97) {
        distinct.insert(*p);
    }
    assert!(distinct.len() > 12, "{} colours", distinct.len());
}

#[test]
fn every_operation_of_a_random_walk_holds_the_oracles() {
    for (seed0, rows, steps) in [(1u64, 18u32, 260u32), (7, 3, 160), (99, 60, 200)] {
        let mut s = Scene::new(rows);
        ok(&mut s, "mount");
        let mut seed = seed0;
        let mut seen = std::collections::BTreeMap::new();
        for k in 0..steps {
            let what = s.step(&mut seed);
            *seen.entry(what).or_insert(0u32) += 1;
            if let Err(e) = s.w.check(&mut NoExt) {
                panic!("seed {seed0}, step {k} ({what}): {e}");
            }
        }
        assert!(seen.len() >= 12, "the walk reached {seen:?}");
        // let the animations come to rest and compare against a fresh mount once more
        let t = s.w.get(s.w.time_slot());
        s.w.set_time(t + 10_000);
        ok(&mut s, "rest");
        assert!(!s.w.anims_active());
        ok(&mut s, "rest again");
    }
}

#[test]
fn several_turns_before_one_frame_hold() {
    let mut s = Scene::new(30);
    ok(&mut s, "mount");
    let mut seed = 5u64;
    for k in 0..60 {
        let mut what = Vec::new();
        for _ in 0..1 + lcg(&mut seed) % 4 {
            what.push(s.step(&mut seed));
            s.w.settle();
        }
        if let Err(e) = s.w.check(&mut NoExt) {
            panic!("batch {k} {what:?}: {e}");
        }
    }
}

#[test]
fn a_hover_touches_two_rows_and_repaints_their_tiles_only() {
    let mut s = Scene::new(40);
    ok(&mut s, "mount");
    let (x, y) = s.row_center(2);
    s.w.pointer_move(40, y);
    let _ = x;
    let before = s.w.stats;
    let i = ok(&mut s, "hover row 2");
    assert_eq!(s.w.stats.records_touched - before.records_touched, 1);
    assert!(i.dirty_tiles >= 10 && i.dirty_tiles <= 22, "{}", i.dirty_tiles);
    assert_eq!(i.choice, ExecutorChoice::Cpu);
    let (_, y3) = s.row_center(3);
    s.w.pointer_move(40, y3);
    let before = s.w.stats;
    let i = ok(&mut s, "hover row 3");
    assert_eq!(s.w.stats.records_touched - before.records_touched, 2, "the row left and the row entered");
    assert!(i.dirty_tiles <= 33 && i.repair_percent < 12.0);
    s.w.pointer_leave();
    let before = s.w.stats;
    ok(&mut s, "leave");
    assert_eq!(s.w.stats.records_touched - before.records_touched, 1);
}

#[test]
fn a_turns_work_does_not_depend_on_how_many_records_exist() {
    let mut per = Vec::new();
    for rows in [10u32, 200, 4000] {
        // only one row reads `one`: a state word with one reader
        let mut s = Scene::new(rows);
        let one = s.w.slot(7u32);
        let extra = s.w.add_record(RecordDesc::rect(ROOT, Rect::new(600, 440, 630, 470), one));
        let _ = extra;
        ok(&mut s, "mount");
        let b = s.w.stats;
        s.w.set(one, 0xff12_3456);
        s.w.settle();
        let a = s.w.stats;
        let turn = (a.binds_evaluated - b.binds_evaluated, a.records_touched - b.records_touched);
        // hover: the keyed rows of two sites
        let (_, y) = s.row_center(1);
        s.w.pointer_move(40, y);
        s.w.settle();
        let (_, y) = s.row_center(2);
        s.w.pointer_move(40, y);
        let b = s.w.stats;
        s.w.settle();
        let a = s.w.stats;
        let hover = (a.binds_evaluated - b.binds_evaluated, a.records_touched - b.records_touched);
        // theme: no binding of a palette fill exists at all
        s.w.pointer_leave();
        ok(&mut s, "leave");
        let b = s.w.stats;
        s.w.set_theme(1);
        s.w.settle();
        let a = s.w.stats;
        let theme_touched = a.records_touched - b.records_touched;
        let i = ok(&mut s, "theme");
        assert!(i.repair_percent > 90.0);
        per.push((turn, hover, theme_touched));
    }
    assert_eq!(per[0].0, (1, 1), "one word, one reader: one binding evaluated, one record touched");
    assert_eq!(per[0].0, per[1].0);
    assert_eq!(per[1].0, per[2].0);
    assert_eq!(per[0].1, per[2].1, "a hover is the rows of two sites whatever the list's length");
    assert_eq!(per[0].1 .1, 2);
    // a theme switch re-evaluates only the rows' own expressions (they look the palette up themselves): the
    // palette-filled records (window, panel, every label) are not visited at all
    assert!(per[0].2 <= 10 + 1 && per[2].2 <= 4000 + 1);
    let mut s = Scene::new(0);
    ok(&mut s, "mount");
    let b = s.w.stats;
    s.w.set_theme(1);
    let i = ok(&mut s, "theme, no rows");
    assert_eq!((s.w.stats.binds_evaluated - b.binds_evaluated, s.w.stats.records_touched - b.records_touched), (0, 0));
    assert!(i.repair_percent > 90.0);
}

#[test]
fn a_scroll_is_a_blit_a_strip_and_seams() {
    // no glass, no header, no cover: a plain list in a panel on a window
    let mut w = FlatWorld::new(800, 608);
    w.add_record(RecordDesc::rect(ROOT, Rect::new(0, 0, 800, 608), 0xfff0_f0f0u32));
    let port = w.add_record(RecordDesc::rect(ROOT, Rect::new(0, 32, 800, 608), 0xffff_ffffu32));
    let sy = w.slot(0i32);
    let list = w.add_space(ROOT, Some(port), true, 0, sy);
    for k in 0..200 {
        w.add_record(RecordDesc::rect(list, Rect::new(8, k * 24 + 2, 792, k * 24 + 22), if k % 2 == 0 { 0xffe0_e8f0u32 } else { 0xfff0_e8e0 }));
        w.add_record(RecordDesc::label(list, Rect::new(16, k * 24 + 8, 400, k * 24 + 16), TextSrc::Static(format!("ITEM {k}")), 1, 0xff10_1010u32));
    }
    // a badge fixed over the list
    w.add_record(RecordDesc::rect(ROOT, Rect::new(700, 500, 780, 540), 0xc0ff_4040u32).radius_px(8));
    w.check(&mut NoExt).unwrap();
    for (k, d) in [8, 8, 24, 3, -17, 120, -60, 1000, -2000, 7].into_iter().enumerate() {
        let at = w.get(sy) + d;
        w.set(sy, at);
        let i = w.check(&mut NoExt).unwrap_or_else(|e| panic!("scroll {k} by {d}: {e}"));
        if d.abs() < 576 {
            assert_eq!(i.blits, 1, "scroll {k} by {d}");
            if d.abs() <= 24 {
                assert!(i.repair_percent < 9.0, "scroll by {d} repaired {}%", i.repair_percent);
            }
        } else {
            assert_eq!(i.blits, 0);
        }
    }
    assert!(w.stats.blits >= 8);
}

#[test]
fn damage_under_an_opaque_cover_is_skipped_and_counted_and_gated_at_the_glass() {
    let mut s = Scene::new(12);
    ok(&mut s, "mount");
    // a box buried under the list's opaque panel, itself under the glass: its tile is skipped, the glass's
    // backdrop is repainted to the same pixels, and the propagation halts there
    let before = s.w.stats;
    s.w.set(s.buried, 0xffff_00ffu32);
    let i = ok(&mut s, "buried colour");
    assert_eq!(s.w.stats.records_touched - before.records_touched, 1);
    assert!(i.occluded_tiles_skipped >= 1, "{i:?}");
    let r = s.w.report();
    assert!(r.blur_gated >= 1 && r.blur_blocks.is_empty(), "the glass's backdrop came out the same: {r:?}");
    assert_eq!(i.dirty_tiles, 0, "nothing visible changed: {r:?}");
    assert_eq!(i.choice, ExecutorChoice::Idle);
    // a box under a cover that is above the glass: its own tiles are skipped, but the glass beside it reads it
    s.w.set(s.hidden, 0xff00_ff00u32);
    let i = ok(&mut s, "hidden colour");
    assert!(i.occluded_tiles_skipped >= 1 && i.dirty_tiles >= 1, "{i:?}");
    assert!(!s.w.report().blur_blocks.is_empty());
    // with the cover translucent the same change is seen directly
    s.w.set(s.cover_on, 128u32);
    ok(&mut s, "cover translucent");
    s.w.set(s.hidden, 0xffff_0000u32);
    let i = ok(&mut s, "hidden colour, seen");
    assert!(i.dirty_tiles >= 2 && i.occluded_tiles_skipped == 0);
}

#[test]
fn clip_overflow_is_a_state_word() {
    let mut s = Scene::new(30);
    ok(&mut s, "mount");
    s.w.set(s.scroll, 37);
    ok(&mut s, "scroll");
    let clipped = s.w.pixels().to_vec();
    s.w.set(s.clip, false);
    let i = ok(&mut s, "clip off");
    assert!(i.dirty_tiles > 0);
    assert_ne!(s.w.pixels(), &clipped[..], "rows past the port show");
    s.w.set(s.clip, true);
    ok(&mut s, "clip on");
    assert_eq!(s.w.pixels(), &clipped[..]);
}

#[test]
fn rows_inserted_and_removed_equal_a_scene_built_that_way() {
    let mut s = Scene::new(8);
    ok(&mut s, "mount");
    s.push_row();
    s.push_row();
    ok(&mut s, "two rows more");
    let mut built = Scene::new(10);
    ok(&mut built, "mount of ten");
    assert_eq!(s.w.pixels(), built.w.pixels(), "eight rows and two more are ten rows");
    assert_eq!(s.w.records(), built.w.records());
    // removing the last row is a scene of nine
    s.remove_row(9);
    ok(&mut s, "one row fewer");
    let mut nine = Scene::new(9);
    ok(&mut nine, "mount of nine");
    assert_eq!(s.w.records(), nine.w.records());
    assert_eq!(s.w.pixels().len(), nine.w.pixels().len());
    // a removed record's handle is refused
    let gone = built.rows[3].0;
    assert!(built.w.remove(gone));
    assert!(!built.w.remove(gone));
    assert!(built.w.screen_rect(gone).is_none());
    ok(&mut built, "a row's box removed");
}

#[test]
fn an_animation_runs_to_its_goal_and_then_costs_nothing() {
    let mut s = Scene::new(4);
    ok(&mut s, "mount");
    s.w.pointer_move(500, 50);
    ok(&mut s, "over the header");
    assert!(s.w.anims_active());
    let mut frames = 0;
    for step in 0..12 {
        let t = s.w.get(s.w.time_slot());
        s.w.set_time(t + 16);
        let i = ok(&mut s, "tick");
        frames += !i.idle as u32;
        if !s.w.anims_active() {
            assert!(step >= 6, "120 ms at 16 ms a tick");
            break;
        }
    }
    assert!(frames >= 6 && !s.w.anims_active());
    let b = s.w.stats;
    let t = s.w.get(s.w.time_slot());
    s.w.set_time(t + 16);
    let i = ok(&mut s, "tick at rest");
    assert!(i.idle);
    assert_eq!(s.w.stats.binds_evaluated, b.binds_evaluated, "a clock tick with nothing in flight evaluates nothing");
}

#[test]
fn a_report_is_ready_for_a_tool() {
    let mut s = Scene::new(20);
    ok(&mut s, "mount");
    s.w.set(s.scroll, 12);
    ok(&mut s, "scroll");
    let r = s.w.report();
    assert_eq!(r.scroll_blits.len(), 1);
    assert!(r.work.fanout >= 1 && r.layers == 2 && r.covers >= 2);
    assert!(!r.blur_blocks.is_empty(), "glass over a scrolled list");
    let j = r.to_json();
    assert!(j.contains("\"scroll_blits\":[{") && j.contains("\"blur_blocks\":[{"));
}

#[test]
fn a_window_resize_repaints_and_holds() {
    let mut s = Scene::new(20);
    ok(&mut s, "mount");
    s.w.resize(700, 420);
    let i = ok(&mut s, "resize");
    assert_eq!(i.dirty_tiles, i.total_tiles);
    s.w.set(s.count, 5);
    ok(&mut s, "count after resize");
}

/// Scrollers inside a scroller, a port that moves, two ports scrolled in one turn, glass that scrolls with its
/// content, sticky overlays: every combination of a random walk holds.
#[test]
fn nested_and_simultaneous_scrolls_hold() {
    let (w, h) = (560, 420);
    for seed0 in [3u64, 11, 42] {
        let mut world = FlatWorld::new(w, h);
        let wd = &mut world;
        wd.add_record(RecordDesc::rect(ROOT, Rect::new(0, 0, w, h), 0xfff4_f0ecu32));
        let (osy, port_x, oclip) = (wd.slot(0i32), wd.slot(20i32), wd.slot(true));
        let mut pd = RecordDesc::rect(ROOT, Rect::new(0, 30, 300, 380), 0xffff_ffffu32);
        pd.x = port_x.into();
        let port = wd.add_record(pd);
        let outer = wd.add_space(ROOT, Some(port), oclip, 0, osy);
        let mut inner = Vec::new();
        for k in 0..12 {
            let y = k * 70;
            // a strip that scrolls sideways inside the list that scrolls down
            let strip = wd.add_record(RecordDesc::rect(outer, Rect::new(10, y + 6, 290, y + 60), if k % 2 == 0 { 0xffe8_eef8u32 } else { 0xfff8_eee8 }).radius_px(4));
            let (isx, iclip) = (wd.slot(0i32), wd.slot(true));
            let sp = wd.add_space(outer, Some(strip), iclip, isx, 0);
            for c in 0..14 {
                let mut d = RecordDesc::rect(sp, Rect::new(c * 44 + 4, 6, c * 44 + 40, 46), 0xff30_6090u32 + (c as u32 * 0x0a_0503)).radius_px(5);
                if c % 4 == 1 {
                    d = d.effect(Effect::new().shadow(0, 0x6000_0000u32, 1, 2, 4, 0));
                }
                if c == 6 && k % 3 == 0 {
                    // glass that scrolls with its strip
                    d = RecordDesc::rect(sp, Rect::new(c * 44 - 10, 2, c * 44 + 50, 50), 0x30ff_ffffu32).effect(Effect::new().backdrop_blur(4));
                }
                wd.add_record(d);
            }
            inner.push((isx, iclip));
        }
        // a second, separate port beside the first, and one overlapping it
        let (bsy, csy) = (wd.slot(0i32), wd.slot(0i32));
        let bport = wd.add_record(RecordDesc::rect(ROOT, Rect::new(360, 30, 540, 200), 0xffff_fff0u32));
        let b = wd.add_space(ROOT, Some(bport), true, 0, bsy);
        let cport = wd.add_record(RecordDesc::rect(ROOT, Rect::new(250, 220, 540, 400), 0xfff0_ffffu32));
        let c = wd.add_space(ROOT, Some(cport), true, 0, csy);
        for k in 0..30 {
            wd.add_record(RecordDesc::rect(b, Rect::new(6, k * 20 + 2, 174, k * 20 + 18), 0xff80_a060u32 + k as u32 * 0x03_0102));
            wd.add_record(RecordDesc::rect(c, Rect::new(6, k * 26 + 3, 284, k * 26 + 23), 0xc0a0_6080u32 + k as u32 * 0x02_0301).radius_px(3));
        }
        // fixed things over all of it: a translucent bar and an opaque badge
        wd.add_record(RecordDesc::rect(ROOT, Rect::new(0, 180, w, 204), 0x80ff_ffffu32));
        wd.add_record(RecordDesc::rect(ROOT, Rect::new(100, 300, 200, 364), 0xff20_2020u32));
        world.check(&mut NoExt).unwrap_or_else(|e| panic!("mount: {e}"));
        let mut seed = seed0;
        let mut blits = 0;
        for step in 0..220 {
            let mut what = Vec::new();
            for _ in 0..1 + lcg(&mut seed) % 2 {
                let d = (lcg(&mut seed) % 61) as i32 - 30;
                match lcg(&mut seed) % 9 {
                    0 | 1 => {
                        let at = (world.get(osy) + d).clamp(-10, 560);
                        world.set(osy, at);
                        what.push("outer");
                    }
                    2 | 3 => {
                        let (isx, _) = inner[(lcg(&mut seed) % 12) as usize];
                        let at = (world.get(isx) + d).clamp(-10, 400);
                        world.set(isx, at);
                        what.push("inner");
                    }
                    4 => {
                        let at = (world.get(bsy) + d).clamp(0, 500);
                        world.set(bsy, at);
                        what.push("second");
                    }
                    5 => {
                        let at = (world.get(csy) + d).clamp(0, 640);
                        world.set(csy, at);
                        what.push("overlapping");
                    }
                    6 => {
                        let (_, iclip) = inner[(lcg(&mut seed) % 12) as usize];
                        let on = world.get(iclip);
                        world.set(iclip, !on);
                        what.push("inner clip");
                    }
                    7 => {
                        let on = world.get(oclip);
                        world.set(oclip, !on);
                        what.push("outer clip");
                    }
                    _ => {
                        world.set(port_x, 10 + (lcg(&mut seed) % 40) as i32);
                        what.push("port moves");
                    }
                }
            }
            match world.check(&mut NoExt) {
                Ok(i) => blits += i.blits,
                Err(e) => panic!("seed {seed0} step {step} {what:?}: {e}"),
            }
        }
        assert!(blits >= 40, "seed {seed0}: only {blits} rigid scrolls in the walk");
    }
}

/// A scroll in the same turn as rows entering and leaving the scrolled space (what a virtual list does), and
/// as patches of described constants: still a blit, and still exact.
#[test]
fn a_scroll_with_rows_entering_leaving_and_patched_in_the_same_turn_holds() {
    let mut w = FlatWorld::new(480, 384);
    w.add_record(RecordDesc::rect(ROOT, Rect::new(0, 0, 480, 384), 0xffe8_e8e8u32));
    let port = w.add_record(RecordDesc::rect(ROOT, Rect::new(16, 32, 400, 352), 0xffff_ffffu32));
    let sy = w.slot(0i32);
    let list = w.add_space(ROOT, Some(port), true, 0, sy);
    let row = |k: i32| RecordDesc::rect(list, Rect::new(4, k * 32 + 2, 380, k * 32 + 30), 0xff40_6080u32 + (k as u32 % 7) * 0x10_1808).radius_px(3);
    let mut rows: std::collections::VecDeque<(i32, RecId)> = (0..12).map(|k| (k, w.add_record(row(k)))).collect();
    // overlays: one fixed, one that comes and goes
    w.add_record(RecordDesc::rect(ROOT, Rect::new(300, 100, 460, 140), 0xa0ff_d040u32));
    let mut badge = None;
    w.check(&mut NoExt).unwrap();
    let (mut seed, mut blits) = (9u64, 0);
    for step in 0..120 {
        let d = (lcg(&mut seed) % 50) as i32 - 12;
        let at = (w.get(sy) + d).max(0);
        w.set(sy, at);
        // rows that left go, rows that entered come (materialised to one port's height around the offset)
        let (first, last) = (at / 32 - 1, (at + 320) / 32 + 1);
        while rows.front().is_some_and(|r| r.0 < first) {
            assert!(w.remove(rows.pop_front().unwrap().1));
        }
        while rows.back().is_some_and(|r| r.0 > last) {
            assert!(w.remove(rows.pop_back().unwrap().1));
        }
        while rows.back().is_none_or(|r| r.0 < last) {
            let k = rows.back().map_or(first, |r| r.0 + 1);
            rows.push_back((k, w.add_record(row(k))));
        }
        while rows.front().is_some_and(|r| r.0 > first.max(0)) {
            let (k, anchor) = *rows.front().unwrap();
            rows.push_front((k - 1, w.insert_before(anchor, row(k - 1))));
        }
        // a patch inside the scrolled space and one outside it
        if step % 3 == 0 {
            let (k, id) = rows[rows.len() / 2];
            w.patch(id, Prop::Fill, 0xff00_0000 | lcg(&mut seed));
            w.patch_rect(id, Rect::new(4 + (step % 9), k * 32 + 2, 380, k * 32 + 30));
        }
        if step % 7 == 3 {
            match badge.take() {
                Some(b) => assert!(w.remove(b)),
                None => badge = Some(w.add_record(RecordDesc::rect(ROOT, Rect::new(40, 200, 120, 240), 0xc020_c040u32))),
            }
        }
        match w.check(&mut NoExt) {
            Ok(i) => blits += i.blits,
            Err(e) => panic!("step {step} (scroll by {d} to {at}): {e}"),
        }
    }
    assert!(blits >= 80, "{blits} of 120 scrolls were blits");
}
