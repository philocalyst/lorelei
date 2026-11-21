//! Which runtime did the work (Integration Card 3's provenance gate).
//!
//! Counters per thread, so a test reads its own: every construction, mount, settle and frame of the legacy
//! runtime (`rt.rs`, through `bough::raw::App`) on one side, every description, world pass, dispatch and paint
//! of the lowered runtime on the other. A lowered application must leave the legacy side at zero.
use std::cell::Cell;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Counts {
    /// Legacy: `Rt` constructions, mounts, settles (turns and input), frames.
    pub rt_new: u64,
    pub rt_mount: u64,
    pub rt_settle: u64,
    pub rt_frame: u64,
    /// Lowered: world mounts, descriptions (re-descriptions included), handler dispatches, one-record patches,
    /// paint passes.
    pub world_mount: u64,
    pub describe: u64,
    pub dispatch: u64,
    pub patch: u64,
    pub paint: u64,
}
impl Counts {
    pub fn legacy(&self) -> u64 {
        self.rt_new + self.rt_mount + self.rt_settle + self.rt_frame
    }
}

thread_local! {
    static C: Cell<Counts> = const { Cell::new(Counts { rt_new: 0, rt_mount: 0, rt_settle: 0, rt_frame: 0, world_mount: 0, describe: 0, dispatch: 0, patch: 0, paint: 0 }) };
}
#[inline]
pub(crate) fn count(f: impl FnOnce(&mut Counts)) {
    C.with(|c| {
        let mut v = c.get();
        f(&mut v);
        c.set(v);
    })
}
/// This thread's counts so far.
pub fn counts() -> Counts {
    C.with(Cell::get)
}
