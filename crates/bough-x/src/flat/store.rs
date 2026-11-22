//! The record bank: every record as one index into parallel dense arrays, and the dependency index.
//!
//! A record is not a type. Its kind is a byte, its properties are words in columns, and what makes a property
//! reactive is a [`Bind`] row beside the store naming the property and the [`Val`] it follows. Properties that
//! are constants have no row and cost nothing after they are written.
use super::expr::{NO_SITE, Val};
use super::state::{DepMask, SlotId, StrSlot};
use crate::damage_engine::{Rect, shadow_reach};

pub const NONE: u32 = u32::MAX;

/// What a record paints.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Kind {
    /// Nothing: a hit area, a scroll port, a blur layer with no tint.
    None = 0,
    #[default]
    Box = 1,
    /// A line of the built-in face.
    Text = 2,
    /// Painted by the embedder ([`super::world::ExtPaint`]): a shaped run, an image.
    Ext = 3,
}

/// A record's bindable properties.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Prop {
    X = 0,
    Y = 1,
    W = 2,
    H = 3,
    /// Fill of a box, ink of a text (straight alpha, red low).
    Fill = 4,
    Border = 5,
    /// 1/256 px.
    BorderW = 6,
    /// 1/256 px.
    Radius = 7,
    /// 0 ..= 255.
    Opacity = 8,
}
pub const PROPS: [Prop; 9] = [Prop::X, Prop::Y, Prop::W, Prop::H, Prop::Fill, Prop::Border, Prop::BorderW, Prop::Radius, Prop::Opacity];

/// Record flags.
pub mod flag {
    pub const LIVE: u32 = 1;
    /// The pointer stops here.
    pub const HIT: u32 = 1 << 1;
    /// `fill` / `border` hold a palette slot, resolved when painted.
    pub const PAL_FILL: u32 = 1 << 2;
    pub const PAL_BORDER: u32 = 1 << 3;
    /// The record is an opaque cover (kept equal to the predicate, on or off screen).
    pub const COVER: u32 = 1 << 4;
    /// Some space takes this record's box as its port.
    pub const PORT: u32 = 1 << 5;
    /// Moved since the spatial index was built: found through the loose list.
    pub const LOOSE: u32 = 1 << 6;
    /// The record blurs its backdrop.
    pub const BLUR: u32 = 1 << 7;
    /// Too tall for the band index: found through the wide list.
    pub const WIDE: u32 = 1 << 8;
}

/// The fields of an effect row.
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
    /// 0: none; 1: top to bottom; 2: left to right.
    pub const GRAD_DIR: usize = 10;
    pub const GRAD_TO: usize = 11;
    /// Backdrop blur radius in pixels (0: none).
    pub const BACKDROP: usize = 12;
    /// Blur of the box itself, pixels.
    pub const ELEM_BLUR: usize = 13;
    pub const N: usize = 14;
}

/// An effect as described: every field a [`Val`], so a shadow that grows on hover or a blur that animates in
/// is a binding like any other.
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
    /// A linear gradient from the fill to `to`: `dir` 1 top to bottom, 2 left to right.
    pub fn gradient(mut self, dir: impl Into<Val>, to: impl Into<Val>) -> Effect {
        self.0[ef::GRAD_DIR] = dir.into();
        self.0[ef::GRAD_TO] = to.into();
        self
    }
    pub fn backdrop_blur(mut self, radius_px: impl Into<Val>) -> Effect {
        self.0[ef::BACKDROP] = radius_px.into();
        self
    }
    pub fn blur(mut self, radius_px: impl Into<Val>) -> Effect {
        self.0[ef::ELEM_BLUR] = radius_px.into();
        self
    }
}

/// A record's handle: its index and the generation the index had when it was issued.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct RecId {
    pub index: u32,
    pub generation: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct SpaceId(pub u32);
/// The window itself.
pub const ROOT: SpaceId = SpaceId(0);

