//! The driver: a world of records, the turn that keeps them current, and the frame that repaints what changed.
//!
//! **A warm turn** ([`FlatWorld::settle`]) is: drain the state block's dirty words; for each word that really
//! changed, walk its CSR rows (bindings of records, of effects and animations, of spaces) and re-evaluate
//! exactly those bindings; a binding whose value differs writes its column and *touches* its record (the
//! record's window before the write is kept in `old_rects`); then every touched record hands
//! `clip(old window) ∪ clip(new window)` to the damage engine at its paint order. Hover is two keyed rows (the
//! site left, the site entered). A theme switch visits no record: fills bound to the palette are resolved when
//! painted, and the tiles such records touch are a mask kept beside the store. A scroll is a rigid shift of a
//! space: one blit and its seams. None of it allocates, and none of it depends on how many records exist.
//!
//! **A cold edit** (records added or removed) is finished by [`FlatWorld::commit`], which rebuilds the paint
//! order, the CSR rows and the spatial index: O(records), and it may allocate.
//!
//! **A frame** ([`FlatWorld::frame`]) settles, lets the engine carry the damage through the blur layers (each
//! layer keeps the backdrop it reads; the engine's gate repaints the part that may have changed and stops the
//! propagation where the pixels came out the same), applies the blits and repaints the dirty tile spans.
use super::expr::{Dep, EvalCx, ExprTape, HOVER_DEPTHS, NO_SITE, Palette, Val, each_dep, eval, lerp_color, lerp_i32, site_depth, spring_q8};
use super::raster::{BlurScratch, BoxPaint, Shadow, Surface, paint_backdrop_blur, paint_box, paint_shadow, paint_text};
use super::state::{DepMask, Slot, SlotId, SlotValue, StateBlock};
use super::store::{Bind, Csr, Effect, FlatStore, Kind, NONE, PROPS, Prop, RecId, RecordDesc, SpaceId, Target, TextRun, TextSrc, ef, flag, touch};
use crate::damage_engine::{BlurLayer, DamageEngine, DamageReport, ExecutorChoice, OpaqueCover, Rect, ScrollBlit, TileMask, Work, blur_level, mutant, opaque_core};

/// Paints records of kind [`Kind::Ext`]: the embedder's shaped text and images. It must be a pure function of
/// its arguments (the oracles and tile repair rest on that).
pub trait ExtPaint {
    /// Paint into `s`, inside `clip` (window coordinates, already inside the surface), the record whose box is
    /// `rect`, with `color` (straight alpha) under `opacity`.
    fn paint(&mut self, s: &mut Surface, rect: Rect, clip: Rect, color: u32, opacity: u32, words: [u32; 4]);
}
/// Paints nothing.
pub struct NoExt;
impl ExtPaint for NoExt {
    fn paint(&mut self, _: &mut Surface, _: Rect, _: Rect, _: u32, _: u32, _: [u32; 4]) {}
}

mod cause {
    pub const SCROLL: u8 = 1;
    pub const CLIP: u8 = 2;
    pub const PORT: u8 = 4;
}

/// A coordinate space: where its records are placed and what they are cut to. A space hangs off a port record
/// of its parent space; its origin is the port's corner less the scroll offset, its clip the parent's clip cut
/// to the port while `clip_on` is not 0. Scrolling and `clip_overflow` are two words of this row.
#[derive(Clone, Copy, Debug, Default)]
pub struct Space {
    pub parent: u32,
    pub port: u32,
    pub clip_on: u32,
    pub sx: i32,
    pub sy: i32,
    /// The window position of the space's (0, 0).
    pub origin: (i32, i32),
    /// What its records are cut to, in the window.
    pub clip: Rect,
    old_origin: (i32, i32),
    old_clip: Rect,
    /// Paint orders of the records of this space and the spaces under it.
    pub z_lo: u32,
    pub z_hi: u32,
    /// The turn its origin or clip last changed; the turn its own words last changed, and which.
    stamp: u32,
    own: u32,
    cause: u8,
    band_first: u32,
    band0: i32,
    band_n: u32,
}

#[derive(Clone, Copy, Debug)]
struct Anim {
    dur_ms: u32,
    color: bool,
    spring: bool,
    from: u32,
    to: u32,
    start: u32,
    active: bool,
}

#[derive(Clone, Debug)]
struct Backdrop {
    rec: u32,
    layer: BlurLayer,
    /// Everything below the layer, over the layer's read rect cut to the window.
    surf: Surface,
    valid: bool,
    /// Painted whole this frame: the gate has nothing to compare against.
    rebuilt: bool,
}

/// The spatial index: per space, 64-pixel bands of the space's own coordinates (a scroll does not move them).
#[derive(Clone, Debug, Default)]
struct Index {
    bands: Csr,
    /// Records too tall for bands, and records that moved since the bands were built.
    wide: Vec<u32>,
    loose: Vec<u32>,
}
const BAND_SHIFT: u32 = 6;
const WIDE_BANDS: i32 = 64;

#[derive(Clone, Debug, Default)]
struct Scratch {
    cand: Vec<u32>,
    seen: Vec<u32>,
    seen_epoch: u32,
    blur: BlurScratch,
    save: Vec<u32>,
}

/// Counters, cumulative since the world was made (a test reads them before and after).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FlatStats {
    pub turns: u64,
    pub frames: u64,
    pub idle_frames: u64,
    pub slots_changed: u64,
    /// Bindings re-evaluated by turns.
    pub binds_evaluated: u64,
    pub records_touched: u64,
    /// Records entered by paint passes (surface repair and backdrop repair).
    pub records_painted: u64,
    pub spans_painted: u64,
    pub tiles_repaired: u64,
    pub blits: u64,
    pub commits: u64,
    pub reindexes: u64,
    pub cover_rebuilds: u64,
    pub layer_rebuilds: u64,
    pub backdrop_repairs: u64,
}

/// What a frame did.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameInfo {
    pub idle: bool,
    pub dirty_tiles: u32,
    pub total_tiles: u32,
    pub repair_percent: f32,
    pub occluded_tiles_skipped: u32,
    pub blits: u32,
    pub choice: ExecutorChoice,
    pub reason: &'static str,
}

/// See the module documentation.
#[derive(Clone, Debug)]
pub struct FlatWorld {
    pub state: StateBlock,
    pub tape: ExprTape,
    pub palette: Palette,
    pub store: FlatStore,
    pub engine: DamageEngine,
    pub stats: FlatStats,
    spaces: Vec<Space>,
    binds: Vec<Bind>,
    /// For each bind: the record whose life it shares (and that record's generation), or `NONE`.
    bind_owner: Vec<(u32, u32)>,
    /// State word -> bindings of record properties and texts.
    slot_offsets_records: Csr,
    /// State word -> bindings of spaces (scroll offsets, clip switches).
    slot_spaces: Csr,
    /// State word -> bindings of effect rows and animation goals.
    slot_layers: Csr,
    /// Hover site -> bindings; animation -> bindings.
    site_index: Csr,
    anim_index: Csr,
    /// Space -> its records.
    space_records: Csr,
    anims: Vec<Anim>,
    anim_target: Vec<Val>,
    anim_vals: Vec<u32>,
    active: Vec<u32>,
    hover: [SlotId; HOVER_DEPTHS],
    time: Slot<u32>,
    theme: Slot<u32>,
    sites: u32,
    /// Live records in paint order.
    order: Vec<u32>,
    next_key: u64,
    index: Index,
    sc: Scratch,
    surface: Surface,
    backdrops: Vec<Backdrop>,
    backdrops_spare: Vec<Backdrop>,
    all: TileMask,
    /// Every tile a palette-filled record touches.
    pal_mask: TileMask,
    w: i32,
    h: i32,
    epoch: u32,
    touched: Vec<u32>,
    changed: Vec<(SlotId, u32, u32)>,
    added: Vec<u32>,
    /// Records removed since the last turn: paint key, window in its space, space, work, and the order just
    /// below where it was (set by `commit`).
    removed: Vec<(u64, Rect, u32, Work, u32)>,
    /// The rigid scrolls of the turn being settled: (root space, blit).
    epoch_blits: Vec<(u32, ScrollBlit)>,
    spaces_committed: usize,
    sub_root: Vec<u32>,
    roots: Vec<u32>,
    structure_dirty: bool,
    spaces_dirty: bool,
    covers_dirty: bool,
    layers_dirty: bool,
    pal_dirty: bool,
    palette_changed: bool,
    invalid: bool,
    frame_open: bool,
    blits_blocked: bool,
    blur_records: u32,
}

struct Painter<'a> {
    store: &'a FlatStore,
    spaces: &'a [Space],
    palette: &'a Palette,
    theme: u32,
    backdrops: &'a [Backdrop],
}

