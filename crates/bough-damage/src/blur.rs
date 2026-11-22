//! Backdrop-blur layers and the exact footprints of their pyramid.
use crate::Rect;

/// A backdrop-blur layer: at order `z` it replaces what is below it inside `rect` with a blur of it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BlurLayer {
    pub id: u32,
    pub z: u32,
    /// Where the layer writes (already cut to its clip).
    pub rect: Rect,
    /// The author's radius in pixels (informational; the footprint is `max_level`'s).
    pub radius: i32,
    /// The coarsest pyramid level, 0 ..= [`MAX_BLUR_LEVEL`] (0: no blur at all).
    pub max_level: u8,
}

pub const MAX_BLUR_LEVEL: u8 = 6;

/// The pyramid depth that realises a blur of `radius` pixels.
pub fn blur_level(radius: i32) -> u8 {
    if radius <= 0 {
        return 0;
    }
    ((32 - (radius as u32).leading_zeros()) as u8).min(MAX_BLUR_LEVEL)
}

/// The inclusive backdrop interval output pixels `lo ..= hi` read through a pyramid of `levels`.
pub fn blur_read(mut lo: i32, mut hi: i32, levels: u8) -> (i32, i32) {
    for _ in 0..levels {
        // level l texel j reads level l+1 texel (j-1)>>1 (j even: i-1; odd: i) up to (j+1)>>1
        lo = (lo - 1) >> 1;
        hi = (hi + 1) >> 1;
    }
    for _ in 0..levels {
        // level l+1 texel i reads level l texels 2i-1 ..= 2i+2
        lo = 2 * lo - 1;
        hi = 2 * hi + 2;
    }
    (lo, hi)
}

/// The inclusive output interval that changes when backdrop pixels `lo ..= hi` change. `blocks`, if given,
/// receives the dirty texel interval of each level `1 ..= levels` on the way down.
pub fn blur_write(mut lo: i32, mut hi: i32, levels: u8, mut blocks: Option<&mut [(i32, i32); MAX_BLUR_LEVEL as usize + 1]>) -> (i32, i32) {
    if let Some(b) = blocks.as_deref_mut() {
        b[0] = (lo, hi);
    }
    for l in 0..levels {
        // level l+1 texel i reads 2i-1 ..= 2i+2: dirty iff 2i-1 <= hi and 2i+2 >= lo
        lo = (lo - 1) >> 1;
        hi = (hi + 1) >> 1;
        if let Some(b) = blocks.as_deref_mut() {
            b[l as usize + 1] = (lo, hi);
        }
    }
    for _ in 0..levels {
        // even texel 2i reads {i-1, i}; odd texel 2i+1 reads {i, i+1}
        lo = 2 * lo - 1;
        hi = 2 * hi + 2;
    }
    (lo, hi)
}

impl BlurLayer {
    /// The backdrop rect the layer's output is a function of (not cut to the window).
    pub fn expanded_read_rect(&self) -> Rect {
        if self.rect.is_empty() {
            return Rect::EMPTY;
        }
        let (x0, x1) = blur_read(self.rect.x0, self.rect.x1 - 1, self.max_level);
        let (y0, y1) = blur_read(self.rect.y0, self.rect.y1 - 1, self.max_level);
        Rect { x0, y0, x1: x1 + 1, y1: y1 + 1 }
    }
    /// The part of the layer's output that changes when the backdrop changed inside `changed`.
    pub fn output_of(&self, changed: Rect) -> Rect {
        if changed.is_empty() {
            return Rect::EMPTY;
        }
        let (x0, x1) = blur_write(changed.x0, changed.x1 - 1, self.max_level, None);
        let (y0, y1) = blur_write(changed.y0, changed.y1 - 1, self.max_level, None);
        Rect { x0, y0, x1: x1 + 1, y1: y1 + 1 }.and(self.rect)
    }
}

/// The dirty texel block of one pyramid level of one layer (inclusive texel coordinates of that level).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BlurBlock {
    pub layer: u32,
    pub level: u8,
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}
