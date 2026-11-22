//! What a record's effects add to its window, and what an opaque record hides.
use crate::Rect;

/// How far a record's ink reaches past its box: one outer shadow (stack several by taking the hull of their
/// windows) and a blur of the element itself. Pixels; `shadow_blur` and `blur` are radii.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct EffectExtents {
    pub shadow_dx: i32,
    pub shadow_dy: i32,
    pub shadow_blur: i32,
    pub shadow_spread: i32,
    pub blur: i32,
    /// Whether there is a shadow at all (a zero-extent shadow still paints under a translucent box).
    pub shadow: bool,
}

/// The reach of a shadow's falloff past its spread box: `ceil(1.5 * blur)`.
#[inline]
pub fn shadow_reach(blur: i32) -> i32 {
    (blur.max(0) * 3 + 1) / 2
}

impl EffectExtents {
    /// The window a record of box `r` can touch: the box grown by its own blur, joined with the shadow's box
    /// (offset, grown by spread and `1.5 * blur`).
    pub fn window(&self, r: Rect) -> Rect {
        if r.is_empty() {
            return Rect::EMPTY;
        }
        let own = r.expand(self.blur.max(0));
        if !self.shadow {
            return own;
        }
        let grow = self.shadow_spread + shadow_reach(self.shadow_blur);
        let sh = r.shift(self.shadow_dx, self.shadow_dy).expand(grow.max(0));
        own.hull(sh)
    }
}

/// The damage of a record that was in `old_win` cut to `old_clip` and is now in `new_win` cut to `new_clip`.
#[inline]
pub fn damage_hull(old_win: Rect, old_clip: Rect, new_win: Rect, new_clip: Rect) -> (Rect, Rect) {
    (old_win.and(old_clip), new_win.and(new_clip))
}

/// A box that is opaque everywhere inside `core_rect`, painted at order `z`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OpaqueCover {
    pub z: u32,
    pub core_rect: Rect,
}

/// The guaranteed-opaque inner box of an opaque rounded box: inset by the part of the corner radius an
/// axis-aligned box cannot keep (`r * (1 - 1/sqrt 2)`, rounded up, plus the antialiased pixel).
pub fn opaque_core(rect: Rect, radius_px: i32) -> Rect {
    if radius_px <= 0 {
        return rect;
    }
    let r = radius_px.min((rect.x1 - rect.x0) / 2).min((rect.y1 - rect.y0) / 2).max(0);
    // 1 - 1/sqrt(2) < 75/256
    let inset = (r * 75 + 255) / 256 + 1;
    let c = Rect { x0: rect.x0 + inset, y0: rect.y0 + inset, x1: rect.x1 - inset, y1: rect.y1 - inset };
    if c.is_empty() { Rect::EMPTY } else { c }
}
