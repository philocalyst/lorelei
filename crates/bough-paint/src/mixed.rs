//! The mixed model: which executor draws a frame is decided per change, from the change.
//!
//! Both executors repair the same retained state (the columns, the layers' pool and the surface are one
//! copy each, in memory Metal wraps), and they write the same words, so the choice is free to change every
//! frame. The chooser never looks at the scene:
//!
//! * a **broad** change (`FrameStats::broad`: a declared-broad block word moved) goes to the GPU with nothing
//!   computed: the whole frame is a resident job buffer, re-submitted;
//! * otherwise the frame's cost on the CPU kernel is predicted from its damage alone (the weights of the
//!   damaged tiles' entries and the layer work scheduled, both O(damage)) and compared with what a staged
//!   Metal frame costs the frame thread whatever it draws. Both constants are **frame-thread instructions**:
//!   the frame thread is the resource the mixed model protects, and instructions do not move with the
//!   machine's load (a first version compared wall times measured at start-up and chose differently from
//!   run to run on a busy machine).
//!
//! Nothing here is a constant of a scene.
use crate::expr::U;
use crate::metal::Gpu;
use crate::scene::{FrameStats, Scene};
use bough_kernel::cpu::Pool;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ran {
    /// Nothing was owed.
    Idle,
    Cpu,
    Gpu,
}
/// Frame-thread instructions of one unit of tile weight on the CPU kernel (`paint::Cost::weight`: a flat
/// entry of a tile is 1, a blurred shadow 24). From the cost model's own calibration (a flat entry about
/// 0.3 us of one core).
pub const CPU_INSTR_PER_WEIGHT: u64 = 3_000;
/// Frame-thread instructions of a resident Metal frame whatever it draws: the command buffer, and each
/// dispatch encoded in it (the plan's dispatches are the same every frame: seven per rank of layers). Measured
/// (`bench/results/fx/slice7.txt`): about 2.5 K per dispatch.
pub const GPU_FRAME_INSTR: u64 = 70_000;
pub const GPU_DISPATCH_INSTR: u64 = 2_600;

pub struct Mixed {
    pub gpu: Gpu,
    words: usize,
    stride: usize,
    tiles: Vec<u32>,
    /// What the last frame's prediction was (weight units), and its GPU seconds (0 on the CPU), for reports.
    pub last_weight: u64,
    pub last_gpu: f64,
    /// The resident plan's generation the surface last showed.
    era: u32,
}
impl Mixed {
    /// # Safety
    /// As `Gpu::new`: the scene outlives this, and is written only between its calls.
    pub unsafe fn new(scene: &Scene, stride: usize) -> Option<Mixed> {
        let words = stride * scene.fh as usize;
        // SAFETY: the caller's contract is `Gpu::new`'s.
        let gpu = unsafe { Gpu::new(scene, words, 256) }?;
        Some(Mixed { gpu, words, stride, tiles: Vec::new(), last_weight: 0, last_gpu: 0.0, era: 0 })
    }
    /// The surface (the GPU's buffer, which the CPU kernel also draws into).
    pub fn out(&self) -> &[u32] {
        self.gpu.out(self.words)
    }
    /// One frame: update, choose, draw. `force`: `None` lets the chooser decide.
    pub fn frame(&mut self, scene: &mut Scene, u: &U, pool: &Pool, force: Option<Ran>) -> (FrameStats, Ran) {
        // the events are routed to the layers only if the CPU is chosen: the GPU's plan finds them itself
        scene.route_off = true;
        let mut fs = scene.update(u);
        // the block with the scene's derived words: what the frame is drawn under
        let u = &scene.block().clone();
        let mut tiles = std::mem::take(&mut self.tiles);
        scene.damage.tiles(&mut tiles);
        let limit = (GPU_FRAME_INSTR + GPU_DISPATCH_INSTR * self.gpu.resident_dispatches() as u64) / CPU_INSTR_PER_WEIGHT;
        let idle = tiles.is_empty() && !scene.owes_layers();
        let ran = if idle {
            Ran::Idle
        } else if let Some(r) = force {
            r
        } else if fs.broad {
            // a broad change never touches the CPU painter, and nothing is computed to decide so
            Ran::Gpu
        } else {
            // the damage's weight before any repair: the layer tiles that are due and the tiles to shade,
            // summed only until the CPU's side of the comparison is lost (the sum is then not needed)
            let mut w = scene.event_weight(limit);
            for &t in &tiles {
                if w > limit {
                    break;
                }
                w += scene.tile_weight(t) as u64;
            }
            self.last_weight = w;
            if w <= limit { Ran::Cpu } else { Ran::Gpu }
        };
        // the CPU kernel repairs the layers first and lets damage stop where nothing changed (its output
        // damage is complete only then); Metal's resident plan finds what the events reach by itself
        if ran == Ran::Cpu {
            scene.route_events();
            scene.prepare(u);
        }
        if !idle {
            scene.damage.tiles(&mut tiles);
        }
        scene.damage.clear();
        fs.tiles = tiles.len() as u32;
        self.last_gpu = 0.0;
        match ran {
            Ran::Idle => {}
            Ran::Cpu => {
                let (words, stride) = (self.words, self.stride);
                fs.raster = scene.shade(u, &tiles, self.gpu.out_mut(words), stride, pool, 0);
            }
            Ran::Gpu => {
                let (t, era) = self.gpu.resident(scene, u, &tiles, Some(self.era), self.stride);
                (self.last_gpu, self.era) = (t.gpu, era);
                fs.tiles = self.gpu.resident_output_tiles();
            }
        }
        self.tiles = tiles;
        (fs, ran)
    }
}