/// Where a text record's characters come from.
#[derive(Clone, Debug, PartialEq)]
pub enum TextSrc {
    Static(String),
    /// `prefix`, the state word as an unsigned decimal, `suffix`.
    Num { prefix: String, slot: SlotId, suffix: String },
    /// A string of the state block's side table.
    Str(StrSlot),
    /// Four words the embedder's painter understands (kind [`Kind::Ext`]).
    Ext([u32; 4]),
}
impl PartialEq for StrSlot {
    fn eq(&self, o: &StrSlot) -> bool {
        self.index == o.index && self.id() == o.id()
    }
}

/// A record as described. Everything but the space and the kind has a default.
#[derive(Clone, Debug)]
pub struct RecordDesc {
    pub space: SpaceId,
    pub kind: Kind,
    pub x: Val,
    pub y: Val,
    pub w: Val,
    pub h: Val,
    pub fill: Val,
    pub border: Val,
    pub border_w: Val,
    pub radius: Val,
    pub opacity: Val,
    pub effect: Option<Effect>,
    pub text: Option<TextSrc>,
    /// Cell scale of the built-in face.
    pub text_scale: u32,
    pub hit: bool,
    /// The hover site this record is ([`NO_SITE`]: none).
    pub site: u32,
    /// What a click here means to the embedder (0: nothing).
    pub handler: u32,
    /// Identity across descriptions (a list item's key; 0: positional).
    pub key: u64,
}
impl Default for RecordDesc {
    fn default() -> RecordDesc {
        RecordDesc {
            space: ROOT,
            kind: Kind::Box,
            x: Val::Const(0),
            y: Val::Const(0),
            w: Val::Const(0),
            h: Val::Const(0),
            fill: Val::Const(0),
            border: Val::Const(0),
            border_w: Val::Const(0),
            radius: Val::Const(0),
            opacity: Val::Const(255),
            effect: None,
            text: None,
            text_scale: 1,
            hit: false,
            site: NO_SITE,
            handler: 0,
            key: 0,
        }
    }
}
impl RecordDesc {
    /// A box at `r` (in its space's coordinates) filled with `fill`.
    pub fn rect(space: SpaceId, r: Rect, fill: impl Into<Val>) -> RecordDesc {
        RecordDesc { space, x: r.x0.into(), y: r.y0.into(), w: (r.x1 - r.x0).into(), h: (r.y1 - r.y0).into(), fill: fill.into(), ..RecordDesc::default() }
    }
    /// A line of text in the box `r`.
    pub fn label(space: SpaceId, r: Rect, src: TextSrc, scale: u32, ink: impl Into<Val>) -> RecordDesc {
        RecordDesc { kind: Kind::Text, text: Some(src), text_scale: scale, ..RecordDesc::rect(space, r, ink) }
    }
    pub fn radius_px(mut self, r: i32) -> RecordDesc {
        self.radius = Val::Const((r.max(0) as u32) << 8);
        self
    }
    pub fn border_px(mut self, w: i32, color: impl Into<Val>) -> RecordDesc {
        self.border_w = Val::Const((w.max(0) as u32) << 8);
        self.border = color.into();
        self
    }
    pub fn effect(mut self, e: Effect) -> RecordDesc {
        self.effect = Some(e);
        self
    }
    pub fn hit(mut self, site: u32, handler: u32) -> RecordDesc {
        self.hit = true;
        self.site = site;
        self.handler = handler;
        self
    }
    pub fn opacity(mut self, v: impl Into<Val>) -> RecordDesc {
        self.opacity = v.into();
        self
    }
    pub fn key(mut self, k: u64) -> RecordDesc {
        self.key = k;
        self
    }
}

#[derive(Clone, Debug)]
pub struct TextRun {
    pub src: TextSrc,
    /// The characters as last formatted.
    pub buf: String,
    pub scale: u32,
    pub owner: u32,
}

/// What a touched record changed this turn.
pub mod touch {
    pub const GEOM: u8 = 1;
    pub const PAINT: u8 = 2;
    /// Its damage was already reported (with its space).
    pub const DONE: u8 = 4;
}

