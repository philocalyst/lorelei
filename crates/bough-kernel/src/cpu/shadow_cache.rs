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

/// Index the two outside runs, skipping the interval where the axis is separable.
/// Empty or touching intervals describe the entire axis.
#[derive(Clone, Copy)]
struct Axis {
    origin: i64,
    len: usize,
    left: usize,
    right: usize,
}

impl Axis {
    fn new(start: i32, end: i32, reach: i64, margin: i64) -> Option<Self> {
        let (start, end) = (i64::from(start), i64::from(end));
        let origin = (start - margin).div_euclid(256);
        let end_px = (end + margin + 255).div_euclid(256);
        let len = usize::try_from(end_px - origin).ok()?;
        // The CPU corner test uses the midpoint rounded down to Q8. The
        // right boundary can therefore lie one Q8 unit inside `end`.
        let core_end = start + 2 * ((end - start) >> 1) - reach;
        let left =
            ((start + reach - 129).div_euclid(256) + 1 - origin).clamp(0, len as i64) as usize;
        let right =
            ((core_end - 128).div_euclid(256) + 1 - origin).clamp(left as i64, len as i64) as usize;
        Some(Self {
            origin,
            len,
            left,
            right,
        })
    }

    fn samples(self) -> usize {
        self.left + self.len - self.right
    }

    #[inline]
    fn index(self, point: i32) -> Option<usize> {
        let pixel = (i64::from(point >> 8) - self.origin) as usize;
        if pixel < self.left {
            Some(pixel)
        } else if pixel >= self.right && pixel < self.len {
            Some(pixel - self.right + self.left)
        } else {
            None
        }
    }
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
        let blur = i64::from(command.rad.y);
        let reach = i64::from(command.rad.x) + blur + (blur >> 1);
        let margin = blur + (blur >> 1) + 256;
        let x_axis = Axis::new(x, command.b.z, reach, margin)?;
        let y_axis = Axis::new(y, command.b.w, reach, margin)?;
        let size = x_axis.samples().checked_mul(y_axis.samples())?;
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
            x: x_axis,
            y: y_axis,
        })
    }
}

pub(super) struct Coverage<'a> {
    samples: &'a mut [u16],
    x: Axis,
    y: Axis,
}

impl Coverage<'_> {
    pub(super) fn row(&mut self, cy: i32) -> Option<CoverageRow<'_>> {
        let row = self.y.index(cy)?;
        let stride = self.x.samples();
        Some(CoverageRow {
            samples: &mut self.samples[row * stride..(row + 1) * stride],
            x: self.x,
        })
    }
}

pub(super) struct CoverageRow<'a> {
    samples: &'a mut [u16],
    x: Axis,
}

impl CoverageRow<'_> {
    #[inline]
    pub(super) fn sample(&mut self, command: Cmd, cx: i32, cy: i32) -> u16 {
        let Some(x) = self.x.index(cx) else {
            return k::shadow(command, cx, cy) as u16;
        };
        let slot = &mut self.samples[x];
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
