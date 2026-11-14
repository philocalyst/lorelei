//! The GPU executors on Linux (crates/bough-fused): bough-metal's fused presenter on Vulkan or OpenGL, drawing the
//! window's surfaces ([`super::Surfaces`]) in place of the CPU executor. On macOS `BOUGH_GPU=vulkan` runs the Vulkan
//! one headless (MoltenVK, `BOUGH_VK_LIB`) in place of the Metal presenter: the same kernels on a real GPU, for gates.
//!
//! Which one runs is decided once, when the app is made (never per frame), by `BOUGH_GPU`:
//! * `vulkan`: the Vulkan presenter on the best device (`BOUGH_VK_DEVICE` names one), software ones included;
//! * `gl`: the OpenGL 4.3 presenter (EGL, no window of its own), software renderers included;
//! * `cpu`: the CPU executor;
//! * unset: Vulkan on a hardware device, otherwise the CPU executor (bough's own SIMD executor beats a software
//!   rasteriser running the GPU kernels).
//!
//! The Vulkan presenter reads the region in place (views of the columns' used prefixes) and writes the window's
//! buffers in place; the GL presenter owns the region (one mapped buffer of the configured capacities) and copies
//! each frame's changed tiles into the window's buffer.
#[cfg(target_os = "linux")]
use bough_fused::gl;
use bough_fused::vk;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    Auto,
    Vulkan,
    Gl,
    Cpu,
}

/// The executor `name` asks for (`Config::executor`), else `BOUGH_GPU`'s.
pub fn want_of(name: Option<&str>) -> Want {
    let env = std::env::var("BOUGH_GPU").ok();
    match name.or(env.as_deref()) {
        Some("vulkan") | Some("vk") => Want::Vulkan,
        Some("gl") | Some("opengl") => Want::Gl,
        Some("cpu") => Want::Cpu,
        _ => Want::Auto,
    }
}

/// At process entry: start building the device the app will use on another thread (`app::prepare`).
pub fn prepare(name: Option<&str>) {
    match want_of(name) {
        w @ (Want::Auto | Want::Vulkan) => vk::prepare_in_background(w == Want::Vulkan),
        #[cfg(target_os = "linux")]
        Want::Gl => gl::prepare_in_background(),
        #[cfg(not(target_os = "linux"))]
        Want::Gl => {}
        Want::Cpu => {}
    }
}

/// A GPU executor of the app's frames.
pub enum Gpu {
    Vk(vk::Fused),
    #[cfg(target_os = "linux")]
    Gl(gl::Fused),
}

/// Report why a requested executor is not running (an explicit request only: the default falls back silently).
pub(crate) fn refused(what: &str, e: &str) {
    eprintln!("bough: BOUGH_GPU={what} requested, but {e}; drawing with the CPU executor");
}

impl Gpu {
    /// The executor, its device and driver.
    pub fn info(&self) -> String {
        match self {
            Gpu::Vk(f) => format!("vulkan: {}", f.info()),
            #[cfg(target_os = "linux")]
            Gpu::Gl(f) => format!("gl: {}", f.info()),
        }
    }

    pub(crate) fn set_layout(&mut self, l: bough_frame::Layout) {
        match self {
            Gpu::Vk(f) => f.set_layout(l),
            #[cfg(target_os = "linux")]
            Gpu::Gl(f) => f.set_layout(l),
        }
    }

    pub(crate) fn sync(&mut self) {
        match self {
            Gpu::Vk(f) => f.sync(),
            #[cfg(target_os = "linux")]
            Gpu::Gl(f) => f.sync(),
        }
    }

    #[allow(irrefutable_let_patterns)]
    pub(crate) fn set_batch(&mut self, k: usize) {
        if let Gpu::Vk(f) = self {
            f.set_batch(k);
        }
    }

    pub(crate) fn resize(&mut self, w: usize, h: usize) {
        match self {
            Gpu::Vk(f) => f.resize(w, h),
            #[cfg(target_os = "linux")]
            Gpu::Gl(f) => f.resize(w, h),
        }
    }

    pub(crate) fn sync_atlas(&mut self, width: u32, height: u32, pixels: &[u8], dirty: bool) -> bool {
        match self {
            Gpu::Vk(f) => f.sync_atlas(width, height, pixels, dirty),
            #[cfg(target_os = "linux")]
            Gpu::Gl(f) => f.sync_atlas(width, height, pixels, dirty),
        }
    }

    pub(crate) fn sync_images(&mut self, pixels: &[u8], rows: std::ops::Range<u32>) {
        match self {
            Gpu::Vk(f) => f.sync_images(pixels, rows),
            #[cfg(target_os = "linux")]
            Gpu::Gl(f) => f.sync_images(pixels, rows),
        }
    }

    #[allow(irrefutable_let_patterns)]
    pub(crate) fn ensure_columns(&mut self, need: &[usize; bough_frame::NGROUPS]) {
        if let Gpu::Vk(f) = self {
            f.ensure_columns(need);
        }
    }

    pub(crate) fn reset_bins(&mut self, cap: u32) {
        match self {
            Gpu::Vk(f) => f.reset_bins(cap),
            #[cfg(target_os = "linux")]
            Gpu::Gl(f) => f.reset_bins(cap),
        }
    }

    pub(crate) fn reference_pixels(&mut self, n_dir: u32) -> Vec<u8> {
        match self {
            Gpu::Vk(f) => f.reference_pixels(n_dir),
            #[cfg(target_os = "linux")]
            Gpu::Gl(f) => f.reference_pixels(n_dir),
        }
    }

    /// Draw surface `i` of `t` and wait for it (the compositor reads a buffer once it is committed): the Vulkan
    /// presenter writes the surface's memory itself (imported once per address), the GL presenter draws its own
    /// surface and copies what changed into it. Returns the tiles repaired.
    pub(crate) fn draw(&mut self, t: &mut dyn super::Surfaces, i: usize, rows: &[u128], full: bool, n_dir: u32, copy: &bough_frame::SlotCopy) -> u32 {
        let h = t.size().1 as usize;
        match self {
            Gpu::Vk(f) => {
                let mut bind = |f: &mut vk::Fused, s: usize| {
                    let (p, bpr) = t.pixels(s);
                    // SAFETY: the target's contract: `h` rows of `bpr` bytes at `p`, page aligned, valid until its next
                    // resize (which `App::resized` makes the presenter forget first), untouched by the compositor while
                    // not in use (the ring's policy draws only surfaces not in use).
                    unsafe { f.bind_surface(s, p, bpr * h, bpr) };
                };
                bind(f, i);
                if !full && !copy.is_none() {
                    bind(f, (copy.src & 3) as usize);
                }
                let n = f.draw(i, rows, full, n_dir, copy);
                f.sync();
                n
            }
            #[cfg(target_os = "linux")]
            Gpu::Gl(f) => {
                let n = f.draw(i, rows, full, n_dir, copy);
                let (p, bpr) = t.pixels(i);
                // SAFETY: as above: `h` rows of `bpr >= 4 w` bytes, not in use.
                unsafe { f.copy_out(i, p, bpr, rows, full, copy) };
                n
            }
        }
    }
}
