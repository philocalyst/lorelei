//! The real comparison apps on a headless host, for the inspector. `lab` has no inspector dependency (the
//! zero-cost control); `inspect` (on by default) registers their typed state and adapts them to `Program`.
#[cfg(feature = "inspect")]
pub mod inspect;
pub mod lab;
#[cfg(feature = "inspect")]
pub mod overlay_host;

#[cfg(feature = "inspect")]
pub use inspect::Fields;
pub use lab::{Event, Lab, Scenario, Shot};
#[cfg(feature = "inspect")]
pub use overlay_host::{Overlay, WithOverlay};
