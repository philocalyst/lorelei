//! End-to-end integration test uniting ALL 5 rethink pillars:
//! 1. Non-generic SoA core (`bough_x::flat::FlatWorld` + `bough::rethink::ui`)
//! 2. Complete effects (`FlatScene`: stacked outer + inset shadows, linear/radial/conic LUT gradients,
//!    backdrop blur glass panel, tile-grain retained group opacity)
//! 3. Deeper animations & interruptible retained-layer `PageRouter` transitions with CPU == Metal parity
//! 4. Unified `DamageEngine` (opaque cover culling, backdrop-blur dilation, rigid scroll blit)
//! 5. Unified cross-crate telemetry (`bough::rethink::bridge`) feeding inspector JSON.

use bough::color::{rgb, Color};
use bough::fx_api::*;
use bough::pages::*;
use bough::rethink::bridge::{
    inspect_flat_effects, step_unified_telemetry, sync_flat_scene_to_damage_engine,
};
use bough::rethink::ui::{column, div, row, text, FlatUi};
use bough::layout::Styled;
use bough_kernel::cpu::Pool;
#[cfg(target_os = "macos")]
use bough_paint::flat_scene::FlatGpu;
use bough_x::damage_engine::{DamageEngine, Rect, Work};

fn make_rich_page(w: u32, h: u32, accent: u32) -> FlatScene {
    let mut s = FlatScene::new(w, h, FlatConfig::default());
    // Conic gradient backdrop
    s.add_box(
        [0.0, 0.0, w as f32, h as f32],
        &BoxStyle::new().gradient(
            Gradient::conic(45.0)
                .at(0.5, 0.5)
                .stop(0.0, rgb(0x0f172a))
                .stop(0.5, rgb(accent))
                .stop(1.0, rgb(0x0f172a)),
        ),
    )
    .expect("backdrop");
    // Card with linear gradient, 2 stacked outer shadows, and 1 inset shadow
    s.add_box(
        [24.0, 24.0, 184.0, 136.0],
        &BoxStyle::new()
            .gradient(
                Gradient::linear(135.0)
                    .stop(0.0, rgb(0x4f46e5))
                    .stop(0.5, rgb(0x9333ea))
                    .stop(1.0, rgb(0xec4899)),
            )
            .rounded(14.0)
            .border(1.0, Color::rgba(255, 255, 255, 70))
            .shadow(ShadowSpec::drop(0.0, 10.0, 24.0, Color::rgba(0, 0, 0, 80)).spread(-4.0))
            .shadow(ShadowSpec::drop(0.0, 2.0, 4.0, Color::rgba(0, 0, 0, 50)))
            .shadow(ShadowSpec::inset(0.0, 1.0, 3.0, Color::rgba(255, 255, 255, 90))),
    )
    .expect("card");
    // Radial gradient badge
    s.add_box(
        [200.0, 32.0, 264.0, 96.0],
        &BoxStyle::new()
            .gradient(
                Gradient::radial()
                    .focal(-0.2, -0.2)
                    .stop(0.0, rgb(0xffffff))
                    .stop(0.4, rgb(0xfbbf24))
                    .stop(1.0, rgb(0xb45309)),
            )
            .rounded(32.0),
    )
    .expect("badge");
    // Backdrop blur glass panel overlapping the card
    s.add_box(
        [110.0, 72.0, 280.0, 180.0],
        &BoxStyle::new()
            .backdrop_blur(BlurSpec::backdrop(8.0).saturate(1.3).tint(Color::rgba(255, 255, 255, 36)))
            .rounded(16.0)
            .border(1.0, Color::rgba(255, 255, 255, 80)),
    )
    .expect("glass");
    // Opaque modal cover in the bottom-right corner (tests opaque cover culling)
    s.add_box(
        [192.0, 128.0, 320.0, 224.0],
        &BoxStyle::new().bg(rgb(0x1e293b)),
    )
    .expect("opaque_modal");
    s
}

