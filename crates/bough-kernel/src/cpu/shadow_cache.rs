//! Exact shadow coverage samples for one worker. Edges outside the corner zone use the separable SIMD path.
use crate::k::{self, Cmd};
use std::collections::HashMap;

const MAX_SAMPLES: usize = 1 << 20;
const MAX_SHAPE: usize = 1 << 15;
const EMPTY: u16 = u16::MAX;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Shape {
    width: i32,
    height: i32,
    phase: [i32; 2],
    radius: [i32; 4],
    shift: u32,
}

pub(super) struct ShadowCache {
    enabled: bool,
    shapes: HashMap<Shape, Box<[u16]>>,
    allocated: usize,
}

impl Default for ShadowCache {
    fn default() -> Self {
        Self {
            enabled: std::env::var_os("BOUGH_SHADOW_CACHE_OFF").is_none(),
            shapes: HashMap::new(),
            allocated: 0,
        }
    }
}

impl ShadowCache {
    pub(super) fn coverage(&mut self, command: &Cmd) -> Option<Coverage<'_>> {
        if !self.enabled || command.xs != 0 || command.rad.x == 0 {
            return None;
        }
        let (x, y) = (command.b.x, command.b.y);
        let bw = i64::from(command.rad.y);
        let margin = bw + bw / 2 + 256;
        let lo = [
            (i64::from(x) - margin).div_euclid(256),
            (i64::from(y) - margin).div_euclid(256),
        ];
        let hi = [
            (i64::from(command.b.z) + margin + 255).div_euclid(256),
            (i64::from(command.b.w) + margin + 255).div_euclid(256),
        ];
        let stride = usize::try_from(hi[0] - lo[0]).ok()?;
        let height = usize::try_from(hi[1] - lo[1]).ok()?;
        let size = stride.checked_mul(height)?;
        if size == 0 || size > MAX_SHAPE {
            return None;
        }
        let key = Shape {
            width: command.b.z.checked_sub(x)?,
            height: command.b.w.checked_sub(y)?,
            phase: [x.rem_euclid(256), y.rem_euclid(256)],
            radius: [command.rad.x, command.rad.y, command.rad.z, command.rad.w],
            shift: command.par.y,
        };
        use std::collections::hash_map::Entry;
        let samples = match self.shapes.entry(key) {
            Entry::Occupied(cell) => cell.into_mut(),
            Entry::Vacant(cell) if self.allocated + size <= MAX_SAMPLES => {
                self.allocated += size;
                cell.insert(vec![EMPTY; size].into_boxed_slice())
            }
            Entry::Vacant(_) => return None,
        };
        Some(Coverage {
            samples,
            origin: [i32::try_from(lo[0]).ok()?, i32::try_from(lo[1]).ok()?],
            stride,
            height,
        })
    }
}

pub(super) struct Coverage<'a> {
    samples: &'a mut [u16],
    origin: [i32; 2],
    stride: usize,
    height: usize,
}

impl Coverage<'_> {
    #[inline]
    pub(super) fn sample(&mut self, command: Cmd, cx: i32, cy: i32) -> u16 {
        let x = ((cx >> 8) - self.origin[0]) as usize;
        let y = ((cy >> 8) - self.origin[1]) as usize;
        if x >= self.stride || y >= self.height {
            return k::shadow(command, cx, cy) as u16;
        }
        let slot = &mut self.samples[y * self.stride + x];
        if *slot == EMPTY {
            *slot = k::shadow(command, cx, cy) as u16;
        }
        if cfg!(feature = "mutant_shadow_cache") {
            *slot / 2
        } else {
            *slot
        }
    }
}