/// The record bank. Column `c[i]` is property `c` of record `i`; a dead index is on the free list with its
/// generation advanced.
#[derive(Clone, Debug, Default)]
pub struct FlatStore {
    /// The box, in the record's space.
    pub rects: Vec<Rect>,
    /// The record's window (box and effects, in its space) before this turn's first write to it.
    pub old_rects: Vec<Rect>,
    /// The space that places and clips the record.
    pub clips: Vec<u32>,
    /// Paint order: 1 is painted first.
    pub z_order: Vec<u32>,
    /// What `z_order` is the rank of (sparse, so an insert between two records needs no renumbering).
    pub z_key: Vec<u64>,
    pub kinds: Vec<u8>,
    pub flags: Vec<u32>,
    pub fill: Vec<u32>,
    pub border: Vec<u32>,
    pub border_w: Vec<u32>,
    pub radius: Vec<u32>,
    pub opacity: Vec<u32>,
    /// Row of `effects` ([`NONE`]: no effect).
    pub effect_id: Vec<u32>,
    /// Row of `texts` ([`NONE`]: no text).
    pub text_run: Vec<u32>,
    pub handlers: Vec<u32>,
    pub site: Vec<u32>,
    pub item_key: Vec<u64>,
    pub gens: Vec<u32>,
    /// The folded set of state words the record's bindings read.
    pub dep_mask: Vec<DepMask>,
    /// The turn that last touched the record, and what it changed.
    pub touch_epoch: Vec<u32>,
    pub touch: Vec<u8>,
    pub free: Vec<u32>,
    pub live: u32,

    pub effects: Vec<[u32; ef::N]>,
    pub effect_owner: Vec<u32>,
    pub effect_free: Vec<u32>,
    pub texts: Vec<TextRun>,
    pub text_free: Vec<u32>,
}

