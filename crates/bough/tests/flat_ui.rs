//! The non-generic builder (`bough::flat::ui`) held to the flat core's oracles, and to its cost bands: a
//! turn costs the bindings that read what changed.
#[path = "../examples/flat_slice.rs"]
mod flat_slice;
use bough::layout::Styled;
use bough::flat::{Effect, ExecutorChoice};
use bough::flat::ui::*;
use flat_slice::FlatSlice;

#[test]
fn the_flat_slice_holds_its_oracles_through_every_kind_of_turn() {
    let mut s = FlatSlice::new(200, 640, 480, 1.0);
    let i = s.ui.check().unwrap();
    assert_eq!(i.dirty_tiles, i.total_tiles);
    assert!(s.ui.world.records() > 800);
    // hover two rows
    s.ui.pointer_move(300, 200);
    let i = s.ui.check().unwrap();
    assert!(i.dirty_tiles > 0 && i.dirty_tiles <= 42 && i.choice == ExecutorChoice::Cpu, "{i:?}");
    s.ui.pointer_move(300, 228);
    s.ui.check().unwrap();
    // the title and the counter
    let t = s.title;
    s.ui.world.state.set_str(t, "Slice 7");
    let i = s.ui.check().unwrap();
    assert!(i.dirty_tiles > 0 && i.dirty_tiles < 30);
    // a button: its action writes the counter
    let before = s.ui.world.get(s.count);
    let mut hit = None;
    for x in (300..640).step_by(4) {
        if s.ui.click(x, 24) {
            hit = Some(x);
            break;
        }
    }
    assert!(hit.is_some(), "a button in the header");
    assert_eq!(s.ui.world.get(s.count), before + 1);
    s.ui.check().unwrap();
    // scroll: a blit
    for k in 1..8 {
        s.ui.world.set(s.scroll, k * 28);
        let i = s.ui.check().unwrap_or_else(|e| panic!("scroll {k}: {e}"));
        assert_eq!(i.blits, 1);
        assert!(i.repair_percent < 30.0, "{}", i.repair_percent);
    }
    // theme: every themed tile, no record visited
    let b = s.ui.world.stats;
    s.ui.world.set_theme(1);
    let i = s.ui.check().unwrap();
    assert!(i.repair_percent > 95.0, "{i:?}");
    assert_eq!(s.ui.world.stats.records_touched - b.records_touched, 0);
    s.ui.pointer_leave();
    s.ui.check().unwrap();
}

#[test]
fn a_turn_on_the_flat_slice_costs_the_same_at_any_length() {
    let mut bands = Vec::new();
    for n in [50usize, 5000] {
        let mut s = FlatSlice::new(n, 640, 480, 1.0);
        s.ui.frame();
        s.ui.pointer_move(300, 200);
        s.ui.frame();
        let b = s.ui.world.stats;
        s.ui.pointer_move(300, 228);
        s.ui.frame();
        let hover = (s.ui.world.stats.binds_evaluated - b.binds_evaluated, s.ui.world.stats.records_touched - b.records_touched);
        let b = s.ui.world.stats;
        let c = s.ui.world.get(s.count);
        s.ui.world.set(s.count, c + 1);
        s.ui.frame();
        let signal = (s.ui.world.stats.binds_evaluated - b.binds_evaluated, s.ui.world.stats.records_touched - b.records_touched);
        let b = s.ui.world.stats;
        s.ui.world.set(s.scroll, 56);
        let i = s.ui.frame();
        let scroll = (s.ui.world.stats.binds_evaluated - b.binds_evaluated, s.ui.world.stats.records_touched - b.records_touched, i.blits);
        let b = s.ui.world.stats;
        s.ui.world.set_theme(1);
        s.ui.frame();
        let theme = (s.ui.world.stats.binds_evaluated - b.binds_evaluated, s.ui.world.stats.records_touched - b.records_touched);
        bands.push((hover, signal, scroll, theme));
    }
    assert_eq!(bands[0], bands[1]);
    let (hover, signal, scroll, theme) = bands[0];
    assert_eq!(hover.1, 2, "the row left and the row entered");
    assert_eq!(signal, (1, 1), "one word, one reader");
    assert_eq!(scroll, (1, 0, 1), "one space binding, no record touched, one blit");
    assert_eq!(theme, (0, 0), "a theme switch evaluates no binding and touches no record");
}

#[test]
fn effects_clip_and_transitions_through_the_builder() {
    let mut ui = FlatUi::new(400, 300, 1.0);
    let (clip, lift, sy) = (ui.world.slot(true), ui.world.slot(4u32), ui.world.slot(0i32));
    let pal = ui.world.pal(0);
    ui.world.set_palette(1, &[0xfff0_f0f0, 0xff20_2020]);
    ui.mount(
        column()
            .w_pct(1.0)
            .h_pct(1.0)
            .p(16.0)
            .gap(12.0)
            .bg(pal)
            .child(
                div()
                    .size(200.0, 60.0)
                    .bg(0xffff_ffffu32)
                    .rounded(8.0)
                    .effect(Effect::new().shadow(0, 0x5000_0000u32, 0, 4, lift, 0))
                    .hover_bg(0xffff_e0c0)
                    .transition(80)
                    .on_click(move |s| {
                        let on = s.get(clip);
                        s.set(clip, !on);
                    }),
            )
            .child(
                column().size(220.0, 90.0).bg(0xffd0_d8e0u32).clip_overflow(clip).child(div().size(300.0, 40.0).bg(0xff30_60c0u32)).child(text("OVERFLOWING TEXT THAT IS LONGER THAN ITS BOX").text_scale(2)),
            )
            .child(column().size(220.0, 60.0).bg(0xffff_ffffu32).scroll(0, sy).children((0..10).map(|k| text(format!("LINE {k}")).h(14.0)))),
    );
    // glass over the top of it all, described after: on top
    ui.mount(div().abs(120.0, 40.0).size(160.0, 120.0).bg(0x30ff_ffffu32).rounded(10.0).effect(Effect::new().backdrop_blur(6)));
    ui.check().unwrap();
    // hover starts the transition; ticks run it
    ui.pointer_move(40, 40);
    ui.check().unwrap();
    assert!(ui.world.anims_active());
    for t in 1..=8 {
        ui.world.set_time(t * 16);
        ui.check().unwrap();
    }
    assert!(!ui.world.anims_active());
    // the click toggles clip_overflow; the shadow grows; the list scrolls under the glass
    assert!(ui.click(40, 40));
    let i = ui.check().unwrap();
    assert!(i.dirty_tiles > 0);
    ui.world.set(lift, 14);
    ui.check().unwrap();
    for k in 1..6 {
        ui.world.set(sy, k * 9);
        ui.check().unwrap();
    }
    assert!(ui.world.stats.blits >= 4 && ui.world.stats.backdrop_repairs >= 1);
    assert!(ui.click(40, 40));
    ui.check().unwrap();
}
