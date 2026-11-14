//! The fused presenter beyond Metal: bough-metal's binned compute repair (`fs_copy`, `fs_bin`, `fs_tile`) on Vulkan and
//! OpenGL, from one kernel source (`shaders::fused`, wgsl-rs) compiled at build time to SPIR-V and GLSL.
pub mod frame;
pub mod shaders;

/// The kernels as SPIR-V (Vulkan), per entry point.
pub mod spirv {
    pub const FS_COPY: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/fs_copy.spv"));
    pub const FS_BIN: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/fs_bin.spv"));
    pub const FS_TILE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/fs_tile.spv"));
}

/// The kernels as GLSL 4.30 core (OpenGL), per entry point: the one-buffer form (build.rs).
pub mod glsl {
    pub const FS_COPY: &str = include_str!(concat!(env!("OUT_DIR"), "/fs_copy.comp"));
    pub const FS_BIN: &str = include_str!(concat!(env!("OUT_DIR"), "/fs_bin.comp"));
    pub const FS_TILE: &str = include_str!(concat!(env!("OUT_DIR"), "/fs_tile.comp"));
}

/// The Vulkan presenter (Linux; macOS through MoltenVK).
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod vk;

/// The OpenGL 4.3 presenter (Linux, EGL).
#[cfg(target_os = "linux")]
pub mod gl;
