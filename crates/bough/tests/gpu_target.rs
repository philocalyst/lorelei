//! The GPU executors under the CPU executor's gate (tests/cpu_target.rs): retained frames equal a full redraw at every
//! ring age, through scrolls (scroll copies), hovers (paint-only frames, held surfaces) and structural changes; a resize
//! draws whole at the new size.
//!
//! * `vulkan`: the mixed model (kernel_backend.rs) with its Vulkan executor (bough-kernel) drawing every frame, judged
//!   by the integer kernel's definition (the CPU's bytes); and the executors switched frame by frame (the GPU, one
//!   core, the pool), which leaves no seam because they write the same bytes. On macOS it runs with
//!   `BOUGH_REPAIR=kernel` and `BOUGH_VK_LIB` (MoltenVK).
//! * `fused-vulkan`, `fused-gl`: bough-metal's fused presenter ported (crates/bough-fused), judged by its own kernels'
//!   full redraw; bins too small for a frame grow and the frame is redrawn before it is presented.
//!
//! Software devices are allowed (lavapipe, llvmpipe). A test passes vacuously, saying so, where its executor does not
//! run (no Vulkan driver, no EGL, macOS without `BOUGH_VK_LIB`).
#![cfg(any(target_os = "linux", target_os = "macos"))]
use bough::builders::*;
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

/// The app on `executor` (the mixed model's executor pinned to `pin`), or `None` (said on stderr) when that executor
/// does not run here. The mixed model's GPU is built at its first GPU frame: the first frame is drawn here.
fn app(executor: &'static str, ring: usize, pin: Option<&str>) -> Option<App<Scene>> {
    let mut c = Config::new(640.0, 480.0, 1.0, bough::ui::default_font());
    c.ring = ring;
    c.executor = Some(executor);
    let mut a = App::new(Scene { y: Signal::new(0.0), n: Signal::new(0) }, c, true);
    if let Some(p) = pin {
        a.pin_executor(p);
    }
    assert!(a.frame());
    let e = a.executor();
    let runs = if executor == "vulkan" { e.starts_with("mixed: vulkan") } else { e.starts_with(executor) };
    if !runs {
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

/// The pinned executors of the steps, in turn (`None`: no pin, one executor for every frame).
fn run(executor: &'static str, ring: usize, pins: &[&str]) -> Option<Vec<u8>> {
    let mut a = app(executor, ring, pins.first().copied())?;
    check(&mut a, &format!("{executor} ring {ring}: first frame"));
    let mut k = 0;
    let mut pin = |a: &mut App<Scene>| {
        if !pins.is_empty() {
            k += 1;
            a.pin_executor(pins[k % pins.len()]);
        }
    };
    for step in 0..12 {
        pin(&mut a);
        a.wheel_points(320.0, 200.0, if step % 4 == 3 { -40.0 } else { 17.0 });
        a.frame();
        check(&mut a, &format!("{executor} ring {ring}: scroll {step}"));
        pin(&mut a);
        a.pointer_move(300.0, 40.0 + 13.0 * step as f32);
        a.frame();
        check(&mut a, &format!("{executor} ring {ring}: hover {step}"));
    }
    pin(&mut a);
    a.turn(|s| {
        s.n.set(7);
    });
    a.frame();
    check(&mut a, &format!("{executor} ring {ring}: rows added"));
    Some(a.presented_pixels())
}

fn every_age(executor: &'static str, pins: &[&str]) {
    let last: Vec<Vec<u8>> = [2, 3, 4, 1].into_iter().filter_map(|r| run(executor, r, pins)).collect();
    // every ring size ends on the same state, so on the same pixels
    for (k, p) in last.iter().enumerate().skip(1) {
        assert!(*p == last[0], "{executor}: ring {} ends on other pixels than ring 1", k + 1);
    }
}

/// Bins of 2 entries per tile row overflow on the next drawn frame (a hover, then a scroll: retained frames, not a
/// first one): they grow and the frame is redrawn before it is presented; the oracle's full redraw grows them too.
fn bins_grow(executor: &'static str) {
    for first in ["hover", "scroll"] {
        let Some(mut a) = app(executor, 2, None) else { return };
        a.reset_bins(2);
        if first == "hover" {
            a.pointer_move(300.0, 70.0);
        } else {
            a.wheel_points(320.0, 200.0, 33.0);
        }
        assert!(a.frame(), "{executor}: the {first} draws");
        check(&mut a, &format!("{executor}: a {first} that overflowed the bins"));
        a.wheel_points(320.0, 200.0, 33.0);
        a.frame();
        check(&mut a, &format!("{executor}: a scroll after the bins grew"));
    }
}

/// A resize: fresh surfaces (the GPU lets go of the old ones), whole frames at the new size.
fn resize(executor: &'static str, pin: Option<&str>) {
    let Some(mut a) = app(executor, 3, pin) else { return };
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
    every_age("vulkan", &["gpu"]);
}

/// The mixed model's point: the executor changes from one frame to the next (GPU, one core, the pool) on the same
/// ring, and every retained frame still equals the definition's full redraw.
#[test]
fn vulkan_switching_executors_leaves_no_seam() {
    every_age("vulkan", &["gpu", "cpu1", "gpu", "cpun", "cpun", "gpu"]);
}

#[test]
fn vulkan_resize() {
    resize("vulkan", Some("gpu"));
}

#[test]
fn fused_vulkan_retained_equals_full_redraw_at_every_age() {
    every_age("fused-vulkan", &[]);
}

#[test]
fn fused_vulkan_bins_grow_and_redraw() {
    bins_grow("fused-vulkan");
}

#[test]
fn fused_vulkan_resize() {
    resize("fused-vulkan", None);
}

#[cfg(target_os = "linux")]
#[test]
fn fused_gl_retained_equals_full_redraw_at_every_age() {
    every_age("fused-gl", &[]);
}

#[cfg(target_os = "linux")]
#[test]
fn fused_gl_bins_grow_and_redraw() {
    bins_grow("fused-gl");
}

#[cfg(target_os = "linux")]
#[test]
fn fused_gl_resize() {
    resize("fused-gl", None);
}

/// The Metal presenter's own bins under the same overflow (macOS's default presenter; the fused ports follow it).
#[cfg(target_os = "macos")]
#[test]
fn metal_bins_grow_and_redraw() {
    if std::env::var("BOUGH_REPAIR").is_ok_and(|r| r == "kernel") {
        return;
    }
    bins_grow("metal");
}
