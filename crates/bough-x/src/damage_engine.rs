//! The damage engine: one place that turns "these things changed" into "repaint exactly these tiles".
//!
//! Standalone (no dependency on the rest of this crate, no `unsafe`, no generic parameters) so that the flat core
//! ([`crate::flat`]), a raster (`bough-paint`) and an inspector can all drive and query the same object.
//!
//! A frame is: [`DamageEngine::begin_frame`]; any number of [`DamageEngine::damage`] /
//! [`DamageEngine::damage_move`] / [`DamageEngine::scroll`] calls; one [`DamageEngine::resolve`]; then the
//! executor reads [`DamageEngine::blits`] (apply first, in order) and [`DamageEngine::mask`] (repaint), and a tool
//! reads [`DamageEngine::report`]. Every vector is retained: a warm frame allocates nothing.
//!
//! What the engine knows about the scene is two small sorted lists the driver keeps current:
//!
//! * **opaque covers** ([`OpaqueCover`]): boxes guaranteed to hide everything below them. Damage at paint order
//!   `z` is not marked on a tile wholly inside a cover with `cover.z > z`.
//! * **backdrop-blur layers** ([`BlurLayer`]): boxes that read what is below them through a mip pyramid. Damage
//!   below a layer (culled by a cover or not: the layer may sit between) is carried through the pyramid's exact
//!   tap footprint and re-emitted as damage of the layer's own output at the layer's `z`, which cascades to the
//!   layers above.
//!
//! The pyramid the footprints are exact for is defined here ([`blur_read`], [`blur_write`]) and implemented by
//! [`crate::flat::raster`]: level `l + 1` texel `i` is `[1 3 3 1] / 8` over level `l` texels `2i-1 ..= 2i+2`; on
//! the way back level `l` texel `2i` is `(t[i-1] + 3 t[i]) / 4` and texel `2i+1` is `(3 t[i] + t[i+1]) / 4`.
//! Texel `j` of level `l` covers pixels `[j << l, (j + 1) << l)` of the window: the pyramid is anchored to the
//! window, not to the layer, so a tile repaired alone equals the same tile of a full repaint.

/// Tiles are 32 x 32 device pixels.
pub const TILE_SHIFT: u32 = 5;
pub const TILE: i32 = 1 << TILE_SHIFT;
/// A super-tile is 8 x 8 tiles (256 x 256 pixels): one bit of the coarse mask.
pub const SUPER_SHIFT: u32 = 3;

/// A half-open rectangle in device pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Rect {
    pub const EMPTY: Rect = Rect { x0: 0, y0: 0, x1: 0, y1: 0 };
    /// Larger than any window, small enough that shifting it cannot overflow.
    pub const EVERYWHERE: Rect = Rect { x0: -(1 << 28), y0: -(1 << 28), x1: 1 << 28, y1: 1 << 28 };
    #[inline]
    pub const fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Rect {
        Rect { x0, y0, x1, y1 }
    }
    #[inline]
    pub const fn xywh(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x0: x, y0: y, x1: x + w, y1: y + h }
    }
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }
    #[inline]
    pub fn and(self, o: Rect) -> Rect {
        let r = Rect { x0: self.x0.max(o.x0), y0: self.y0.max(o.y0), x1: self.x1.min(o.x1), y1: self.y1.min(o.y1) };
        if r.is_empty() { Rect::EMPTY } else { r }
    }
    /// The bounding box of both (an empty rect is the identity).
    #[inline]
    pub fn hull(self, o: Rect) -> Rect {
        if self.is_empty() {
            return o;
        }
        if o.is_empty() {
            return self;
        }
        Rect { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }
    #[inline]
    pub fn hits(self, o: Rect) -> bool {
        self.x0 < o.x1 && o.x0 < self.x1 && self.y0 < o.y1 && o.y0 < self.y1 && !self.is_empty() && !o.is_empty()
    }
    #[inline]
    pub fn contains(self, o: Rect) -> bool {
        o.is_empty() || (self.x0 <= o.x0 && self.y0 <= o.y0 && self.x1 >= o.x1 && self.y1 >= o.y1)
    }
    #[inline]
    pub fn has(self, x: i32, y: i32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }
    #[inline]
    pub fn shift(self, dx: i32, dy: i32) -> Rect {
        if self.is_empty() {
            return Rect::EMPTY;
        }
        Rect { x0: self.x0 + dx, y0: self.y0 + dy, x1: self.x1 + dx, y1: self.y1 + dy }
    }
    #[inline]
    pub fn expand(self, n: i32) -> Rect {
        self.expand4(n, n, n, n)
    }
    #[inline]
    pub fn expand4(self, l: i32, t: i32, r: i32, b: i32) -> Rect {
        if self.is_empty() {
            return Rect::EMPTY;
        }
        Rect { x0: self.x0 - l, y0: self.y0 - t, x1: self.x1 + r, y1: self.y1 + b }
    }
    #[inline]
    pub fn area(self) -> i64 {
        if self.is_empty() { 0 } else { (self.x1 - self.x0) as i64 * (self.y1 - self.y0) as i64 }
    }
}

// ---- the tile mask -----------------------------------------------------------------------------------------

/// A window's dirty tiles: one bit per 32 x 32 tile in 64-bit row words, and one bit per 256 x 256 super-tile
/// saying "some tile of this super-tile is set". Marking is O(1) per row word touched; iteration and clearing
/// visit only super-tiles that have a bit; the count is kept, so the repaired fraction is a division.
#[derive(Clone, Debug, Default)]
pub struct TileMask {
    w: i32,
    h: i32,
    tw: u32,
    th: u32,
    /// Words per tile row.
    wpr: usize,
    bits: Vec<u64>,
    sw: u32,
    sh: u32,
    /// Words per super-tile row.
    cpr: usize,
    coarse: Vec<u64>,
    count: u32,
}

#[inline]
fn range_mask(lo: u32, hi: u32) -> u64 {
    // bits lo..=hi of a word, 0 <= lo <= hi <= 63
    (u64::MAX >> (63 - (hi - lo))) << lo
}

