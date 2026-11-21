//! Offscreen zgx canvas probe for the effects fixture.
use std::{sync::Arc, time::Instant};
use zgx::canvas::GradientStop;
use zgx::decoration::{Background, Decoration, FillShader};
use zgx::scene::{BoxShadow, Color, Effects, NodeKind, QuadStyle, Scene, Style, Transform};
use zgx_gpu::GpuRenderer;

fn style(width: f32, height: f32) -> Style {
    Style {
        absolute: true,
        width: Some(width),
        height: Some(height),
        ..Style::default()
    }
}

fn color(value: u32, alpha: u8) -> Color {
    Color((value >> 16) as u8, (value >> 8) as u8, value as u8, alpha)
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let cards: usize = args.first().expect("card count").parse().unwrap();
    assert!([200, 1000, 5000].contains(&cards));
    let mut scene = Scene::new(640.0, 400.0);
    let root = scene.root();
    scene.append(
        root,
        NodeKind::Rect(color(0x142638, 255)),
        style(640.0, 400.0),
    );
    let shadows: Arc<[BoxShadow]> = Arc::from([
        BoxShadow {
            color: Color(0, 0, 0, 72),
            offset: Transform { x: 2.0, y: 5.0 },
            blur_radius: 8.0,
            spread: 1.0,
        },
        BoxShadow {
            color: Color(7, 12, 20, 44),
            offset: Transform { x: -2.0, y: 2.0 },
            blur_radius: 3.0,
            spread: 0.0,
        },
    ]);
    let mut foreground = root;
    for i in 0..cards {
        let slot = i % 100;
        let stagger = ((i / 100) % 8) as f32 * 0.25;
        let x = (slot % 10) as f32 * 64.0 + stagger;
        let y = (slot / 10) as f32 * 40.0 + stagger;
        let base = (i as u32).wrapping_mul(0x0077_9b83) ^ 0x22b080;
        let decoration = Decoration {
            background: Some(Background::Linear {
                angle: 0.6,
                stops: Arc::from([
                    GradientStop {
                        offset: 0.0,
                        color: color(base, 212),
                    },
                    GradientStop {
                        offset: 1.0,
                        color: color(base ^ 0x3b2b18, 199),
                    },
                ]),
            }),
            shadows: Some(shadows.clone()),
            ..Decoration::default()
        };
        let card = QuadStyle {
            radius: 6.0,
            border_width: 1.0,
            border_color: color(0x69849d, 212),
            decoration: Some(Arc::new(decoration)),
            ..QuadStyle::default()
        };
        foreground = scene.append(root, NodeKind::Quad(card), style(60.0, 36.0));
        scene.set_transform(foreground, Transform { x, y });
    }
    let paint = FillShader::new(include_str!("ramp.wgsl"), None);
    let overlay = QuadStyle {
        radius: 12.0,
        decoration: Some(Arc::new(Decoration {
            background: Some(Background::Shader(paint)),
            ..Decoration::default()
        })),
        ..QuadStyle::default()
    };
    let pane = scene.append(root, NodeKind::Quad(overlay), style(112.0, 76.0));
    scene.set_transform(pane, Transform { x: 455.0, y: 298.0 });
    scene.set_effects(
        pane,
        Effects {
            blur_radius: 6.0,
            ..Effects::default()
        },
    );
    let mut renderer = GpuRenderer::new(640, 400).expect("offscreen GPU");
    let first = scene.flush();
    renderer
        .render(&scene, &first.damage)
        .expect("first render");
    let before = renderer.readback().expect("first readback");
    assert_eq!(before.len(), 640 * 400 * 4);
    for k in 0..20 {
        let start = Instant::now();
        scene.set_transform(
            foreground,
            Transform {
                x: 576.0 + k as f32,
                y: 360.0,
            },
        );
        let report = scene.flush();
        renderer
            .render(&scene, &report.damage)
            .expect("step render");
        let ns = start.elapsed().as_nanos();
        println!("zgx,{cards},{k},{ns},{}", report.damage.len());
    }
    let partial = renderer.readback().expect("retained readback");
    let mut cold = GpuRenderer::new(640, 400).expect("fresh offscreen GPU");
    let fresh = Scene::new(640.0, 400.0).flush();
    cold.render(&scene, &fresh.damage).expect("full redraw");
    assert_eq!(partial, cold.readback().expect("full readback"));
    assert!(before != partial, "foreground never changed pixels");
}