impl Painter<'_> {
    #[inline]
    fn color(&self, i: usize, v: u32, pal: u32) -> u32 {
        if self.store.flags[i] & pal != 0 { self.palette.get(self.theme, v as u16) } else { v }
    }
    /// Paint record `i` inside `cut`.
    fn record(&self, t: &mut Surface, cut: Rect, i: usize, blur: &mut BlurScratch, ext: &mut dyn ExtPaint) {
        let st = self.store;
        let sp = &self.spaces[st.clips[i] as usize];
        let clip = sp.clip.and(cut).and(t.bounds());
        let opacity = st.opacity[i];
        if clip.is_empty() || opacity == 0 {
            return;
        }
        let r = st.rects[i].shift(sp.origin.0, sp.origin.1);
        if !st.window(i, false).shift(sp.origin.0, sp.origin.1).hits(clip) {
            return;
        }
        let fill = self.color(i, st.fill[i], flag::PAL_FILL);
        let mut bp = BoxPaint { fill, border_color: 0, border_w: st.border_w[i], radius: st.radius[i], opacity, grad_dir: 0, grad_to: 0, elem_blur: 0 };
        if let Some(e) = st.effect(i) {
            for o in [ef::SH0_COLOR, ef::SH1_COLOR] {
                let sh = Shadow { color: e[o], dx: e[o + 1] as i32, dy: e[o + 2] as i32, blur: e[o + 3] as i32, spread: e[o + 4] as i32 };
                paint_shadow(t, clip, r, st.radius[i], &sh, opacity);
            }
            if e[ef::BACKDROP] != 0 {
                if let Some(b) = self.backdrops.iter().find(|b| b.rec == i as u32) {
                    paint_backdrop_blur(t, clip, r, st.radius[i], b.layer.max_level, opacity, &b.surf, blur);
                }
            }
            bp.grad_dir = e[ef::GRAD_DIR];
            bp.grad_to = e[ef::GRAD_TO];
            bp.elem_blur = e[ef::ELEM_BLUR] as i32;
        }
        match st.kinds[i] {
            k if k == Kind::Box as u8 => {
                if bp.border_w != 0 {
                    bp.border_color = self.color(i, st.border[i], flag::PAL_BORDER);
                }
                if fill >> 24 != 0 || bp.border_w != 0 || bp.grad_dir != 0 {
                    paint_box(t, clip, r, &bp);
                }
            }
            k if k == Kind::Text as u8 => {
                if let Some(run) = st.texts.get(st.text_run[i] as usize) {
                    paint_text(t, clip, r, &run.buf, run.scale, fill, opacity);
                }
            }
            k if k == Kind::Ext as u8 => {
                if let Some(TextRun { src: TextSrc::Ext(words), .. }) = st.texts.get(st.text_run[i] as usize) {
                    ext.paint(t, r, clip.and(r), fill, opacity, *words);
                }
            }
            _ => {}
        }
    }
}

/// The records whose windows touch `region` of the window, each once, in no order.
fn collect(store: &FlatStore, spaces: &[Space], index: &Index, region: Rect, sc: &mut Scratch) {
    sc.cand.clear();
    if sc.seen.len() < store.len() {
        sc.seen.resize(store.len(), 0);
    }
    sc.seen_epoch = sc.seen_epoch.wrapping_add(1);
    if sc.seen_epoch == 0 {
        sc.seen.fill(0);
        sc.seen_epoch = 1;
    }
    let e = sc.seen_epoch;
    for s in spaces {
        let c = region.and(s.clip);
        if c.is_empty() || s.band_n == 0 {
            continue;
        }
        let l = c.shift(-s.origin.0, -s.origin.1);
        let b0 = (l.y0 >> BAND_SHIFT).max(s.band0);
        let b1 = ((l.y1 - 1) >> BAND_SHIFT).min(s.band0 + s.band_n as i32 - 1);
        for b in b0..=b1 {
            for &i in index.bands.row((s.band_first as i32 + (b - s.band0)) as usize) {
                let iu = i as usize;
                if sc.seen[iu] != e {
                    sc.seen[iu] = e;
                    if store.is_live(iu) && store.window(iu, false).hits(l) {
                        sc.cand.push(i);
                    }
                }
            }
        }
    }
    for &i in index.wide.iter().chain(&index.loose) {
        let iu = i as usize;
        if iu < store.len() && sc.seen[iu] != e && store.is_live(iu) {
            sc.seen[iu] = e;
            let s = &spaces[store.clips[iu] as usize];
            if store.window(iu, false).shift(s.origin.0, s.origin.1).hits(region.and(s.clip)) {
                sc.cand.push(i);
            }
        }
    }
}

/// Paint, in paint order, the records below `z_limit` that touch `region`.
fn paint_region(p: &Painter<'_>, index: &Index, t: &mut Surface, region: Rect, z_limit: u32, sc: &mut Scratch, ext: &mut dyn ExtPaint) -> u64 {
    collect(p.store, p.spaces, index, region, sc);
    let z = &p.store.z_order;
    sc.cand.sort_unstable_by_key(|i| z[*i as usize]);
    let mut n = 0;
    for k in 0..sc.cand.len() {
        let i = sc.cand[k] as usize;
        if z[i] >= z_limit {
            break;
        }
        p.record(t, region, i, &mut sc.blur, ext);
        n += 1;
    }
    n
}

impl FlatWorld {
    /// A world for a window of `w` x `h` device pixels, holding nothing.
    pub fn new(w: i32, h: i32) -> FlatWorld {
        let mut state = StateBlock::new();
        let mut hover = [SlotId(0); HOVER_DEPTHS];
        for hv in hover.iter_mut() {
            *hv = state.alloc(NO_SITE).id();
        }
        let (time, theme) = (state.alloc(0u32), state.alloc(0u32));
        let win = Rect::new(0, 0, w.max(0), h.max(0));
        let mut all = TileMask::new(w, h);
        all.mark_all();
        FlatWorld {
            state,
            tape: ExprTape::default(),
            palette: Palette::default(),
            store: FlatStore::default(),
            engine: DamageEngine::new(w, h),
            stats: FlatStats::default(),
            spaces: vec![Space { parent: NONE, port: NONE, clip: win, old_clip: win, ..Space::default() }],
            binds: Vec::new(),
            bind_owner: Vec::new(),
            slot_offsets_records: Csr::default(),
            slot_spaces: Csr::default(),
            slot_layers: Csr::default(),
            site_index: Csr::default(),
            anim_index: Csr::default(),
            space_records: Csr::default(),
            anims: Vec::new(),
            anim_target: Vec::new(),
            anim_vals: Vec::new(),
            active: Vec::new(),
            hover,
            time,
            theme,
            sites: 0,
            order: Vec::new(),
            next_key: 1 << 20,
            index: Index::default(),
            sc: Scratch::default(),
            surface: Surface::new(w, h),
            backdrops: Vec::new(),
            backdrops_spare: Vec::new(),
            all,
            pal_mask: TileMask::new(w, h),
            w: w.max(0),
            h: h.max(0),
            epoch: 2,
            touched: Vec::new(),
            changed: Vec::new(),
            added: Vec::new(),
            removed: Vec::new(),
            epoch_blits: Vec::new(),
            spaces_committed: 0,
            sub_root: Vec::new(),
            roots: Vec::new(),
            structure_dirty: true,
            spaces_dirty: false,
            covers_dirty: true,
            layers_dirty: true,
            pal_dirty: true,
            palette_changed: false,
            invalid: true,
            frame_open: false,
            blits_blocked: false,
            blur_records: 0,
        }
    }
    pub fn size(&self) -> (i32, i32) {
        (self.w, self.h)
    }
    fn window(&self) -> Rect {
        Rect::new(0, 0, self.w, self.h)
    }
    /// Take a new window size: everything is repainted.
    pub fn resize(&mut self, w: i32, h: i32) {
        self.w = w.max(0);
        self.h = h.max(0);
        self.engine.resize(w, h);
        self.surface = Surface::new(w, h);
        self.all = TileMask::new(w, h);
        self.all.mark_all();
        self.pal_mask = TileMask::new(w, h);
        self.backdrops.clear();
        self.frame_open = false;
        self.invalid = true;
        self.spaces_dirty = true;
        self.covers_dirty = true;
        self.layers_dirty = true;
        self.pal_dirty = true;
        // the root's clip is the window
        self.spaces[0].own = self.epoch;
        self.spaces[0].cause |= cause::CLIP;
    }
    #[inline]
    fn bare(&self) -> bool {
        self.engine.mutant(mutant::NO_EFFECT_HULL)
    }

    // ---- state ----

    /// The clock word (milliseconds): what [`super::expr::ExprOp::TimeProg`] and animations read.
    pub fn time_slot(&self) -> Slot<u32> {
        self.time
    }
    /// The word naming the palette's current theme.
    pub fn theme_slot(&self) -> Slot<u32> {
        self.theme
    }
    pub fn set_time(&mut self, ms: u32) {
        self.state.set(self.time, ms);
    }
    pub fn set_theme(&mut self, theme: u32) {
        self.state.set(self.theme, theme);
    }
    pub fn slot<T: SlotValue>(&mut self, init: T) -> Slot<T> {
        self.state.alloc(init)
    }
    #[track_caller]
    pub fn set<T: SlotValue>(&mut self, s: Slot<T>, v: T) -> bool {
        self.state.set(s, v)
    }
    #[track_caller]
    pub fn get<T: SlotValue>(&self, s: Slot<T>) -> T {
        self.state.get(s)
    }
    /// Replace the palette: `per_theme` colours for each theme, theme after theme.
    pub fn set_palette(&mut self, per_theme: u32, colors: &[u32]) {
        self.palette.per_theme = per_theme;
        self.palette.colors.clear();
        self.palette.colors.extend_from_slice(colors);
        self.palette_changed = true;
    }
    /// A palette colour as a value bound to the world's theme word.
    pub fn pal(&self, slot: u16) -> Val {
        Val::Palette { slot, theme_slot: self.theme.id().0 }
    }
    /// A new hover site of `depth` (0 ..= 7): records of different depths can be hovered together (a panel
    /// and the row inside it).
    pub fn add_site(&mut self, depth: u32) -> u32 {
        assert!((depth as usize) < HOVER_DEPTHS && self.sites < (1 << 24) - 1);
        self.sites += 1;
        self.structure_dirty = true;
        depth << 24 | (self.sites - 1)
    }
    /// `on` while `site` is hovered, else `off`.
    pub fn hover_sel(&self, site: u32, off: u32, on: u32) -> Val {
        Val::HoverSel { site, off, on }
    }
    /// A value that follows `target` over `dur_ms` instead of jumping: colours per channel if `color`, else
    /// as integers; along a critically damped spring if `spring`, else linearly. Driven by the clock word.
    pub fn add_anim(&mut self, target: Val, dur_ms: u32, color: bool, spring: bool) -> Val {
        assert!(!matches!(target, Val::Anim(_)), "an animation follows a value, not another animation");
        let a = self.anims.len() as u32;
        self.anims.push(Anim { dur_ms, color, spring, from: 0, to: 0, start: 0, active: false });
        self.anim_target.push(target);
        self.anim_vals.push(0);
        self.push_bind(Target::Anim, a, target, NONE);
        self.structure_dirty = true;
        Val::Anim(a)
    }
    pub fn anims_active(&self) -> bool {
        !self.active.is_empty()
    }