impl TileMask {
    pub fn new(w: i32, h: i32) -> TileMask {
        let mut m = TileMask::default();
        m.resize(w, h);
        m
    }
    /// Take the size of a window of `w` x `h` pixels; every bit is cleared.
    pub fn resize(&mut self, w: i32, h: i32) {
        let (w, h) = (w.max(0), h.max(0));
        self.w = w;
        self.h = h;
        self.tw = ((w + TILE - 1) >> TILE_SHIFT) as u32;
        self.th = ((h + TILE - 1) >> TILE_SHIFT) as u32;
        self.wpr = (self.tw as usize).div_ceil(64);
        self.sw = self.tw.div_ceil(1 << SUPER_SHIFT);
        self.sh = self.th.div_ceil(1 << SUPER_SHIFT);
        self.cpr = (self.sw as usize).div_ceil(64);
        self.bits.clear();
        self.bits.resize(self.wpr * self.th as usize, 0);
        self.coarse.clear();
        self.coarse.resize(self.cpr * self.sh as usize, 0);
        self.count = 0;
    }
    pub fn size(&self) -> (i32, i32) {
        (self.w, self.h)
    }
    pub fn tiles_wide(&self) -> u32 {
        self.tw
    }
    pub fn tiles_high(&self) -> u32 {
        self.th
    }
    /// Tiles set.
    #[inline]
    pub fn count(&self) -> u32 {
        self.count
    }
    #[inline]
    pub fn total(&self) -> u32 {
        self.tw * self.th
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// The fraction of the window's tiles that are set, 0.0 ..= 1.0.
    pub fn fraction(&self) -> f32 {
        if self.total() == 0 { 0.0 } else { self.count as f32 / self.total() as f32 }
    }
    #[inline]
    pub fn test(&self, tx: u32, ty: u32) -> bool {
        tx < self.tw && ty < self.th && self.bits[ty as usize * self.wpr + (tx >> 6) as usize] >> (tx & 63) & 1 != 0
    }
    /// The pixel rect of tile (tx, ty), cut to the window.
    #[inline]
    pub fn tile_rect(&self, tx: u32, ty: u32) -> Rect {
        Rect {
            x0: (tx as i32) << TILE_SHIFT,
            y0: (ty as i32) << TILE_SHIFT,
            x1: (((tx + 1) as i32) << TILE_SHIFT).min(self.w),
            y1: (((ty + 1) as i32) << TILE_SHIFT).min(self.h),
        }
    }
    /// The inclusive tile range a pixel rect touches, or `None` if it misses the window.
    #[inline]
    pub fn tile_range(&self, r: Rect) -> Option<(u32, u32, u32, u32)> {
        let r = r.and(Rect { x0: 0, y0: 0, x1: self.w, y1: self.h });
        if r.is_empty() {
            return None;
        }
        Some((
            (r.x0 >> TILE_SHIFT) as u32,
            (r.y0 >> TILE_SHIFT) as u32,
            ((r.x1 - 1) >> TILE_SHIFT) as u32,
            ((r.y1 - 1) >> TILE_SHIFT) as u32,
        ))
    }
    /// Set one tile. Returns whether it was clear.
    #[inline]
    pub fn mark_tile(&mut self, tx: u32, ty: u32) -> bool {
        if tx >= self.tw || ty >= self.th {
            return false;
        }
        let w = &mut self.bits[ty as usize * self.wpr + (tx >> 6) as usize];
        let b = 1u64 << (tx & 63);
        if *w & b != 0 {
            return false;
        }
        *w |= b;
        self.count += 1;
        let (sx, sy) = (tx >> SUPER_SHIFT, ty >> SUPER_SHIFT);
        self.coarse[sy as usize * self.cpr + (sx >> 6) as usize] |= 1 << (sx & 63);
        true
    }
    /// Set every tile a pixel rect touches. Returns how many were clear.
    pub fn mark_rect(&mut self, r: Rect) -> u32 {
        let Some((tx0, ty0, tx1, ty1)) = self.tile_range(r) else {
            return 0;
        };
        let mut new = 0;
        for ty in ty0..=ty1 {
            let row = ty as usize * self.wpr;
            for wi in (tx0 >> 6)..=(tx1 >> 6) {
                let lo = if wi == tx0 >> 6 { tx0 & 63 } else { 0 };
                let hi = if wi == tx1 >> 6 { tx1 & 63 } else { 63 };
                let m = range_mask(lo, hi);
                let w = &mut self.bits[row + wi as usize];
                new += (m & !*w).count_ones();
                *w |= m;
            }
        }
        self.count += new;
        let (sx0, sx1) = (tx0 >> SUPER_SHIFT, tx1 >> SUPER_SHIFT);
        for sy in (ty0 >> SUPER_SHIFT)..=(ty1 >> SUPER_SHIFT) {
            let row = sy as usize * self.cpr;
            for wi in (sx0 >> 6)..=(sx1 >> 6) {
                let lo = if wi == sx0 >> 6 { sx0 & 63 } else { 0 };
                let hi = if wi == sx1 >> 6 { sx1 & 63 } else { 63 };
                self.coarse[row + wi as usize] |= range_mask(lo, hi);
            }
        }
        new
    }
    pub fn mark_all(&mut self) {
        self.mark_rect(Rect { x0: 0, y0: 0, x1: self.w, y1: self.h });
    }
    /// Set every tile `o` has set (same window size).
    pub fn or(&mut self, o: &TileMask) {
        assert!(self.w == o.w && self.h == o.h, "two masks of one window");
        if o.count == 0 {
            return;
        }
        let mut n = 0;
        for (a, b) in self.bits.iter_mut().zip(&o.bits) {
            *a |= *b;
            n += a.count_ones();
        }
        self.count = n;
        for (a, b) in self.coarse.iter_mut().zip(&o.coarse) {
            *a |= *b;
        }
    }
    /// Clear every bit, visiting only super-tiles that have one.
    pub fn clear(&mut self) {
        if self.count == 0 && self.coarse.iter().all(|w| *w == 0) {
            return;
        }
        for sy in 0..self.sh {
            for cw in 0..self.cpr {
                let mut c = std::mem::take(&mut self.coarse[sy as usize * self.cpr + cw]);
                while c != 0 {
                    let sx = (cw as u32) << 6 | c.trailing_zeros();
                    c &= c - 1;
                    let tx0 = sx << SUPER_SHIFT;
                    let tx1 = (tx0 + (1 << SUPER_SHIFT) - 1).min(self.tw - 1);
                    for ty in (sy << SUPER_SHIFT)..((sy + 1) << SUPER_SHIFT).min(self.th) {
                        // a super-tile is 8 tiles wide and 8 divides 64: it never straddles a word
                        self.bits[ty as usize * self.wpr + (tx0 >> 6) as usize] &= !range_mask(tx0 & 63, tx1 & 63);
                    }
                }
            }
        }
        self.count = 0;
    }
    /// Every set tile, row by row, left to right.
    pub fn each_tile(&self, f: &mut dyn FnMut(u32, u32)) {
        self.each_run(&mut |ty, tx0, tx1| {
            for tx in tx0..=tx1 {
                f(tx, ty)
            }
        });
    }
    /// Every maximal horizontal run of set tiles as a pixel rect cut to the window: what an executor repaints.
    pub fn each_span(&self, f: &mut dyn FnMut(Rect)) {
        self.each_run(&mut |ty, tx0, tx1| {
            f(Rect {
                x0: (tx0 as i32) << TILE_SHIFT,
                y0: (ty as i32) << TILE_SHIFT,
                x1: (((tx1 + 1) as i32) << TILE_SHIFT).min(self.w),
                y1: (((ty + 1) as i32) << TILE_SHIFT).min(self.h),
            })
        });
    }
    /// Runs `(tile row, first tile, last tile)`. Rows of super-tiles with no bit are never entered.
    fn each_run(&self, f: &mut dyn FnMut(u32, u32, u32)) {
        if self.count == 0 {
            return;
        }
        for sy in 0..self.sh {
            let crow = &self.coarse[sy as usize * self.cpr..(sy as usize + 1) * self.cpr];
            if crow.iter().all(|w| *w == 0) {
                continue;
            }
            for ty in (sy << SUPER_SHIFT)..((sy + 1) << SUPER_SHIFT).min(self.th) {
                let row = &self.bits[ty as usize * self.wpr..(ty as usize + 1) * self.wpr];
                let mut run: Option<(u32, u32)> = None;
                for (wi, word) in row.iter().enumerate() {
                    let mut w = *word;
                    while w != 0 {
                        let lo = w.trailing_zeros();
                        let len = (w >> lo).trailing_ones();
                        let (a, b) = ((wi as u32) << 6 | lo, ((wi as u32) << 6 | lo) + len - 1);
                        w = if lo + len >= 64 { 0 } else { w & !range_mask(lo, lo + len - 1) };
                        run = match run {
                            Some((s, e)) if e + 1 == a => Some((s, b)),
                            Some((s, e)) => {
                                f(ty, s, e);
                                Some((a, b))
                            }
                            None => Some((a, b)),
                        };
                    }
                }
                if let Some((s, e)) = run {
                    f(ty, s, e);
                }
            }
        }
    }
}

// ---- effect hulls ------------------------------------------------------------------------------------------

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

// ---- covers and blur layers --------------------------------------------------------------------------------

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

// ---- scroll ------------------------------------------------------------------------------------------------

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

// ---- the chooser -------------------------------------------------------------------------------------------

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

// ---- the report --------------------------------------------------------------------------------------------

/// A frame's damage as data, for an inspector or the `damage_map` tool.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DamageReport {
    pub width: i32,
    pub height: i32,
    pub tile: i32,
    /// Dirty tiles as (column, row).
    pub tiles: Vec<(u16, u16)>,
    pub dirty_tiles: u32,
    pub total_tiles: u32,
    pub repair_percent: f32,
    /// Distinct tiles some damage wanted and no damage got: hidden under an opaque cover.
    pub occluded_tiles_skipped: u32,
    /// (damage, tile) pairs skipped.
    pub occlusion_skips: u32,
    pub blur_blocks: Vec<BlurBlock>,
    /// Blur propagations a gate halted (the backdrop repainted to the same pixels).
    pub blur_gated: u32,
    pub scroll_blits: Vec<ScrollBlit>,
    pub covers: u32,
    pub layers: u32,
    pub work: WorkEstimate,
    pub cost: u64,
    pub choice: ExecutorChoice,
    pub reason: &'static str,
}

