//! The mixed model: which executor draws a frame is decided per change, from the change.
//!
//! Both executors repair the same retained state (the columns, the layers' pool and the surface are one
//! copy each, in memory Metal wraps), and they write the same words, so the choice is free to change every
//! frame. The chooser never looks at the scene:
//!
//! * a **broad** change (`FrameStats::broad`: a declared-broad block word moved) goes to the GPU with nothing
//!   computed: the whole frame is a resident job buffer, re-submitted;
//! * otherwise the frame's CPU cost is predicted from its damage alone (the weights of the damaged tiles'
//!   entries and the layer work scheduled, both O(damage)) times one device constant, and compared with the
//!   GPU's round trip, the other device constant. Both are measured once, at start-up (`calibrate`).
//!
//! Nothing here is a constant of a scene.
use crate::expr::U;
use crate::metal::Gpu;
use crate::scene::{FrameStats, Scene};
use bough_kernel::cpu::Pool;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ran {
    /// Nothing was owed.
    Idle,
    Cpu,
    Gpu,
}
pub struct Mixed {
    pub gpu: Gpu,
    words: usize,
    stride: usize,
    /// Device constant 1: nanoseconds of one CPU core per unit of tile weight.
    pub cpu_ns_per_weight: f64,
    /// Device constant 2: nanoseconds of a staged GPU frame that does nothing (the command buffer's floor).
    pub gpu_floor_ns: f64,
    tiles: Vec<u32>,
    /// What the last frame's prediction was (weight units), and its GPU seconds (0 on the CPU), for reports.
    pub last_weight: u64,
    pub last_gpu: f64,
}
impl Mixed {
    /// # Safety
    /// As `Gpu::new`: the scene outlives this, and is written only between its calls.
    pub unsafe fn new(scene: &Scene, stride: usize) -> Option<Mixed> {
        let words = stride * scene.fh as usize;
        // SAFETY: the caller's contract is `Gpu::new`'s.
        let gpu = unsafe { Gpu::new(scene, words, 256) }?;
        // defaults for an M-class machine until `calibrate` runs: 15 ns a weight unit, 250 us a round trip
        Some(Mixed { gpu, words, stride, cpu_ns_per_weight: 15.0, gpu_floor_ns: 250_000.0, tiles: Vec::new(), last_weight: 0, last_gpu: 0.0 })
    }
    /// The surface (the GPU's buffer, which the CPU kernel also draws into).
    pub fn out(&self) -> &[u32] {
        self.gpu.out(self.words)
    }
    /// Measure the two device constants on this machine, after the first frame has been drawn: the GPU's
    /// floor as the fastest of a few frames that dispatch nothing, the CPU's rate from shading a sample of
    /// the scene's tiles into scratch. Wall-clock, so it wants a quiet moment; it changes speed, never pixels.
    pub fn calibrate(&mut self, scene: &Scene, u: &U, pool: &Pool) {
        let mut floor = f64::MAX;
        for _ in 0..5 {
            floor = floor.min(self.gpu.staged(scene, u, &[], self.stride).wall * 1e9);
        }
        let nt = (scene.tx * scene.ty) as u32;
        let sample: Vec<u32> = (0..nt).step_by((nt as usize / 24).max(1)).collect();
        let weight: u64 = sample.iter().map(|&t| scene.tile_weight(t) as u64).sum();
        let mut scratch = vec![0u32; self.words];
        let mut best = f64::MAX;
        for _ in 0..3 {
            let t = Instant::now();
            scene.shade(u, &sample, &mut scratch, self.stride, pool, 0);
            best = best.min(t.elapsed().as_secs_f64() * 1e9);
        }
        if weight > 0 {
            (self.gpu_floor_ns, self.cpu_ns_per_weight) = (floor, best / weight as f64);
        }
    }
    /// One frame: update, choose, draw. `force`: `None` lets the chooser decide.
    pub fn frame(&mut self, scene: &mut Scene, u: &U, pool: &Pool, force: Option<Ran>) -> (FrameStats, Ran) {
        let mut fs = scene.update(u);
        let mut tiles = std::mem::take(&mut self.tiles);
        scene.damage.tiles(&mut tiles);
        scene.damage.clear();
        fs.tiles = tiles.len() as u32;
        let idle = tiles.is_empty() && !scene.plan.full && scene.plan.work.is_empty();
        let ran = if idle {
            Ran::Idle
        } else if let Some(r) = force {
            r
        } else if fs.broad {
            // a broad change never touches the CPU painter, and nothing is computed to decide so
            Ran::Gpu
        } else {
            // the damage's weight: the tiles to shade, the prefix tiles (a tile program each) and the
            // pyramid pixels (16 taps each, about a flat entry per 16 pixels)
            let w: u64 = tiles.iter().map(|&t| scene.tile_weight(t) as u64).sum::<u64>() + scene.plan.work.iter().flat_map(|w| w.prefix.iter()).map(|&t| scene.tile_weight(t) as u64).sum::<u64>() + scene.plan.px / 16;
            self.last_weight = w;
            if (w as f64) * self.cpu_ns_per_weight <= self.gpu_floor_ns { Ran::Cpu } else { Ran::Gpu }
        };
        self.last_gpu = 0.0;
        match ran {
            Ran::Idle => {}
            Ran::Cpu => {
                scene.prepare(u);
                let (words, stride) = (self.words, self.stride);
                fs.raster = scene.shade(u, &tiles, self.gpu.out_mut(words), stride, pool, 0);
            }
            Ran::Gpu => {
                self.last_gpu = self.gpu.staged(scene, u, &tiles, self.stride).gpu;
            }
        }
        self.tiles = tiles;
        (fs, ran)
    }
}