    // ---- description ----

    fn push_bind(&mut self, target: Target, idx: u32, val: Val, owner: u32) {
        self.binds.push(Bind { target, idx, val, fresh: true });
        self.bind_owner.push(if owner == NONE { (NONE, 0) } else { (owner, self.store.gens[owner as usize]) });
    }
    /// A space under `parent`, placed at the corner of `port` (a record of `parent`; `None`: at the parent's
    /// origin), cut to the port while `clip_on` is not 0, scrolled by (`sx`, `sy`).
    pub fn add_space(&mut self, parent: SpaceId, port: Option<RecId>, clip_on: impl Into<Val>, sx: impl Into<Val>, sy: impl Into<Val>) -> SpaceId {
        assert!((parent.0 as usize) < self.spaces.len(), "a space of this world");
        let port = match port {
            Some(id) => {
                let i = self.store.resolve(id).expect("a live port record");
                assert_eq!(self.store.clips[i], parent.0, "a port is a record of the parent space");
                self.store.flags[i] |= flag::PORT;
                i as u32
            }
            None => NONE,
        };
        let j = self.spaces.len() as u32;
        self.spaces.push(Space { parent: parent.0, port, ..Space::default() });
        for (target, v) in [(Target::SpaceClip, clip_on.into()), (Target::SpaceSx, sx.into()), (Target::SpaceSy, sy.into())] {
            match v {
                Val::Const(c) => {
                    self.write_space(j as usize, target, c);
                }
                v => self.push_bind(target, j, v, NONE),
            }
        }
        self.structure_dirty = true;
        SpaceId(j)
    }
    fn write_space(&mut self, j: usize, target: Target, v: u32) -> bool {
        let s = &mut self.spaces[j];
        let (cur, bit) = match target {
            Target::SpaceClip => (s.clip_on, cause::CLIP),
            Target::SpaceSx => (s.sx as u32, cause::SCROLL),
            _ => (s.sy as u32, cause::SCROLL),
        };
        if cur == v {
            return false;
        }
        match target {
            Target::SpaceClip => s.clip_on = v,
            Target::SpaceSx => s.sx = v as i32,
            _ => s.sy = v as i32,
        }
        if s.own != self.epoch {
            s.own = self.epoch;
            s.cause = 0;
        }
        s.cause |= bit;
        true
    }
    pub fn space(&self, s: SpaceId) -> &Space {
        &self.spaces[s.0 as usize]
    }
    pub fn spaces(&self) -> usize {
        self.spaces.len()
    }
    /// Add a record on top of everything described so far.
    pub fn add_record(&mut self, d: RecordDesc) -> RecId {
        let key = self.next_key;
        self.next_key += 1 << 20;
        self.describe(d, key)
    }
    /// Add a record painted just below `anchor`.
    pub fn insert_before(&mut self, anchor: RecId, d: RecordDesc) -> RecId {
        let a = self.store.resolve(anchor).expect("a live anchor record");
        let ak = self.store.z_key[a];
        // the largest key under the anchor's (records added since the last commit included)
        let mut below = 0u64;
        for i in 0..self.store.len() {
            if self.store.is_live(i) && self.store.z_key[i] < ak {
                below = below.max(self.store.z_key[i]);
            }
        }
        if ak - below < 2 {
            // no room: spread every key out again, in order
            let mut live: Vec<usize> = (0..self.store.len()).filter(|i| self.store.is_live(*i)).collect();
            live.sort_unstable_by_key(|i| self.store.z_key[*i]);
            for (rank, i) in live.iter().enumerate() {
                self.store.z_key[*i] = (rank as u64 + 1) << 20;
            }
            self.next_key = (live.len() as u64 + 1) << 20;
            return self.insert_before(anchor, d);
        }
        self.describe(d, below + (ak - below) / 2)
    }
    fn describe(&mut self, d: RecordDesc, key: u64) -> RecId {
        assert!((d.space.0 as usize) < self.spaces.len(), "a space of this world");
        let i = self.store.alloc();
        let st = &mut self.store;
        st.clips[i] = d.space.0;
        st.z_key[i] = key;
        st.z_order[i] = u32::MAX;
        st.kinds[i] = d.kind as u8;
        st.rects[i] = Rect::EMPTY;
        st.fill[i] = 0;
        st.border[i] = 0;
        st.border_w[i] = 0;
        st.radius[i] = 0;
        st.opacity[i] = 255;
        st.handlers[i] = d.handler;
        st.site[i] = d.site;
        st.item_key[i] = d.key;
        st.touch_epoch[i] = 0;
        if d.hit {
            st.flags[i] |= flag::HIT;
        }
        let theme = self.theme.id().0;
        for (p, v) in PROPS.into_iter().zip([d.x, d.y, d.w, d.h, d.fill, d.border, d.border_w, d.radius, d.opacity]) {
            match v {
                Val::Const(c) => self.store.set_prop(i, p, c),
                // a fill or a border on the world's theme is resolved when painted: no binding, no visit
                Val::Palette { slot, theme_slot } if theme_slot == theme && matches!(p, Prop::Fill | Prop::Border) => {
                    self.store.set_prop(i, p, slot as u32);
                    self.store.flags[i] |= if p == Prop::Fill { flag::PAL_FILL } else { flag::PAL_BORDER };
                }
                v => self.push_bind(Target::Prop(p), i as u32, v, i as u32),
            }
        }
        if let Some(Effect(fields)) = d.effect {
            let e = self.store.alloc_effect(i);
            self.store.effect_id[i] = e as u32;
            for (f, v) in fields.into_iter().enumerate() {
                match v {
                    Val::Const(c) => self.store.effects[e][f] = c,
                    v => self.push_bind(Target::Effect(f as u8), e as u32, v, i as u32),
                }
            }
        }
        if let Some(src) = d.text {
            let (buf, dep) = match &src {
                TextSrc::Static(s) => (s.clone(), None),
                TextSrc::Num { slot, .. } => (String::new(), Some(*slot)),
                TextSrc::Str(s) => (String::new(), Some(s.id())),
                TextSrc::Ext(_) => (String::new(), None),
            };
            let run = self.store.alloc_text(TextRun { src, buf, scale: d.text_scale.max(1), owner: i as u32 });
            self.store.text_run[i] = run as u32;
            if let Some(slot) = dep {
                self.push_bind(Target::Text, run as u32, Val::Slot(slot.0), i as u32);
            }
        }
        self.added.push(i as u32);
        self.structure_dirty = true;
        self.store.id(i)
    }
    /// Remove a record. Returns whether the handle named a live one.
    pub fn remove(&mut self, id: RecId) -> bool {
        let Some(i) = self.store.resolve(id) else {
            return false;
        };
        if self.store.z_order[i] != u32::MAX {
            // it has been on screen: where it was is damaged by the next turn
            self.removed.push((self.store.z_key[i], self.store.window(i, false), self.store.clips[i], self.work_of(i), 0));
        }
        if self.store.effect(i).is_some_and(|e| e[ef::BACKDROP] != 0) {
            self.layers_dirty = true;
        }
        self.store.release(i);
        self.structure_dirty = true;
        true
    }
    /// Write a property that was described as a constant (a re-description found it different). Returns
    /// whether it changed; the next turn damages the record where it was and where it is.
    pub fn patch(&mut self, id: RecId, p: Prop, v: u32) -> bool {
        let Some(i) = self.store.resolve(id) else {
            return false;
        };
        if self.store.prop(i, p) == v {
            return false;
        }
        let geom = matches!(p, Prop::X | Prop::Y | Prop::W | Prop::H | Prop::Radius | Prop::BorderW);
        self.touch(i, if geom { touch::GEOM } else { touch::PAINT });
        self.store.set_prop(i, p, v);
        true
    }
    /// [`FlatWorld::patch`] for the whole box (in the record's space).
    pub fn patch_rect(&mut self, id: RecId, r: Rect) -> bool {
        let Some(i) = self.store.resolve(id) else {
            return false;
        };
        if self.store.rects[i] == r {
            return false;
        }
        self.touch(i, touch::GEOM);
        self.store.rects[i] = r;
        true
    }
    /// Replace the words of an [`Kind::Ext`] record.
    pub fn patch_ext(&mut self, id: RecId, words: [u32; 4]) -> bool {
        let Some(i) = self.store.resolve(id) else {
            return false;
        };
        let Some(run) = self.store.texts.get(self.store.text_run[i] as usize) else {
            return false;
        };
        if run.src == TextSrc::Ext(words) {
            return false;
        }
        self.touch(i, touch::PAINT);
        let run = self.store.text_run[i] as usize;
        self.store.texts[run].src = TextSrc::Ext(words);
        true
    }
    /// Whether anything waits for a turn: a written word, a patch, a structural edit.
    pub fn pending(&self) -> bool {
        self.state.any_dirty() || !self.touched.is_empty() || self.structure_dirty || self.invalid || !self.active.is_empty() || self.palette_changed || self.spaces_dirty
    }
    pub fn records(&self) -> usize {
        self.store.live as usize
    }
    /// The record's box in the window (not cut to its clip).
    pub fn screen_rect(&self, id: RecId) -> Option<Rect> {
        let i = self.store.resolve(id)?;
        let s = &self.spaces[self.store.clips[i] as usize];
        Some(self.store.rects[i].shift(s.origin.0, s.origin.1))
    }

