//! Offscreen zgui canvas probe using painted CSS effects and a framework animation.
use std::{cell::RefCell, rc::Rc};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use zgui::bits::DamageSet;
use zgui::geom::{CssPx, DevicePx, Point, Size};
use zgui::platform::{Surface, SurfaceEvent};
use zgui::prelude::*;
use zgui::reactive::RwSignal;
use zgui::render::{
    ExternalTexture, FrameOutcome, MemoryReport, RenderCapabilities, RenderTarget, Renderer,
    TextureHandle,
};
use zgui::runtime::{App, AppError};
use zgui::view::{Anchor, BuildCx, ClassName};
use zgui::vocab::{
    Modifiers, PointerAction, PointerEvent, PointerId, PointerKind, ScrollDelta, ScrollPhase,
    Timestamp, WheelEvent,
};
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
    let args: Vec<_> = std::env::args().collect();
    let gate = args.iter().any(|a| a == "check");
    let cell = args
        .get(2)
        .filter(|x| x.as_str() != "check")
        .map(String::as_str)
        .unwrap_or("one");
    if cell == "video" {
        std::process::exit(2);
    }
    assert!(
        [
            "startup", "idle", "one", "many", "hover", "theme", "scroll", "resize"
        ]
        .contains(&cell)
    );
    let count = args
        .get(3)
        .and_then(|x| x.parse::<usize>().ok())
        .unwrap_or(240);
    let strip = cell == "scroll";
    let thematic = cell == "theme";
    let many = cell == "many";
    let image_path = std::env::var("BOUGH_REFERENCE_TILE").ok();
    let matched = image_path.is_some();
    let startup = Instant::now();
    let theme = RwSignal::new_local(false);
    let witness = gate.then(Witness::default);
    let renderer_witness = witness.clone();
    let moving = RwSignal::new_local(false);
    let css = "
        :root { background-color: #142638; }
        .port { width: 100%; height: 100%; overflow: scroll; }
        .layer { position: relative; width: 640px; height: 400px; }
        .card:hover { background: #f0b65dd4 !important; }
        .many-motion { width:55px !important; opacity:0.83; transform:translate(9px,-1px); }
        .card { position: absolute; box-sizing: border-box; width: 60px; height: 36px; border-radius: 6px;
                border: 1px solid #69849dd4; box-shadow: 2px 5px 8px 1px #00000048, -2px 2px 3px #070c142c;
                transition: transform 300ms ease-in-out, opacity 300ms ease-in-out, width 300ms ease-in-out; }
        .chip { position: absolute; left: 7px; top: 9px; width:12px; height:12px; }
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
            if strip { layer = layer.style_text(Some(format!("height:{}px", cards.div_ceil(10)*40))); }
            for i in 0..cards {
                let slot = if strip { i } else { i % 100 };
                let stagger = if strip { 0.0 } else { ((i / 100) % 8) as f32 * 0.25 };
                let x = (slot % 10) as f32 * 64.0 + stagger;
                let y = (slot / 10) as f32 * 40.0 + stagger;
                let base = ((i as u32).wrapping_mul(0x0077_9b83) ^ 0x22b080) & 0xffffff;
                let end = base ^ 0x3b2b18;
                let css = move || {
                    let color = if thematic && theme.get() { base ^ 0x22105f } else { base };
                    if matched {
                        Some(format!("left:{x}px;top:{y}px;background:#{color:06x}d4;box-shadow:none"))
                    } else {
                        Some(format!("left:{x}px;top:{y}px;background:linear-gradient(124deg,#{color:06x}d4,#{end:06x}c7)"))
                    }
                };
                let mut card = zgui::elements::control().class("card");
                card = if thematic { card.style_text(css) } else { card.style_text(css()) };
                if i + 1 == cards {
                    card = card.class_toggle(ClassName::new("moving"), move || moving.get());
                } else if many {
                    card = card.class_toggle(ClassName::new("many-motion"), move || moving.get());
                }
                if matched && i+1 < cards {
                    card = card.child(zgui::elements::r#box().class("chip").style_text(Some(format!("background:#{base:06x}"))));
                }
                layer = layer.child(card);
                if let Some(path) = image_path.as_ref().filter(|_| i.is_multiple_of(16)) {
                    layer = layer.child(zgui::elements::image().src(Some(path.clone())).style_text(Some(format!(
                        "position:absolute;left:{}px;top:{}px;width:12px;height:12px;pointer-events:none", x+34.0,y+12.0))));
                }
            }
            if !matched {
                layer = layer.child(zgui::elements::control().class("pane"));
                layer = layer.child(zgui::elements::control().class("sdf"));
            }
            if strip {
                Box::new(zgui::elements::column().class("port").child(layer).into_view().build(cx))
            } else {
                Box::new(layer.into_view().build(cx))
            }
        }).expect("runtime");
    let mut harness = Harness::new(handler);
    if matched {
        harness.platform().offscreens()[0].set_refresh_rate_millihertz(Some(120_000));
    }
    harness.deliver_to_first(SurfaceEvent::Resized(Size::new(
        DevicePx(640.0),
        DevicePx(400.0),
    )));
    harness.settle(512);
    if matched {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            harness.settle(64);
            let report = harness.app_mut().windows_mut()[0].budget_report();
            if report
                .line(zgui::runtime::budget::CacheId::DecodedImages)
                .report
                .resident
                > 0
            {
                break;
            }
            assert!(Instant::now() < deadline, "required PNG never decoded");
            std::thread::yield_now();
        }
        if let Some(ref witness) = witness {
            witness.borrow_mut().clear();
        }
    }
    let baseline = harness.redraws_requested();
    if cell == "startup" {
        println!(
            "zgui_sample,{cards},{cell},0,{},{}",
            startup.elapsed().as_nanos(),
            baseline
        );
        return;
    }
    if cell == "idle" {
        let time = Instant::now();
        harness.advance(Duration::from_secs(1));
        harness.pump();
        assert_eq!(baseline, harness.redraws_requested());
        println!(
            "zgui_sample,{cards},{cell},0,{},0",
            time.elapsed().as_nanos()
        );
        return;
    }
    for k in 0..count {
        let start = Instant::now();
        match cell {
            "one" | "many" if matches!(k % 40, 0 | 12) => moving.set(k % 40 == 0),
            "theme" => theme.set(k % 2 == 0),
            "hover" => {
                let j = k % 40;
                let j = if j < 20 { j } else { 39 - j };
                harness.deliver_to_first(SurfaceEvent::Pointer {
                    action: PointerAction::Moved,
                    event: PointerEvent::mouse(Point::new(
                        CssPx((j % 10) as f32 * 64.0 + 20.0),
                        CssPx((j / 10) as f32 * 40.0 + 20.0),
                    )),
                    modifiers: Modifiers::NONE,
                    timestamp: Timestamp::ORIGIN,
                });
            }
            "scroll" => {
                let delta = if k % 40 < 20 { -11.0 } else { 11.0 };
                harness.deliver_to_first(SurfaceEvent::Wheel {
                    event: WheelEvent {
                        delta: ScrollDelta::Pixels(Size::new(CssPx(0.0), CssPx(delta))),
                        phase: ScrollPhase::Discrete,
                        position: Point::new(CssPx(100.0), CssPx(100.0)),
                        id: PointerId::MOUSE,
                        kind: PointerKind::Mouse,
                    },
                    modifiers: Modifiers::NONE,
                    timestamp: Timestamp::ORIGIN,
                });
            }
            "resize" => {
                let (w, h) = if k % 2 == 0 {
                    (704.0, 448.0)
                } else {
                    (640.0, 400.0)
                };
                harness
                    .deliver_to_first(SurfaceEvent::Resized(Size::new(DevicePx(w), DevicePx(h))));
            }
            _ => {}
        }
        harness.advance(Duration::from_nanos(8_333_334));
        harness.pump();
        println!(
            "zgui_sample,{cards},{cell},{k},{},{}",
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
