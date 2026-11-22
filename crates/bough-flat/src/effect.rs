//! A record's effects: two outer shadows, a two-colour gradient, a backdrop blur and a blur of the box itself.
//!
//! An effect is described with every field a [`Val`] ([`Effect`]), so a shadow that grows on hover or a blur
//! that animates in is a binding like any other. It is kept as a row of resolved words; [`EffectRow`] is the
//! one place that knows which word is what.
use super::expr::Val;
use super::raster::Shadow;

/// The fields of an effect row (what a binding of an effect targets).
pub mod ef {
    pub const SH0_COLOR: usize = 0;
    pub const SH0_DX: usize = 1;
    pub const SH0_DY: usize = 2;
    pub const SH0_BLUR: usize = 3;
    pub const SH0_SPREAD: usize = 4;
    pub const SH1_COLOR: usize = 5;
    pub const SH1_DX: usize = 6;
    pub const SH1_DY: usize = 7;
    pub const SH1_BLUR: usize = 8;
    pub const SH1_SPREAD: usize = 9;
    /// A [`super::GradDir`] as a word.
    pub const GRAD_DIR: usize = 10;
    pub const GRAD_TO: usize = 11;
    /// Backdrop blur radius in pixels (0: none).
    pub const BACKDROP: usize = 12;
    /// Blur of the box itself, pixels.
    pub const ELEM_BLUR: usize = 13;
    pub const N: usize = 14;
}

/// Which way a box's gradient runs from its fill to its second colour.
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GradDir {
    /// No gradient: the fill alone.
    #[default]
    None = 0,
    /// The fill at the top, the second colour at the bottom.
    Down = 1,
    /// The fill at the left, the second colour at the right.
    Right = 2,
}
impl GradDir {
    /// The direction a row's word means (a word that is none of them: no gradient).
    #[inline]
    pub fn from_word(w: u32) -> GradDir {
        match w {
            1 => GradDir::Down,
            2 => GradDir::Right,
            _ => GradDir::None,
        }
    }
}
impl From<GradDir> for Val {
    fn from(d: GradDir) -> Val {
        Val::Const(d as u32)
    }
}

/// An effect as described.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effect(pub [Val; ef::N]);
impl Default for Effect {
    fn default() -> Effect {
        Effect([Val::Const(0); ef::N])
    }
}
impl Effect {
    pub fn new() -> Effect {
        Effect::default()
    }
    /// Shadow `k` (0 or 1; 0 is painted first). Lengths in pixels.
    pub fn shadow(mut self, k: usize, color: impl Into<Val>, dx: impl Into<Val>, dy: impl Into<Val>, blur: impl Into<Val>, spread: impl Into<Val>) -> Effect {
        let o = if k == 0 { ef::SH0_COLOR } else { ef::SH1_COLOR };
        self.0[o] = color.into();
        self.0[o + 1] = dx.into();
        self.0[o + 2] = dy.into();
        self.0[o + 3] = blur.into();
        self.0[o + 4] = spread.into();
        self
    }
    /// A linear gradient from the fill to `to`. `dir` is a [`GradDir`], or a binding whose word is one.
    pub fn gradient(mut self, dir: impl Into<Val>, to: impl Into<Val>) -> Effect {
        self.0[ef::GRAD_DIR] = dir.into();
        self.0[ef::GRAD_TO] = to.into();
        self
    }
    /// Blur what is beneath the box (a radius in pixels; 0: none).
    pub fn backdrop_blur(mut self, radius_px: impl Into<Val>) -> Effect {
        self.0[ef::BACKDROP] = radius_px.into();
        self
    }
    /// Blur the box itself.
    pub fn blur(mut self, radius_px: impl Into<Val>) -> Effect {
        self.0[ef::ELEM_BLUR] = radius_px.into();
        self
    }
}

/// A record's effect as it stands: the resolved words of its row, read by name.
#[derive(Clone, Copy, Debug)]
pub struct EffectRow<'a>(pub &'a [u32; ef::N]);
impl EffectRow<'_> {
    /// The two outer shadows, in paint order (one whose colour is transparent is absent: `Shadow::is_some`).
    #[inline]
    pub fn shadows(&self) -> [Shadow; 2] {
        [ef::SH0_COLOR, ef::SH1_COLOR].map(|o| Shadow { color: self.0[o], dx: self.0[o + 1] as i32, dy: self.0[o + 2] as i32, blur: self.0[o + 3] as i32, spread: self.0[o + 4] as i32 })
    }
    #[inline]
    pub fn has_shadow(&self) -> bool {
        self.0[ef::SH0_COLOR] >> 24 != 0 || self.0[ef::SH1_COLOR] >> 24 != 0
    }
    /// The gradient's direction and second colour.
    #[inline]
    pub fn gradient(&self) -> (GradDir, u32) {
        (GradDir::from_word(self.0[ef::GRAD_DIR]), self.0[ef::GRAD_TO])
    }
    /// The backdrop blur's radius in pixels (0: none).
    #[inline]
    pub fn backdrop_blur(&self) -> i32 {
        self.0[ef::BACKDROP] as i32
    }
    /// The blur of the box itself, pixels.
    #[inline]
    pub fn blur(&self) -> i32 {
        self.0[ef::ELEM_BLUR] as i32
    }
}
