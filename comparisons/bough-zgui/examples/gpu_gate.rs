//! The executors' pixel gate on zgui's laid-out gallery (text in several faces, rounded boxes, borders, gradients,
//! shadows, vector icons, the turning spinner image, pulses, the progress slide, the text field, scrolling):
//! `gpu_gate [out_dir] [steps]`.
//!
//! Every step drives the document (scroll by wheel lines, the 60 Hz clock, hover and clicks on the swatches, typing
//! into the field), draws a frame through whatever executor the app runs (`BOUGH_GPU`; on macOS the Metal presenter)
//! and checks the presented surface against a from-scratch full redraw by the same kernels: a retained frame (a ring
//! surface repaired by its age, a scroll copy with certificates, held surfaces) must hold exactly the full redraw's
//! bytes. With `out_dir`, the presented frames of the checkpoint steps are written there as raw BGRA (`<exec>-<step>.bgra`)
//! for comparisons across executors (`gpu_cmp`). Prints per-step diffs and the frame times; exits 1 on any diff.
use bough::input::{Button, Key, Mods};
use bough::raw::*;
use bough_zgui::gallery::{Doc, GalleryL, LINE};
use bough_zgui::*;
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let out = a.get(1).filter(|s| *s != "-").cloned();
    let steps: usize = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(48);
    let t0 = Instant::now();
    let mut app = App::new(GalleryL::fresh(), config(WIDTH, HEIGHT), true);
    let made = t0.elapsed();
    let exec = app.executor();
    let tag = exec.split(':').next().unwrap_or("x").to_string();
    eprintln!("executor: {exec}; app made in {:.1} ms", made.as_secs_f64() * 1e3);
    let mut bad_steps = 0;
    let mut times = Vec::new();
    let mut check = |app: &mut App<GalleryL>, step: usize, what: &str, dt: f64| {
        let (px, r) = (app.presented_pixels(), app.reference_pixels());
        let w = app.surface_width();
        let bad = px.iter().zip(&r).filter(|(x, y)| x != y).count();
        if bad != 0 || px.len() != r.len() {
            bad_steps += 1;
            let i = px.iter().zip(&r).position(|(x, y)| x != y).unwrap_or(0) / 4;
            eprintln!("step {step} ({what}): {bad} bytes differ from a full redraw (len {} vs {}); first at ({}, {}) {:?} vs {:?}", px.len(), r.len(), i % w, i / w, &px[i * 4..i * 4 + 4], &r[i * 4..i * 4 + 4]);
        } else {
            eprintln!("step {step} ({what}): exact, {} tiles, {:.2} ms", app.repaired_tiles, dt * 1e3);
        }
        if let Some(d) = &out {
            if step % 8 == 0 || step + 1 == steps {
                std::fs::create_dir_all(d).unwrap();
                std::fs::write(format!("{d}/{tag}-{step:03}.bgra"), &px).unwrap();
            }
        }
    };
    let f0 = Instant::now();
    app.frame();
    app.wait_presented();
    let d = f0.elapsed().as_secs_f64();
    times.push(d);
    check(&mut app, 0, "first frame", d);
    let (sx, sy) = (WIDTH * 0.5, HEIGHT * 0.5);
    for step in 1..steps {
        let what = match step % 8 {
            // scroll by a wheel line, down mostly (a scroll copy of the port and its repair)
            0 | 1 | 2 => {
                app.wheel_points(sx, sy, if step % 16 == 2 { -LINE } else { LINE });
                "scroll"
            }
            // the 60 Hz clock: spinner turn, pulses, the progress slide (paint-only frames)
            3 | 4 => {
                GalleryL::turn(&mut app, 1.0 / 60.0);
                "clock"
            }
            // hover across the page
            5 => {
                app.pointer_move(120.0 + 37.0 * step as f32, 80.0 + 23.0 * step as f32);
                "hover"
            }
            // a click (a swatch, wherever the scroll put them)
            6 => {
                let (x, y) = (200.0 + 11.0 * step as f32, 300.0);
                app.pointer_down(x, y, Button::Primary);
                app.pointer_up(x, y, Button::Primary);
                "click"
            }
            // typing (into the field when it has focus; a no-op otherwise)
            _ => {
                app.key_down(Key::Char('x'), Mods::default());
                "type"
            }
        };
        let f = Instant::now();
        let drew = app.frame();
        app.wait_presented();
        let d = f.elapsed().as_secs_f64();
        if drew {
            times.push(d);
        }
        check(&mut app, step, what, d);
    }
    times.sort_by(f64::total_cmp);
    let p = |q: f64| times[((times.len() - 1) as f64 * q) as usize] * 1e3;
    eprintln!("{exec}: {} frames, p50 {:.2} ms, p90 {:.2} ms, max {:.2} ms; {bad_steps} steps differ", times.len(), p(0.5), p(0.9), p(1.0));
    std::process::exit(if bad_steps == 0 { 0 } else { 1 });
}
