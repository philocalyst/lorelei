//! A page that uses every part of a box's style at once (a conic backdrop, a linear card under two drop
//! shadows and an inset one, a radial badge, a glass pane, an opaque panel), pushed by a blur reveal and
//! popped in mid-flight: the CPU tile kernel, the specification and Metal must agree word for word on every
//! frame of it.
use bough::color::{rgb, Color};
use bough::motion::*;
use bough::pages::*;
use bough::style::*;
use bough_kernel::cpu::Pool;
#[cfg(target_os = "macos")]
use bough_paint::flat_scene::FlatGpu;

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
fn a_styled_page_is_lowered_to_one_node_per_part() {
    let scene = make_rich_page(320, 224, 0x1e3a8a);
    let n = scene.kind_counts();
    assert_eq!(n[EffectKind::Shadow as usize], 2);
    assert_eq!(n[EffectKind::Inset as usize], 1);
    // the conic backdrop, the linear card, the radial badge
    assert_eq!(n[EffectKind::Ramp as usize], 3);
    assert_eq!(n[EffectKind::Glass as usize], 1);
    // the card's and the pane's
    assert_eq!(n[EffectKind::Border as usize], 2);
    assert_eq!(n[EffectKind::Solid as usize], 1);
    assert_eq!(n.iter().sum::<u32>() as usize, scene.nodes());
}

#[test]
fn an_interrupted_blur_reveal_is_the_same_on_cpu_spec_and_metal() {
    let (w, h) = (320u32, 224u32);
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

    for step in 0..12u64 {
        let now = ClockNs::from_ms(step * 16);
        if step == 5 {
            // Interrupt mid-flight!
            router.pop(now);
            twin.pop(now);
        }
        let _ = twin.tick(now);
        let st = router.frame(now, &mut cpu_px, w as usize, &pool, 1);
        assert!(st.animating, "the transition is still in flight at step {step}");
        router.stage_mut().spec(&mut spec_px, w as usize);
        assert_eq!(cpu_px, spec_px, "CPU != spec at step {step}");
        #[cfg(target_os = "macos")]
        if let Some(g) = gpu.as_mut() {
            g.frame(twin.stage_mut(), &pool, 1);
            assert_eq!(g.out(), spec_px.as_slice(), "Metal != spec at step {step}");
        }
    }
}