    // ---- evaluation ----

    #[inline]
    fn eval_val(&self, v: Val) -> u32 {
        eval(v, &EvalCx { state: &self.state, tape: &self.tape, palette: &self.palette, anims: &self.anim_vals, hover: &self.hover })
    }
    fn format_text(&mut self, run: usize) {
        use std::fmt::Write;
        let t = &mut self.store.texts[run];
        match &t.src {
            TextSrc::Num { prefix, slot, suffix } => {
                t.buf.clear();
                let _ = write!(t.buf, "{prefix}{}{suffix}", self.state.word(*slot));
            }
            TextSrc::Str(s) => {
                t.buf.clear();
                t.buf.push_str(self.state.str_at(s.index));
            }
            _ => {}
        }
    }
    #[inline]
    fn touch(&mut self, i: usize, what: u8) {
        let st = &mut self.store;
        if st.touch_epoch[i] != self.epoch {
            st.touch_epoch[i] = self.epoch;
            st.touch[i] = 0;
            st.old_rects[i] = st.window(i, self.engine.mutant(mutant::NO_EFFECT_HULL));
            self.touched.push(i as u32);
        }
        st.touch[i] |= what;
    }
    /// Re-evaluate one binding; write and touch if its value changed.
    fn apply_bind(&mut self, bi: usize) {
        let b = self.binds[bi];
        let new = self.eval_val(b.val);
        self.stats.binds_evaluated += 1;
        let idx = b.idx as usize;
        match b.target {
            Target::Prop(p) => {
                if self.store.prop(idx, p) != new {
                    // a radius or a border width changes the opaque core, which is geometry to the engine
                    let geom = matches!(p, Prop::X | Prop::Y | Prop::W | Prop::H | Prop::Radius | Prop::BorderW);
                    self.touch(idx, if geom { touch::GEOM } else { touch::PAINT });
                    self.store.set_prop(idx, p, new);
                }
            }
            Target::Effect(f) => {
                if self.store.effects[idx][f as usize] != new {
                    let owner = self.store.effect_owner[idx] as usize;
                    self.touch(owner, touch::GEOM);
                    self.store.effects[idx][f as usize] = new;
                }
            }
            Target::Text => {
                let owner = self.store.texts[idx].owner as usize;
                self.touch(owner, touch::PAINT);
                self.format_text(idx);
            }
            Target::SpaceClip | Target::SpaceSx | Target::SpaceSy => {
                if self.write_space(idx, b.target, new) {
                    self.spaces_dirty = true;
                }
            }
            Target::Anim => {
                if self.anims[idx].to != new {
                    let now = self.state.get(self.time);
                    let a = &mut self.anims[idx];
                    a.from = self.anim_vals[idx];
                    a.to = new;
                    a.start = now;
                    if !a.active {
                        a.active = true;
                        self.active.push(idx as u32);
                    }
                }
            }
        }
    }
    /// Evaluate one binding and write it, with no comparison and no damage (a mount).
    fn write_bind(&mut self, bi: usize) {
        let b = self.binds[bi];
        let new = self.eval_val(b.val);
        let idx = b.idx as usize;
        match b.target {
            Target::Prop(p) => self.store.set_prop(idx, p, new),
            Target::Effect(f) => self.store.effects[idx][f as usize] = new,
            Target::Text => self.format_text(idx),
            Target::SpaceClip => self.spaces[idx].clip_on = new,
            Target::SpaceSx => self.spaces[idx].sx = new as i32,
            Target::SpaceSy => self.spaces[idx].sy = new as i32,
            Target::Anim => {
                let a = &mut self.anims[idx];
                a.from = new;
                a.to = new;
                a.active = false;
                self.anim_vals[idx] = new;
            }
        }
    }
    fn fan_row(&mut self, which: u8, k: usize) {
        let drop_last = which == 0 && self.engine.mutant(mutant::CSR_DROP_LAST);
        let mut j = 0;
        loop {
            let row = match which {
                0 => self.slot_offsets_records.row(k),
                1 => self.slot_layers.row(k),
                2 => self.slot_spaces.row(k),
                3 => self.site_index.row(k),
                _ => self.anim_index.row(k),
            };
            let n = if drop_last { row.len().saturating_sub(1) } else { row.len() };
            if j >= n {
                self.engine.note_fanout(n as u32);
                return;
            }
            let bi = row[j] as usize;
            j += 1;
            self.apply_bind(bi);
        }
    }

    // ---- predicates over a record ----

    fn is_cover(&self, i: usize) -> bool {
        let st = &self.store;
        if st.kinds[i] != Kind::Box as u8 || st.opacity[i] != 255 {
            return false;
        }
        let opaque = if st.flags[i] & flag::PAL_FILL != 0 { self.palette.always_opaque(st.fill[i] as u16) } else { st.fill[i] >> 24 == 255 };
        opaque && st.effect(i).is_none_or(|e| e[ef::ELEM_BLUR] == 0 && (e[ef::GRAD_DIR] == 0 || e[ef::GRAD_TO] >> 24 == 255))
    }
    /// The part of a cover that is its fill alone, in the window, cut to its clip.
    fn cover_core(&self, i: usize) -> Rect {
        let st = &self.store;
        let s = &self.spaces[st.clips[i] as usize];
        let mut c = opaque_core(st.rects[i], ((st.radius[i] + 255) >> 8) as i32);
        if st.border_w[i] != 0 {
            c = c.expand(-(((st.border_w[i] + 128) >> 8) as i32 + 1));
            if c.is_empty() {
                return Rect::EMPTY;
            }
        }
        c.shift(s.origin.0, s.origin.1).and(s.clip)
    }
    fn is_uniform(&self, i: usize) -> bool {
        self.store.flags[i] & flag::COVER != 0 && self.store.effect(i).is_none_or(|e| e[ef::GRAD_DIR] == 0)
    }
    fn work_of(&self, i: usize) -> Work {
        match self.store.effect(i) {
            Some(e) if e[ef::SH0_COLOR] >> 24 != 0 || e[ef::SH1_COLOR] >> 24 != 0 => Work::Shadow,
            Some(e) if e[ef::GRAD_DIR] != 0 => Work::Gradient,
            _ => Work::Flat,
        }
    }
    fn screen_window(&self, i: usize) -> Rect {
        let s = &self.spaces[self.store.clips[i] as usize];
        self.store.window(i, self.bare()).shift(s.origin.0, s.origin.1).and(s.clip)
    }

    // ---- spaces ----

