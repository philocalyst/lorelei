//! Rigid scrolls as blits.
use crate::Rect;

/// Move the pixels of `clip_rect` by (`dx`, `dy`): the pixel at `p` becomes what was at `p - (dx, dy)`, for
/// every `p` whose source is also inside `clip_rect`. Apply before repainting the mask.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScrollBlit {
    pub clip_rect: Rect,
    pub dx: i32,
    pub dy: i32,
}

impl ScrollBlit {
    /// The part of the clip whose pixels the blit supplies.
    pub fn kept(&self) -> Rect {
        self.clip_rect.and(self.clip_rect.shift(self.dx, self.dy))
    }
}