#[test]
fn unified_all_five_pillars_cpu_metal_damage_and_telemetry() {
    let (w, h) = (320u32, 224u32);
    let scene = make_rich_page(w, h, 0x1e3a8a);

    // 1. Verify full effects inventory on non-generic FlatScene
    let fx = inspect_flat_effects(&scene);
    assert_eq!(fx.drop_shadows, 2);
    assert_eq!(fx.inset_shadows, 1);
    assert_eq!(fx.linear_gradients, 1);
    assert_eq!(fx.radial_gradients, 1);
    assert_eq!(fx.conic_gradients, 1);
    assert_eq!(fx.backdrop_blurs, 1);

    // 2. Verify DamageEngine occlusion culling + blur propagation + rigid scroll synced from FlatScene
    let mut engine = DamageEngine::new(w as i32, h as i32);
    engine.begin_frame();
    sync_flat_scene_to_damage_engine(&scene, &mut engine);
    // Damage strictly behind the opaque modal at z=0 -> must be occluded!
    engine.damage(0, Rect::new(224, 160, 288, 192), Work::Flat);
    // Damage behind the backdrop blur glass panel -> must trigger blur propagation!
    engine.damage(0, Rect::new(128, 96, 160, 128), Work::Flat);
    // Rigid scroll on left band -> must emit ScrollBlit!
    let blit = engine.scroll(0, 2, Rect::new(0, 0, 96, 224), 0, -16);
    assert!(blit.is_some());
    engine.resolve(&mut |_layer, rect| Some(rect));
    let rep = engine.report();
    assert!(rep.occluded_tiles_skipped > 0, "opaque modal must cull occluded tiles");
    assert!(!rep.blur_blocks.is_empty(), "backdrop blur must produce dilated blur blocks");
    assert!(!rep.scroll_blits.is_empty(), "rigid scroll must emit a scroll blit");
    assert!(rep.to_json().contains("\"repair_percent\""));

    // 3. Verify PageRouter with mid-flight interruption AND word-for-word CPU == Spec == Metal parity
    let mut router = PageRouter::new(w, h, RouterConfig::default()).expect("router");
    let p1 = router.add_page(make_rich_page(w, h, 0x1e3a8a)).expect("p1");
    let p2 = router.add_page(make_rich_page(w, h, 0x0f766e)).expect("p2");
    router.show(p1);
    router.push(p2, PageTransition::blur_reveal(), ClockNs::from_ms(0));

    let mut twin = PageRouter::new(w, h, RouterConfig::default()).expect("twin");
    let tp1 = twin.add_page(make_rich_page(w, h, 0x1e3a8a)).expect("tp1");
    let tp2 = twin.add_page(make_rich_page(w, h, 0x0f766e)).expect("tp2");
    twin.show(tp1);
    twin.push(tp2, PageTransition::blur_reveal(), ClockNs::from_ms(0));

    let pool = Pool::new(2);
    let mut cpu_px = vec![0u32; (w * h) as usize];
    let mut spec_px = vec![0u32; (w * h) as usize];
    #[cfg(target_os = "macos")]
    let mut gpu = unsafe { FlatGpu::new(twin.stage(), 256) };

    let motion = Motion::new();
    for step in 0..12u64 {
        let now = ClockNs::from_ms(step * 16);
        if step == 5 {
            // Interrupt mid-flight!
            router.pop(now);
            twin.pop(now);
        }
        let tel = step_unified_telemetry(
            &scene,
            &mut engine,
            &mut router,
            &motion,
            &[(0, Rect::new(128, 96, 160, 128), Work::Flat)],
            now,
        );
        let _ = twin.tick(now);
        assert!(tel.router_in_flight);
        router.frame(now, &mut cpu_px, w as usize, &pool, 1);
        router.stage_mut().spec(&mut spec_px, w as usize);
        assert_eq!(cpu_px, spec_px, "CPU != spec at step {step}");
        #[cfg(target_os = "macos")]
        if let Some(g) = gpu.as_mut() {
            g.frame(twin.stage_mut(), &pool, 1);
            assert_eq!(g.out(), spec_px.as_slice(), "Metal != spec at step {step}");
        }
    }

    // 4. Verify non-generic FlatUi builder + zero-unsafe SoA core
    let mut ui = FlatUi::new(320, 224, 1.0);
    let count = ui.world.slot(0u32);
    let root = column()
        .w_pct(1.0)
        .h_pct(1.0)
        .bg(0xff10_141cu32)
        .child(row().h(40.0).bg(0xff1b_2434u32).child(text("Unified Rethink").color(0xffff_ffffu32)))
        .child(div().grow(1.0).bg(0xff14_1c28u32));
    ui.mount(root);
    ui.world.set(count, 42);
    ui.check().expect("FlatUi oracle check passed");
}
