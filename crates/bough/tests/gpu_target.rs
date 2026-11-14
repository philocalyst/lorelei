//! The GPU executors (backend/gpu.rs: bough-metal's fused presenter on Vulkan and OpenGL, crates/bough-fused) under
//! the CPU executor's gate (tests/cpu_target.rs): retained frames equal a full redraw by the same kernels at every
//! ring age, through scrolls (scroll copies with certificates), hovers (paint-only frames, held surfaces) and
//! structural changes; bins too small for a frame grow and the frame is redrawn before it is presented; a resize
//! draws whole at the new size. Each executor is forced (software devices allowed: lavapipe, llvmpipe), and its test
//! passes vacuously, saying so, where it does not run (no Vulkan driver, no EGL, macOS without `BOUGH_VK_LIB`).
#![cfg(any(target_os = "linux", target_os = "macos"))]
use bough::prelude::*;
use bough::raw::{App, Config};

struct Scene {
    y: Signal<f32>,
    n: Signal<u32>,
}

impl View for Scene {
    fn view(&self) -> impl Element<Self> + '_ {
        div().w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff)).scroll(sig!(self.y)).child(each((0..60 + *self.n as usize).map(|i| {
            row((text(format!("Row {i:05}")).color(rgb(0x1f2328)), text(format!("{}", i * 7919 % 10000)).color(rgb(0x6e7781))))
                .w_pct(1.0)
                .h(32.0)
                .shrink(0.0)
                .px(12.0)
                .items(Align::Center)
                .justify(Justify::SpaceBetween)
                .bg(rgb(if i % 2 == 0 { 0xffffff } else { 0xf6f8fa }))
                .rounded(if i % 3 == 0 { 4.0 } else { 0.0 })
                .border(if i % 5 == 0 { 1.0 } else { 0.0 }, rgb(0xd0d7de))
                .hover_bg(rgb(0xcfe3ff))
        })))
    }
}

/// The app on `executor`, or `None` (said on stderr) when that executor does not run here.
fn app(executor: &'static str, ring: usize) -> Option<App<Scene>> {
    let mut c = Config::new(640.0, 480.0, 1.0, bough::ui::default_font());
    c.ring = ring;
    c.executor = Some(executor);
    let a = App::new(Scene { y: Signal::new(0.0), n: Signal::new(0) }, c, true);
    let e = a.executor();
    if !e.starts_with(executor) {
        eprintln!("{executor}: not available here ({e}); nothing tested");
        return None;
    }
    eprintln!("{executor} ring {ring}: {e}");
    Some(a)
}

fn check(a: &mut App<Scene>, what: &str) {
    let (px, r) = (a.presented_pixels(), a.reference_pixels());
    assert_eq!(px.len(), r.len(), "{what}: sizes");
    let bad = px.iter().zip(&r).filter(|(x, y)| x != y).count();
    if bad != 0 {
        let w = a.surface_width();
        let i = px.iter().zip(&r).position(|(x, y)| x != y).unwrap() / 4;
        panic!("{what}: {bad} bytes differ from a full redraw; first ({}, {}) {:?} vs {:?}", i % w, i / w, &px[i * 4..i * 4 + 4], &r[i * 4..i * 4 + 4]);
    }
}

fn run(executor: &'static str, ring: usize) -> Option<Vec<u8>> {
    let mut a = app(executor, ring)?;
    assert!(a.frame());
    check(&mut a, &format!("{executor} ring {ring}: first frame"));
    for step in 0..12 {
        a.wheel_points(320.0, 200.0, if step % 4 == 3 { -40.0 } else { 17.0 });
        a.frame();
        check(&mut a, &format!("{executor} ring {ring}: scroll {step}"));
        a.pointer_move(300.0, 40.0 + 13.0 * step as f32);
        a.frame();
        check(&mut a, &format!("{executor} ring {ring}: hover {step}"));
    }
    a.turn(|s| {
        s.n.set(7);
    });
    a.frame();
    check(&mut a, &format!("{executor} ring {ring}: rows added"));
    Some(a.presented_pixels())
}

fn every_age(executor: &'static str) {
    let last: Vec<Vec<u8>> = [2, 3, 4, 1].into_iter().filter_map(|r| run(executor, r)).collect();
    // every ring size ends on the same state, so on the same pixels
    for (k, p) in last.iter().enumerate().skip(1) {
        assert!(*p == last[0], "{executor}: ring {} ends on other pixels than ring 1", k + 1);
    }
}

/// Bins of 2 entries per tile row overflow on the first frame: they grow and the frame is redrawn, whole.
fn bins_grow(executor: &'static str) {
    let Some(mut a) = app(executor, 2) else { return };
    a.reset_bins(2);
    assert!(a.frame());
    check(&mut a, &format!("{executor}: after the bins grew"));
    a.wheel_points(320.0, 200.0, 33.0);
    a.frame();
    check(&mut a, &format!("{executor}: a scroll after the bins grew"));
}

/// A resize: fresh surfaces, bins and certificates at the new size, a whole frame.
fn resize(executor: &'static str) {
    let Some(mut a) = app(executor, 3) else { return };
    assert!(a.frame());
    for (w, h) in [(800.0, 520.0), (512.0, 384.0), (640.0, 480.0)] {
        a.resize(w, h);
        a.frame();
        check(&mut a, &format!("{executor}: resized to {w} x {h}"));
        a.wheel_points(100.0, 100.0, 21.0);
        a.frame();
        check(&mut a, &format!("{executor}: a scroll at {w} x {h}"));
    }
}

#[test]
fn vulkan_retained_equals_full_redraw_at_every_age() {
    every_age("vulkan");
}

#[test]
fn vulkan_bins_grow_and_redraw() {
    bins_grow("vulkan");
}

#[test]
fn vulkan_resize() {
    resize("vulkan");
}

#[cfg(target_os = "linux")]
#[test]
fn gl_retained_equals_full_redraw_at_every_age() {
    every_age("gl");
}

#[cfg(target_os = "linux")]
#[test]
fn gl_bins_grow_and_redraw() {
    bins_grow("gl");
}

#[cfg(target_os = "linux")]
#[test]
fn gl_resize() {
    resize("gl");
}
