//! The façade on the flat core (`bough_x::flat`): no type-level dataflow, no specialisation, no field offsets.
//!
//! Two ways in, one engine under both:
//!
//! * [`FlatApp`]: any `bough::prelude` view, unchanged. The view is described as data by the lowering every
//!   element already has ([`crate::lower`]), and the description is kept in a [`bough_x::flat::FlatWorld`]:
//!   a turn describes again and *patches* the records that differ, a scrolled port is a space whose offset is
//!   a state word (so a scroll is a blit and its seams), and the damage engine decides what is repainted.
//!   Generic only in its outermost shell (it holds the author's `V`); everything it calls is not.
//! * [`ui`]: a small non-generic builder for descriptions written against state words directly. Its bindings
//!   are the flat core's own (`Val`): a turn costs what changed, whatever the size of the description.
//!
//! Contract of this path, stated where the numbers are used (`examples/flat_cost.rs`): the reference raster on
//! the calling thread, one surface, device pixels; shaped text and images are painted by the lowered runtime's
//! canvas through `ExtPaint`; `ui` text is the core's built-in 5 x 7 face. Backdrop blur, shadows, gradients
//! and transitions exist in `ui` and in the core; the prelude's elements do not describe them yet, so
//! [`FlatApp`] never sees one.
pub mod app;
pub mod bridge;
pub mod ui;

pub use app::FlatApp;
pub use bough_x::flat;