    fn space_geom(&self, j: usize) -> ((i32, i32), Rect) {
        let s = &self.spaces[j];
        let (po, pc) = if s.parent == NONE { ((0, 0), self.window()) } else { (self.spaces[s.parent as usize].origin, self.spaces[s.parent as usize].clip) };
        let port = (s.port != NONE && self.store.is_live(s.port as usize)).then(|| self.store.rects[s.port as usize]);
        let (px, py) = port.map_or((0, 0), |r| (r.x0, r.y0));
        let origin = (po.0 + px - s.sx, po.1 + py - s.sy);
        let clip = match port {
            Some(r) if s.clip_on != 0 => pc.and(r.shift(po.0, po.1)),
            _ => pc,
        };
        (origin, clip)
    }
    /// Recompute every space; returns whether any moved or was cut differently. Changed spaces keep what they
    /// were in `old_origin` / `old_clip` and are stamped with this turn.
    fn respace(&mut self, quiet: bool, from: usize) -> bool {
        let mut any = false;
        for j in from..self.spaces.len() {
            let (o, c) = self.space_geom(j);
            let s = &mut self.spaces[j];
            if quiet {
                (s.origin, s.clip, s.old_origin, s.old_clip) = (o, c, o, c);
            } else if o != s.origin || c != s.clip {
                (s.old_origin, s.old_clip) = (s.origin, s.clip);
                (s.origin, s.clip) = (o, c);
                s.stamp = self.epoch;
                any = true;
            }
        }
        any
    }
    fn spaces_pass(&mut self) {
        if !self.respace(false, 0) {
            return;
        }
        let n = self.spaces.len();
        self.sub_root.clear();
        self.sub_root.resize(n, NONE);
        self.roots.clear();
        for j in 0..n {
            let p = self.spaces[j].parent;
            let pr = if p == NONE { NONE } else { self.sub_root[p as usize] };
            if pr != NONE {
                self.sub_root[j] = pr;
            } else if self.spaces[j].stamp == self.epoch {
                self.sub_root[j] = j as u32;
                self.roots.push(j as u32);
            }
        }
        // a root moves rigidly if all that changed is its own scroll offset, nothing under it changed on its
        // own, and nothing else was damaged before it this frame
        let mut rigid = !self.blits_blocked;
        for k in 0..self.roots.len() {
            let r = self.roots[k] as usize;
            let s = &self.spaces[r];
            rigid &= s.own == self.epoch && s.cause == cause::SCROLL && s.old_clip == s.clip && !s.clip.is_empty();
            rigid &= (0..n).all(|j| j == r || self.sub_root[j] != r as u32 || self.spaces[j].own != self.epoch);
            // two roots at once: only if their ports do not overlap
            rigid &= (0..k).all(|q| !self.spaces[self.roots[q] as usize].clip.hits(s.clip));
        }
        for k in 0..self.roots.len() {
            let r = self.roots[k] as usize;
            if rigid {
                self.scroll_rigid(r);
            } else {
                self.damage_subtree(r);
            }
        }
        self.covers_dirty = true;
        self.pal_dirty = true;
        self.layers_dirty |= self.blur_records != 0;
    }
    fn scroll_rigid(&mut self, r: usize) {
        let s = self.spaces[r];
        if s.z_lo > s.z_hi {
            return;
        }
        let (dx, dy) = (s.origin.0 - s.old_origin.0, s.origin.1 - s.old_origin.1);
        let Some(blit) = self.engine.scroll(s.z_lo, s.z_hi, s.clip, dx, dy) else {
            return;
        };
        self.stats.blits += 1;
        self.epoch_blits.push((r as u32, blit));
        // whatever shows inside the port without moving with it
        collect(&self.store, &self.spaces, &self.index, blit.clip_rect, &mut self.sc);
        for k in 0..self.sc.cand.len() {
            let i = self.sc.cand[k] as usize;
            let st = &self.store;
            if self.sub_root[st.clips[i] as usize] == r as u32 || st.opacity[i] == 0 {
                continue;
            }
            if st.kinds[i] == Kind::None as u8 && st.effect(i).is_none() {
                continue;
            }
            let z = st.z_order[i];
            // an opaque uniform fill below the content that holds the whole port: its shift is itself
            if z < s.z_lo && self.is_uniform(i) && self.cover_core(i).contains(blit.clip_rect) {
                continue;
            }
            let f = self.spaces[st.clips[i] as usize];
            let (now, moved) = (self.screen_window(i), st.touch_epoch[i] == self.epoch && st.touch[i] & touch::GEOM != 0);
            let was = st.old_rects[i].shift(f.origin.0, f.origin.1).and(f.clip);
            self.engine.seam(&blit, z, now);
            if moved {
                self.engine.seam(&blit, z, was);
            }
        }
    }
    /// Damage every record of the spaces under `r` where it was and where it is.
    fn damage_subtree(&mut self, r: usize) {
        for j in 0..self.spaces.len() {
            if self.sub_root[j] != r as u32 {
                continue;
            }
            let s = self.spaces[j];
            let (oo, oc) = if s.stamp == self.epoch { (s.old_origin, s.old_clip) } else { (s.origin, s.clip) };
            for k in 0..self.space_records.row(j).len() {
                let i = self.space_records.row(j)[k] as usize;
                if !self.store.is_live(i) {
                    continue;
                }
                let newl = self.store.window(i, self.bare());
                let was_touched = self.store.touch_epoch[i] == self.epoch;
                let oldl = if was_touched { self.store.old_rects[i] } else { newl };
                let (z, work) = (self.store.z_order[i], self.work_of(i));
                self.engine.damage_move(z, oldl.shift(oo.0, oo.1).and(oc), newl.shift(s.origin.0, s.origin.1).and(s.clip), work);
                if was_touched {
                    self.store.touch[i] |= touch::DONE;
                }
            }
        }
    }

    // ---- the cold path ----

    fn open_frame(&mut self) {
        if !self.frame_open {
            self.engine.begin_frame();
            self.frame_open = true;
            self.blits_blocked = false;
        }
    }
    /// Finish a structural edit: paint order, dependency rows, spatial index. O(records); may allocate.
    pub fn commit(&mut self) {
        if !self.structure_dirty {
            return;
        }
        self.structure_dirty = false;
        self.stats.commits += 1;
        self.open_frame();
        if self.engine.pending() {
            // damage reported earlier this frame names paint orders that are about to be renumbered
            self.engine.rebase_orders();
        }
        // bindings of removed records go with them
        let (st, owners) = (&self.store, &self.bind_owner);
        let mut k = 0;
        self.binds.retain(|_| {
            let (o, g) = owners[k];
            k += 1;
            o == NONE || (st.is_live(o as usize) && st.gens[o as usize] == g)
        });
        self.bind_owner.retain(|(o, g)| *o == NONE || (st.is_live(*o as usize) && st.gens[*o as usize] == *g));
        // paint order
        self.order.clear();
        self.order.extend((0..self.store.len() as u32).filter(|i| self.store.is_live(*i as usize)));
        let keys = &self.store.z_key;
        self.order.sort_unstable_by_key(|i| keys[*i as usize]);
        for (rank, i) in self.order.iter().enumerate() {
            self.store.z_order[*i as usize] = rank as u32 + 1;
        }
        // dependency rows
        let (binds, tape, nslots) = (&self.binds, &self.tape, self.state.len());
        let rows = |want: u8, f: &mut dyn FnMut(usize, u32)| {
            for (bi, b) in binds.iter().enumerate() {
                let class = match b.target {
                    Target::Prop(_) | Target::Text => 0,
                    Target::Effect(_) | Target::Anim => 1,
                    _ => 2,
                };
                each_dep(b.val, tape, &mut |d| match d {
                    Dep::Slot(s) if want == class && (s.0 as usize) < nslots => f(s.0 as usize, bi as u32),
                    Dep::Site(site) if want == 3 => f((site & 0xff_ffff) as usize, bi as u32),
                    Dep::Anim(a) if want == 4 => f(a as usize, bi as u32),
                    _ => {}
                });
            }
        };
        self.slot_offsets_records.build(nslots, &mut |f| rows(0, f));
        self.slot_layers.build(nslots, &mut |f| rows(1, f));
        self.slot_spaces.build(nslots, &mut |f| rows(2, f));
        self.site_index.build(self.sites as usize, &mut |f| rows(3, f));
        self.anim_index.build(self.anims.len(), &mut |f| rows(4, f));
        for m in self.store.dep_mask.iter_mut() {
            *m = DepMask::EMPTY;
        }
        for (b, (o, _)) in self.binds.iter().zip(&self.bind_owner) {
            if *o != NONE {
                let m = &mut self.store.dep_mask[*o as usize];
                each_dep(b.val, &self.tape, &mut |d| {
                    if let Dep::Slot(s) = d {
                        m.add(s)
                    }
                });
            }
        }
        let (store, nspaces) = (&self.store, self.spaces.len());
        self.space_records.build(nspaces, &mut |f| {
            for i in 0..store.len() {
                if store.is_live(i) {
                    f(store.clips[i] as usize, i as u32)
                }
            }
        });
        // what was described since the last commit takes its first values: animation goals first, since the
        // rest may read an animation
        for pass in 0..2 {
            for bi in 0..self.binds.len() {
                let b = self.binds[bi];
                if b.fresh && (pass == 0) == (b.target == Target::Anim) {
                    self.write_bind(bi);
                    self.binds[bi].fresh = false;
                }
            }
        }
        self.reflag();
        // spaces described since the last commit take their places; the others keep theirs until the turn
        // compares them (a port may have been patched)
        self.respace(true, self.spaces_committed);
        self.spaces_committed = self.spaces.len();
        self.spaces_dirty = true;
        self.z_ranges();
        self.reindex();
        self.covers_dirty = true;
        self.layers_dirty = true;
        self.pal_dirty = true;
        for b in &mut self.backdrops {
            b.valid = false;
        }
        // what went away is damaged by the turn, at the order of the record just below where it was: covers and
        // layers above that were above it
        for k in 0..self.removed.len() {
            let key = self.removed[k].0;
            self.removed[k].4 = self.order.partition_point(|i| self.store.z_key[*i as usize] < key) as u32;
        }
    }
    /// Bring the predicate flags of every record in line with its columns.
    fn reflag(&mut self) {
        self.blur_records = 0;
        for i in 0..self.store.len() {
            if !self.store.is_live(i) {
                continue;
            }
            let cover = self.is_cover(i);
            let blur = self.store.effect(i).is_some_and(|e| e[ef::BACKDROP] != 0);
            let f = &mut self.store.flags[i];
            *f = *f & !(flag::COVER | flag::BLUR) | if cover { flag::COVER } else { 0 } | if blur { flag::BLUR } else { 0 };
            self.blur_records += blur as u32;
        }
    }
    fn z_ranges(&mut self) {
        for s in &mut self.spaces {
            (s.z_lo, s.z_hi) = (u32::MAX, 0);
        }
        for i in 0..self.store.len() {
            if self.store.is_live(i) {
                let (s, z) = (&mut self.spaces[self.store.clips[i] as usize], self.store.z_order[i]);
                (s.z_lo, s.z_hi) = (s.z_lo.min(z), s.z_hi.max(z));
            }
        }
        for j in (1..self.spaces.len()).rev() {
            let (c, p) = (self.spaces[j], self.spaces[j].parent as usize);
            let ps = &mut self.spaces[p];
            (ps.z_lo, ps.z_hi) = (ps.z_lo.min(c.z_lo), ps.z_hi.max(c.z_hi));
        }
    }
    /// Rebuild the band index from the records' windows now. Reuses its vectors.
    fn reindex(&mut self) {
        self.stats.reindexes += 1;
        for s in &mut self.spaces {
            (s.band0, s.band_n, s.band_first) = (i32::MAX, 0, 0);
        }
        self.index.wide.clear();
        self.index.loose.clear();
        // pass 1: the band range of each space, and who is too tall for bands
        let mut hi = std::mem::take(&mut self.sub_root);
        hi.clear();
        hi.resize(self.spaces.len(), i32::MIN as u32);
        for i in 0..self.store.len() {
            if !self.store.is_live(i) {
                continue;
            }
            self.store.flags[i] &= !(flag::LOOSE | flag::WIDE);
            let w = self.store.window(i, false);
            if w.is_empty() {
                // nothing to find; if it grows it is touched and goes on the loose list
                self.store.flags[i] |= flag::WIDE;
                self.index.wide.push(i as u32);
                continue;
            }
            let (b0, b1) = (w.y0 >> BAND_SHIFT, (w.y1 - 1) >> BAND_SHIFT);
            if b1 - b0 >= WIDE_BANDS {
                self.store.flags[i] |= flag::WIDE;
                self.index.wide.push(i as u32);
                continue;
            }
            let j = self.store.clips[i] as usize;
            let s = &mut self.spaces[j];
            s.band0 = s.band0.min(b0);
            hi[j] = (hi[j] as i32).max(b1) as u32;
        }
        let mut total = 0u32;
        for (j, s) in self.spaces.iter_mut().enumerate() {
            if s.band0 != i32::MAX {
                s.band_n = (hi[j] as i32 - s.band0 + 1) as u32;
                s.band_first = total;
                total += s.band_n;
            }
        }
        self.sub_root = hi;
        let (store, spaces) = (&self.store, &self.spaces);
        self.index.bands.build(total as usize, &mut |f| {
            for i in 0..store.len() {
                if store.is_live(i) && store.flags[i] & flag::WIDE == 0 {
                    let (w, s) = (store.window(i, false), &spaces[store.clips[i] as usize]);
                    for b in w.y0 >> BAND_SHIFT..=(w.y1 - 1) >> BAND_SHIFT {
                        f((s.band_first as i32 + (b - s.band0)) as usize, i as u32);
                    }
                }
            }
        });
    }
    fn rebuild_covers(&mut self) {
        self.covers_dirty = false;
        self.stats.cover_rebuilds += 1;
        self.engine.covers_clear();
        collect(&self.store, &self.spaces, &self.index, self.window(), &mut self.sc);
        for k in 0..self.sc.cand.len() {
            let i = self.sc.cand[k] as usize;
            if self.store.flags[i] & flag::COVER != 0 {
                let core = self.cover_core(i);
                if !core.is_empty() {
                    self.engine.cover_push(OpaqueCover { z: self.store.z_order[i], core_rect: core });
                }
            }
        }
        self.engine.covers_commit();
    }
    fn layer_of(&self, i: usize) -> Option<BlurLayer> {
        let st = &self.store;
        let radius = st.effect(i)?[ef::BACKDROP] as i32;
        if radius <= 0 || st.opacity[i] == 0 {
            return None;
        }
        let s = &self.spaces[st.clips[i] as usize];
        let rect = st.rects[i].shift(s.origin.0, s.origin.1).and(s.clip).and(self.window());
        (!rect.is_empty()).then_some(BlurLayer { id: i as u32, z: st.z_order[i], rect, radius, max_level: blur_level(radius) })
    }
    fn rebuild_layers(&mut self) {
        self.layers_dirty = false;
        self.stats.layer_rebuilds += 1;
        collect(&self.store, &self.spaces, &self.index, self.window(), &mut self.sc);
        let mut old = std::mem::take(&mut self.backdrops);
        let mut new = std::mem::take(&mut self.backdrops_spare);
        new.clear();
        let win = self.window();
        for k in 0..self.sc.cand.len() {
            let i = self.sc.cand[k] as usize;
            if self.store.flags[i] & flag::BLUR == 0 {
                continue;
            }
            let Some(layer) = self.layer_of(i) else {
                continue;
            };
            let read = layer.expanded_read_rect().and(win);
            match old.iter().position(|b| b.rec == i as u32) {
                Some(at) => {
                    let mut b = old.swap_remove(at);
                    if b.layer != layer || !b.valid {
                        b.surf.reset(read);
                        b.valid = false;
                    }
                    b.layer = layer;
                    new.push(b);
                }
                None => new.push(Backdrop { rec: i as u32, layer, surf: Surface::at(read), valid: false, rebuilt: false }),
            }
        }
        new.sort_unstable_by_key(|b| (b.layer.z, b.layer.id));
        self.engine.layers_clear();
        for b in &new {
            self.engine.layer_push(b.layer);
        }
        self.engine.layers_commit();
        old.clear();
        self.backdrops = new;
        self.backdrops_spare = old;
    }
    fn rebuild_pal_mask(&mut self) {
        self.pal_dirty = false;
        self.pal_mask.clear();
        collect(&self.store, &self.spaces, &self.index, self.window(), &mut self.sc);
        for k in 0..self.sc.cand.len() {
            let i = self.sc.cand[k] as usize;
            if self.store.flags[i] & (flag::PAL_FILL | flag::PAL_BORDER) != 0 {
                let w = self.screen_window(i);
                self.pal_mask.mark_rect(w);
            }
        }
    }

