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
    if let Some(cell) = args.get(1) {
        let count = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(240);
        return suite(cards, cell, count, args.iter().any(|s| s == "check"));
    }
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

fn suite(cards: usize, cell: &str, frames: usize, check: bool) {
    use zgx::image::ImageData;
    use zgx::scene::{Layout, Rect};
    assert!([200, 1000, 5000].contains(&cards));
    assert!(
        [
            "startup", "idle", "one", "many", "hover", "theme", "scroll", "resize", "video"
        ]
        .contains(&cell)
    );
    let start = Instant::now();
    let strip = cell == "scroll";
    let mut scene = Scene::new(640.0, 400.0);
    let root = scene.root();
    scene.append(
        root,
        NodeKind::Rect(color(0x142638, 255)),
        Style {
            absolute: true,
            width_percent: Some(1.0),
            height_percent: Some(1.0),
            ..Style::default()
        },
    );
    let layer = scene.append(
        root,
        NodeKind::Container(Layout::Overlay),
        style(
            640.0,
            if strip {
                cards.div_ceil(10) as f32 * 40.0
            } else {
                400.0
            },
        ),
    );
    let card = |i: usize, dark: bool, hot: bool, alpha: u8| {
        let base = (i as u32).wrapping_mul(0x0077_9b83) ^ if dark { 0x00bfdf } else { 0x22b080 };
        NodeKind::Quad(QuadStyle {
            fill: color(if hot { 0xf0b65d } else { base }, alpha),
            radius: 6.0,
            border_width: 1.0,
            border_color: color(0x69849d, alpha),
            ..QuadStyle::default()
        })
    };
    let position = |i: usize| {
        let slot = if strip { i } else { i % 100 };
        let stagger = if strip {
            0.0
        } else {
            ((i / 100) % 8) as f32 * 0.25
        };
        (
            (slot % 10) as f32 * 64.0 + stagger,
            (slot / 10) as f32 * 40.0 + stagger,
        )
    };
    let mut ids = Vec::with_capacity(cards);
    // The author fixture's procedural 12x12 PNG, with exactly the same decoded RGBA samples.
    let tile: Vec<_> = (0..12)
        .flat_map(|y| {
            (0..12).flat_map(move |x| [60 + x * 16, 15 + y * 19, 248, 75 + ((x + y) % 8) * 25])
        })
        .collect();
    let picture = Arc::new(ImageData::new(12, 12, tile).unwrap());
    for i in 0..cards {
        let (x, y) = position(i);
        let id = scene.append(layer, card(i, false, false, 212), style(60.0, 36.0));
        scene.set_transform(id, Transform { x, y });
        ids.push(id);
        if i.is_multiple_of(16) {
            let media = scene.append(layer, NodeKind::Image(picture.clone()), style(12.0, 12.0));
            scene.set_transform(
                media,
                Transform {
                    x: x + 34.0,
                    y: y + 12.0,
                },
            );
        }
    }
    let video_sample = |k: usize| -> Arc<ImageData> {
        let mut rgba = Vec::with_capacity(256 * 144 * 4);
        for y in 0..144u32 {
            for x in 0..256u32 {
                let t = k as u32;
                rgba.extend([
                    (x + t * 17) as u8,
                    (y * 3 + t * 11) as u8,
                    (x ^ y ^ (t * 5)) as u8,
                    255,
                ]);
            }
        }
        Arc::new(ImageData::new(256, 144, rgba).unwrap())
    };
    let video = if cell == "video" {
        let image = scene.append(root, NodeKind::Image(video_sample(0)), style(256.0, 144.0));
        scene.set_transform(image, Transform { x: 256.0, y: 124.0 });
        let mut panes = Vec::new();
        for (x, y) in [(14.0, 18.0), (514.0, 18.0), (328.0, 150.0), (482.0, 302.0)] {
            let pane = scene.append(
                root,
                NodeKind::Quad(QuadStyle {
                    fill: color(0x6789a9, 82),
                    radius: 12.0,
                    ..QuadStyle::default()
                }),
                style(112.0, 76.0),
            );
            scene.set_transform(pane, Transform { x, y });
            scene.set_effects(
                pane,
                Effects {
                    blur_radius: 6.0,
                    brightness: 0.8,
                    saturate: 1.4,
                    ..Effects::default()
                },
            );
            panes.push(pane);
        }
        Some((image, panes[2]))
    } else {
        None
    };
    let mut w = 640;
    let mut h = 400;
    let mut renderer = GpuRenderer::new(w, h).expect("offscreen GPU");
    let mut full = check.then(|| GpuRenderer::new(w, h).expect("full offscreen GPU"));
    scene.flush();
    let bounds = |w, h| Rect::new(0.0, 0.0, w as f32, h as f32);
    renderer.render(&scene, &[bounds(w, h)]).unwrap();
    let initial = if check {
        Some(renderer.readback().unwrap())
    } else {
        None
    };
    if cell == "startup" {
        println!(
            "zgx_sample,{cards},{cell},0,{},1",
            start.elapsed().as_nanos()
        );
        return;
    }
    let mut draws = 0;
    let mut hot = None;
    let mut changed = false;
    let mut scroll = 0.0f32;
    for step in 0..if cell == "idle" { 1 } else { frames } {
        let t = Instant::now();
        match cell {
            "one" | "many" => {
                let k = step % 40;
                // Explicit 120 Hz curve sampling at the Scene boundary.
                let progress = (if k < 12 {
                    k as f32
                } else {
                    12.0 - (k - 12) as f32
                }) / 12.0;
                let e = progress.clamp(-1.0, 1.0);
                let e = e * e * (3.0 - 2.0 * e.abs()) * e.signum();
                let first = if cell == "one" { cards - 1 } else { 0 };
                for (i, id) in ids.iter().copied().enumerate().skip(first) {
                    let (x, y) = position(i);
                    let dist = if i + 1 == cards {
                        10.0
                    } else {
                        5.0 + (i % 9) as f32
                    };
                    scene.set_transform(
                        id,
                        Transform {
                            x: x + e * dist,
                            y: y - if i + 1 == cards {
                                0.0
                            } else {
                                e * (i % 3) as f32
                            },
                        },
                    );
                    if i + 1 < cards {
                        scene.set_style(id, style(60.0 - 5.0 * e.abs(), 36.0));
                        scene.set_leaf_kind(
                            id,
                            card(i, false, false, (212.0 - 36.0 * e.abs()).round() as u8),
                        );
                    }
                }
            }
            "theme" => {
                for (i, id) in ids.iter().enumerate() {
                    scene.set_leaf_kind(*id, card(i, step % 2 == 0, false, 212));
                }
            }
            "hover" => {
                let p = step % 40;
                let p = if p < 20 { p } else { 39 - p };
                let id =
                    scene.hit_test((p % 10) as f32 * 64.0 + 20.0, (p / 10) as f32 * 40.0 + 20.0);
                let next = id.and_then(|id| ids.iter().position(|&x| x == id));
                if hot != next {
                    if let Some(i) = hot {
                        scene.set_leaf_kind(ids[i], card(i, false, false, 212));
                    }
                    if let Some(i) = next {
                        scene.set_leaf_kind(ids[i], card(i, false, true, 212));
                    }
                    hot = next;
                }
            }
            "scroll" => {
                let max = (cards.div_ceil(10) * 40 - 400) as f32;
                scroll = (scroll + if step % 40 < 20 { 11.0 } else { -11.0 }).clamp(0.0, max);
                scene.set_transform(layer, Transform { x: 0.0, y: -scroll });
            }
            "resize" => {
                (w, h) = if step % 2 == 0 {
                    (704, 448)
                } else {
                    (640, 400)
                };
                scene.resize(w as f32, h as f32);
                renderer.resize(w, h);
                if let Some(f) = &mut full {
                    f.resize(w, h);
                }
            }
            "video" => {
                let (image, panel) = video.unwrap();
                if step.is_multiple_of(4) {
                    scene.set_image_frame(image, video_sample(step / 4 + 1));
                }
                scene.set_transform(
                    panel,
                    Transform {
                        x: 328.0 + 10.0 * ((step as f32) * 0.066).sin(),
                        y: 150.0,
                    },
                );
            }
            "idle" => std::thread::sleep(std::time::Duration::from_secs(1)),
            _ => unreachable!(),
        }
        let report = scene.flush();
        if !report.damage.is_empty() {
            renderer.render(&scene, &report.damage).unwrap();
            draws += 1;
        }
        println!(
            "zgx_sample,{cards},{cell},{step},{},{draws}",
            t.elapsed().as_nanos()
        );
        if let Some(full) = &mut full {
            full.render(&scene, &[bounds(w, h)]).unwrap();
            let actual = renderer.readback().unwrap();
            assert!(
                actual == full.readback().unwrap(),
                "retained differs on {cell} at step {step}"
            );
            changed |= initial.as_ref() != Some(&actual);
        }
    }
    if cell == "idle" {
        assert_eq!(draws, 0);
    }
    if check && cell != "idle" {
        assert!(changed, "{cell} did not change output");
    }
}