impl DamageReport {
    /// The report as one line of JSON (no dependency: the fields are numbers and fixed strings).
    pub fn to_json(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let _ = write!(
            s,
            "{{\"width\":{},\"height\":{},\"tile\":{},\"dirty_tiles\":{},\"total_tiles\":{},\"repair_percent\":{:.3},\"occluded_tiles_skipped\":{},\"occlusion_skips\":{},\"blur_gated\":{},\"covers\":{},\"layers\":{},\"fanout\":{},\"affected_records\":{},\"shadow_tiles\":{},\"grad_tiles\":{},\"blur_tiles\":{},\"cost\":{},\"choice\":\"{:?}\",\"reason\":\"{}\",\"tiles\":[",
            self.width, self.height, self.tile, self.dirty_tiles, self.total_tiles, self.repair_percent,
            self.occluded_tiles_skipped, self.occlusion_skips, self.blur_gated, self.covers, self.layers,
            self.work.fanout, self.work.affected_records, self.work.shadow_tiles, self.work.grad_tiles,
            self.work.blur_tiles, self.cost, self.choice, self.reason
        );
        for (i, (x, y)) in self.tiles.iter().enumerate() {
            let _ = write!(s, "{}[{x},{y}]", if i == 0 { "" } else { "," });
        }
        s.push_str("],\"blur_blocks\":[");
        for (i, b) in self.blur_blocks.iter().enumerate() {
            let _ = write!(s, "{}{{\"layer\":{},\"level\":{},\"x0\":{},\"y0\":{},\"x1\":{},\"y1\":{}}}", if i == 0 { "" } else { "," }, b.layer, b.level, b.x0, b.y0, b.x1, b.y1);
        }
        s.push_str("],\"scroll_blits\":[");
        for (i, b) in self.scroll_blits.iter().enumerate() {
            let c = b.clip_rect;
            let _ = write!(s, "{}{{\"clip\":[{},{},{},{}],\"dx\":{},\"dy\":{}}}", if i == 0 { "" } else { "," }, c.x0, c.y0, c.x1, c.y1, b.dx, b.dy);
        }
        s.push_str("]}");
        s
    }
}

// ---- the engine --------------------------------------------------------------------------------------------

/// Negative controls: each bit breaks one rule, and the oracle that rests on the rule must fail with it
/// (`tests/flat_mutants.rs`). Compiled only with the `flat_mutants` feature.
pub mod mutant {
    /// A cover culls damage whatever its paint order (the `cover.z > z` test is dropped).
    pub const OCCLUSION_ANY_Z: u32 = 1 << 0;
    /// A record that moved is damaged only where it is now.
    pub const NO_OLD_WIN: u32 = 1 << 1;
    /// Damage under a blur layer reaches the layer undilated (the pyramid footprint is dropped).
    pub const NO_BLUR_DILATE: u32 = 1 << 2;
    /// A scroll blit's uncovered edge strip is not marked.
    pub const NO_SCROLL_STRIP: u32 = 1 << 3;
    /// (flat core) the last target of every dependency list is skipped.
    pub const CSR_DROP_LAST: u32 = 1 << 4;
    /// (flat core) effect extents are left out of a record's window.
    pub const NO_EFFECT_HULL: u32 = 1 << 5;
    /// Fixed records over a scrolled port get no seam damage.
    pub const NO_SCROLL_SEAM: u32 = 1 << 6;
    /// Damage hidden under a cover does not reach a blur layer that sits between them.
    pub const CULL_BEFORE_BLUR: u32 = 1 << 7;
}

#[derive(Clone, Copy, Debug)]
struct Dmg {
    z: u32,
    rect: Rect,
    /// Carried to blur layers but not marked: the pixels were supplied another way (a blit) or marked as a mask.
    virt: bool,
    /// Its order is no longer known (the driver renumbered): no cover may cull it.
    nocull: bool,
}

/// See the module documentation.
#[derive(Clone, Debug, Default)]
pub struct DamageEngine {
    w: i32,
    h: i32,
    mask: TileMask,
    /// Tiles some damage was culled on this frame.
    culled: TileMask,
    direct: Vec<Dmg>,
    layer_out: Vec<Dmg>,
    covers: Vec<OpaqueCover>,
    /// Per tile: 1 + the highest `z` of a cover whose core holds the whole tile (0: none).
    cover_z: Vec<u32>,
    layers: Vec<BlurLayer>,
    blits: Vec<ScrollBlit>,
    blur_blocks: Vec<BlurBlock>,
    work: WorkEstimate,
    occlusion_skips: u32,
    blur_gated: u32,
    resolved: bool,
    chooser: Chooser,
    choice: (ExecutorChoice, &'static str),
    #[cfg(feature = "flat_mutants")]
    pub mutants: u32,
}

impl DamageEngine {
    pub fn new(w: i32, h: i32) -> DamageEngine {
        let mut e = DamageEngine::default();
        e.resize(w, h);
        e
    }
    /// Take a window size. Covers and layers are dropped (the driver hands them over again).
    pub fn resize(&mut self, w: i32, h: i32) {
        self.w = w.max(0);
        self.h = h.max(0);
        self.mask.resize(w, h);
        self.culled.resize(w, h);
        self.cover_z.clear();
        self.cover_z.resize(self.mask.total() as usize, 0);
        self.covers.clear();
        self.layers.clear();
        self.begin_frame();
    }
    #[inline]
    pub fn mutant(&self, _bit: u32) -> bool {
        #[cfg(feature = "flat_mutants")]
        {
            self.mutants & _bit != 0
        }
        #[cfg(not(feature = "flat_mutants"))]
        {
            false
        }
    }
    pub fn window(&self) -> Rect {
        Rect { x0: 0, y0: 0, x1: self.w, y1: self.h }
    }

    // ---- the scene's occluders and readers ----