    // ---- the warm path ----

    /// Bring the records up to date with the state block and report what changed to the damage engine.
    /// Allocation-free once warm; the work is in the bindings that read a changed word.
    pub fn settle(&mut self) {
        if self.structure_dirty {
            self.commit();
        }
        self.open_frame();
        self.stats.turns += 1;
        self.epoch_blits.clear();
        if self.invalid {
            self.invalid = false;
            self.engine.damage_mask(&self.all);
        }
        self.blits_blocked |= self.engine.pending();
        // 1. the words that changed, and the bindings that read them
        let mut changed = std::mem::take(&mut self.changed);
        changed.clear();
        self.state.drain_changed(&mut |s, old, new| changed.push((s, old, new)));
        let mut themed = std::mem::take(&mut self.palette_changed);
        if themed {
            // the palette itself was replaced: every binding that could read it
            for bi in 0..self.binds.len() {
                if matches!(self.binds[bi].val, Val::Palette { .. } | Val::Expr(_)) {
                    self.apply_bind(bi);
                }
            }
            for i in 0..self.store.len() {
                if self.store.is_live(i) && self.store.flags[i] & flag::PAL_FILL != 0 && self.is_cover(i) != (self.store.flags[i] & flag::COVER != 0) {
                    self.store.flags[i] ^= flag::COVER;
                    self.covers_dirty = true;
                }
            }
        }
        for &(s, old, new) in &changed {
            self.stats.slots_changed += 1;
            if (s.0 as usize) < HOVER_DEPTHS {
                for site in [old, new] {
                    if site != NO_SITE && site_depth(site) == s.0 as usize {
                        self.fan_row(3, (site & 0xff_ffff) as usize);
                    }
                }
            }
            themed |= s == self.theme.id();
            for which in 0..3 {
                self.fan_row(which, s.0 as usize);
            }
        }
        self.changed = changed;
        // 2. animations in flight
        if !self.active.is_empty() {
            self.tick();
        }
        // 3. ports that moved take their spaces with them
        for k in 0..self.touched.len() {
            let i = self.touched[k] as usize;
            if self.store.flags[i] & flag::PORT != 0 && self.store.touch[i] & touch::GEOM != 0 {
                for j in 0..self.spaces.len() {
                    if self.spaces[j].port == i as u32 {
                        let s = &mut self.spaces[j];
                        if s.own != self.epoch {
                            s.own = self.epoch;
                            s.cause = 0;
                        }
                        s.cause |= cause::PORT;
                        self.spaces_dirty = true;
                    }
                }
            }
        }
        // 4. spaces: a rigid scroll is a blit and its seams; anything else damages the records under it
        if std::mem::take(&mut self.spaces_dirty) {
            self.spaces_pass();
        }
        // 4b. what was added since the last turn, and where what was removed had been. After the spaces: under
        // a rigid scroll a removed record's stale pixels are where the blit carried them
        let rigid = !self.epoch_blits.is_empty();
        for k in 0..self.added.len() {
            let i = self.added[k] as usize;
            if self.store.is_live(i) && self.store.z_order[i] != u32::MAX {
                let s = &self.spaces[self.store.clips[i] as usize];
                let win = self.store.window(i, false).shift(s.origin.0, s.origin.1).and(s.clip);
                self.engine.damage(self.store.z_order[i], win, self.work_of(i));
            }
        }
        self.added.clear();
        for k in 0..self.removed.len() {
            let (_, local, space, work, z) = self.removed[k];
            let s = self.spaces[space as usize];
            let (o, c) = if s.stamp == self.epoch && !rigid { (s.old_origin, s.old_clip) } else { (s.origin, s.clip) };
            let win = local.shift(o.0, o.1).and(c);
            self.engine.damage(z, win, work);
            for q in 0..self.epoch_blits.len() {
                let (root, blit) = self.epoch_blits[q];
                if self.sub_root[space as usize] != root {
                    self.engine.seam(&blit, z, win);
                }
            }
        }
        self.removed.clear();
        // 5. the touched records: where each was and where it is
        for k in 0..self.touched.len() {
            let i = self.touched[k] as usize;
            if !self.store.is_live(i) {
                continue;
            }
            self.stats.records_touched += 1;
            let t = self.store.touch[i];
            if t & touch::DONE == 0 {
                let s = &self.spaces[self.store.clips[i] as usize];
                let old = self.store.old_rects[i].shift(s.origin.0, s.origin.1).and(s.clip);
                let new = self.store.window(i, self.bare()).shift(s.origin.0, s.origin.1).and(s.clip);
                let (z, work) = (self.store.z_order[i], self.work_of(i));
                self.engine.damage_move(z, old, new, work);
            }
            let geom = t & touch::GEOM != 0;
            let (was_cover, is_cover) = (self.store.flags[i] & flag::COVER != 0, self.is_cover(i));
            if was_cover != is_cover {
                self.store.flags[i] ^= flag::COVER;
            }
            self.covers_dirty |= was_cover != is_cover || (is_cover && geom);
            let (was_blur, is_blur) = (self.store.flags[i] & flag::BLUR != 0, self.store.effect(i).is_some_and(|e| e[ef::BACKDROP] != 0));
            if was_blur != is_blur {
                self.store.flags[i] ^= flag::BLUR;
                self.blur_records = if is_blur { self.blur_records + 1 } else { self.blur_records - 1 };
            }
            // a layer's opacity decides whether it is one at all
            self.layers_dirty |= was_blur != is_blur || (is_blur && (geom || self.layer_of(i).is_some() != self.backdrops.iter().any(|b| b.rec == i as u32)));
            if geom {
                self.pal_dirty |= self.store.flags[i] & (flag::PAL_FILL | flag::PAL_BORDER) != 0;
                if self.store.flags[i] & flag::LOOSE == 0 {
                    self.store.flags[i] |= flag::LOOSE;
                    self.index.loose.push(i as u32);
                }
            }
        }
        if self.index.loose.len() > 32 + self.store.live as usize / 8 {
            self.reindex();
        }
        // 6. what the engine culls with and propagates through
        if self.covers_dirty {
            self.rebuild_covers();
        }
        if self.layers_dirty {
            self.rebuild_layers();
        }
        // 7. a theme switch: the tiles themed records touch, with no record visited
        if themed {
            if self.pal_dirty {
                self.rebuild_pal_mask();
            }
            self.engine.damage_mask(&self.pal_mask);
        }
        // what is touched from here on (a patch between turns) belongs to the next turn
        self.touched.clear();
        self.epoch = self.epoch.wrapping_add(1).max(2);
    }
    fn tick(&mut self) {
        let now = self.state.get(self.time);
        let mut k = 0;
        while k < self.active.len() {
            let a = self.active[k] as usize;
            let an = self.anims[a];
            let el = now.saturating_sub(an.start) as u64;
            let t = if an.dur_ms == 0 || el >= an.dur_ms as u64 { 256 } else { (el * 256 / an.dur_ms as u64) as u32 };
            let q = if an.spring { spring_q8(t) } else { t };
            let v = if t >= 256 {
                an.to
            } else if an.color {
                lerp_color(an.from, an.to, q)
            } else {
                lerp_i32(an.from, an.to, q)
            };
            if v != self.anim_vals[a] {
                self.anim_vals[a] = v;
                self.fan_row(4, a);
            }
            if t >= 256 {
                self.anims[a].active = false;
                self.active.swap_remove(k);
            } else {
                k += 1;
            }
        }
    }

