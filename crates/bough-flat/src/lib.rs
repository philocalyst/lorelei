//! The flat core: a UI as plain data.
//!
//! The typed-dataflow core (`bough-dataflow`) makes every binding a Rust type: an expression is a
//! type, a row is a tuple of them, a column is one monomorphisation per call site. It is fast where it is
//! admitted and it is paid for in compile time, in specialisation, in raw field offsets and in everything a
//! type cannot say (a flag that turns on at run time, a child that appears). This crate is the same idea with
//! the types taken out:
//!
//! * [`state`]: one block of 32-bit words behind typed, branded handles; a dirty bit per word.
//! * [`expr`]: a binding is a sixteen-byte [`Val`]: a constant, a word, a palette entry, a hover select, an
//!   animation, or an expression of a flat register tape. No type parameters anywhere.
//! * [`store`]: records as parallel arrays ([`FlatStore`]) and a CSR index from each state word to exactly the
//!   bindings that read it.
//! * [`world`]: the driver ([`FlatWorld`]). A turn drains the dirty words, walks their CSR rows, re-evaluates
//!   those bindings, and hands the changed records' windows to the damage engine (`bough-damage`); nothing is
//!   allocated and the work does not depend on how many records exist.
//! * [`effect`]: shadows, a gradient and blurs as one row of bindable words.
//! * [`raster`]: the reference raster and the oracles' other half.
//!
//! There is no `unsafe` in this crate or in the damage engine.
#![forbid(unsafe_code)]
pub mod effect;
pub mod expr;
pub mod raster;
pub mod state;
pub mod store;
pub mod world;

pub use bough_damage::{DamageEngine, DamageReport, ExecutorChoice, Rect};
pub use expr::{ExprBuilder, ExprOp, ExprTape, NO_SITE, Palette, Reg, Val};
pub use raster::Surface;
pub use state::{DepMask, Slot, SlotId, SlotValue, StateBlock, StrSlot, WrongBlock};
pub use effect::{Effect, EffectRow, GradDir};
pub use store::{FlatStore, Kind, Prop, RecId, RecordDesc, SpaceId, TextSrc};
pub use world::{ExtPaint, FlatStats, FlatWorld, FrameInfo, NoExt};