impl FlatStore {
    pub fn len(&self) -> usize {
        self.rects.len()
    }
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }
    #[inline]
    pub fn is_live(&self, i: usize) -> bool {
        self.flags[i] & flag::LIVE != 0
    }
    pub fn id(&self, i: usize) -> RecId {
        RecId { index: i as u32, generation: self.gens[i] }
    }
    /// The index of a live record, or `None` for a handle whose record was removed.
    pub fn resolve(&self, id: RecId) -> Option<usize> {
        let i = id.index as usize;
        (i < self.len() && self.gens[i] == id.generation && self.is_live(i)).then_some(i)
    }
    /// A new all-default row; returns its index.
    pub(crate) fn alloc(&mut self) -> usize {
        self.live += 1;
        if let Some(i) = self.free.pop() {
            let i = i as usize;
            self.flags[i] = flag::LIVE;
            return i;
        }
        self.rects.push(Rect::EMPTY);
        self.old_rects.push(Rect::EMPTY);
        self.clips.push(0);
        self.z_order.push(0);
        self.z_key.push(0);
        self.kinds.push(0);
        self.flags.push(flag::LIVE);
        self.fill.push(0);
        self.border.push(0);
        self.border_w.push(0);
        self.radius.push(0);
        self.opacity.push(255);
        self.effect_id.push(NONE);
        self.text_run.push(NONE);
        self.handlers.push(0);
        self.site.push(NO_SITE);
        self.item_key.push(0);
        self.gens.push(0);
        self.dep_mask.push(DepMask::EMPTY);
        self.touch_epoch.push(0);
        self.touch.push(0);
        self.rects.len() - 1
    }
    pub(crate) fn release(&mut self, i: usize) {
        self.flags[i] = 0;
        self.gens[i] = self.gens[i].wrapping_add(1);
        self.kinds[i] = 0;
        self.rects[i] = Rect::EMPTY;
        if self.effect_id[i] != NONE {
            self.effect_free.push(self.effect_id[i]);
            self.effect_owner[self.effect_id[i] as usize] = NONE;
            self.effect_id[i] = NONE;
        }
        if self.text_run[i] != NONE {
            self.text_free.push(self.text_run[i]);
            self.text_run[i] = NONE;
        }
        self.free.push(i as u32);
        self.live -= 1;
    }
    pub(crate) fn alloc_effect(&mut self, owner: usize) -> usize {
        let e = match self.effect_free.pop() {
            Some(e) => e as usize,
            None => {
                self.effects.push([0; ef::N]);
                self.effect_owner.push(NONE);
                self.effects.len() - 1
            }
        };
        self.effects[e] = [0; ef::N];
        self.effect_owner[e] = owner as u32;
        e
    }
    pub(crate) fn alloc_text(&mut self, run: TextRun) -> usize {
        match self.text_free.pop() {
            Some(t) => {
                self.texts[t as usize] = run;
                t as usize
            }
            None => {
                self.texts.push(run);
                self.texts.len() - 1
            }
        }
    }
    #[inline]
    pub fn prop(&self, i: usize, p: Prop) -> u32 {
        let r = &self.rects[i];
        match p {
            Prop::X => r.x0 as u32,
            Prop::Y => r.y0 as u32,
            Prop::W => (r.x1 - r.x0) as u32,
            Prop::H => (r.y1 - r.y0) as u32,
            Prop::Fill => self.fill[i],
            Prop::Border => self.border[i],
            Prop::BorderW => self.border_w[i],
            Prop::Radius => self.radius[i],
            Prop::Opacity => self.opacity[i],
        }
    }
    #[inline]
    pub(crate) fn set_prop(&mut self, i: usize, p: Prop, v: u32) {
        let r = &mut self.rects[i];
        match p {
            Prop::X => {
                let w = r.x1 - r.x0;
                r.x0 = v as i32;
                r.x1 = r.x0 + w;
            }
            Prop::Y => {
                let h = r.y1 - r.y0;
                r.y0 = v as i32;
                r.y1 = r.y0 + h;
            }
            Prop::W => r.x1 = r.x0 + (v as i32).max(0),
            Prop::H => r.y1 = r.y0 + (v as i32).max(0),
            Prop::Fill => self.fill[i] = v,
            Prop::Border => self.border[i] = v,
            Prop::BorderW => self.border_w[i] = v,
            Prop::Radius => self.radius[i] = v,
            Prop::Opacity => self.opacity[i] = v.min(255),
        }
    }
    /// The effect row of record `i`, if it has one.
    #[inline]
    pub fn effect(&self, i: usize) -> Option<&[u32; ef::N]> {
        let e = self.effect_id[i];
        if e == NONE { None } else { Some(&self.effects[e as usize]) }
    }
    /// The record's window in its space: its box, grown by its own blur, joined with its shadows' boxes. With
    /// `bare` (the `NO_EFFECT_HULL` mutant) the box alone.
    #[inline]
    pub fn window(&self, i: usize, bare: bool) -> Rect {
        let mut r = self.rects[i];
        if r.is_empty() {
            return Rect::EMPTY;
        }
        if self.kinds[i] == Kind::Text as u8 {
            // a line of the built-in face inks only its cells, from the box's corner
            if let Some(t) = self.texts.get(self.text_run[i] as usize) {
                let (cw, ch) = super::raster::cell(t.scale);
                r.x1 = r.x1.min(r.x0 + t.buf.len() as i32 * cw);
                r.y1 = r.y1.min(r.y0 + ch);
                if r.is_empty() {
                    return Rect::EMPTY;
                }
            }
        }
        let Some(e) = self.effect(i) else {
            return r;
        };
        if bare {
            return r;
        }
        let mut w = r.expand((e[ef::ELEM_BLUR] as i32).max(0));
        for o in [ef::SH0_COLOR, ef::SH1_COLOR] {
            if e[o] >> 24 != 0 {
                let grow = (e[o + 4] as i32 + shadow_reach(e[o + 3] as i32)).max(0);
                w = w.hull(r.shift(e[o + 1] as i32, e[o + 2] as i32).expand(grow));
            }
        }
        w
    }
}

/// What a binding writes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    /// Property of record `idx`.
    Prop(Prop),
    /// Field of effect row `idx`.
    Effect(u8),
    /// The characters of text run `idx`.
    Text,
    /// Whether space `idx` clips to its port.
    SpaceClip,
    SpaceSx,
    SpaceSy,
    /// The goal of animation `idx`.
    Anim,
}