    // ---- frames ----

    /// Settle and repaint what is damaged.
    pub fn frame(&mut self) -> FrameInfo {
        self.frame_with(&mut NoExt)
    }
    pub fn frame_with(&mut self, ext: &mut dyn ExtPaint) -> FrameInfo {
        self.settle();
        self.stats.frames += 1;
        let theme = self.state.get(self.theme);
        let FlatWorld { engine, store, spaces, palette, index, sc, surface, backdrops, stats, .. } = self;
        // layers that appeared or moved: their backdrops whole, bottom up
        for k in 0..backdrops.len() {
            if !backdrops[k].valid {
                let (lower, rest) = backdrops.split_at_mut(k);
                let b = &mut rest[0];
                let p = Painter { store, spaces, palette, theme, backdrops: lower };
                let region = b.surf.bounds();
                stats.records_painted += paint_region(&p, index, &mut b.surf, region, b.layer.z, sc, ext);
                b.valid = true;
                b.rebuilt = true;
            }
        }
        engine.resolve(&mut |layer, rect| {
            let Some(k) = backdrops.iter().position(|b| b.rec == layer.id) else {
                return Some(rect);
            };
            if backdrops[k].rebuilt {
                return Some(rect);
            }
            let (lower, rest) = backdrops.split_at_mut(k);
            let b = &mut rest[0];
            let c = rect.and(b.surf.bounds());
            if c.is_empty() {
                return None;
            }
            // repaint this part of what the layer reads, and see what came out different
            let mut save = std::mem::take(&mut sc.save);
            save.clear();
            for y in c.y0..c.y1 {
                save.extend_from_slice(b.surf.row(y, c.x0, c.x1));
            }
            b.surf.clear(c);
            let p = Painter { store, spaces, palette, theme, backdrops: lower };
            stats.records_painted += paint_region(&p, index, &mut b.surf, c, layer.z, sc, ext);
            stats.backdrop_repairs += 1;
            let w = (c.x1 - c.x0) as usize;
            let mut diff = Rect::EMPTY;
            for y in c.y0..c.y1 {
                let (new, old) = (b.surf.row(y, c.x0, c.x1), &save[(y - c.y0) as usize * w..(y - c.y0 + 1) as usize * w]);
                if new != old {
                    let first = new.iter().zip(old).position(|(a, b)| a != b).unwrap_or(0) as i32;
                    let last = new.iter().zip(old).rposition(|(a, b)| a != b).unwrap_or(0) as i32;
                    diff = diff.hull(Rect::new(c.x0 + first, y, c.x0 + last + 1, y + 1));
                }
            }
            sc.save = save;
            (!diff.is_empty()).then_some(diff)
        });
        for b in backdrops.iter_mut() {
            b.rebuilt = false;
        }
        for b in engine.blits() {
            surface.blit(b);
        }
        let p = Painter { store, spaces, palette, theme, backdrops };
        let (mut spans, mut painted) = (0u64, 0u64);
        engine.mask().each_span(&mut |span| {
            surface.clear(span);
            painted += paint_region(&p, index, surface, span, u32::MAX, sc, ext);
            spans += 1;
        });
        stats.spans_painted += spans;
        stats.records_painted += painted;
        stats.tiles_repaired += engine.mask().count() as u64;
        let idle = engine.mask().is_empty() && engine.blits().is_empty();
        stats.idle_frames += idle as u64;
        self.frame_open = false;
        FrameInfo {
            idle,
            dirty_tiles: self.engine.mask().count(),
            total_tiles: self.engine.mask().total(),
            repair_percent: self.engine.repair_percent(),
            occluded_tiles_skipped: self.engine.occluded_tiles_skipped(),
            blits: self.engine.blits().len() as u32,
            choice: self.engine.choice(),
            reason: self.engine.reason(),
        }
    }
    /// The last frame's damage as data.
    pub fn report(&self) -> DamageReport {
        self.engine.report()
    }
    /// The retained surface (premultiplied RGBA8, red low).
    pub fn pixels(&self) -> &[u32] {
        &self.surface.px
    }
    pub fn surface(&self) -> &Surface {
        &self.surface
    }

    // ---- the pointer ----

    fn under(&mut self, x: i32, y: i32) {
        collect(&self.store, &self.spaces, &self.index, Rect::xywh(x, y, 1, 1), &mut self.sc);
    }
    /// The pointer is at (x, y): for each hover depth, the topmost site under it becomes the hovered one.
    pub fn pointer_move(&mut self, x: i32, y: i32) {
        if self.structure_dirty {
            self.commit();
        }
        self.under(x, y);
        let mut top = [(0u32, NO_SITE); HOVER_DEPTHS];
        for &i in &self.sc.cand {
            let i = i as usize;
            let st = &self.store;
            if st.flags[i] & flag::HIT == 0 || st.site[i] == NO_SITE {
                continue;
            }
            let s = &self.spaces[st.clips[i] as usize];
            if st.rects[i].shift(s.origin.0, s.origin.1).and(s.clip).has(x, y) {
                let d = site_depth(st.site[i]) & (HOVER_DEPTHS - 1);
                if st.z_order[i] >= top[d].0 {
                    top[d] = (st.z_order[i], st.site[i]);
                }
            }
        }
        for d in 0..HOVER_DEPTHS {
            self.state.set_word(self.hover[d], top[d].1);
        }
    }
    pub fn pointer_leave(&mut self) {
        for d in 0..HOVER_DEPTHS {
            self.state.set_word(self.hover[d], NO_SITE);
        }
    }
    /// The topmost record under (x, y) that stops the pointer, and its handler word.
    pub fn hit(&mut self, x: i32, y: i32) -> Option<(RecId, u32)> {
        if self.structure_dirty {
            self.commit();
        }
        self.under(x, y);
        let mut top: Option<usize> = None;
        for &i in &self.sc.cand {
            let i = i as usize;
            let st = &self.store;
            if st.flags[i] & flag::HIT == 0 {
                continue;
            }
            let s = &self.spaces[st.clips[i] as usize];
            if st.rects[i].shift(s.origin.0, s.origin.1).and(s.clip).has(x, y) && top.is_none_or(|t| st.z_order[i] > st.z_order[t]) {
                top = Some(i);
            }
        }
        top.map(|i| (self.store.id(i), self.store.handlers[i]))
    }
    /// The handler word of the topmost record under (x, y) that has one (0: none).
    pub fn handler_at(&mut self, x: i32, y: i32) -> u32 {
        if self.structure_dirty {
            self.commit();
        }
        self.under(x, y);
        let mut top = (0u32, 0u32);
        for &i in &self.sc.cand {
            let i = i as usize;
            let st = &self.store;
            if st.flags[i] & flag::HIT == 0 || st.handlers[i] == 0 {
                continue;
            }
            let s = &self.spaces[st.clips[i] as usize];
            if st.rects[i].shift(s.origin.0, s.origin.1).and(s.clip).has(x, y) && st.z_order[i] >= top.0 {
                top = (st.z_order[i], st.handlers[i]);
            }
        }
        top.1
    }