    pub fn covers_clear(&mut self) {
        self.covers.clear();
    }
    pub fn cover_push(&mut self, c: OpaqueCover) {
        let core = c.core_rect.and(self.window());
        // a core that holds no whole tile culls nothing
        if !core.is_empty() {
            self.covers.push(OpaqueCover { z: c.z, core_rect: core });
        }
    }
    /// Sort the covers front to back and rebuild the per-tile cover order.
    pub fn covers_commit(&mut self) {
        self.covers.sort_unstable_by(|a, b| b.z.cmp(&a.z));
        self.cover_z.fill(0);
        let (tw, w, h) = (self.mask.tiles_wide() as usize, self.w, self.h);
        for c in &self.covers {
            let r = c.core_rect;
            // whole tiles inside the core; a tile cut by the window's edge is whole if the core reaches the edge
            let tx0 = (r.x0 + TILE - 1) >> TILE_SHIFT;
            let ty0 = (r.y0 + TILE - 1) >> TILE_SHIFT;
            let tx1 = if r.x1 >= w { (w + TILE - 1) >> TILE_SHIFT } else { r.x1 >> TILE_SHIFT };
            let ty1 = if r.y1 >= h { (h + TILE - 1) >> TILE_SHIFT } else { r.y1 >> TILE_SHIFT };
            for ty in ty0..ty1 {
                for tx in tx0..tx1 {
                    let cz = &mut self.cover_z[ty as usize * tw + tx as usize];
                    *cz = (*cz).max(c.z + 1);
                }
            }
        }
    }
    /// The covers, front to back.
    pub fn covers(&self) -> &[OpaqueCover] {
        &self.covers
    }
    pub fn layers_clear(&mut self) {
        self.layers.clear();
    }
    pub fn layer_push(&mut self, l: BlurLayer) {
        self.layers.push(BlurLayer { max_level: l.max_level.min(MAX_BLUR_LEVEL), ..l });
    }
    /// Sort the layers back to front.
    pub fn layers_commit(&mut self) {
        self.layers.sort_unstable_by_key(|l| (l.z, l.id));
    }
    /// The blur layers, back to front.
    pub fn layers(&self) -> &[BlurLayer] {
        &self.layers
    }

    // ---- a frame ----

    pub fn begin_frame(&mut self) {
        self.mask.clear();
        self.culled.clear();
        self.direct.clear();
        self.layer_out.clear();
        self.blits.clear();
        self.blur_blocks.clear();
        self.work = WorkEstimate::default();
        self.occlusion_skips = 0;
        self.blur_gated = 0;
        self.resolved = false;
        self.choice = (ExecutorChoice::Idle, "nothing damaged");
    }
    /// Whether anything was reported since `begin_frame`.
    pub fn pending(&self) -> bool {
        !self.direct.is_empty() || !self.blits.is_empty() || !self.mask.is_empty()
    }
    /// A record at order `z` changed inside `rect` (its effect-expanded window, cut to its clip).
    pub fn damage(&mut self, z: u32, rect: Rect, work: Work) {
        let rect = rect.and(self.window());
        if rect.is_empty() {
            return;
        }
        self.work.affected_records += 1;
        let t = ((rect.area() + 1023) / 1024) as u32;
        match work {
            Work::Flat => {}
            Work::Shadow => self.work.shadow_tiles += t,
            Work::Gradient => self.work.grad_tiles += t,
        }
        self.direct.push(Dmg { z, rect, virt: false, nocull: false });
    }
    /// A record at order `z` was in `old` and is in `new` (windows cut to their clips): both are damaged, as one
    /// rect when they touch and as two when they are apart (a jump across the window does not repaint the gap).
    pub fn damage_move(&mut self, z: u32, old: Rect, new: Rect, work: Work) {
        let old = if self.mutant(mutant::NO_OLD_WIN) { Rect::EMPTY } else { old };
        if old == new || old.is_empty() || new.is_empty() || old.expand(TILE).hits(new) {
            self.damage(z, old.hull(new), work);
        } else {
            self.damage(z, old, work);
            self.damage(z, new, work);
        }
    }
    /// The backdrop changed inside `rect` at order `z` in a way the driver repairs itself: carried to the blur
    /// layers above, not marked.
    pub fn damage_virtual(&mut self, z: u32, rect: Rect) {
        let rect = rect.and(self.window());
        if !rect.is_empty() {
            self.direct.push(Dmg { z, rect, virt: true, nocull: false });
        }
    }
    /// Mark the tiles of `m` (a set the driver maintains, such as "every tile a themed record touches"), and
    /// tell every blur layer its whole backdrop may have changed.
    pub fn damage_mask(&mut self, m: &TileMask) {
        self.mask.or(m);
        if !m.is_empty() {
            let w = self.window();
            self.direct.push(Dmg { z: 0, rect: w, virt: true, nocull: false });
        }
    }
    /// The driver is about to renumber its paint orders (records were added or removed) with damage already
    /// reported this frame: that damage keeps its rect and forgets its order. It reaches every blur layer and
    /// no cover culls it.
    pub fn rebase_orders(&mut self) {
        for d in &mut self.direct {
            d.z = 0;
            d.nocull = true;
        }
    }
    pub fn note_fanout(&mut self, n: u32) {
        self.work.fanout += n;
    }
    /// The content of `clip_rect`, painted at orders `z_lo ..= z_hi`, moved rigidly by (`dx`, `dy`). Returns the
    /// blit (recorded for the executor) if one is worth it, having damaged only the strip the blit cannot
    /// supply; otherwise `None`, having damaged the whole clip. Report what does not move with the content
    /// through [`DamageEngine::seam`].
    ///
    /// The strip is damaged at `z_hi`: a cover that moved with the content must not cull it (the retained
    /// pixels under that cover are not the cover's yet), a fixed cover above all of the content may.
    pub fn scroll(&mut self, z_lo: u32, z_hi: u32, clip_rect: Rect, dx: i32, dy: i32) -> Option<ScrollBlit> {
        let clip = clip_rect.and(self.window());
        if clip.is_empty() || (dx == 0 && dy == 0) {
            return None;
        }
        let b = ScrollBlit { clip_rect: clip, dx, dy };
        let kept = b.kept();
        if kept.is_empty() {
            self.damage(z_hi, clip, Work::Flat);
            self.damage_virtual(z_lo.saturating_sub(1), clip);
            return None;
        }
        // the layers above (and the ones that moved with the content) read a backdrop that moved everywhere
        self.damage_virtual(z_lo.saturating_sub(1), clip);
        if !self.mutant(mutant::NO_SCROLL_STRIP) {
            // clip minus kept: a horizontal strip over the full width, and a vertical one beside what was kept
            if dy > 0 {
                self.damage(z_hi, Rect { y1: kept.y0, ..clip }, Work::Flat);
            } else if dy < 0 {
                self.damage(z_hi, Rect { y0: kept.y1, ..clip }, Work::Flat);
            }
            if dx > 0 {
                self.damage(z_hi, Rect { x0: clip.x0, x1: kept.x0, y0: kept.y0, y1: kept.y1 }, Work::Flat);
            } else if dx < 0 {
                self.damage(z_hi, Rect { x0: kept.x1, x1: clip.x1, y0: kept.y0, y1: kept.y1 }, Work::Flat);
            }
        }
        self.blits.push(b);
        self.work.scroll_blits += 1;
        Some(b)
    }
    /// Something at order `z` whose window is `rect` shows inside a blitted clip without moving with it (a fixed
    /// header, a sticky cell, glass, what shows through from below): it is damaged where it is and where the
    /// blit carried its pixels. (Both at its own order: under a cover that moved with the content the blit
    /// supplied the cover's pixels, and a fixed cover above it reports its own seam.)
    ///
    /// The one thing that needs no seam is an opaque uniform fill below the content whose core holds the whole
    /// clip: a shift of it is itself. The driver decides that; the engine is told only what does need one.
    pub fn seam(&mut self, blit: &ScrollBlit, z: u32, rect: Rect) {
        if self.mutant(mutant::NO_SCROLL_SEAM) {
            return;
        }
        let here = rect.and(blit.clip_rect);
        if here.is_empty() {
            return;
        }
        let there = here.shift(blit.dx, blit.dy).and(blit.clip_rect);
        self.damage(z, here, Work::Flat);
        self.damage(z, there, Work::Flat);
    }