/// One reactive property: `target` of `idx` follows `val`.
#[derive(Clone, Copy, Debug)]
pub struct Bind {
    pub target: Target,
    pub idx: u32,
    pub val: Val,
    /// Not evaluated yet (described since the last commit).
    pub fresh: bool,
}

/// A compressed sparse row index: `targets[offsets[k] .. offsets[k + 1]]` are the targets of key `k`.
#[derive(Clone, Debug, Default)]
pub struct Csr {
    pub offsets: Vec<u32>,
    pub targets: Vec<u32>,
}
impl Csr {
    #[inline]
    pub fn row(&self, k: usize) -> &[u32] {
        match (self.offsets.get(k), self.offsets.get(k + 1)) {
            (Some(a), Some(b)) => &self.targets[*a as usize..*b as usize],
            _ => &[],
        }
    }
    /// Build from (key, target) pairs given twice by `each` (a count pass and a fill pass); rows keep the
    /// order the pairs come in.
    pub fn build(&mut self, keys: usize, each: &mut dyn FnMut(&mut dyn FnMut(usize, u32))) {
        self.offsets.clear();
        self.offsets.resize(keys + 1, 0);
        each(&mut |k, _| self.offsets[k + 1] += 1);
        for k in 0..keys {
            self.offsets[k + 1] += self.offsets[k];
        }
        self.targets.clear();
        self.targets.resize(self.offsets[keys] as usize, 0);
        // fill using the offsets as cursors, then shift them back
        each(&mut |k, t| {
            let at = self.offsets[k];
            self.targets[at as usize] = t;
            self.offsets[k] = at + 1;
        });
        for k in (1..=keys).rev() {
            self.offsets[k] = self.offsets[k - 1];
        }
        self.offsets[0] = 0;
    }
    pub fn len(&self) -> usize {
        self.targets.len()
    }
    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_csr_keeps_each_keys_targets_in_order() {
        let pairs = [(3usize, 30u32), (0, 1), (3, 31), (2, 20), (0, 2), (3, 32)];
        let mut c = Csr::default();
        c.build(5, &mut |f| {
            for (k, t) in pairs {
                f(k, t)
            }
        });
        assert_eq!(c.row(0), &[1, 2]);
        assert_eq!(c.row(1), &[] as &[u32]);
        assert_eq!(c.row(2), &[20]);
        assert_eq!(c.row(3), &[30, 31, 32]);
        assert_eq!(c.row(4), &[] as &[u32]);
        assert_eq!(c.row(99), &[] as &[u32]);
        assert_eq!(c.len(), 6);
    }

    #[test]
    fn a_removed_records_handle_is_refused_and_its_index_reused() {
        let mut s = FlatStore::default();
        let a = s.alloc();
        let ida = s.id(a);
        assert_eq!(s.resolve(ida), Some(a));
        s.release(a);
        assert_eq!(s.resolve(ida), None);
        let b = s.alloc();
        assert_eq!(b, a);
        assert_eq!(s.resolve(ida), None, "the old handle does not name the new record");
        assert_eq!(s.resolve(s.id(b)), Some(b));
    }

    #[test]
    fn a_window_holds_the_shadows_and_the_blur() {
        let mut s = FlatStore::default();
        let i = s.alloc();
        s.rects[i] = Rect::new(100, 100, 200, 160);
        assert_eq!(s.window(i, false), s.rects[i]);
        let e = s.alloc_effect(i);
        s.effect_id[i] = e as u32;
        s.effects[e][ef::SH0_COLOR] = 0x8000_0000;
        s.effects[e][ef::SH0_DY] = 8;
        s.effects[e][ef::SH0_BLUR] = 12;
        s.effects[e][ef::SH0_SPREAD] = 2;
        assert_eq!(s.window(i, false), Rect::new(80, 88, 220, 188));
        assert_eq!(s.window(i, true), s.rects[i]);
        s.effects[e][ef::ELEM_BLUR] = 30;
        assert_eq!(s.window(i, false), Rect::new(70, 70, 230, 190));
        s.set_prop(i, Prop::X, 10);
        assert_eq!((s.prop(i, Prop::X), s.prop(i, Prop::W)), (10, 100));
    }
}