    // ---- the oracles ----

    /// Everything painted again from the stores into a new surface, record by record in paint order, with new
    /// backdrops: no tile mask, no spatial index, no retained pixel.
    pub fn full_redraw(&self, ext: &mut dyn ExtPaint) -> Surface {
        let theme = self.state.get(self.theme);
        let mut blur = BlurScratch::default();
        let mut layers: Vec<BlurLayer> = self.order.iter().filter_map(|i| self.layer_of(*i as usize)).collect();
        layers.sort_unstable_by_key(|l| (l.z, l.id));
        let mut fresh: Vec<Backdrop> = Vec::new();
        for layer in layers {
            let mut surf = Surface::at(layer.expanded_read_rect().and(self.window()));
            let p = Painter { store: &self.store, spaces: &self.spaces, palette: &self.palette, theme, backdrops: &fresh };
            let region = surf.bounds();
            for &i in &self.order {
                if self.store.z_order[i as usize] >= layer.z {
                    break;
                }
                p.record(&mut surf, region, i as usize, &mut blur, ext);
            }
            fresh.push(Backdrop { rec: layer.id, layer, surf, valid: true, rebuilt: false });
        }
        let mut out = Surface::new(self.w, self.h);
        let p = Painter { store: &self.store, spaces: &self.spaces, palette: &self.palette, theme, backdrops: &fresh };
        let region = out.bounds();
        for &i in &self.order {
            p.record(&mut out, region, i as usize, &mut blur, ext);
        }
        out
    }
    /// The same description and the same state mounted from nothing: every binding evaluated, every space
    /// placed, every index built, one whole frame painted. Animations rest on their goals.
    pub fn fresh_mount(&self, ext: &mut dyn ExtPaint) -> FlatWorld {
        let mut w = self.clone();
        #[cfg(feature = "flat_mutants")]
        {
            w.engine.mutants = 0;
        }
        w.commit();
        w.state.settle_all();
        w.active.clear();
        for pass in 0..2 {
            for bi in 0..w.binds.len() {
                if (pass == 0) == (w.binds[bi].target == Target::Anim) {
                    w.write_bind(bi);
                }
            }
        }
        for a in 0..w.anims.len() {
            if let Val::Const(c) = w.anim_target[a] {
                w.anim_vals[a] = c;
                w.anims[a].active = false;
            }
        }
        w.reflag();
        w.respace(true, 0);
        w.reindex();
        w.backdrops.clear();
        w.surface = Surface::new(w.w, w.h);
        w.engine = DamageEngine::new(w.w, w.h);
        w.frame_open = false;
        w.invalid = true;
        w.spaces_dirty = false;
        w.covers_dirty = true;
        w.layers_dirty = true;
        w.pal_dirty = true;
        w.palette_changed = false;
        w.touched.clear();
        w.frame_with(ext);
        w
    }
    /// Present a frame and hold it to the oracles: the retained surface equals a full redraw of the stores,
    /// and (when no animation is in flight) equals the same state mounted from nothing. `Err` names the pair
    /// that differs, how many pixels, and their bounding box.
    pub fn check(&mut self, ext: &mut dyn ExtPaint) -> Result<FrameInfo, String> {
        let info = self.frame_with(ext);
        let diff = |a: &[u32], b: &[u32], w: i32| {
            let (mut n, mut r) = (0, Rect::EMPTY);
            for (k, (x, y)) in a.iter().zip(b).enumerate() {
                if x != y {
                    n += 1;
                    r = r.hull(Rect::xywh(k as i32 % w, k as i32 / w, 1, 1));
                }
            }
            (n, r)
        };
        let full = self.full_redraw(ext);
        if self.surface.px != full.px {
            let (n, r) = diff(&self.surface.px, &full.px, self.w);
            return Err(format!("retained != full redraw ({n} pixels inside {r:?})"));
        }
        if !self.anims_active() {
            let fresh = self.fresh_mount(ext);
            if full.px != fresh.surface.px {
                let (n, r) = diff(&full.px, &fresh.surface.px, self.w);
                return Err(format!("retained != fresh mount ({n} pixels inside {r:?})"));
            }
        }
        Ok(info)
    }
}

#[cfg(test)]
mod tests {
    use super::super::store::ROOT;
    use super::*;

    /// A small world with one of everything: small enough for Miri, complete enough that a walk over it enters
    /// every warm path (keyed hover, expression, palette, animation, rigid scroll with seams, clip toggle,
    /// stacked shadows, occlusion, gated backdrop blur) and the cold one (insert, remove).
    #[test]
    fn a_small_world_holds_its_oracles_through_every_path() {
        let (w, h) = (128, 96);
        let mut wd = FlatWorld::new(w, h);
        wd.set_palette(2, &[0xfff0_f0f0, 0xff20_2020, 0xff10_1010, 0xffe0_e0e0]);
        let (count, sy, clip, x, blur, buried) = (wd.slot(1u32), wd.slot(0i32), wd.slot(true), wd.slot(70i32), wd.slot(3u32), wd.slot(0xff00_00ffu32));
        wd.add_record(RecordDesc::rect(ROOT, Rect::new(0, 0, w, h), wd.pal(0)));
        wd.add_record(RecordDesc::rect(ROOT, Rect::new(36, 36, 60, 60), buried));
        let port = wd.add_record(RecordDesc::rect(ROOT, Rect::new(0, 16, 64, 96), 0xffff_ffffu32));
        let list = wd.add_space(ROOT, Some(port), clip, 0, sy);
        let mut rows = Vec::new();
        for k in 0..6 {
            let site = wd.add_site(1);
            let mut b = wd.tape.build();
            let (hv, on, off) = (b.hovered(site), b.konst(0xffa0_c0ffu32), b.konst(0xffd0_d0d0u32));
            let f = b.select(hv, on, off);
            let fill = b.finish(f);
            rows.push(wd.add_record(RecordDesc::rect(list, Rect::new(2, k * 20 + 2, 62, k * 20 + 18), fill).hit(site, k as u32 + 1)));
            wd.add_record(RecordDesc::label(list, Rect::new(4, k * 20 + 6, 60, k * 20 + 14), TextSrc::Num { prefix: "N".into(), slot: count.id(), suffix: String::new() }, 1, wd.pal(1)));
        }
        let mut card = RecordDesc::rect(ROOT, Rect::new(0, 8, 30, 30), 0xffff_c080u32).radius_px(4).effect(Effect::new().shadow(0, 0x6000_0000u32, 1, 2, blur, 1).shadow(1, 0x40ff_0000u32, -2, 0, 2, 0));
        card.x = x.into();
        wd.add_record(card);
        wd.add_record(RecordDesc::rect(ROOT, Rect::new(40, 30, 100, 70), 0x40ff_ffffu32).effect(Effect::new().backdrop_blur(2)));
        let site = wd.add_site(0);
        let tint = wd.add_anim(wd.hover_sel(site, 0x80ff_ffff, 0x80ff_8080), 40, true, true);
        wd.add_record(RecordDesc { fill: tint, ..RecordDesc::rect(ROOT, Rect::new(0, 0, w, 20), 0u32).hit(site, 0) });
        wd.check(&mut NoExt).unwrap();
        let ops: &[&dyn Fn(&mut FlatWorld, &mut Vec<RecId>)] = &[
            &|wd, _| wd.pointer_move(10, 50),
            &|wd, _| {
                let c = wd.get(count);
                wd.set(count, c + 9);
            },
            &|wd, _| wd.set_theme(1),
            &|wd, _| {
                wd.set(sy, 5);
            },
            &|wd, _| {
                wd.set(buried, 0xff00_ff00);
            },
            &|wd, _| {
                wd.set(x, 20);
                wd.set(blur, 6);
            },
            &|wd, _| {
                wd.pointer_move(100, 5);
                wd.set_time(10);
            },
            &|wd, _| {
                wd.set(clip, false);
            },
            &|wd, rows| {
                let gone = rows.remove(1);
                assert!(wd.remove(gone));
            },
            &|wd, _| wd.set_time(200),
            &|wd, rows| {
                let d = RecordDesc::rect(SpaceId(1), Rect::new(10, 40, 50, 52), 0xff00_8000u32);
                let at = rows[0];
                rows.push(wd.insert_before(at, d));
            },
            &|wd, _| {
                wd.set(sy, -3);
                wd.set(clip, true);
            },
            &|wd, _| wd.pointer_leave(),
        ];
        for (k, op) in ops.iter().enumerate() {
            op(&mut wd, &mut rows);
            if let Err(e) = wd.check(&mut NoExt) {
                panic!("op {k}: {e}");
            }
        }
        assert!(wd.stats.blits >= 1 && wd.stats.backdrop_repairs >= 1 && wd.stats.commits >= 3);
        // a handle of another world's state is refused
        let other = FlatWorld::new(8, 8);
        assert!(other.state.try_get(count).is_err());
    }
}