    /// Carry the damage through the blur layers and mark the tiles. `gate` is asked, for a layer and a part of
    /// its backdrop that may have changed, what did change: `Some(rect)` (a part of it, or all of it) to
    /// propagate, `None` if the repainted backdrop is pixel for pixel what it was, which halts the propagation
    /// there. An executor that keeps each layer's backdrop repaints it inside this call.
    pub fn resolve(&mut self, gate: &mut dyn FnMut(&BlurLayer, Rect) -> Option<Rect>) {
        assert!(!self.resolved, "one resolve per frame");
        self.resolved = true;
        self.direct.sort_unstable_by_key(|d| d.z);
        let tw = self.mask.tiles_wide() as usize;
        for li in 0..self.layers.len() {
            let l = self.layers[li];
            let read = l.expanded_read_rect().and(self.window());
            if read.is_empty() {
                continue;
            }
            let (nd, nl) = (self.direct.len(), self.layer_out.len());
            for k in 0..nd + nl {
                let d = if k < nd { self.direct[k] } else { self.layer_out[k - nd] };
                if k < nd && d.z >= l.z {
                    // sorted: the rest of the direct damage is at or above the layer; go on to the layers' own
                    continue;
                }
                let c = d.rect.and(read);
                if c.is_empty() {
                    continue;
                }
                if self.mutant(mutant::CULL_BEFORE_BLUR) && !d.virt && !d.nocull {
                    if let Some((tx0, ty0, tx1, ty1)) = self.mask.tile_range(c) {
                        let all = (ty0..=ty1).all(|ty| (tx0..=tx1).all(|tx| self.cover_z[ty as usize * tw + tx as usize] as u64 > d.z as u64 + 1));
                        if all {
                            continue;
                        }
                    }
                }
                let Some(changed) = gate(&l, c) else {
                    self.blur_gated += 1;
                    continue;
                };
                let changed = changed.and(c);
                if changed.is_empty() {
                    self.blur_gated += 1;
                    continue;
                }
                let out = if self.mutant(mutant::NO_BLUR_DILATE) { changed.and(l.rect) } else { l.output_of(changed) };
                // the dirty blocks of every level, for the executor that keeps the pyramid and for the report
                let (mut bx, mut by) = ([(0, 0); MAX_BLUR_LEVEL as usize + 1], [(0, 0); MAX_BLUR_LEVEL as usize + 1]);
                blur_write(changed.x0, changed.x1 - 1, l.max_level, Some(&mut bx));
                blur_write(changed.y0, changed.y1 - 1, l.max_level, Some(&mut by));
                for lv in 0..=l.max_level as usize {
                    self.blur_blocks.push(BlurBlock { layer: l.id, level: lv as u8, x0: bx[lv].0, y0: by[lv].0, x1: bx[lv].1, y1: by[lv].1 });
                    let texels = (bx[lv].1 - bx[lv].0 + 1) as i64 * (by[lv].1 - by[lv].0 + 1) as i64;
                    self.work.blur_tiles += ((texels + 1023) / 1024) as u32;
                }
                self.work.blur_blocks += l.max_level as u32 + 1;
                if !out.is_empty() {
                    self.layer_out.push(Dmg { z: l.z, rect: out, virt: false, nocull: false });
                }
            }
        }
        // mark, culling what an opaque cover above hides
        let any_cover = !self.covers.is_empty();
        let any_z = self.mutant(mutant::OCCLUSION_ANY_Z);
        for k in 0..self.direct.len() + self.layer_out.len() {
            let d = if k < self.direct.len() { self.direct[k] } else { self.layer_out[k - self.direct.len()] };
            if d.virt {
                continue;
            }
            if !any_cover || d.nocull {
                self.mask.mark_rect(d.rect);
                continue;
            }
            let Some((tx0, ty0, tx1, ty1)) = self.mask.tile_range(d.rect) else {
                continue;
            };
            for ty in ty0..=ty1 {
                for tx in tx0..=tx1 {
                    let cz = self.cover_z[ty as usize * tw + tx as usize];
                    if cz as u64 > d.z as u64 + 1 || (any_z && cz != 0) {
                        self.occlusion_skips += 1;
                        self.culled.mark_tile(tx, ty);
                    } else {
                        self.mask.mark_tile(tx, ty);
                    }
                }
            }
        }
        self.work.dirty_tiles = self.mask.count();
        self.choice = self.chooser.choose(&self.work);
    }
    /// [`DamageEngine::resolve`] with no gate: every backdrop that may have changed is taken to have changed.
    pub fn resolve_ungated(&mut self) {
        self.resolve(&mut |_, r| Some(r));
    }

    // ---- what a frame came to ----

