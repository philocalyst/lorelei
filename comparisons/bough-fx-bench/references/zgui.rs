//! Offscreen zgui canvas probe using painted CSS effects and a framework animation.
use std::{cell::RefCell, rc::Rc};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use zgui::bits::DamageSet;
use zgui::geom::{DevicePx, Size};
use zgui::platform::{Surface, SurfaceEvent};
use zgui::prelude::*;
use zgui::reactive::RwSignal;
use zgui::render::{
    ExternalTexture, FrameOutcome, MemoryReport, RenderCapabilities, RenderTarget, Renderer,
    TextureHandle,
};
use zgui::runtime::{App, AppError};
use zgui::view::{Anchor, BuildCx, ClassName};
use zgui_platform_headless::Harness;

#[repr(C)]
#[derive(Clone, Copy, Default, ShaderParams)]
struct Mask {
    fraction: f32,
}

static SDF: ShaderEffect<Mask> = shader! {
    name: "effects-circle-difference",
    mode: Coverage,
    params: Mask,
    source: r#"
        struct Params { fraction: f32, }
        fn coverage(in: ShaderInput, params: Params) -> f32 {
            let p = in.local / in.size;
            let outer = length(p - vec2<f32>(0.48, 0.5)) - 0.43;
            let inner = length(p - vec2<f32>(0.64, 0.40)) - (0.25 + params.fraction);
            return 1.0 - smoothstep(-0.01, 0.01, max(outer, -inner));
        }
    "#,
};

type Witness = Rc<RefCell<Vec<u64>>>;

struct PixelGate {
    renderer: zgui_render_wgpu::WgpuRenderer,
    pixels: Witness,
}

impl Renderer for PixelGate {
    fn capabilities(&self) -> RenderCapabilities {
        self.renderer.capabilities()
    }
    fn configure(&mut self, target: RenderTarget) {
        self.renderer.configure(target);
    }
    fn target(&self) -> Option<RenderTarget> {
        self.renderer.target()
    }
    fn draw(&mut self, scene: &zgui::scene::Scene, damage: &DamageSet) -> FrameOutcome {
        let outcome = self.renderer.draw(scene, damage);
        let retained = self.renderer.read_composed();
        assert!(!retained.bytes().is_empty());
        let hash = retained
            .bytes()
            .iter()
            .fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
                (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
            });
        self.pixels.borrow_mut().push(hash);
        outcome
    }
    fn register_external(&mut self, texture: ExternalTexture) -> TextureHandle {
        self.renderer.register_external(texture)
    }
    fn release_external(&mut self, handle: TextureHandle) {
        self.renderer.release_external(handle);
    }
    fn memory(&self) -> MemoryReport {
        self.renderer.memory()
    }
    fn texture_sink(&mut self) -> &mut dyn zgui::atlas::TextureSink {
        self.renderer.texture_sink()
    }
}

fn renderer(target: RenderTarget, gate: Option<Witness>) -> Result<Box<dyn Renderer>, AppError> {
    let renderer = zgui_render_wgpu::Builder::new()
        .offscreen(
            target,
            zgui_render_wgpu::wgpu::TextureFormat::Bgra8Unorm,
            false,
        )
        .map_err(AppError::GpuUnavailable)?;
    Ok(match gate {
        Some(pixels) => Box::new(PixelGate { renderer, pixels }),
        None => Box::new(renderer),
    })
}

fn main() {
    let cards: usize = std::env::args()
        .nth(1)
        .expect("card count")
        .parse()
        .unwrap();
    assert!([200, 1000, 5000].contains(&cards));
    let gate = std::env::args().any(|a| a == "check");
    let witness = gate.then(Witness::default);
    let renderer_witness = witness.clone();
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
        .sdf { position: absolute; left: 72px; top: 134px; width: 80px; height: 64px;
               --zgui-shape: effects-circle-difference; background-color: #ec8f6fa6; }
    ";
    let handler = App::new().with_size(640.0, 400.0).with_stylesheet(css)
        .with_renderer(Box::new(move |_: &Arc<dyn Surface>, target| renderer(target, renderer_witness.clone())))
        .into_handler(move |cx: &mut BuildCx<'_>| -> Box<dyn Anchor> {
            let _effect = SDF.register();
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
            layer = layer.child(zgui::elements::control().class("sdf"));
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
    if let Some(witness) = witness {
        let pixels = witness.borrow();
        let first = pixels.first().expect("a captured frame");
        assert!(
            pixels.iter().any(|hash| hash != first),
            "the trace did not change pixels"
        );
        for (frame, hash) in pixels.iter().enumerate() {
            println!("zgui_pixel,{cards},{frame},{hash:016x}");
        }
    }
    assert!(
        harness.redraws_requested() > baseline,
        "motion never requested redraw"
    );
}
