//! The damage engine: one place that turns "these things changed" into "repaint exactly these tiles".
//!
//! Standalone (no dependency, no `unsafe`, no generic parameters) so that the flat core
//! (`bough-flat`), a raster (`bough-paint`) and an inspector can all drive and query the same object.
//!
//! A frame is: [`DamageEngine::begin_frame`]; any number of [`DamageEngine::damage`] /
//! [`DamageEngine::damage_move`] / [`DamageEngine::scroll`] calls; one [`DamageEngine::resolve`]; then the
//! executor reads [`DamageEngine::blits`] (apply first, in order) and [`DamageEngine::mask`] (repaint), and a tool
//! reads [`DamageEngine::report`]. Every vector is retained: a warm frame allocates nothing.
//!
//! What the engine knows about the scene is two small sorted lists the driver keeps current:
//!
//! * **opaque covers** ([`OpaqueCover`]): boxes guaranteed to hide everything below them. Damage at paint order
//!   `z` is not marked on a tile wholly inside a cover with `cover.z > z`.
//! * **backdrop-blur layers** ([`BlurLayer`]): boxes that read what is below them through a mip pyramid. Damage
//!   below a layer (culled by a cover or not: the layer may sit between) is carried through the pyramid's exact
//!   tap footprint and re-emitted as damage of the layer's own output at the layer's `z`, which cascades to the
//!   layers above.
//!
//! The pyramid the footprints are exact for is defined here ([`blur_read`], [`blur_write`]) and implemented by
//! `bough_flat::raster`: level `l + 1` texel `i` is `[1 3 3 1] / 8` over level `l` texels `2i-1 ..= 2i+2`; on
//! the way back level `l` texel `2i` is `(t[i-1] + 3 t[i]) / 4` and texel `2i+1` is `(3 t[i] + t[i+1]) / 4`.
//! Texel `j` of level `l` covers pixels `[j << l, (j + 1) << l)` of the window: the pyramid is anchored to the
//! window, not to the layer, so a tile repaired alone equals the same tile of a full repaint.
#![forbid(unsafe_code)]

/// Tiles are 32 x 32 device pixels.
pub const TILE_SHIFT: u32 = 5;
pub const TILE: i32 = 1 << TILE_SHIFT;
/// A super-tile is 8 x 8 tiles (256 x 256 pixels): one bit of the coarse mask.
pub const SUPER_SHIFT: u32 = 3;

mod rect;
mod mask;
mod effect;
mod blur;
mod scroll;
mod chooser;
mod report;
mod engine;

pub use blur::{BlurBlock, BlurLayer, MAX_BLUR_LEVEL, blur_level, blur_read, blur_write};
pub use chooser::{Chooser, ExecutorChoice, GPU_COST, Work, WorkEstimate};
pub use effect::{EffectExtents, OpaqueCover, damage_hull, opaque_core, shadow_reach};
pub use engine::{DamageEngine, mutant};
pub use mask::TileMask;
pub use rect::Rect;
pub use report::DamageReport;
pub use scroll::ScrollBlit;
