//! The fused presenter's kernels in wgsl-rs (`fused`): one Rust source, compiled by build.rs to WGSL, validated by
//! naga, and written as SPIR-V (Vulkan) and GLSL 4.30 (OpenGL). Nothing parses or compiles a shader at run time but
//! the GL driver's own GLSL compiler.
pub mod fused;
