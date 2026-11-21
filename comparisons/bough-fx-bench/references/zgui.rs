//! Offscreen zgui canvas probe using painted CSS effects and a framework animation.
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use zgui::geom::{DevicePx, Size};
use zgui::platform::{Surface, SurfaceEvent};
use zgui::prelude::*;
use zgui::reactive::RwSignal;
use zgui::render::{RenderTarget, Renderer};
use zgui::runtime::{App, AppError};
use zgui::view::{Anchor, BuildCx, ClassName};
use zgui_platform_headless::Harness;

fn renderer(_: &Arc<dyn Surface>, target: RenderTarget) -> Result<Box<dyn Renderer>, AppError> {
    let renderer = zgui_render_wgpu::Builder::new()
        .offscreen(
            target,
            zgui_render_wgpu::wgpu::TextureFormat::Bgra8Unorm,
            false,
        )
        .map_err(AppError::GpuUnavailable)?;
    Ok(Box::new(renderer))
}

fn main() {
    let cards: usize = std::env::args()
        .nth(1)
        .expect("card count")
        .parse()
        .unwrap();
    assert!([200, 1000, 5000].contains(&cards));
    let moving = RwSignal::new_local(false);
    let css = "
        :root { background-color: #142638; }
        .layer { position: relative; width: 640px; height: 400px; }
        .card { position: absolute; width: 60px; height: 36px; border-radius: 6px;
                border: 1px solid #69849dd4; box-shadow: 2px 5px 8px 1px #00000048, -2px 2px 3px #070c142c;
                transition: transform 300ms ease-in-out; }
        .moving { transform: translateX(10px); }
        .pane { position: absolute; left: 455px; top: 298px; width: 112px; height: 76px;
                border-radius: 12px; backdrop-filter: blur(6px); background-color: #568cd15c; }
    ";
    let handler = App::new().with_size(640.0, 400.0).with_stylesheet(css)
        .with_renderer(Box::new(renderer))
        .into_handler(move |cx: &mut BuildCx<'_>| -> Box<dyn Anchor> {
            let mut layer = zgui::elements::column().class("layer");
            for i in 0..cards {
                let slot = i % 100;
                let stagger = ((i / 100) % 8) as f32 * 0.25;
                let x = (slot % 10) as f32 * 64.0 + stagger;
                let y = (slot / 10) as f32 * 40.0 + stagger;
                let base = ((i as u32).wrapping_mul(0x0077_9b83) ^ 0x22b080) & 0xffffff;
                let end = base ^ 0x3b2b18;
                let mut card = zgui::elements::control().class("card").style_text(Some(format!(
                    "left:{x}px;top:{y}px;background:linear-gradient(124deg,#{base:06x}d4,#{end:06x}c7)"
                )));
                if i + 1 == cards {
                    card = card.class_toggle(ClassName::new("moving"), move || moving.get());
                }
                layer = layer.child(card);
            }
            layer = layer.child(zgui::elements::control().class("pane"));
            Box::new(layer.into_view().build(cx))
        }).expect("runtime");
    let mut harness = Harness::new(handler);
    harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
        DevicePx(640.0),
        DevicePx(400.0),
    )));
    harness.settle(512);
    let baseline = harness.redraws_requested();
    for k in 0..20 {
        let start = Instant::now();
        if k == 0 || k == 12 {
            moving.set(k == 0);
        }
        harness.advance(Duration::from_nanos(8_333_333));
        harness.pump();
        println!(
            "zgui,{cards},{k},{},{}",
            start.elapsed().as_nanos(),
            harness.redraws_requested()
        );
    }
    assert!(
        harness.redraws_requested() > baseline,
        "motion never requested redraw"
    );
}
