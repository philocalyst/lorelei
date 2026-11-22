//! The work model and the choice of executor.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ExecutorChoice {
    #[default]
    Idle,
    Cpu,
    Gpu,
}

/// What a damage call is made of, for the work model.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Work {
    #[default]
    Flat,
    Shadow,
    Gradient,
}

/// The work a frame's damage comes to.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct WorkEstimate {
    pub dirty_tiles: u32,
    pub affected_records: u32,
    pub blur_blocks: u32,
    /// Texels the dirty blur blocks hold over all levels, in units of 1024 (one tile's worth).
    pub blur_tiles: u32,
    pub shadow_tiles: u32,
    pub grad_tiles: u32,
    pub scroll_blits: u32,
    /// Records, spaces and layers the changed state words fanned out to.
    pub fanout: u32,
}

impl WorkEstimate {
    /// Cost units: a flat tile is 4; shadow, gradient and blur tiles pay their extra passes on top.
    pub fn cost(&self) -> u64 {
        self.dirty_tiles as u64 * 4
            + self.shadow_tiles as u64 * 12
            + self.grad_tiles as u64 * 4
            + self.blur_tiles as u64 * 24
            + self.affected_records as u64
            + self.scroll_blits as u64 * 2
    }
    /// What kinds of work the frame has: the hysteresis key.
    pub fn signature(&self) -> u8 {
        (self.blur_blocks != 0) as u8
            | ((self.shadow_tiles != 0) as u8) << 1
            | ((self.grad_tiles != 0) as u8) << 2
            | ((self.scroll_blits != 0) as u8) << 3
    }
}

/// Frames at or above this cost go to the GPU; a signature that last went there stays until under half of it.
pub const GPU_COST: u64 = 2048;

/// Chooses an executor per frame from the frame's own work, with hysteresis per work signature: a hover in one
/// corner is never dragged to the GPU because a theme switch went there, and a scroll that sits near the
/// threshold does not flap.
#[derive(Clone, Debug, Default)]
pub struct Chooser {
    last: [ExecutorChoice; 16],
    pub gpu_cost: u64,
}

impl Chooser {
    pub fn choose(&mut self, w: &WorkEstimate) -> (ExecutorChoice, &'static str) {
        if w.dirty_tiles == 0 && w.scroll_blits == 0 {
            return (ExecutorChoice::Idle, "nothing damaged");
        }
        let at = if self.gpu_cost == 0 { GPU_COST } else { self.gpu_cost };
        let sig = w.signature() as usize;
        let cost = w.cost();
        let (c, why) = match self.last[sig] {
            ExecutorChoice::Gpu if cost >= at / 2 => (ExecutorChoice::Gpu, "broad work, held on the GPU (hysteresis)"),
            _ if cost >= at => (ExecutorChoice::Gpu, "broad work: cost at or over the GPU threshold"),
            ExecutorChoice::Gpu => (ExecutorChoice::Cpu, "work fell under half the GPU threshold"),
            _ => (ExecutorChoice::Cpu, "sparse work: cost under the GPU threshold"),
        };
        self.last[sig] = c;
        (c, why)
    }
}
