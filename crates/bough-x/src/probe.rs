//! Phase marks for splitting a pass's cost (feature `phase_probe`; nothing without it).
//!
//! A pass calls [`mark`] with a phase number at each boundary; whoever measures installs a hook
//! (`bough::typed` charges the thread's instructions since the previous mark to the phase named). Without the
//! feature a mark is nothing at all: no load, no branch.
#[cfg(feature = "phase_probe")]
thread_local! {
    pub static HOOK: core::cell::Cell<Option<fn(u32)>> = const { core::cell::Cell::new(None) };
}
/// The work since the previous mark was phase `p`.
#[cfg(feature = "phase_probe")]
#[inline]
pub fn mark(p: u32) {
    HOOK.with(|h| {
        if let Some(f) = h.get() {
            f(p)
        }
    })
}
#[cfg(not(feature = "phase_probe"))]
#[inline(always)]
pub fn mark(_: u32) {}

/// Phases of a description pass (`World::redescribe` and what the typed prelude path does inside it).
pub mod ph {
    pub const OUTSIDE: u32 = 0;
    /// The block gathered and compared, the delta of changed words.
    pub const BLOCK: u32 = 1;
    /// Running the author's `view` and converting the element value.
    pub const VIEW: u32 = 2;
    /// The fixed part's layout (words, tree, solve).
    pub const LAYOUT: u32 = 3;
    /// The fixed row compared with its columns.
    pub const ROW: u32 = 4;
    /// Scopes and lists (rows described, compared, laid out).
    pub const LISTS: u32 = 5;
    /// The column table's pointers again.
    pub const REDERIVE: u32 = 6;
    /// Sweeps, the learn loop (relations observed), refocus.
    pub const LEARN: u32 = 7;
    pub const REFOCUS: u32 = 8;
    /// Entering the pass (pass bookkeeping before the view runs).
    pub const ENTER: u32 = 9;
    /// Shaping a text the run store does not hold.
    pub const SHAPE: u32 = 10;
    pub const N: usize = 11;
    pub const NAMES: [&str; N] = [
        "outside",
        "block+delta",
        "view+convert",
        "layout",
        "fixed row",
        "lists",
        "rederive",
        "learn",
        "refocus",
        "enter",
        "shape",
    ];
}