    /// The tiles to repaint (after `resolve`).
    pub fn mask(&self) -> &TileMask {
        &self.mask
    }
    /// The blits to apply, in order, before repainting.
    pub fn blits(&self) -> &[ScrollBlit] {
        &self.blits
    }
    pub fn blur_blocks(&self) -> &[BlurBlock] {
        &self.blur_blocks
    }
    pub fn work(&self) -> &WorkEstimate {
        &self.work
    }
    pub fn choice(&self) -> ExecutorChoice {
        self.choice.0
    }
    pub fn reason(&self) -> &'static str {
        self.choice.1
    }
    /// (damage, tile) pairs a cover culled this frame.
    pub fn occlusion_skips(&self) -> u32 {
        self.occlusion_skips
    }
    /// Distinct tiles some damage wanted and none got.
    pub fn occluded_tiles_skipped(&self) -> u32 {
        let mut n = 0;
        self.culled.each_tile(&mut |tx, ty| n += !self.mask.test(tx, ty) as u32);
        n
    }
    pub fn blur_gated(&self) -> u32 {
        self.blur_gated
    }
    pub fn repair_percent(&self) -> f32 {
        self.mask.fraction() * 100.0
    }
    /// Fill `r` with this frame (its vectors are reused).
    pub fn report_into(&self, r: &mut DamageReport) {
        r.width = self.w;
        r.height = self.h;
        r.tile = TILE;
        r.tiles.clear();
        self.mask.each_tile(&mut |tx, ty| r.tiles.push((tx as u16, ty as u16)));
        r.dirty_tiles = self.mask.count();
        r.total_tiles = self.mask.total();
        r.repair_percent = self.repair_percent();
        r.occluded_tiles_skipped = self.occluded_tiles_skipped();
        r.occlusion_skips = self.occlusion_skips;
        r.blur_blocks.clear();
        r.blur_blocks.extend_from_slice(&self.blur_blocks);
        r.blur_gated = self.blur_gated;
        r.scroll_blits.clear();
        r.scroll_blits.extend_from_slice(&self.blits);
        r.covers = self.covers.len() as u32;
        r.layers = self.layers.len() as u32;
        r.work = self.work;
        r.cost = self.work.cost();
        r.choice = self.choice.0;
        r.reason = self.choice.1;
    }
    pub fn report(&self) -> DamageReport {
        let mut r = DamageReport::default();
        self.report_into(&mut r);
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lcg(s: &mut u64) -> u32 {
        *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (*s >> 33) as u32
    }

    #[test]
    fn a_mask_equals_a_naive_bitmap_under_random_marks() {
        let (w, h) = if cfg!(miri) { (300, 200) } else { (2600, 1500) };
        let mut m = TileMask::new(w, h);
        let (tw, th) = (m.tiles_wide(), m.tiles_high());
        let mut naive = vec![false; (tw * th) as usize];
        let mut s = 7u64;
        for round in 0..if cfg!(miri) { 6 } else { 60 } {
            let x = (lcg(&mut s) % (w as u32 + 200)) as i32 - 100;
            let y = (lcg(&mut s) % (h as u32 + 200)) as i32 - 100;
            let r = Rect::xywh(x, y, (lcg(&mut s) % 700) as i32, (lcg(&mut s) % 300) as i32);
            let mut new = 0;
            let c = r.and(Rect::new(0, 0, w, h));
            if !c.is_empty() {
                for ty in c.y0 >> 5..=(c.y1 - 1) >> 5 {
                    for tx in c.x0 >> 5..=(c.x1 - 1) >> 5 {
                        let b = &mut naive[(ty as u32 * tw + tx as u32) as usize];
                        new += !*b as u32;
                        *b = true;
                    }
                }
            }
            assert_eq!(m.mark_rect(r), new);
            assert_eq!(m.count(), naive.iter().filter(|b| **b).count() as u32);
            let mut seen = Vec::new();
            m.each_tile(&mut |tx, ty| seen.push((tx, ty)));
            let want: Vec<_> = (0..th).flat_map(|ty| (0..tw).map(move |tx| (tx, ty))).filter(|(tx, ty)| naive[(ty * tw + tx) as usize]).collect();
            assert_eq!(seen, want);
            // spans cover exactly the set tiles, each a maximal run
            let mut area = 0i64;
            let mut last: Option<Rect> = None;
            m.each_span(&mut |r| {
                area += r.area();
                if let Some(p) = last {
                    assert!(!(p.y0 == r.y0 && p.x1 == r.x0), "two spans that touch are one span");
                }
                last = Some(r);
            });
            let want_area: i64 = want.iter().map(|(tx, ty)| m.tile_rect(*tx, *ty).area()).sum();
            assert_eq!(area, want_area);
            if round % 20 == 19 {
                m.clear();
                naive.fill(false);
                assert!(m.is_empty());
                let mut n = 0;
                m.each_tile(&mut |_, _| n += 1);
                assert_eq!(n, 0);
            }
        }
    }

    #[test]
    fn a_mask_wider_than_a_word_keeps_runs_across_words() {
        let mut m = TileMask::new(64 * 32 * 3, 64);
        m.mark_rect(Rect::new(60 * 32, 0, 130 * 32, 32));
        let mut runs = Vec::new();
        m.each_span(&mut |r| runs.push(r));
        assert_eq!(runs, vec![Rect::new(60 * 32, 0, 130 * 32, 32)]);
        assert_eq!(m.count(), 70);
        assert!(m.test(63, 0) && m.test(64, 0) && m.test(129, 0) && !m.test(130, 0) && !m.test(64, 1));
    }

    /// The pyramid of the module documentation, in one dimension over i64 (no rounding: the footprint is what is
    /// tested), with clamped reads at the ends of the domain.
    fn pyramid(src: &[i64], levels: u8) -> Vec<i64> {
        fn down(t: &[i64], lo: i32) -> (Vec<i64>, i32) {
            // t[k] is level-l texel lo + k; the next level covers every texel that has a tap inside
            let hi = lo + t.len() as i32 - 1;
            let (a, b) = (lo >> 1, hi >> 1);
            let at = |j: i32| t[(j.clamp(lo, hi) - lo) as usize];
            ((a..=b).map(|i| at(2 * i - 1) + 3 * at(2 * i) + 3 * at(2 * i + 1) + at(2 * i + 2)).collect(), a)
        }
        fn up(t: &[i64], lo: i32, out_lo: i32, out_hi: i32) -> Vec<i64> {
            let hi = lo + t.len() as i32 - 1;
            let at = |i: i32| t[(i.clamp(lo, hi) - lo) as usize];
            (out_lo..=out_hi).map(|j| if j & 1 == 0 { at((j >> 1) - 1) + 3 * at(j >> 1) } else { 3 * at(j >> 1) + at((j >> 1) + 1) }).collect()
        }
        let mut stack = vec![(src.to_vec(), 0i32)];
        for _ in 0..levels {
            let (t, lo) = stack.last().unwrap();
            let d = down(t, *lo);
            stack.push(d);
        }
        let (mut cur, mut lo) = stack.pop().unwrap();
        while let Some((t, tlo)) = stack.pop() {
            cur = up(&cur, lo, tlo, tlo + t.len() as i32 - 1);
            lo = tlo;
        }
        cur
    }

    #[test]
    fn the_blur_footprint_is_exact_for_the_pyramid() {
        let n = if cfg!(miri) { 40 } else { 96 };
        for levels in 0..=if cfg!(miri) { 2 } else { 4u8 } {
            let base: Vec<i64> = (0..n).map(|i| (i * 37 % 101) as i64).collect();
            let want = pyramid(&base, levels);
            // away from the clamped ends, so the footprint is the taps' alone
            let margin = 6 << levels;
            for p in (margin..n - margin).step_by(if cfg!(miri) { 7 } else { 1 }) {
                let mut src = base.clone();
                src[p as usize] += 1 << 20;
                let got = pyramid(&src, levels);
                let changed: Vec<i32> = (0..n).filter(|i| got[*i as usize] != want[*i as usize]).collect();
                let (lo, hi) = blur_write(p, p, levels, None);
                assert!(changed.iter().all(|c| *c >= lo && *c <= hi), "level {levels}: pixel {p} reached outside {lo}..={hi}: {changed:?}");
                // exact: both ends are reached
                assert_eq!((changed[0], *changed.last().unwrap()), (lo, hi), "level {levels} pixel {p}");
                // and the read interval is the transpose
                for c in [lo, hi] {
                    let (rl, rh) = blur_read(c, c, levels);
                    assert!(rl <= p && p <= rh);
                }
            }
        }
    }

    #[test]
    fn a_shadow_window_holds_offset_spread_and_one_and_a_half_blurs() {
        let e = EffectExtents { shadow: true, shadow_dx: 4, shadow_dy: -6, shadow_blur: 10, shadow_spread: 3, blur: 0 };
        let r = Rect::new(100, 100, 200, 150);
        assert_eq!(e.window(r), Rect::new(100 + 4 - 18, 100 - 6 - 18, 200 + 4 + 18, 150 + 18 - 6).hull(r));
        assert_eq!(EffectExtents { blur: 5, ..Default::default() }.window(r), r.expand(5));
        assert_eq!(EffectExtents::default().window(r), r);
    }

    fn engine() -> DamageEngine {
        DamageEngine::new(640, 480)
    }

    #[test]
    fn damage_under_a_cover_is_culled_and_counted_and_damage_over_it_is_not() {
        let mut e = engine();
        e.cover_push(OpaqueCover { z: 10, core_rect: Rect::new(64, 64, 320, 320) });
        e.covers_commit();
        e.begin_frame();
        // wholly under the cover
        e.damage(3, Rect::new(100, 100, 200, 200), Work::Flat);
        e.resolve_ungated();
        assert_eq!(e.mask().count(), 0);
        assert_eq!(e.occluded_tiles_skipped(), 16);
        assert_eq!(e.choice(), ExecutorChoice::Idle);
        // above it
        e.begin_frame();
        e.damage(11, Rect::new(100, 100, 200, 200), Work::Flat);
        e.resolve_ungated();
        assert_eq!((e.mask().count(), e.occlusion_skips()), (16, 0));
        // the cover's own change is not culled by itself
        e.begin_frame();
        e.damage(10, Rect::new(64, 64, 320, 320), Work::Flat);
        e.resolve_ungated();
        assert_eq!(e.mask().count(), 64);
        // straddling the edge: only the tiles inside the core are culled
        e.begin_frame();
        e.damage(3, Rect::new(32, 96, 128, 128), Work::Flat);
        e.resolve_ungated();
        assert_eq!((e.mask().count(), e.occluded_tiles_skipped()), (1, 2));
    }

    #[test]
    fn a_core_that_is_not_tile_aligned_culls_only_whole_tiles() {
        let mut e = engine();
        e.cover_push(OpaqueCover { z: 5, core_rect: Rect::new(70, 70, 200, 200) });
        e.covers_commit();
        e.begin_frame();
        e.damage(1, Rect::new(64, 64, 224, 224), Work::Flat);
        e.resolve_ungated();
        // tiles 3..=5 (96..192) are whole inside 70..200
        assert_eq!(e.occluded_tiles_skipped(), 9);
        assert_eq!(e.mask().count(), 25 - 9);
    }

    #[test]
    fn a_rounded_cover_has_a_smaller_core() {
        let r = Rect::new(0, 0, 200, 200);
        assert_eq!(opaque_core(r, 0), r);
        let c = opaque_core(r, 40);
        // the corner circle's 45 degree point is at 40 * (1 - 0.7071) = 11.7 from each side
        assert!(c.x0 >= 12 && c.x0 <= 14 && c.x1 == 200 - c.x0);
        assert!(opaque_core(Rect::new(0, 0, 4, 4), 100).is_empty());
    }

    #[test]
    fn damage_under_a_blur_layer_reaches_the_layers_output_dilated_and_cascades() {
        let mut e = engine();
        let glass = BlurLayer { id: 1, z: 10, rect: Rect::new(200, 100, 400, 300), radius: 8, max_level: 3 };
        let upper = BlurLayer { id: 2, z: 20, rect: Rect::new(380, 100, 600, 300), radius: 4, max_level: 2 };
        e.layer_push(upper);
        e.layer_push(glass);
        e.layers_commit();
        assert_eq!(e.layers()[0].id, 1);
        // a pixel left of the glass, inside its read rect
        let read = glass.expanded_read_rect();
        assert!(read.x0 < 200 - 8 && read.x0 > 200 - 4 * 8 - 8);
        e.begin_frame();
        let d = Rect::new(190, 150, 194, 154);
        e.damage(2, d, Work::Flat);
        e.resolve_ungated();
        let out = glass.output_of(d);
        assert!(!out.is_empty() && out.x0 == 200 && out.x1 > 200);
        // the glass's dirty output tiles are marked although the damage itself is outside the glass
        assert!(e.mask().test(200 / 32, 150 / 32));
        assert_eq!(e.blur_blocks().iter().filter(|b| b.layer == 1).count(), 4);
        // far from the upper layer: it is not reached
        assert_eq!(e.blur_blocks().iter().filter(|b| b.layer == 2).count(), 0);

        // damage near the glass's right edge cascades: glass output -> the upper layer's read rect
        e.begin_frame();
        let d = Rect::new(370, 200, 372, 202);
        e.damage(2, d, Work::Flat);
        e.resolve_ungated();
        assert!(e.blur_blocks().iter().any(|b| b.layer == 2), "the glass's output is the upper layer's backdrop");
        // damage above the glass does not go through it
        e.begin_frame();
        e.damage(15, Rect::new(250, 150, 260, 160), Work::Flat);
        e.resolve_ungated();
        assert!(e.blur_blocks().iter().all(|b| b.layer != 1));
    }

    #[test]
    fn a_gate_that_finds_the_backdrop_unchanged_halts_the_propagation() {
        let mut e = engine();
        e.layer_push(BlurLayer { id: 1, z: 10, rect: Rect::new(200, 100, 400, 300), radius: 8, max_level: 3 });
        e.layers_commit();
        e.begin_frame();
        e.damage(2, Rect::new(186, 150, 196, 160), Work::Flat);
        let mut asked = 0;
        e.resolve(&mut |l, r| {
            asked += 1;
            assert_eq!(l.id, 1);
            assert!(l.expanded_read_rect().contains(r));
            None
        });
        // only the damage's own two tiles; nothing of the glass
        assert_eq!((asked, e.blur_gated(), e.mask().count()), (1, 1, 2));
        assert!(e.blur_blocks().is_empty());
        // a gate that narrows the change narrows the output
        e.begin_frame();
        e.damage(2, Rect::new(180, 100, 191, 300), Work::Flat);
        e.resolve(&mut |_, r| Some(Rect { y1: r.y0 + 1, ..r }));
        let rows: std::collections::BTreeSet<u32> = {
            let mut s = std::collections::BTreeSet::new();
            e.mask().each_tile(&mut |tx, ty| {
                if tx >= 200 / 32 {
                    s.insert(ty);
                }
            });
            s
        };
        assert!(rows.iter().all(|ty| *ty <= (100 + 40) / 32), "{rows:?}");
    }

    #[test]
    fn damage_hidden_by_a_cover_still_reaches_a_blur_layer_between_them() {
        let mut e = engine();
        // z 2: damage; z 10: glass over 200..400; z 30: an opaque cover over the damage only
        e.layer_push(BlurLayer { id: 1, z: 10, rect: Rect::new(200, 100, 400, 300), radius: 8, max_level: 3 });
        e.layers_commit();
        e.cover_push(OpaqueCover { z: 30, core_rect: Rect::new(160, 128, 192, 192) });
        e.covers_commit();
        e.begin_frame();
        e.damage(2, Rect::new(164, 132, 188, 188), Work::Flat);
        e.resolve_ungated();
        // the damage's own tile is under the cover: skipped
        assert!(!e.mask().test(5, 4) && e.occluded_tiles_skipped() >= 1);
        // the glass beside the cover reads it: its output is dirty
        assert!(e.mask().test(200 / 32, 160 / 32));
    }

    #[test]
    fn a_scroll_blits_and_marks_only_the_uncovered_strip_and_the_seams() {
        let mut e = DamageEngine::new(1280, 800);
        let clip = Rect::new(0, 64, 1280, 800);
        e.begin_frame();
        let b = e.scroll(5, 9, clip, 0, -40).expect("a blit");
        assert_eq!(b.kept(), Rect::new(0, 64, 1280, 760));
        // a fixed badge over the list
        e.seam(&b, 50, Rect::new(1100, 100, 1200, 140));
        e.resolve_ungated();
        assert_eq!(e.blits(), &[b]);
        let pct = e.repair_percent();
        assert!(pct > 1.0 && pct < 10.0, "{pct}");
        // strip rows: 760..800 -> tile rows 23, 24; the seam where it is (rows 3, 4) and where it was carried (2, 3)
        let mut rows = std::collections::BTreeSet::new();
        e.mask().each_tile(&mut |_, ty| {
            rows.insert(ty);
        });
        assert_eq!(rows.into_iter().collect::<Vec<_>>(), vec![2, 3, 4, 23, 24]);
        // a jump of a whole port is a repaint, not a blit
        e.begin_frame();
        assert!(e.scroll(5, 9, clip, 0, 900).is_none());
        e.resolve_ungated();
        assert_eq!(e.mask().count(), 40 * 23);
        // both axes
        e.begin_frame();
        let b = e.scroll(5, 9, Rect::new(100, 100, 500, 500), 10, 20).unwrap();
        assert_eq!(b.kept(), Rect::new(110, 120, 500, 500));
        e.resolve_ungated();
        assert!(e.mask().test(3, 3) && e.mask().test(15, 3) && e.mask().test(3, 15) && !e.mask().test(8, 8));
    }

    #[test]
    fn a_scroll_under_glass_dirties_the_glass() {
        let mut e = engine();
        e.layer_push(BlurLayer { id: 9, z: 40, rect: Rect::new(0, 0, 640, 48), radius: 8, max_level: 3 });
        e.layers_commit();
        e.begin_frame();
        e.scroll(5, 9, Rect::new(0, 0, 640, 480), 0, -8);
        e.resolve_ungated();
        assert!(e.mask().test(0, 0) && e.mask().test(19, 1));
    }

    #[test]
    fn the_chooser_is_idle_sparse_on_the_cpu_and_broad_on_the_gpu_with_hysteresis_per_signature() {
        let mut e = DamageEngine::new(1920, 1088);
        e.begin_frame();
        e.resolve_ungated();
        assert_eq!(e.choice(), ExecutorChoice::Idle);
        // two rows of a hover
        e.begin_frame();
        e.damage(1, Rect::new(0, 100, 600, 124), Work::Flat);
        e.damage(1, Rect::new(0, 124, 600, 148), Work::Flat);
        e.resolve_ungated();
        assert_eq!(e.choice(), ExecutorChoice::Cpu);
        // a theme switch: everything
        e.begin_frame();
        e.damage(0, Rect::new(0, 0, 1920, 1088), Work::Flat);
        e.resolve_ungated();
        assert_eq!(e.choice(), ExecutorChoice::Gpu);
        // the hover after it is still the CPU's although the last flat frame went to the GPU: it is far under half
        e.begin_frame();
        e.damage(1, Rect::new(0, 100, 600, 124), Work::Flat);
        e.resolve_ungated();
        assert_eq!(e.choice(), ExecutorChoice::Cpu);
        // hysteresis: 400 tiles is 1600 + 1, under the threshold: CPU from below, GPU once it has been there
        let mid = Rect::new(0, 0, 20 * 32, 20 * 32);
        e.begin_frame();
        e.damage(1, mid, Work::Flat);
        e.resolve_ungated();
        assert_eq!(e.choice(), ExecutorChoice::Cpu);
        e.begin_frame();
        e.damage(0, Rect::new(0, 0, 1920, 1088), Work::Flat);
        e.resolve_ungated();
        e.begin_frame();
        e.damage(1, mid, Work::Flat);
        e.resolve_ungated();
        assert_eq!((e.choice(), e.reason()), (ExecutorChoice::Gpu, "broad work, held on the GPU (hysteresis)"));
        // a shadow frame has its own signature: the flat signature's history does not touch it
        e.begin_frame();
        e.damage(1, Rect::new(0, 0, 200, 200), Work::Shadow);
        e.resolve_ungated();
        assert_eq!(e.choice(), ExecutorChoice::Cpu);
        // blur blocks are dear: a small damage under a large deep glass goes to the GPU
        e.layer_push(BlurLayer { id: 1, z: 10, rect: Rect::new(0, 0, 1920, 1088), radius: 32, max_level: 5 });
        e.layers_commit();
        e.begin_frame();
        e.damage(1, Rect::new(0, 0, 1920, 1088), Work::Flat);
        e.resolve_ungated();
        assert_eq!(e.choice(), ExecutorChoice::Gpu);
    }

    #[test]
    fn a_report_names_the_tiles_the_skips_the_blocks_the_blits_and_the_verdict() {
        let mut e = engine();
        e.cover_push(OpaqueCover { z: 10, core_rect: Rect::new(0, 0, 64, 64) });
        e.covers_commit();
        e.layer_push(BlurLayer { id: 3, z: 20, rect: Rect::new(320, 0, 640, 100), radius: 4, max_level: 2 });
        e.layers_commit();
        e.begin_frame();
        e.damage(1, Rect::new(0, 0, 100, 40), Work::Gradient);
        e.damage(1, Rect::new(330, 10, 340, 20), Work::Flat);
        e.scroll(2, 4, Rect::new(0, 200, 640, 480), 0, 16);
        e.note_fanout(7);
        e.resolve_ungated();
        let r = e.report();
        assert_eq!(r.dirty_tiles as usize, r.tiles.len());
        assert_eq!(r.occluded_tiles_skipped, 4);
        assert!(r.tiles.contains(&(2, 0)) && !r.tiles.contains(&(0, 0)));
        assert_eq!(r.scroll_blits.len(), 1);
        assert!(r.blur_blocks.iter().any(|b| b.layer == 3 && b.level == 2));
        assert_eq!((r.work.fanout, r.covers, r.layers), (7, 1, 1));
        assert!(r.work.grad_tiles >= 4 && r.choice == ExecutorChoice::Cpu);
        assert!((r.repair_percent - r.dirty_tiles as f32 * 100.0 / r.total_tiles as f32).abs() < 1e-3);
        let j = r.to_json();
        assert!(j.starts_with('{') && j.ends_with('}') && j.contains("\"choice\":\"Cpu\"") && j.contains("\"scroll_blits\":[{\"clip\":[0,200,640,480],\"dx\":0,\"dy\":16}]"));
        // a second report into the same value reuses it
        let mut again = DamageReport::default();
        e.report_into(&mut again);
        assert_eq!(again, r);
    }

    #[test]
    fn damage_reported_before_a_renumbering_is_neither_culled_nor_kept_from_any_layer() {
        let mut e = engine();
        e.cover_push(OpaqueCover { z: 10, core_rect: Rect::new(64, 64, 320, 320) });
        e.covers_commit();
        e.layer_push(BlurLayer { id: 1, z: 4, rect: Rect::new(400, 100, 500, 200), radius: 4, max_level: 2 });
        e.layers_commit();
        e.begin_frame();
        // above the cover and above the layer, as the orders stood
        e.damage(12, Rect::new(100, 100, 120, 120), Work::Flat);
        e.damage(12, Rect::new(410, 110, 420, 120), Work::Flat);
        e.rebase_orders();
        e.resolve_ungated();
        assert!(e.mask().test(3, 3) && e.occlusion_skips() == 0);
        assert!(!e.blur_blocks().is_empty());
    }

    #[test]
    fn a_moved_record_is_damaged_where_it_was_and_where_it_is() {
        let mut e = engine();
        e.begin_frame();
        e.damage_move(1, Rect::new(0, 0, 20, 20), Rect::new(610, 450, 630, 470), Work::Flat);
        e.resolve_ungated();
        assert!(e.mask().test(0, 0) && e.mask().test(19, 14) && e.mask().count() == 2);
        e.begin_frame();
        e.damage_move(1, Rect::new(0, 0, 20, 20), Rect::new(10, 0, 30, 20), Work::Flat);
        e.resolve_ungated();
        assert_eq!(e.mask().count(), 1);
    }
}
