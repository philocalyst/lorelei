//! The scene: columns of records (one per archetype), the paint order, the window lane, the tile bins, and
//! the CPU frame. The columns live in one page-aligned arena whose words are the GPU's buffer (`metal.rs`
//! wraps it without a copy), in the seam's page layout: 64 records per page, rect lanes then records.
//!
//! A frame is: compare the block with the one last drawn (a mask); for the columns whose archetype reads a
//! changed word, one pass over the column (`window`: the window lane, the damage); rebuild the bins if a
//! window moved; shade the damaged tiles. A column's kernels are reached through its `&'static Desc`:
//! once per column for the window pass, once per run of a tile's entries for shading. Nothing is decoded,
//! restaged or looked up per record.
use crate::expr::{Cx, U, U_TIME};
use crate::layer::{self, Layers};
use crate::paint::{Cost, EMPTY, LaySink, NO_CLIP, NO_HIT, PCode, Paint, SpecSink, StageSink, WinSink};
use crate::tile::{Flat, Fx, Shade, Stats, Tile};
use bough_expr::ahead;
use bough_kernel::Cmd;
use bough_kernel::cpu::Pool;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use wgsl_rs::std::{Vec4, Vec4i, Vec4u};

/// The largest surface side, and the largest magnitude of a rect lane in pixels: windows are packed as
/// `i16` after the stager has clamped them to this range (`ops::window`), so a box that reaches further is
/// still drawn exactly where it crosses the surface, and one that lies further away is refused at admission
/// (`Scene::push` / `set`) rather than wrapped.
pub const MAX_PX: i32 = 32767;
/// How far from the origin a rect lane may lie, in pixels (a scrolled world): the record is staged where
/// its pan puts it, and shaded only when that is near the frame.
pub const MAX_WORLD_PX: i32 = 1 << 20;
/// Lanes before the records in a page: x0, y0, x1, y1 (Q8) and the paint-order key.
pub const LANES: usize = 5;
/// `order` word: column in the high 12 bits, record index in the low 20.
pub const IDX_BITS: u32 = 20;
pub const IDX_MASK: u32 = (1 << IDX_BITS) - 1;
/// The arena's alignment: a VM page on Apple silicon, so Metal can wrap it without a copy.
pub const VM_PAGE: usize = 16384;

/// A column's records as raw words (what a `Desc` kernel receives). It borrows the scene it views, can only
/// be made by `Scene::view`, and its unchecked accessors are this crate's alone: callers inside the crate
/// index it with `i < len`, which holds for every index that comes from the scene's own order lane or from
/// a loop over `0..len` (the scene's columns and order are not writable from outside).
#[derive(Clone, Copy)]
pub struct ColView<'a> {
    base: *const u32,
    /// The scene's palette tables (`PAL_WORDS` words of its arena).
    pal: *const u32,
    pub(crate) len: u32,
    /// Words per record.
    pub(crate) w: u32,
    _scene: PhantomData<&'a Arena>,
}
impl<'a> ColView<'a> {
    /// The palette tables, for an evaluation context.
    #[inline(always)]
    pub(crate) fn pal(&self) -> &'a [u32] {
        // SAFETY: `view_of` made the pointer from the arena this view borrows: `PAL_WORDS` initialised
        // words inside it, not written while the view lives (it borrows the arena shared).
        unsafe { std::slice::from_raw_parts(self.pal, if cfg!(feature = "mutant_pal_stale") { PAL_SLOTS } else { PAL_WORDS }) }
    }
    #[inline(always)]
    fn page(&self, i: u32) -> *const u32 {
        // SAFETY (callers): i < len <= capacity, so the page is inside the column's allocation.
        unsafe { self.base.add((i as usize >> 6) * 64 * (LANES + self.w as usize)) }
    }
    #[inline(always)]
    pub(crate) fn rect(&self, i: u32) -> [i32; 4] {
        debug_assert!(i < self.len);
        let (p, r) = (self.page(i), i as usize & 63);
        // SAFETY: the four rect lanes are the page's first 256 words.
        unsafe { [*p.add(r) as i32, *p.add(64 + r) as i32, *p.add(128 + r) as i32, *p.add(192 + r) as i32] }
    }
    #[inline(always)]
    pub(crate) fn z(&self, i: u32) -> u32 {
        debug_assert!(i < self.len);
        // SAFETY: the key lane is words 256..320 of the page.
        unsafe { *self.page(i).add(256 + (i as usize & 63)) }
    }
    /// Record `i` in place.
    ///
    /// # Safety
    /// `P` is the column's archetype (`size_of::<P>() == 4 * self.w`) and `i < self.len`.
    #[inline(always)]
    pub(crate) unsafe fn rec<P: Paint>(&self, i: u32) -> &P {
        // SAFETY: `P: Words` (whole words, alignment <= 4, any bits valid); the record's words follow the lanes.
        unsafe { &*(self.page(i).add(64 * LANES + (i as usize & 63) * self.w as usize) as *const P) }
    }
}

/// A column of `arena` as a view (borrowing the arena only, so the scene's lanes can be written beside it).
fn view_of<'a>(arena: &'a Arena, pal_base: usize, c: &Col) -> ColView<'a> {
    // SAFETY: `base` is inside the arena (checked at `Scene::column`).
    ColView { base: unsafe { arena.ptr().add(c.base) }, pal: unsafe { arena.ptr().add(pal_base) }, len: c.len as u32, w: c.desc.words as u32, _scene: PhantomData }
}

/// Damaged tiles of a frame, one bit each, row-major.
pub struct Damage {
    pub bits: Vec<u64>,
    pub tx: usize,
    pub ty: usize,
}
impl Damage {
    pub fn new(tx: usize, ty: usize) -> Damage {
        Damage { bits: vec![0; (tx * ty).div_ceil(64)], tx, ty }
    }
    pub fn clear(&mut self) {
        self.bits.fill(0);
    }
    pub fn all(&mut self) {
        let nt = self.tx * self.ty;
        self.bits.fill(u64::MAX);
        if nt % 64 != 0 {
            *self.bits.last_mut().unwrap() = (1u64 << (nt % 64)) - 1;
        }
    }
    /// The tiles a pixel window touches.
    #[inline]
    pub fn add(&mut self, w: [i16; 4]) {
        let x0 = (w[0].max(0) as usize) >> 5;
        let y0 = (w[1].max(0) as usize) >> 5;
        let x1 = ((w[2].max(0) as usize + 31) >> 5).min(self.tx);
        let y1 = ((w[3].max(0) as usize + 31) >> 5).min(self.ty);
        for y in y0..y1 {
            for x in x0..x1 {
                let t = y * self.tx + x;
                self.bits[t / 64] |= 1 << (t % 64);
            }
        }
    }
    /// The tiles a pixel window of the record with key `z` touches *and is visible in*: a tile whose list
    /// is cut at an opaque cover above the record (`floor[t] > z`) cannot show any change of it. `floor`
    /// is the bins' as they stood before this frame: a cover that moves this frame damages its own old and
    /// new windows, so a tile left out here is one where nothing at or above its floor changed.
    #[inline]
    pub fn add_vis(&mut self, w: [i16; 4], z: u32, floor: &[u32]) {
        if floor.len() != self.tx * self.ty {
            return self.add(w);
        }
        let x0 = (w[0].max(0) as usize) >> 5;
        let y0 = (w[1].max(0) as usize) >> 5;
        let x1 = ((w[2].max(0) as usize + 31) >> 5).min(self.tx);
        let y1 = ((w[3].max(0) as usize + 31) >> 5).min(self.ty);
        for y in y0..y1 {
            for x in x0..x1 {
                let t = y * self.tx + x;
                if z >= floor[t] {
                    self.bits[t / 64] |= 1 << (t % 64);
                }
            }
        }
    }
    pub fn tiles(&self, out: &mut Vec<u32>) {
        out.clear();
        for (i, &w) in self.bits.iter().enumerate() {
            let mut m = w;
            while m != 0 {
                out.push((i * 64) as u32 + m.trailing_zeros());
                m &= m - 1;
            }
        }
    }
    pub fn count(&self) -> usize {
        self.bits.iter().map(|w| w.count_ones() as usize).sum()
    }
}

/// What the scene knows about an archetype: its constants (from the paint type) and its kernels (the paint
/// type's code, monomorphised). One per paint type, in rodata.
pub struct Desc {
    pub name: &'static str,
    /// Words per record.
    pub words: usize,
    pub reads: u64,
    pub geom_reads: u64,
    pub opaque_core: bool,
    pub hit: bool,
    pub cost: Cost,
    pub layers: usize,
    /// Its records own a layer each (`Paint::LAYER`).
    pub layer: bool,
    pub code: &'static PCode,
    /// The window pass: for every record, its window and cover under `new` into the lanes (by key), and
    /// damage for a record whose window moved or whose operands' values changed. Returns whether a window or
    /// cover moved.
    /// `only`: the records to visit (a time-only frame: the column's active set); `None`: all of them.
    pub(crate) window: fn(ColView, old: &U, new: &U, mask: u64, first: bool, only: Option<&[u32]>, p: &mut Pass) -> bool,
    pub(crate) flat: fn(ColView, zs: &[u32], order: &[u32], u: &U, t: &mut Tile),
    /// `first`: the index of `zs[0]` in the tile's list (its occlusion mask).
    pub(crate) shade: fn(ColView, zs: &[u32], first: usize, order: &[u32], u: &U, t: &mut Tile, fx: &Fx),
    /// The specification at one pixel: (blended accumulator, some layer inked the pixel).
    pub(crate) spec: fn(ColView, i: u32, u: &U, x: i32, y: i32, acc: Vec4u, atlas: &[u8], aw: u32, tabs: &[u32], bd: &dyn Fn(Cmd, i32, i32) -> Vec4u, media: &[u32]) -> (Vec4u, bool),
    /// The backdrop command of a record that owns a layer.
    pub(crate) lay: fn(ColView, i: u32, u: &U) -> Option<Cmd>,
    /// The record's window joined with its halo under `u` (unpacked: for the scroll index).
    pub(crate) reach: fn(ColView, i: u32, u: &U) -> Option<Vec4i>,
    /// The record as the generic kernel's commands.
    pub(crate) stage: fn(ColView, i: u32, u: &U, out: &mut bough_kernel::stage::Scene),
    pub(crate) pick: fn(ColView, i: u32, u: &U) -> u32,
    /// One rewritten record: damage where it was and where it is now, its lanes brought up to date. Returns
    /// whether its window or cover moved.
    pub(crate) rewin: fn(ColView, i: u32, u: &U, p: &mut Pass) -> bool,
}

/// What a window pass writes: the lanes (by key), the damage, and what the frame's later steps need to
/// know about the records it visited.
pub(crate) struct Pass<'a> {
    pub win: &'a mut [[i16; 4]],
    pub cover: &'a mut [[i16; 4]],
    pub halo: &'a mut [[i16; 4]],
    pub dmg: &'a mut Damage,
    pub floor: &'a [u32],
    /// Every damaged window with its record's key, for the layers above it (kept only when the scene has
    /// layers).
    pub events: &'a mut Vec<([i32; 4], u32)>,
    pub track: bool,
    /// Keys of layer-owning records whose geometry was evaluated.
    pub regeo: &'a mut Vec<u32>,
    /// Indices of the visited records that still depend on the clock after this frame.
    pub active: &'a mut Vec<u32>,
    /// Keys of the records whose window, halo or cover changed: the bins are edited for these.
    pub rebin: &'a mut Vec<u32>,
    /// The frame's size in pixels.
    pub frame: [i16; 2],
}
impl Pass<'_> {
    /// Record `z`'s window meets the frame: what it does with the clock can be seen. (The mutant parks a
    /// reader in the lower half of the frame.)
    #[inline(always)]
    fn shows(&self, z: usize) -> bool {
        let (w, fh) = (self.win[z], if cfg!(feature = "mutant_park") { self.frame[1] / 2 } else { self.frame[1] });
        w[0] < w[2] && w[1] < w[3] && w[0] < self.frame[0] && w[1] < fh && w[2] > 0 && w[3] > 0
    }
    fn clock_visible<P: Paint>(&self, p: &P, z: usize, cx: &Cx) -> bool {
        if self.shows(z) {
            return true;
        }
        if P::GEOM_READS & (1 << U_TIME) == 0 {
            return false;
        }
        let [x, y, r, b] = self.halo[z];
        if P::LAYER && x < r && y < b && x < self.frame[0] && y < self.frame[1] && r > 0 && b > 0 {
            return true;
        }
        p.motion_hull(cx).is_none_or(|w| {
            w.x < w.z && w.y < w.w && w.x < i32::from(self.frame[0]) && w.y < i32::from(self.frame[1]) && w.z > 0 && w.w > 0
        })
    }
    /// A layer record's halo is now `h`. The halo is where the record flattens the accumulator, so the
    /// tiles a changed halo left and entered are damaged (and are events for the layers above). Returns
    /// whether it changed.
    #[inline]
    fn halo_to(&mut self, z: usize, h: [i16; 4], first: bool) -> bool {
        if self.halo[z] == h {
            return false;
        }
        if !first && !cfg!(feature = "mutant_halo_damage") {
            self.mark(self.halo[z], z as u32);
            self.mark(h, z as u32);
        }
        self.halo[z] = h;
        self.rebin.push(z as u32);
        true
    }
    #[inline(always)]
    fn mark(&mut self, w: [i16; 4], z: u32) {
        self.dmg.add_vis(w, z, self.floor);
        if self.track && w[0] < w[2] {
            self.events.push(([w[0] as i32, w[1] as i32, w[2] as i32, w[3] as i32], z));
        }
    }
}

fn i16s(v: Vec4i) -> [i16; 4] {
    [v.x as i16, v.y as i16, v.z as i16, v.w as i16]
}

fn window_pass<P: Paint>(c: ColView, old: &U, new: &U, mask: u64, first: bool, only: Option<&[u32]>, ps: &mut Pass) -> bool {
    let geom = first || P::GEOM_READS & mask != 0;
    let timed = P::READS >> U_TIME & 1 != 0;
    let now = new.w[U_TIME];
    let mut moved = false;
    let n = only.map_or(c.len, |o| o.len() as u32);
    for k in 0..n {
        let i = only.map_or(k, |o| o[k as usize]);
        // SAFETY: this function is only stored in `Desc::of::<P>()`, which a column of `P` holds; the
        // active set holds indices below the column's length (`Scene::update` keeps it so).
        let (rect, z, p) = (c.rect(i), c.z(i) as usize, unsafe { c.rec::<P>(i) });
        let cn = Cx { u: new, rect, pal: c.pal() };
        let mut still = false;
        if timed {
            let rests = !ahead(p.until(&cn), now);
            if cfg!(feature = "mutant_settle") && only.is_some() && rests {
                continue;
            }
            still = !rests;
        }
        if geom {
            let mut ws = WinSink(EMPTY, EMPTY);
            p.paint(&cn, NO_CLIP, &mut ws);
            let w = i16s(ws.0);
            let cv = if !P::OPAQUE_CORE {
                [0; 4]
            } else if cfg!(feature = "mutant_cut") {
                w
            } else {
                i16s(p.cover(&cn, NO_CLIP))
            };
            if P::LAYER {
                ps.halo_to(z, i16s(ws.1), first);
                ps.regeo.push(z as u32);
            }
            if first || w != ps.win[z] || cv != ps.cover[z] {
                if !first {
                    ps.mark(ps.win[z], z as u32);
                }
                ps.mark(w, z as u32);
                (ps.win[z], ps.cover[z]) = (w, cv);
                ps.rebin.push(z as u32);
                moved = true;
                if still && ps.clock_visible(p, z, &cn) {
                    ps.active.push(i);
                }
                continue;
            }
            moved |= ps.rebin.last() == Some(&(z as u32));
        }
        if still && ps.clock_visible(p, z, &cn) {
            ps.active.push(i);
        }
        if !first {
            let cw = p.changed_win(&Cx { u: old, rect, pal: c.pal() }, &cn, mask, NO_CLIP);
            if cw.x < cw.z {
                ps.mark(i16s(cw), z as u32);
            }
        }
    }
    moved
}
fn rewin_one<P: Paint>(c: ColView, i: u32, u: &U, ps: &mut Pass) -> bool {
    // SAFETY: as `window_pass`.
    let (rect, z, p) = (c.rect(i), c.z(i) as usize, unsafe { c.rec::<P>(i) });
    let cx = Cx { u, rect, pal: c.pal() };
    let mut ws = WinSink(EMPTY, EMPTY);
    p.paint(&cx, NO_CLIP, &mut ws);
    let w = i16s(ws.0);
    let cv = if P::OPAQUE_CORE { i16s(p.cover(&cx, NO_CLIP)) } else { [0; 4] };
    let mut moved = w != ps.win[z] || cv != ps.cover[z];
    // (the mutant: a rewritten record that stayed where it was, a video showing a new frame, damages nothing)
    if !(cfg!(feature = "mutant_video") && !moved) {
        ps.mark(ps.win[z], z as u32);
        ps.mark(w, z as u32);
    }
    (ps.win[z], ps.cover[z]) = (w, cv);
    if P::LAYER {
        moved |= ps.halo_to(z, i16s(ws.1), false);
        ps.regeo.push(z as u32);
    }
    if moved {
        ps.rebin.push(z as u32);
    }
    if P::READS >> U_TIME & 1 != 0 && ahead(p.until(&cx), u.w[U_TIME]) && ps.clock_visible(p, z, &cx) {
        ps.active.push(i);
    }
    moved
}
fn flat_run<P: Paint>(c: ColView, zs: &[u32], order: &[u32], u: &U, t: &mut Tile) {
    for &z in zs {
        let i = order[z as usize] & IDX_MASK;
        // SAFETY: as `window_pass`; `order` names records of this column for these keys.
        unsafe { c.rec::<P>(i) }.paint(&Cx { u, rect: c.rect(i), pal: c.pal() }, NO_CLIP, &mut Flat(t));
    }
}
fn shade_run<P: Paint>(c: ColView, zs: &[u32], first: usize, order: &[u32], u: &U, t: &mut Tile, fx: &Fx) {
    for (j, &z) in zs.iter().enumerate() {
        t.cur = first + j;
        let i = order[z as usize] & IDX_MASK;
        let lid = if P::LAYER { fx.lid(z) } else { layer::NONE };
        // SAFETY: as `flat_run`.
        unsafe { c.rec::<P>(i) }.paint(&Cx { u, rect: c.rect(i), pal: c.pal() }, NO_CLIP, &mut Shade(t, fx, lid));
    }
}
fn reach_one<P: Paint>(c: ColView, i: u32, u: &U) -> Option<Vec4i> {
    // SAFETY: as `window_pass`, this descriptor is only used for its own column's records.
    let p = unsafe { c.rec::<P>(i) };
    let cx = Cx { u, rect: c.rect(i), pal: c.pal() };
    if P::GEOM_READS & (1 << U_TIME) != 0 {
        let mut w = p.motion_hull(&cx)?;
        if P::LAYER {
            // The halo is aligned to the frame tile grid. A fractional pan changes that alignment.
            w.y -= 32;
            w.w += 32;
        }
        return Some(w);
    }
    let mut ws = WinSink(EMPTY, EMPTY);
    p.paint(&cx, NO_CLIP, &mut ws);
    Some(crate::ops::ops::x_union(ws.0, ws.1))
}
fn lay_one<P: Paint>(c: ColView, i: u32, u: &U) -> Option<Cmd> {
    let mut s = LaySink(None);
    // SAFETY: as `window_pass`.
    unsafe { c.rec::<P>(i) }.paint(&Cx { u, rect: c.rect(i), pal: c.pal() }, NO_CLIP, &mut s);
    s.0
}
fn spec_one<P: Paint>(c: ColView, i: u32, u: &U, x: i32, y: i32, acc: Vec4u, atlas: &[u8], aw: u32, tabs: &[u32], bd: &dyn Fn(Cmd, i32, i32) -> Vec4u, media: &[u32]) -> (Vec4u, bool) {
    let mut s = SpecSink { x, y, acc, atlas, aw, tabs, ink: false, bd, media };
    // SAFETY: as `window_pass`.
    unsafe { c.rec::<P>(i) }.paint(&Cx { u, rect: c.rect(i), pal: c.pal() }, NO_CLIP, &mut s);
    (s.acc, s.ink)
}
fn stage_one<P: Paint>(c: ColView, i: u32, u: &U, out: &mut bough_kernel::stage::Scene) {
    // SAFETY: as `window_pass`.
    unsafe { c.rec::<P>(i) }.paint(&Cx { u, rect: c.rect(i), pal: c.pal() }, NO_CLIP, &mut StageSink(out));
}
fn pick_one<P: Paint>(c: ColView, i: u32, u: &U) -> u32 {
    // SAFETY: as `window_pass`.
    unsafe { c.rec::<P>(i) }.pick_id(&Cx { u, rect: c.rect(i), pal: c.pal() })
}

struct D<P>(PhantomData<P>);
impl<P: Paint> D<P> {
    const DESC: Desc = Desc {
        name: std::any::type_name::<P>(),
        words: {
            assert!(size_of::<P>() % 4 == 0 && align_of::<P>() <= 4);
            size_of::<P>() / 4
        },
        reads: P::READS,
        geom_reads: P::GEOM_READS,
        opaque_core: P::OPAQUE_CORE,
        hit: P::HIT,
        cost: P::COST,
        layers: P::LAYERS,
        layer: P::LAYER,
        lay: lay_one::<P>,
        reach: reach_one::<P>,
        code: P::CODE,
        window: window_pass::<P>,
        flat: flat_run::<P>,
        shade: shade_run::<P>,
        spec: spec_one::<P>,
        stage: stage_one::<P>,
        pick: pick_one::<P>,
        rewin: rewin_one::<P>,
    };
}
impl Desc {
    pub const fn of<P: Paint>() -> &'static Desc {
        &D::<P>::DESC
    }
}

pub struct Col {
    pub desc: &'static Desc,
    /// First word in the arena.
    pub base: usize,
    /// Capacity in records (a multiple of 64).
    pub cap: usize,
    pub len: usize,
    /// Indices of its records that still depend on the clock (`Paint::until` ahead of the block last
    /// seen): what a time-only frame visits.
    pub active: Vec<u32>,
}
pub struct ColId<P>(pub usize, PhantomData<P>);
impl<P> Clone for ColId<P> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<P> Copy for ColId<P> {}

/// Words in VM-page-aligned memory that never moves.
pub struct Arena {
    ptr: *mut u32,
    words: usize,
}
impl Arena {
    pub fn new(words: usize) -> Arena {
        let bytes = (words * 4).div_ceil(VM_PAGE).max(1) * VM_PAGE;
        let layout = std::alloc::Layout::from_size_align(bytes, VM_PAGE).unwrap();
        // SAFETY: non-zero size; a null return is checked.
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) } as *mut u32;
        assert!(!ptr.is_null());
        Arena { ptr, words: bytes / 4 }
    }
    pub fn ptr(&self) -> *mut u32 {
        self.ptr
    }
    pub fn bytes(&self) -> usize {
        self.words * 4
    }
    pub fn words(&self) -> &[u32] {
        // SAFETY: `ptr` is `words` initialised (zeroed) words owned by this arena.
        unsafe { std::slice::from_raw_parts(self.ptr, self.words) }
    }
    pub(crate) fn words_mut(&mut self) -> &mut [u32] {
        // SAFETY: as `words`, uniquely borrowed.
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.words) }
    }
}
impl Drop for Arena {
    fn drop(&mut self) {
        // SAFETY: allocated in `new` with this layout.
        unsafe { std::alloc::dealloc(self.ptr as *mut u8, std::alloc::Layout::from_size_align(self.words * 4, VM_PAGE).unwrap()) }
    }
}
// SAFETY: the arena owns its memory; shared access is read-only (`words`), mutation needs `&mut`.
unsafe impl Send for Arena {}
unsafe impl Sync for Arena {}

/// Per-tile entry lists (paint-order keys, back to front). The lists are kept whole and edited in place
/// when a record's window moves (`update`: removed from the tiles it left, inserted in those it entered);
/// the cut at the last opaque cover is a start index per tile, recomputed for the tiles a change touched.
/// Each tile also keeps the layer-owning records listed in it: how an event finds the layers above it.
#[derive(Default)]
pub struct Bins {
    lists: Vec<Vec<u32>>,
    /// Per tile: the first visible entry of its list (the cover it is cut at, or 0).
    start: Vec<u32>,
    /// Per tile: the key of the opaque cover its list is cut at (0: not cut).
    pub floor: Vec<u32>,
    /// Per tile: the layer-owning records listed in it, ascending.
    lay: Vec<Vec<u32>>,
    /// By key: the window the lists hold the record under.
    binned: Vec<[i16; 4]>,
    tx: usize,
    ty: usize,
    /// Tiles whose list or cut changed since `take_changed` (an executor that mirrors the bins).
    changed: Vec<u32>,
    chg_mark: Vec<bool>,
    /// Tiles edited since the last `recut_touched`, and a flag per tile for it.
    touched: Vec<u32>,
    mark: Vec<bool>,
}
/// The lanes the bins are made from.
pub struct Lanes<'a> {
    pub win: &'a [[i16; 4]],
    pub cover: &'a [[i16; 4]],
    /// The halo lane and the layer-of-key table (both empty in a scene without layers): a layer's record is
    /// listed over its halo too.
    pub halo: &'a [[i16; 4]],
    pub lay: &'a [u32],
    pub cut: bool,
}
impl Lanes<'_> {
    #[inline]
    fn is_lay(&self, z: usize) -> bool {
        self.lay.get(z).is_some_and(|&l| l != layer::NONE)
    }
    #[inline]
    fn bin(&self, z: usize) -> [i16; 4] {
        let (w, h) = (self.win[z], if self.is_lay(z) { self.halo[z] } else { [0; 4] });
        if h[0] >= h[2] {
            w
        } else if w[0] >= w[2] {
            h
        } else {
            [w[0].min(h[0]), w[1].min(h[1]), w[2].max(h[2]), w[3].max(h[3])]
        }
    }
}
impl Bins {
    #[inline]
    pub fn tile(&self, t: usize) -> &[u32] {
        &self.lists[t][self.start[t] as usize..]
    }
    /// The layer-owning records listed in tile `t`, ascending.
    #[inline]
    pub fn layers(&self, t: usize) -> &[u32] {
        &self.lay[t]
    }
    /// The certificate: the least key above `z` in tile `t` whose opaque cover holds all of window `w`
    /// inside the tile, among the keys beneath `below` (`u32::MAX`: none). Whatever is stored for this tile above that record cannot depend
    /// on a change of record `z` confined to `w`: every pixel of the change is under an opaque pixel.
    pub fn cover_above(&self, t: usize, z: u32, w: [i32; 4], cover: &[[i16; 4]], below: u32) -> u32 {
        let (ox, oy) = ((t % self.tx) as i32 * 32, (t / self.tx) as i32 * 32);
        let r = [w[0].max(ox), w[1].max(oy), w[2].min(ox + 32), w[3].min(oy + 32)];
        let l = &self.lists[t];
        for &k in &l[l.partition_point(|&k| k <= z)..] {
            if k >= below {
                break;
            }
            let c = cover[k as usize];
            let holds = c[0] as i32 <= r[0] && c[1] as i32 <= r[1] && c[2] as i32 >= r[2] && c[3] as i32 >= r[3];
            let meets = (c[0] as i32) < r[2] && (c[1] as i32) < r[3] && c[2] as i32 > r[0] && c[3] as i32 > r[1];
            if c[0] < c[2] && (holds || (cfg!(feature = "mutant_cert") && meets)) {
                return k;
            }
        }
        u32::MAX
    }
    #[inline]
    fn span(&self, w: &[i16; 4]) -> (usize, usize, usize, usize) {
        if w[0] >= w[2] || w[1] >= w[3] {
            return (0, 0, 0, 0);
        }
        ((w[0].max(0) as usize) >> 5, (w[1].max(0) as usize) >> 5, ((w[2].max(0) as usize + 31) >> 5).min(self.tx), ((w[3].max(0) as usize + 31) >> 5).min(self.ty))
    }
    /// The cut of tile `t`: its list starts at the topmost opaque cover holding the whole tile that lies
    /// beneath every layer record listed there (a layer's prefix needs what is under a later cover).
    fn recut(&mut self, t: usize, l: &Lanes) {
        let (ox, oy) = ((t % self.tx) as i32 * 32, (t / self.tx) as i32 * 32);
        let (mut start, mut floor) = (0, 0);
        if l.cut {
            for (i, &z) in self.lists[t].iter().enumerate() {
                if l.is_lay(z as usize) {
                    break;
                }
                let c = l.cover[z as usize];
                if c[0] < c[2] && c[0] as i32 <= ox && c[1] as i32 <= oy && c[2] as i32 >= ox + 32 && c[3] as i32 >= oy + 32 {
                    (start, floor) = (i as u32, z);
                }
            }
        }
        (self.start[t], self.floor[t]) = (start, floor);
        if !self.chg_mark[t] {
            self.chg_mark[t] = true;
            self.changed.push(t as u32);
        }
    }
    /// From nothing: every record, in paint order.
    pub fn build(&mut self, l: &Lanes, tx: usize, ty: usize) {
        let nt = tx * ty;
        (self.tx, self.ty) = (tx, ty);
        for v in [&mut self.lists, &mut self.lay] {
            v.iter_mut().for_each(Vec::clear);
            v.resize_with(nt, Vec::new);
        }
        self.start.clear();
        self.start.resize(nt, 0);
        self.floor.clear();
        self.floor.resize(nt, 0);
        self.binned.clear();
        self.binned.resize(l.win.len(), [0; 4]);
        self.touched.clear();
        self.mark.clear();
        self.mark.resize(nt, false);
        for z in 0..l.win.len() {
            let b = l.bin(z);
            self.binned[z] = b;
            let (x0, y0, x1, y1) = self.span(&b);
            let lz = l.is_lay(z);
            for y in y0..y1 {
                for x in x0..x1 {
                    self.lists[y * tx + x].push(z as u32);
                    if lz {
                        self.lay[y * tx + x].push(z as u32);
                    }
                }
            }
        }
        self.changed.clear();
        self.chg_mark.clear();
        self.chg_mark.resize(nt, false);
        for t in 0..nt {
            self.recut(t, l);
        }
    }
    /// Record `z`'s window, halo or cover changed: edit the lists of the tiles it left and entered, and
    /// note every tile it was or is in for `recut_touched` (the cut is recomputed once per tile, however many
    /// records moved there). Cost: those tiles, not the scene.
    pub fn update(&mut self, z: u32, l: &Lanes) {
        let (old, new) = (self.binned[z as usize], l.bin(z as usize));
        let (o, n) = (self.span(&old), self.span(&new));
        let inside = |s: (usize, usize, usize, usize), x: usize, y: usize| x >= s.0 && x < s.2 && y >= s.1 && y < s.3;
        let lz = l.is_lay(z as usize);
        for y in o.1..o.3 {
            for x in o.0..o.2 {
                let t = y * self.tx + x;
                if !inside(n, x, y) {
                    if let Ok(i) = self.lists[t].binary_search(&z) {
                        self.lists[t].remove(i);
                    }
                    if lz {
                        self.lay[t].retain(|&k| k != z);
                    }
                }
                self.touch(t);
            }
        }
        for y in n.1..n.3 {
            for x in n.0..n.2 {
                if inside(o, x, y) || cfg!(feature = "mutant_bins_stale") {
                    continue;
                }
                let t = y * self.tx + x;
                if let Err(i) = self.lists[t].binary_search(&z) {
                    self.lists[t].insert(i, z);
                }
                if lz {
                    if let Err(i) = self.lay[t].binary_search(&z) {
                        self.lay[t].insert(i, z);
                    }
                }
                self.touch(t);
            }
        }
        self.binned[z as usize] = new;
    }
    #[inline]
    fn touch(&mut self, t: usize) {
        if !self.mark[t] {
            self.mark[t] = true;
            self.touched.push(t as u32);
        }
    }
    /// Recompute the cut of the tiles `update` touched since the last call.
    pub fn recut_touched(&mut self, l: &Lanes) {
        let touched = std::mem::take(&mut self.touched);
        for &t in &touched {
            self.mark[t as usize] = false;
            self.recut(t as usize, l);
        }
        self.touched = touched;
        self.touched.clear();
    }
    /// The visible lists as one array and an offset per tile (plus the end): what a GPU executor uploads.
    pub fn csr(&self) -> (Vec<u32>, Vec<u32>) {
        let mut list = Vec::with_capacity(self.lists.iter().map(Vec::len).sum());
        let mut off = Vec::with_capacity(self.lists.len() + 1);
        for t in 0..self.lists.len() {
            off.push(list.len() as u32);
            list.extend_from_slice(self.tile(t));
        }
        off.push(list.len() as u32);
        (list, off)
    }
    /// Entries of the visible lists in all.
    pub fn total(&self) -> usize {
        (0..self.lists.len()).map(|t| self.tile(t).len()).sum()
    }
    /// Forget which tiles changed (after an executor mirrored them).
    pub fn take_changed(&mut self) -> Vec<u32> {
        for &t in &self.changed {
            self.chg_mark[t as usize] = false;
        }
        std::mem::take(&mut self.changed)
    }
}

/// A column's pan and its records by scrolled-space row.
struct Scroll {
    wx: usize,
    wy: usize,
    /// Rows of 256 px of the scrolled space -> the records whose reach (window and halo, with the pan taken
    /// out) meets them.
    rows: std::collections::HashMap<i32, Vec<u32>>,
    /// Per record: the rows it is listed in (first, one past last); empty when it has no reach.
    at: Vec<(i32, i32)>,
    /// Records without a local pan certificate. Every pan must reconsider them.
    unbounded: std::collections::BTreeSet<u32>,
}
impl Scroll {
    /// Record `i` reaches rows `r` (of the scrolled space) now.
    fn place(&mut self, i: u32, rows: Option<(i32, i32)>) {
        let r = match rows {
            Some(r) if r.1 - r.0 <= 256 => {
                self.unbounded.remove(&i);
                r
            }
            _ => {
                self.unbounded.insert(i);
                (0, 0)
            }
        };
        if self.at.len() <= i as usize {
            self.at.resize(i as usize + 1, (0, 0));
        }
        let old = self.at[i as usize];
        if old == r {
            return;
        }
        for row in old.0..old.1 {
            if let Some(v) = self.rows.get_mut(&row) {
                v.retain(|&k| k != i);
            }
        }
        for row in r.0..r.1 {
            self.rows.entry(row).or_default().push(i);
        }
        self.at[i as usize] = r;
    }
}
/// Retained geometry for one rigid, clock-free scroll space. The same packed lists reach Metal.
#[derive(Default)]
pub(crate) struct WorldPan {
    slots: [usize; 2],
    pub(crate) version: u64,
    pub(crate) packed: Vec<u32>,
}
impl WorldPan {
    fn mask(&self) -> u64 {
        1 << self.slots[0] | 1 << self.slots[1]
    }

    fn candidates(&self, t: usize, tx: usize, u: &U) -> (&[u32], &[u32]) {
        let data = &self.packed;
        let cell = |coordinate: usize, slot| {
            let pan = u.w[slot] as i32;
            coordinate as i32 - (pan >> 13) - i32::from(pan & 8191 != 0)
        };
        let (x, y) = (cell(t % tx, self.slots[0]), cell(t / tx, self.slots[1]));
        let fixed = 4 + 2 * t;
        let header = 4 + 2 * data[2] as usize;
        let cells = data[header..header + 4 * data[3] as usize].as_chunks::<4>().0;
        let at = cells.partition_point(|r| (r[0] as i32, r[1] as i32) < (y, x));
        let list = |at: u32, len: u32| &data[at as usize..at as usize + len as usize];
        let world = cells.get(at).filter(|r| (r[0] as i32, r[1] as i32) == (y, x)).map_or(&[][..], |r| list(r[2], r[3]));
        (list(data[fixed], data[fixed + 1]), world)
    }

    fn merged(&self, t: usize, tx: usize, u: &U, out: &mut Vec<u32>) {
        let (mut fixed, mut world) = self.candidates(t, tx, u);
        out.clear();
        while let (Some(&f), Some(&w)) = (fixed.first(), world.first()) {
            if f < w {
                out.push(f);
                fixed = &fixed[1..];
            } else {
                out.push(w);
                world = &world[1..];
            }
        }
        out.extend_from_slice(fixed);
        out.extend_from_slice(world);
    }
}

/// A column's key word and its records by key.
struct Keyed {
    word: usize,
    by_id: std::collections::HashMap<u32, Vec<u32>>,
    id: Vec<u32>,
}

/// What a frame did.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    /// Block words that changed.
    pub mask: u64,
    /// Columns whose window pass ran, and the records they hold.
    pub cols_visited: u32,
    pub recs_visited: u32,
    pub rebinned: bool,
    /// A broad word changed: the frame is damaged whole and no record was asked about it.
    pub broad: bool,
    /// Layer work scheduled: prefix tiles, pyramid blocks, pyramid pixels.
    pub prefix_tiles: u32,
    pub blocks: u32,
    pub layer_px: u64,
    pub tiles: u32,
    pub raster: Stats,
}

pub struct Scene {
    pub fw: u32,
    pub fh: u32,
    pub tx: usize,
    pub ty: usize,
    pub(crate) arena: Arena,
    used: usize,
    pub(crate) cols: Vec<Col>,
    /// Paint-order key -> (column << 20 | index).
    pub(crate) order: Vec<u32>,
    /// By key: the record's pixel window and its opaque cover under the block last seen.
    pub win: Vec<[i16; 4]>,
    pub cover: Vec<[i16; 4]>,
    /// By key: the halo of a layer-owning record (frame pixels whose prefix its layer keeps).
    pub halo: Vec<[i16; 4]>,
    /// The layers (`layer.rs`), this frame's stages for them, and the events that scheduled them.
    pub layers: Layers,
    pub plan: layer::Plan,
    events: Vec<([i32; 4], u32)>,
    regeo: Vec<u32>,
    act: Vec<u32>,
    rebin: Vec<u32>,
    /// Layers with something due (pending tiles, entered strips, or fresh) and not yet scheduled or repaired.
    lay_todo: std::collections::BTreeSet<u32>,
    /// Per column: its declared pan and the index of its records by where they are in the scrolled space.
    scrolls: Vec<Option<Scroll>>,
    world_pan: Option<WorldPan>,
    world_frame: bool,
    /// Per column: the block word its records are compared with, and its records by key.
    keys: Vec<Option<Keyed>>,
    /// Bumped when the bins were rebuilt (an executor that mirrors them re-reads them).
    pub bins_epoch: u64,
    /// Keys whose window, halo or cover lane changed since `take_lane_dirty` (each once), and "all of
    /// them" (the first frame): what an executor that mirrors the lanes rewrites.
    lane_dirty: Vec<u32>,
    lane_mark: Vec<bool>,
    lane_all: bool,
    /// The executor finds the layers an event reaches itself (`metal::Gpu::resident`): `update` does not
    /// route events to layers. The CPU's `prepare` must not be used while this is set.
    pub route_off: bool,
    /// Measurement switch, never a result's configuration: events are routed without the cover certificate.
    pub no_cert: bool,
    /// This frame's events are routed with the certificate (`update` decides).
    cert: bool,
    /// Measurement switch, never a result's configuration: every layer is recomputed whole on every
    /// frame and no tile restarts from a stored prefix (what a compositor that redraws its layers does).
    pub fx_nocache: bool,
    /// Block words declared **broad**: a change of one (a theme's mix, a palette word) is never
    /// differentiated. The frame is damaged as one fact and every layer is due as one fact; no record is
    /// visited, no bins are rebuilt, nothing is enumerated. Only for words no geometry operand reads (a
    /// column whose geometry reads the word takes its window pass regardless).
    pub broad: u64,
    /// The media pool video records sample.
    pub media: crate::media::Pool,
    pub bins: Bins,
    pub damage: Damage,
    /// The block the surface shows.
    pub prev: U,
    pub first: bool,
    pub atlas: Vec<u8>,
    pub aw: u32,
    pub cut: bool,
    pub flat: bool,
    tiles: Vec<u32>,
    /// Keys of records rewritten since the last `update` (`set`).
    touched: Vec<u32>,
    /// The corner tables: a region of the arena after the columns (so the GPU reads them from the buffer it
    /// already has), handles by key, and the tables allocated but not yet filled.
    /// Give memoisable layers their tables (on by default; off: every such layer takes its closed form).
    pub memoise: bool,
    /// Admit complete shadow patches when mounting or rewriting a record.
    pub memoise_coverage: bool,
    tab_base: usize,
    /// The palette tables' first word in the arena (they end where the corner tables begin).
    pal_base: usize,
    /// Derived clock words: (word, start word, duration word). `update` computes `word` from `Time`.
    clocks: Vec<(usize, usize, usize)>,
    /// The block the last `update` ran under: the host's, with the derived words filled in.
    block: U,
    tab_used: usize,
    tab_map: std::collections::HashMap<(i32, i32), u32>,
    tab_jobs: Vec<TabJob>,
    patch_used: usize,
    patch_map: std::collections::HashMap<[u32; 4], u32>,
    /// Colour tables by content hash (`Scene::lut`): handles of the tables with that hash.
    lut_map: std::collections::HashMap<u64, Vec<u32>>,
    lut_free: Vec<u32>,
    luts: usize,
    /// When the shadow coverage store is full, start it over rather than leave later shadows without a
    /// patch (off by default: the owner must then rewrite the shadows it shows, `patch_clear`).
    pub patch_recycle: bool,
    /// Times the shadow coverage store was started over.
    pub patch_resets: u32,
    /// Pixel rects of records whose content changed without a record write (`Scene::damage_part`).
    part: Vec<([i32; 4], u32)>,
}

/// Words of a colour table.
pub const LUT_WORDS: usize = 256;

/// A gradient's stops as a 256-entry table of premultiplied RGBA8 (entry i is the colour at i / 255), and
/// whether every entry is opaque. `stops`: (position 0..1, straight RGBA8 with red in the low byte), in
/// any order; positions are clamped and sorted (stable), as CSS resolves them. Colours are interpolated
/// premultiplied. No stop: transparent; one stop: its colour throughout.
pub fn bake_ramp(stops: &[(f32, u32)]) -> ([u32; LUT_WORDS], bool) {
    let mut st: Vec<(f32, [f32; 4])> = stops
        .iter()
        .map(|&(t, c)| {
            let a = (c >> 24) as f32 / 255.0;
            (if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 }, [(c & 255) as f32 * a, ((c >> 8) & 255) as f32 * a, ((c >> 16) & 255) as f32 * a, (c >> 24) as f32])
        })
        .collect();
    st.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = [0u32; LUT_WORDS];
    if st.is_empty() {
        return (out, false);
    }
    let mut seg = 0;
    for (i, o) in out.iter_mut().enumerate() {
        let t = i as f32 / 255.0;
        while seg + 1 < st.len() && st[seg + 1].0 <= t {
            seg += 1;
        }
        let (a, b) = (st[seg], st[(seg + 1).min(st.len() - 1)]);
        let f = if t <= a.0 || b.0 <= a.0 { 0.0 } else { ((t - a.0) / (b.0 - a.0)).clamp(0.0, 1.0) };
        let ch = |k: usize| (a.1[k] + (b.1[k] - a.1[k]) * f).round().clamp(0.0, 255.0) as u32;
        let al = ch(3);
        *o = ch(0).min(al) | ch(1).min(al) << 8 | ch(2).min(al) << 16 | al << 24;
    }
    let opaque = out.iter().all(|w| w >> 24 == 255);
    (out, opaque)
}

/// Words reserved for corner tables (16 MiB of address space; pages are touched only when a table is filled).
pub const TAB_WORDS: usize = 1 << 22;
const PATCH_WORDS: usize = 1 << 20;
/// Slots of a palette table, tables a scene keeps, and their words in all.
pub const PAL_SLOTS: usize = 256;
pub const PAL_TABLES: usize = 16;
pub const PAL_WORDS: usize = PAL_SLOTS * PAL_TABLES;
/// The Q15 progress of `now` from `start` over `dur` ticks (wrapping time: a start in the future is 0).
pub fn clock_at(now: u32, start: u32, dur: u32) -> u32 {
    let e = now.wrapping_sub(start);
    if e >= 1 << 31 {
        0
    } else if dur == 0 || e >= dur {
        32_768
    } else {
        ((e as u64 * 32_768) / dur as u64) as u32
    }
}
/// A table to fill: its handle (word offset in the table region), key, and entries per side.
#[derive(Clone, Copy, Debug)]
pub struct TabJob {
    pub base: u32,
    pub r: i32,
    pub w: i32,
    pub n: u32,
}

/// `BOUGH_PAINT_PROF=1`: nanoseconds and tile entries per column, accumulated by `shade` (a measurement
/// build's switch: it times every run, so the totals are for proportions, never for a result).
pub static PROF: std::sync::Mutex<[(u64, u64); 32]> = std::sync::Mutex::new([(0, 0); 32]);
static PROF_ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

struct OutPtr(*mut u32);
// SAFETY: workers write disjoint tiles through it (`Scene::shade`).
unsafe impl Sync for OutPtr {}

impl Scene {
    pub fn new(fw: u32, fh: u32, arena_words: usize) -> Scene {
        // windows are 16-bit pixel coordinates (the stager clamps them there): the surface must fit
        assert!(fw >= 1 && fh >= 1 && fw <= MAX_PX as u32 && fh <= MAX_PX as u32, "a surface of {fw} x {fh} px does not fit the window lane");
        let (tx, ty) = (fw.div_ceil(32) as usize, fh.div_ceil(32) as usize);
        Scene {
            fw,
            fh,
            tx,
            ty,
            arena: Arena::new(arena_words + PAL_WORDS + TAB_WORDS),
            pal_base: arena_words,
            clocks: Vec::new(),
            block: U::default(),
            memoise: std::env::var("BOUGH_NO_TABS").is_err(),
            memoise_coverage: true,
            tab_base: arena_words + PAL_WORDS,
            tab_used: 0,
            tab_map: Default::default(),
            tab_jobs: Vec::new(),
            patch_used: 0,
            patch_map: Default::default(),
            lut_map: Default::default(),
            lut_free: Vec::new(),
            luts: 0,
            patch_recycle: false,
            patch_resets: 0,
            part: Vec::new(),
            used: 0,
            cols: Vec::new(),
            order: Vec::new(),
            win: Vec::new(),
            cover: Vec::new(),
            halo: Vec::new(),
            layers: Layers::default(),
            plan: layer::Plan::default(),
            events: Vec::new(),
            regeo: Vec::new(),
            act: Vec::new(),
            rebin: Vec::new(),
            lay_todo: Default::default(),
            scrolls: Vec::new(),
            world_pan: None,
            world_frame: false,
            keys: Vec::new(),
            bins_epoch: 0,
            lane_dirty: Vec::new(),
            lane_mark: Vec::new(),
            lane_all: true,
            route_off: false,
            no_cert: false,
            cert: true,
            fx_nocache: std::env::var("BOUGH_FX_NOCACHE").is_ok(),
            broad: 0,
            media: crate::media::Pool::new(0),
            bins: Bins::default(),
            damage: Damage::new(tx, ty),
            prev: U::default(),
            first: true,
            atlas: Vec::new(),
            aw: 1,
            cut: true,
            flat: true,
            tiles: Vec::new(),
            touched: Vec::new(),
        }
    }
    /// A column for archetype `P` holding up to `cap` records.
    pub fn column<P: Paint>(&mut self, cap: usize) -> ColId<P> {
        let desc = Desc::of::<P>();
        let cap = cap.div_ceil(64).max(1) * 64;
        let words = cap * (LANES + desc.words);
        assert!(self.used + words <= self.pal_base, "arena too small");
        assert!(self.cols.len() < 1 << (32 - IDX_BITS) && cap <= 1 << IDX_BITS);
        self.cols.push(Col { desc, base: self.used, cap, len: 0, active: Vec::new() });
        self.scrolls.push(None);
        self.keys.push(None);
        self.used += words;
        ColId(self.cols.len() - 1, PhantomData)
    }
    /// Declare that block words `wx` and `wy` are column `col`'s pan: its records depend on them only through
    /// a translation of their boxes by (`u[wx]`, `u[wy]`) (Q8), as `Off<Lane, I<Uw<wx>>, I<Uw<wy>>>` does.
    /// A change of the pan then visits only the records near the old and the new viewport: the scene keeps
    /// the column's records by where they are in the scrolled space. The declaration is trusted (a column
    /// that read the words any other way would be drawn stale; `tests/fx.rs` holds the declared scene equal
    /// to the undeclared one).
    pub fn scroll<P: Paint>(&mut self, col: ColId<P>, wx: usize, wy: usize) {
        assert!(wx < 64 && wy < 64 && wx != wy);
        self.scrolls[col.0] = Some(Scroll { wx, wy, rows: Default::default(), at: Vec::new(), unbounded: Default::default() });
        self.first = true;
    }
    /// Attempt to keep one declared scroll space in world coordinates. Its scrolled columns must all
    /// use these block words and their `scroll` translation contract. The scene takes ordinary frame
    /// bins for clocks, backdrop layers, unbounded hulls, and turns that mix pan with other inputs.
    /// Use `Scene::frame` or the Metal staged/resident executors. The public window lanes describe the
    /// last ordinary frame while a pure pan is drawn directly from the world lists.
    pub fn uniform_pan(&mut self, wx: usize, wy: usize) {
        assert!(wx < U_TIME && wy < U_TIME && wx != wy);
        let version = self.world_pan.as_ref().map_or(0, |world| world.version);
        self.world_pan = Some(WorldPan { slots: [wx, wy], version, packed: Vec::new() });
        self.first = true;
    }

    pub(crate) fn world_frame(&self) -> Option<&WorldPan> {
        self.world_pan.as_ref().filter(|_| self.world_frame)
    }

    fn build_world_pan(&self, u: &U, slots: [usize; 2]) -> Option<Vec<u32>> {
        if self.layers.any() || !self.clocks.is_empty() {
            return None;
        }
        let mask = 1 << slots[0] | 1 << slots[1];
        let mut world = std::collections::BTreeMap::<(i32, i32), Vec<u32>>::new();
        let mut refs = 0u64;
        for (ci, col) in self.cols.iter().enumerate() {
            if col.desc.reads & (1 << U_TIME) != 0 {
                return None;
            }
            let Some(scroll) = &self.scrolls[ci] else {
                if col.desc.reads & mask != 0 { return None; }
                continue;
            };
            if [scroll.wx, scroll.wy] != slots {
                return None;
            }
            let view = self.view(ci);
            for i in 0..view.len {
                let [x0, y0, ..] = view.rect(i);
                let (px, py) = (-(x0 >> 8), -(y0 >> 8));
                let mut local = *u;
                (local.w[slots[0]], local.w[slots[1]]) = ((px << 8) as u32, (py << 8) as u32);
                let w = (col.desc.reach)(view, i, &local)?;
                if w.x >= w.z || w.y >= w.w {
                    // An empty sample at one subpixel phase need not be empty at all phases.
                    return None;
                }
                // At the staging clip we cannot distinguish a clipped object from its full translated footprint.
                if w.x <= -16000 || w.y <= -16000 || w.z >= 16000 || w.w >= 16000 {
                    return None;
                }
                // A cell starting at a multiple of 32 is 64 pixels wide. Include a one-pixel
                // margin because the translation can move the pixel-centre rounding by one.
                let (a, b) = ((w.x - px - 64) >> 5, (w.z - px) >> 5);
                let (c, d) = ((w.y - py - 64) >> 5, (w.w - py) >> 5);
                refs += (b - a + 1) as u64 * (d - c + 1) as u64;
                if refs > 1 << 22 {
                    return None;
                }
                let z = view.z(i);
                for y in c..=d {
                    for x in a..=b {
                        world.entry((y, x)).or_default().push(z);
                    }
                }
            }
        }
        let nt = self.tx * self.ty;
        let mut data = vec![0; 4 + 2 * nt + 4 * world.len()];
        data[..4].copy_from_slice(&[slots[0] as u32, slots[1] as u32, nt as u32, world.len() as u32]);
        for (tile, keys) in self.bins.lists.iter().enumerate() {
            let offset = data.len();
            data.extend(keys.iter().copied().filter(|&z| self.scrolls[(self.order[z as usize] >> IDX_BITS) as usize].is_none()));
            let count = data.len() - offset;
            data[4 + 2 * tile..][..2].copy_from_slice(&[offset as u32, count as u32]);
        }
        for (i, ((y, x), mut keys)) in world.into_iter().enumerate() {
            keys.sort_unstable();
            let at = data.len() as u32;
            data[4 + 2 * nt + 4 * i..][..4].copy_from_slice(&[y as u32, x as u32, at, keys.len() as u32]);
            data.extend(keys);
        }
        Some(data)
    }

    /// Declare that column `col`'s records depend on block word `word` only by comparing it with their own
    /// key (`Eq<Uw<word>, Rec>`): a change of the word visits the records keyed by its old and its new
    /// value (`set_key`). Trusted, as `scroll`.
    pub fn keyed<P: Paint>(&mut self, col: ColId<P>, word: usize) {
        assert!(word < 64);
        self.keys[col.0] = Some(Keyed { word, by_id: Default::default(), id: Vec::new() });
    }
    /// Record `i` of a keyed column has key `id`.
    pub fn set_key<P: Paint>(&mut self, col: ColId<P>, i: usize, id: u32) {
        let k = self.keys[col.0].as_mut().expect("the column is not keyed");
        if k.id.len() <= i {
            k.id.resize(i + 1, u32::MAX);
        }
        let old = std::mem::replace(&mut k.id[i], id);
        if old != id {
            if let Some(v) = k.by_id.get_mut(&old) {
                v.retain(|&r| r != i as u32);
            }
            k.by_id.entry(id).or_default().push(i as u32);
        }
    }
    /// Bring record `i` of a scrolled column to where it is in the scrolled space under `u`: its reach is
    /// evaluated under a pan that puts its box at the origin (so nothing is clamped), then the pan is taken out.
    fn reindex(scroll: &mut Scroll, d: &Desc, v: ColView, i: u32, u: &U) {
        let rect = v.rect(i);
        let (px, py) = (-(rect[0] >> 8), -(rect[1] >> 8));
        let mut at = *u;
        (at.w[scroll.wx], at.w[scroll.wy]) = ((px << 8) as u32, (py << 8) as u32);
        let rows = (d.reach)(v, i, &at).map(|w| {
            if w.x < w.z && w.y < w.w {
                ((w.y - py - 1) >> 8, ((w.w - py) >> 8) + 1)
            } else {
                (0, 0)
            }
        });
        scroll.place(i, rows);
    }
    /// Append a record: its rect lanes (Q8) and its value. Paint order is mount order. Returns its key.
    pub fn push<P: Paint>(&mut self, col: ColId<P>, rect: [i32; 4], p: P) -> u32 {
        let p = self.memo(rect, p);
        let z = self.order.len() as u32;
        let c = &mut self.cols[col.0];
        assert!(std::ptr::eq(c.desc, Desc::of::<P>()) || c.desc.name == Desc::of::<P>().name);
        assert!(c.len < c.cap, "column {} full", c.desc.name);
        let (i, w, base) = (c.len, c.desc.words, c.base);
        c.len += 1;
        let page = base + (i >> 6) * 64 * (LANES + w);
        let r = i & 63;
        let words = self.arena.words_mut();
        for l in 0..4 {
            words[page + l * 64 + r] = rect[l] as u32;
        }
        words[page + 256 + r] = z;
        let at = page + 64 * LANES + r * w;
        // SAFETY: `P: Words`: `w` whole words with no padding; the destination is `w` words inside the arena.
        unsafe { std::ptr::copy_nonoverlapping(&p as *const P as *const u32, words[at..at + w].as_mut_ptr(), w) };
        self.order.push((col.0 as u32) << IDX_BITS | i as u32);
        self.win.push([0; 4]);
        self.cover.push([0; 4]);
        self.halo.push([0; 4]);
        if P::LAYER {
            self.layers.mount(z);
        }
        self.first = true;
        z
    }
    /// Rewrite record `i` of a column in place (a turn's write: a rect the layout moved, an operand a CPU
    /// stage expression re-evaluated, a transition retargeted). Returns its key. The next `update` damages
    /// where it was and where it is; nothing else about the frame changes.
    pub fn set<P: Paint>(&mut self, col: ColId<P>, i: usize, rect: [i32; 4], p: P) -> u32 {
        let p = self.memo(rect, p);
        let c = &self.cols[col.0];
        assert!(std::ptr::eq(c.desc, Desc::of::<P>()) || c.desc.name == Desc::of::<P>().name);
        assert!(i < c.len);
        let (w, base) = (c.desc.words, c.base);
        let page = base + (i >> 6) * 64 * (LANES + w);
        let r = i & 63;
        let words = self.arena.words_mut();
        for l in 0..4 {
            words[page + l * 64 + r] = rect[l] as u32;
        }
        let z = words[page + 256 + r];
        let at = page + 64 * LANES + r * w;
        // SAFETY: as `push`.
        unsafe { std::ptr::copy_nonoverlapping(&p as *const P as *const u32, words[at..at + w].as_mut_ptr(), w) };
        self.touched.push(z);
        z
    }
    /// Give the record's memoisable layers their table handles (`Paint::memo`), allocating a table for a
    /// key not seen before. Cold: once per record mount or rewrite.
    fn memo<P: Paint>(&mut self, rect: [i32; 4], mut p: P) -> P {
        // admission: the kernels' Q8 arithmetic (offsets from a pixel centre, products with a blur's
        // inverse width) is proved for coordinates of at most 2^15 pixels
        // ... and that is about the pixel being shaded against the staged box: windows are clamped to 16 bits
        // and a box is only ever shaded inside its window, so what must be bounded is a record's own extent
        // (a box wider than the range has edges no pixel of the frame is near) and, loosely, where it lies
        // (so that Q8 lanes and a pan added to them stay inside 32 bits).
        let near = rect.iter().all(|v| v.abs() <= MAX_PX * 256 + 255);
        let far_but_small = rect.iter().all(|v| v.abs() <= MAX_WORLD_PX * 256) && (rect[2] as i64 - rect[0] as i64).abs() <= MAX_PX as i64 * 256 + 255 && (rect[3] as i64 - rect[1] as i64).abs() <= MAX_PX as i64 * 256 + 255;
        assert!(near || far_but_small, "a rect lane beyond +-{MAX_PX} px of a record wider than that, or beyond +-{MAX_WORLD_PX} px: {rect:?}");
        if !self.memoise {
            return p;
        }
        let zero = U::default();
        let (map, jobs, used) = (&mut self.tab_map, &mut self.tab_jobs, &mut self.tab_used);
        p.memo(&Cx::at(&zero, rect), &mut |r, w| {
            *map.entry((r, w)).or_insert_with(|| {
                let n = crate::ops::ops::t_n(r, w);
                let words = 2 + (n * crate::ops::ops::t_stride(n) / 2) as usize;
                if *used + words > TAB_WORDS - PATCH_WORDS {
                    return crate::ops::ops::T_MISS;
                }
                let base = *used as u32;
                *used += words;
                jobs.push(TabJob { base, r, w, n });
                base
            })
        });
        // a fresh table's header names no key until it is filled
        let tb = self.tab_base;
        let words = self.arena.words_mut();
        for j in &self.tab_jobs {
            if words[tb + j.base as usize] == 0 && words[tb + j.base as usize + 1] == 0 {
                (words[tb + j.base as usize], words[tb + j.base as usize + 1]) = (u32::MAX, u32::MAX);
            }
        }
        if self.memoise_coverage {
            p.memo_coverage(&Cx::at(&zero, rect), &mut |cmd| self.shadow_patch(cmd));
        }
        p
    }
    fn shadow_patch(&mut self, cmd: Cmd) -> Option<u32> {
        use crate::ops::ops as x;
        let s = x::x_patch_shape(cmd);
        if cmd.b.x >= cmd.b.z || cmd.b.y >= cmd.b.w || s.x >= 4096 || s.y >= 4096 {
            return None;
        }
        let key = [cmd.rad.x as u32, cmd.rad.y as u32, x::x_patch_phase(cmd), x::x_patch_dims(s)];
        if let Some(&handle) = self.patch_map.get(&key) {
            return Some(handle);
        }
        let (width, height) = (s.x + 2 * s.w + 1, s.y + 2 * s.w + 1);
        let samples = width as usize * height as usize;
        let needed = 4 + samples.div_ceil(2);
        if needed > PATCH_WORDS {
            return None;
        }
        if self.patch_used + needed > PATCH_WORDS {
            if !self.patch_recycle {
                return None;
            }
            self.patch_clear();
        }
        self.patch_used += needed;
        let base = TAB_WORDS - self.patch_used;
        let phase = key[2] as i32;
        let local = Vec4 { x: phase & 255, y: (phase >> 8) & 255, z: (s.x << 8) + ((phase >> 16) & 255), w: (s.y << 8) + ((phase >> 24) & 255) };
        let ink = Vec4 { x: 0, y: 0, z: 0, w: 0 };
        let canonical = x::mk_shadow(local, cmd.rad.x, cmd.rad.y, ink, x::T_MISS, NO_CLIP);
        debug_assert_eq!(canonical.rad, cmd.rad);
        let at = self.tab_base + base;
        let table = &mut self.arena.words_mut()[at..at + needed];
        table[..4].copy_from_slice(&key);
        for (i, word) in table[4..].iter_mut().enumerate() {
            let sample = |n: usize| {
                if n >= samples { return 0; }
                let (x, y) = (n as i32 % width - s.w, n as i32 / width - s.w);
                bough_kernel::k::shadow(canonical, x * 256 + 128, y * 256 + 128)
            };
            *word = sample(2 * i) | (sample(2 * i + 1) << 16);
        }
        let handle = x::PATCH_BIT | base as u32;
        self.patch_map.insert(key, handle);
        Some(handle)
    }
    /// The handle of the colour table holding `words` (premultiplied RGBA8, `bake_ramp`), written into the
    /// table region once per distinct content: every gradient with the same stops shares one table, on
    /// both executors (the region is the arena the GPU already binds). `None` when the region is full.
    pub fn lut_words(&mut self, words: &[u32; LUT_WORDS]) -> Option<u32> {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        words.hash(&mut h);
        let key = h.finish();
        let tb = self.tab_base;
        if let Some(list) = self.lut_map.get(&key) {
            for &handle in list {
                if self.arena.words()[tb + handle as usize..tb + handle as usize + LUT_WORDS] == words[..] {
                    return Some(handle);
                }
            }
        }
        let handle = match self.lut_free.pop() {
            Some(handle) => handle,
            None => {
                if self.tab_used + LUT_WORDS > TAB_WORDS - PATCH_WORDS {
                    return None;
                }
                self.tab_used += LUT_WORDS;
                (self.tab_used - LUT_WORDS) as u32
            }
        };
        self.arena.words_mut()[tb + handle as usize..tb + handle as usize + LUT_WORDS].copy_from_slice(words);
        self.lut_map.entry(key).or_default().push(handle);
        self.luts += 1;
        Some(handle)
    }
    /// The table for `stops` (`bake_ramp`) and whether it is opaque.
    pub fn lut(&mut self, stops: &[(f32, u32)]) -> Option<(u32, bool)> {
        let (words, opaque) = bake_ramp(stops);
        Some((self.lut_words(&words)?, opaque))
    }
    /// Give a table back: no record may name `handle` any more. Its words are reused by the next new table.
    pub fn lut_release(&mut self, handle: u32) {
        let mut found = false;
        self.lut_map.retain(|_, list| {
            let n = list.len();
            list.retain(|&h| h != handle);
            found |= list.len() != n;
            !list.is_empty()
        });
        if found {
            self.lut_free.push(handle);
            self.luts -= 1;
        }
    }
    /// Give back every table `keep` does not want (no record may name one of them any more).
    pub fn lut_retain(&mut self, mut keep: impl FnMut(u32) -> bool) {
        let (free, luts) = (&mut self.lut_free, &mut self.luts);
        self.lut_map.retain(|_, list| {
            list.retain(|&h| {
                let k = keep(h);
                if !k {
                    free.push(h);
                    *luts -= 1;
                }
                k
            });
            !list.is_empty()
        });
    }
    /// Distinct colour tables the scene holds.
    pub fn luts(&self) -> usize {
        self.luts
    }
    /// The content record `z` shows changed inside the pixel rect `r` without a record write (a retained
    /// surface repainted in place): the next `update` damages `r` cut to the record's window, and it is an
    /// event for the layers above, exactly as a rewrite is, but for those pixels only.
    pub fn damage_part(&mut self, z: u32, r: [i32; 4]) {
        if r[0] < r[2] && r[1] < r[3] && (z as usize) < self.order.len() {
            self.part.push((r, z));
        }
    }
    /// Forget every record, keeping the columns (so an executor bound to this scene stays valid: re-`sync`
    /// it), the tables and the media pool. The next frame is a first frame.
    pub fn clear_records(&mut self) {
        for c in self.cols.iter_mut() {
            c.len = 0;
            c.active.clear();
        }
        self.order.clear();
        self.win.clear();
        self.cover.clear();
        self.halo.clear();
        self.layers.reset();
        self.plan = layer::Plan::default();
        self.events.clear();
        self.regeo.clear();
        self.lay_todo.clear();
        for sc in self.scrolls.iter_mut().flatten() {
            sc.rows.clear();
            sc.at.clear();
            sc.unbounded.clear();
        }
        for k in self.keys.iter_mut().flatten() {
            k.by_id.clear();
            k.id.clear();
        }
        self.touched.clear();
        self.part.clear();
        self.lane_dirty.clear();
        self.lane_mark.clear();
        self.lane_all = true;
        self.world_frame = false;
        self.bins = Bins::default();
        self.bins_epoch += 1;
        self.first = true;
    }
    /// Start the shadow coverage store over. Every patch's header is unwritten first, so a record that
    /// still names one finds no key there and takes the analytic profile, exactly. The owner of the
    /// records should rewrite the shadows it still shows (they take fresh patches): a stale handle is
    /// harmless only while no later patch's samples happen to read as its key.
    pub fn patch_clear(&mut self) {
        let tb = self.tab_base;
        let words = self.arena.words_mut();
        for &handle in self.patch_map.values() {
            let at = tb + (handle & crate::ops::ops::PATCH_MASK) as usize;
            words[at..at + 4].fill(u32::MAX);
        }
        self.patch_map.clear();
        self.patch_used = 0;
        self.patch_resets += 1;
    }
    /// Bytes of retained Q15 shadow coverage in this scene.
    pub fn shadow_coverage_bytes(&self) -> usize {
        4 * self.patch_used
    }

    /// The corner tables' words (handles index it).
    pub fn tabs(&self) -> &[u32] {
        &self.arena.words()[self.tab_base..self.tab_base + TAB_WORDS]
    }
    /// The table region's first word in the arena, and its words in use.
    pub fn tab_region(&self) -> (usize, usize) {
        (self.tab_base, self.tab_used)
    }
    /// Tables allocated and not yet filled.
    pub fn tab_jobs(&self) -> &[TabJob] {
        &self.tab_jobs
    }
    /// The pending tables' entries have been written by someone else (the GPU, into this arena): name
    /// their keys in the headers, which is what makes a shader use them.
    pub fn tabs_filled(&mut self) {
        let tb = self.tab_base;
        let jobs = std::mem::take(&mut self.tab_jobs);
        let words = self.arena.words_mut();
        for j in &jobs {
            (words[tb + j.base as usize], words[tb + j.base as usize + 1]) = (j.r as u32, j.w as u32);
        }
    }
    /// Fill the pending tables on this thread (the CPU-only path; the same leaf the GPU kernel runs).
    pub fn fill_tabs(&mut self) {
        let tb = self.tab_base;
        let jobs = self.tab_jobs.clone();
        let words = self.arena.words_mut();
        for j in &jobs {
            let wpr = crate::ops::ops::t_stride(j.n) / 2;
            for y in 0..j.n {
                for k in 0..wpr {
                    let v = crate::ops::ops::x_cfillw(j.r, j.w, k, y);
                    words[tb + j.base as usize + 2 + (y * wpr + k) as usize] = if cfg!(feature = "mutant_tab") { v + 1 } else { v };
                }
            }
        }
        self.tabs_filled();
    }
    pub(crate) fn view(&self, col: usize) -> ColView<'_> {
        view_of(&self.arena, self.pal_base, &self.cols[col])
    }
    /// The columns, in creation order (read only: the kernels' unchecked reads rest on them).
    pub fn cols(&self) -> &[Col] {
        &self.cols
    }
    /// Paint-order key -> (column << 20 | index).
    pub fn order(&self) -> &[u32] {
        &self.order
    }
    /// The column arena's address and size, for an executor that binds it.
    pub fn arena(&self) -> &Arena {
        &self.arena
    }
    pub fn records(&self) -> usize {
        self.order.len()
    }
    /// Words the columns occupy (lanes and records, by capacity).
    pub fn used_words(&self) -> usize {
        self.used
    }

    /// Steps 1 to 3 of a frame: the mask, the window passes, the bins; then the layers' schedule (their
    /// stages are in `self.plan`: `prepare` runs them). Leaves the output damage in `self.damage`.
    pub fn update(&mut self, u: &U) -> FrameStats {
        // the derived words: a clock's progress is a block word the scene computes, so its readers read an
        // ordinary (broad) word and none of them reads `Time`
        let mut eff = *u;
        for &(w, s, d) in &self.clocks {
            eff.w[w] = clock_at(u.w[U_TIME], u.w[s], u.w[d]);
        }
        self.block = eff;
        let u = &eff;
        let mut fs = FrameStats::default();
        if !self.tab_jobs.is_empty() {
            self.fill_tabs();
        }
        // A backward seek invalidates future-only hulls. Ordinary unsigned wrap is forward time.
        let changed = self.prev.diff(u, 64);
        let first = self.first || (u.w[U_TIME].wrapping_sub(self.prev.w[U_TIME]) as i32) < 0;
        if !first && self.touched.is_empty() {
            if self.world_frame && changed == 0 {
                return fs;
            }
            if let Some(world) = &self.world_pan {
                if !world.packed.is_empty() && changed != 0 && changed & !world.mask() == 0 {
                    self.world_frame = true;
                    self.prev = *u;
                    self.damage.all();
                    return FrameStats { mask: changed, broad: true, ..fs };
                }
            }
        }
        let first = first || std::mem::take(&mut self.world_frame);
        let mask = if first { u64::MAX } else { changed };
        let rebuild_world = self.world_pan.is_some() && (first || !self.touched.is_empty() || self.cols.iter().any(|c| c.desc.geom_reads & mask != 0));
        fs.mask = mask;
        if first {
            self.damage.all();
            // a full redraw recomputes every layer whole
            self.layers.list.iter_mut().for_each(|l| l.fresh = true);
        }
        let mut moved = first;
        let mut touched = std::mem::take(&mut self.touched);
        let (mut events, mut regeo, mut act, mut rebin) = (std::mem::take(&mut self.events), std::mem::take(&mut self.regeo), std::mem::take(&mut self.act), std::mem::take(&mut self.rebin));
        rebin.clear();
        events.clear();
        regeo.clear();
        let track = self.layers.any() && !first;
        macro_rules! pass {
            () => {
                Pass { win: &mut self.win, cover: &mut self.cover, halo: &mut self.halo, dmg: &mut self.damage, floor: &self.bins.floor, events: &mut events, track, regeo: &mut regeo, active: &mut act, rebin: &mut rebin, frame: [self.fw as i16, self.fh as i16] }
            };
        }
        if !first {
            for &z in &touched {
                let o = self.order[z as usize];
                let ci = (o >> IDX_BITS) as usize;
                let v = view_of(&self.arena, self.pal_base, &self.cols[ci]);
                act.clear();
                moved |= (self.cols[ci].desc.rewin)(v, o & IDX_MASK, u, &mut pass!());
                // a rewritten record that now depends on the clock joins its column's active set
                let i = o & IDX_MASK;
                let active = &mut self.cols[ci].active;
                if act.is_empty() {
                    if let Some(at) = active.iter().position(|&j| i == j) {
                        active.swap_remove(at);
                    }
                } else if !active.contains(&i) {
                    active.push(i);
                }
                if let Some(sc) = self.scrolls[ci].as_mut() {
                    Self::reindex(sc, self.cols[ci].desc, view_of(&self.arena, self.pal_base, &self.cols[ci]), o & IDX_MASK, u);
                }
            }
        }
        touched.clear();
        self.touched = touched;
        let mut part = std::mem::take(&mut self.part);
        if !first {
            for &(r, z) in &part {
                let w = self.win[z as usize];
                let c = [r[0].max(w[0] as i32), r[1].max(w[1] as i32), r[2].min(w[2] as i32), r[3].min(w[3] as i32)];
                if c[0] < c[2] && c[1] < c[3] {
                    pass!().mark([c[0] as i16, c[1] as i16, c[2] as i16, c[3] as i16], z);
                }
            }
        }
        part.clear();
        self.part = part;
        // the clock alone never sends a pass over a whole column: its readers said when they stop
        let tbit = 1u64 << U_TIME;
        // a broad word is not differentiated: it damages the frame as one fact (below)
        let bword = if first { 0 } else { mask & self.broad };
        let mut broad_hit = false;
        for ci in 0..self.cols.len() {
            let d = self.cols[ci].desc;
            let mask = if d.geom_reads & bword == 0 {
                broad_hit |= d.reads & bword != 0;
                mask & !bword
            } else {
                mask
            };
            // what moved for this column besides the clock; a pan or a key word it declared is answered by
            // its index (the records that can show a difference), anything else by the whole column
            let bits = d.reads & mask & !tbit;
            let timed = mask & d.reads & tbit != 0 && !self.cols[ci].active.is_empty();
            let mut cand: Option<Vec<u32>> = None;
            if !first && bits != 0 {
                let (mut rest, mut c) = (bits, Vec::new());
                if let Some(sc) = self.scrolls[ci].as_ref() {
                    if rest & (1 << sc.wx | 1 << sc.wy) != 0 {
                        c.extend(sc.unbounded.iter().copied());
                        // the rows of the scrolled space the frame showed and shows
                        for (k, b) in [&self.prev, u].into_iter().enumerate() {
                            if k == 0 && cfg!(feature = "mutant_scroll_band") {
                                continue;
                            }
                            let py = (b.w[sc.wy] as i32) >> 8;
                            for row in (-py) >> 8..=(self.fh as i32 - py) >> 8 {
                                if let Some(v) = sc.rows.get(&row) {
                                    c.extend_from_slice(v);
                                }
                            }
                        }
                        rest &= !(1 << sc.wx | 1 << sc.wy);
                    }
                }
                if let Some(k) = self.keys[ci].as_ref() {
                    if rest & (1 << k.word) != 0 {
                        for (j, id) in [self.prev.w[k.word], u.w[k.word]].into_iter().enumerate() {
                            if j == 0 && cfg!(feature = "mutant_key_old") {
                                continue;
                            }
                            if let Some(v) = k.by_id.get(&id) {
                                c.extend_from_slice(v);
                            }
                        }
                        rest &= !(1 << k.word);
                    }
                }
                if rest == 0 {
                    if timed {
                        c.extend_from_slice(&self.cols[ci].active);
                    }
                    c.sort_unstable();
                    c.dedup();
                    cand = Some(c);
                }
            }
            let indexed = cand.is_some();
            let full = first || (bits != 0 && !indexed);
            if !full && !indexed && !timed {
                continue;
            }
            // The index already includes future motion. Refresh it only for changes that can move that
            // sweep within the scrolled space, rather than when the record moves inside its sweep.
            let reindex = self.scrolls[ci].as_ref().is_some_and(|sc| first || bits & d.geom_reads & !(1 << sc.wx | 1 << sc.wy) != 0);
            let keep_active = indexed && !timed;
            let only = if full { None } else if indexed { cand } else { Some(std::mem::take(&mut self.cols[ci].active)) };
            fs.cols_visited += 1;
            fs.recs_visited += only.as_ref().map_or(self.cols[ci].len, Vec::len) as u32;
            let v = view_of(&self.arena, self.pal_base, &self.cols[ci]);
            act.clear();
            moved |= (d.window)(v, &self.prev, u, mask, first, only.as_deref(), &mut pass!());
            if keep_active {
                // Reconsider what was visited and preserve the unvisited indices.
                let active = &mut self.cols[ci].active;
                let visited = only.as_deref().unwrap();
                active.retain(|i| visited.binary_search(i).is_err());
                if !cfg!(feature = "mutant_park_wake") {
                    active.extend_from_slice(&act);
                }
            } else {
                std::mem::swap(&mut self.cols[ci].active, &mut act);
            }
            if reindex {
                if let Some(sc) = self.scrolls[ci].as_mut() {
                    let v = view_of(&self.arena, self.pal_base, &self.cols[ci]);
                    match only.as_deref() {
                        Some(o) => o.iter().for_each(|&i| Self::reindex(sc, d, v, i, u)),
                        None => (0..self.cols[ci].len as u32).for_each(|i| Self::reindex(sc, d, v, i, u)),
                    }
                }
            }
        }
        // the layers follow their records
        let lay_on = self.layers.any();
        let mut relaid = false;
        if lay_on {
            if self.fx_nocache {
                self.layers.list.iter_mut().for_each(|l| l.fresh = true);
                relaid = true;
            }
            for &z in &regeo {
                let o = self.order[z as usize];
                let ci = (o >> IDX_BITS) as usize;
                let c = (self.cols[ci].desc.lay)(view_of(&self.arena, self.pal_base, &self.cols[ci]), o & IDX_MASK, u);
                self.layers.regeo(self.layers.lid(z), c, self.fw, self.fh);
                relaid = true;
            }
            relaid |= self.layers.repack();
            self.layers.pool.ensure(self.layers.used);
        }
        if first {
            self.lane_all = true;
        }
        self.lane_mark.resize(self.order.len(), false);
        for &z in &rebin {
            if !self.lane_mark[z as usize] {
                self.lane_mark[z as usize] = true;
                self.lane_dirty.push(z);
            }
        }
        // the certificate is a scan per event and tile of what lies above the event: when a good part of
        // the scene moved (the same line the bins draw between editing and rebuilding) what is above moved
        // too and the scan proves nothing (measured at 600 dense: a pan's turn 13.9 M without it, 18.3 M with)
        self.cert = !self.no_cert && rebin.len() * 4 <= self.order.len();
        if moved {
            let lanes = Lanes { win: &self.win, cover: &self.cover, halo: &self.halo, lay: &self.layers.of, cut: self.cut };
            // edits pay per moved record and tile; when a good part of the scene moved (a pan) one pass over
            // every record is cheaper (measured on the canvas's pan: edits alone were 12x the rebuild)
            if first || rebin.len() * 4 > self.order.len() {
                self.bins.build(&lanes, self.tx, self.ty);
            } else {
                for &z in &rebin {
                    self.bins.update(z, &lanes);
                }
                self.bins.recut_touched(&lanes);
            }
            self.bins_epoch += 1;
            fs.rebinned = true;
        }
        // the layers' stages, in paint order. An event reaches, through each tile's own list of layer
        // records, the layers above it there; a layer's repair is output damage of its record and an event
        // for the layers above that
        self.plan = layer::Plan::default();
        if broad_hit {
            self.damage.all();
            fs.broad = true;
        }
        if broad_hit && lay_on && !cfg!(feature = "mutant_broad_layers") {
            // every layer whole, said in one word: the executor has (or makes once) the jobs for it
            self.plan.full = true;
        } else if lay_on && (first || relaid || !events.is_empty()) {
            // what is due, and no more: the events reach, through each tile's own list of layer records,
            // the layers above them there. What that repair goes on to damage is found when it is done
            // (`prepare`: only what really changed) or assumed (`plan_eager`: everything it could reach).
            if first || self.fx_nocache {
                self.lay_todo.extend(0..self.layers.list.len() as u32);
            }
            self.lay_todo.extend(regeo.iter().map(|&z| self.layers.lid(z)));
            for &(w, z) in events.iter().filter(|_| !self.route_off) {
                Self::route(&self.bins, self.cert, &self.cover, &mut self.layers, &mut self.lay_todo, w, z, self.fx_nocache);
            }
        }
        if rebuild_world {
            let slots = self.world_pan.as_ref().unwrap().slots;
            let packed = self.build_world_pan(u, slots).unwrap_or_default();
            let world = self.world_pan.as_mut().unwrap();
            world.packed = packed;
            world.version = world.version.wrapping_add(1);
        }
        (fs.prefix_tiles, fs.blocks, fs.layer_px) = (self.plan.prefix_tiles, self.plan.blocks, self.plan.px);
        (self.events, self.regeo, self.act, self.rebin) = (events, regeo, act, rebin);
        self.prev = *u;
        self.first = false;
        fs
    }
    /// Route an event to the next prefix checkpoint on each tile. Eager scheduling has no repair result,
    /// so it routes to every later reader. Level-0 windows are tile-aligned and clipped to the frame.
    /// Any nonempty intersection contains the whole frame portion of the tile.
    fn route(bins: &Bins, cert: bool, cover: &[[i16; 4]], layers: &mut Layers, todo: &mut std::collections::BTreeSet<u32>, w: [i32; 4], z: u32, eager: bool) {
        let (tx, ty) = (bins.tx as i32, bins.ty as i32);
        for y in (w[1] >> 5).max(0)..((w[3] + 31) >> 5).min(ty) {
            for x in (w[0] >> 5).max(0)..((w[2] + 31) >> 5).min(tx) {
                let t = (y * tx + x) as usize;
                let readers = bins.layers(t);
                let readers = &readers[readers.partition_point(|&key| key <= z)..];
                let Some(&last) = readers.last() else { continue };
                let holds_prefix = |key: u32| {
                    let level = &layers.list[layers.of[key as usize] as usize].lv[0];
                    let r = level.r;
                    !level.empty() && r[0] < (x + 1) * 32 && r[1] < (y + 1) * 32 && r[2] > x * 32 && r[3] > y * 32
                };
                let first;
                let readers = if eager {
                    readers
                } else if let Some(&key) = readers.iter().find(|&&key| holds_prefix(key)) {
                    first = [key];
                    &first
                } else {
                    continue;
                };
                let below = if eager { last } else { readers[0] };
                let stop = if cert { bins.cover_above(t, z, w, cover, below) } else { u32::MAX };
                for &key in readers {
                    if key > stop {
                        break;
                    }
                    let lid = layers.of[key as usize];
                    layers.list[lid as usize].pending.push(t as u32);
                    todo.insert(lid);
                }
            }
        }
    }
    /// The layers' stages for this frame, assuming every repair changes everything it could reach: the
    /// plan an executor needs when it cannot look at results before scheduling (Metal's staged frame). Adds
    /// the readers' output damage. Call after `update`, before reading the damage; `prepare` then runs
    /// exactly this plan. (Without this call `prepare` schedules as it repairs and stops where a repair
    /// changed nothing.)
    pub fn plan_eager(&mut self) -> &layer::Plan {
        let mut todo = std::mem::take(&mut self.lay_todo);
        // `update` may have queued the nearest checkpoints. Replay the original events for the eager plan.
        if !self.plan.full {
            for &(w, z) in &self.events {
                Self::route(&self.bins, self.cert, &self.cover, &mut self.layers, &mut todo, w, z, true);
            }
        }
        let mut outs: Vec<[i32; 4]> = Vec::new();
        while let Some(lid) = todo.pop_first() {
            outs.clear();
            self.layers.schedule(lid, self.tx, &mut self.plan, |r| outs.push(r));
            let z = self.layers.list[lid as usize].z;
            for r in &outs {
                self.damage.add_vis([r[0] as i16, r[1] as i16, r[2] as i16, r[3] as i16], z, &self.bins.floor);
                Self::route(&self.bins, self.cert, &self.cover, &mut self.layers, &mut todo, *r, z, true);
            }
        }
        self.lay_todo = todo;
        &self.plan
    }
    /// The weight of what is due in the layers before anything is repaired (the tiles events touched, and
    /// the whole of a fresh layer): the chooser's estimate of layer work, O(due), cut short once it passes
    /// `enough` (the chooser needs to know only on which side of its threshold the sum falls).
    pub fn due_weight(&self, enough: u64) -> u64 {
        let mut w = 0;
        for &lid in &self.lay_todo {
            let l = &self.layers.list[lid as usize];
            if l.fresh {
                w += (((l.lv[0].r[2] - l.lv[0].r[0]) as u64 * (l.lv[0].r[3] - l.lv[0].r[1]) as u64) >> 10) * 8;
            }
            for &t in &l.pending {
                if w > enough {
                    return w;
                }
                w += self.tile_weight(t) as u64;
            }
        }
        w
    }
    /// The windows that changed this frame with their records' keys (the events the layers answer).
    pub fn events(&self) -> &[([i32; 4], u32)] {
        &self.events
    }
    /// Route the last `update`'s events to the layers now (for a scene with `route_off`, when the CPU is to
    /// repair this frame after all).
    pub fn route_events(&mut self) {
        let events = std::mem::take(&mut self.events);
        if self.layers.any() && !self.plan.full {
            for &(w, z) in &events {
                Self::route(&self.bins, self.cert, &self.cover, &mut self.layers, &mut self.lay_todo, w, z, self.fx_nocache);
            }
        }
        self.events = events;
    }
    /// An estimate of the layer work the last `update`'s events cause, before any routing: per tile an
    /// event touches, the tile's weight for every layer listed there above the event (plus what is due by
    /// rule). O(event tiles), cut short once it passes `enough`.
    pub fn event_weight(&self, enough: u64) -> u64 {
        let mut w = self.due_weight(enough);
        if !self.layers.any() {
            return w;
        }
        for &(r, z) in &self.events {
            for y in (r[1] >> 5).max(0)..((r[3] + 31) >> 5).min(self.ty as i32) {
                for x in (r[0] >> 5).max(0)..((r[2] + 31) >> 5).min(self.tx as i32) {
                    if w > enough {
                        return w;
                    }
                    let t = y as usize * self.tx + x as usize;
                    let ls = self.bins.layers(t);
                    let n = ls.len() - ls.partition_point(|&k| k <= z);
                    if n > 0 {
                        w += self.tile_weight(t as u32) as u64 * n as u64;
                    }
                }
            }
        }
        w
    }
    /// The last `update` left something for the layers: an event, a layer due by rule, or a broad change.
    /// (With no output damage either, the frame is idle and no executor need be called.)
    pub fn owes_layers(&self) -> bool {
        self.plan.full || !self.lay_todo.is_empty() || (self.layers.any() && !self.events.is_empty())
    }
    /// The keys whose lanes changed since the last call, or `None` for "every key".
    pub fn take_lane_dirty(&mut self) -> Option<Vec<u32>> {
        for &z in &self.lane_dirty {
            self.lane_mark[z as usize] = false;
        }
        let d = std::mem::take(&mut self.lane_dirty);
        if std::mem::take(&mut self.lane_all) { None } else { Some(d) }
    }
    /// What is due in the layers by rule (fresh storage, pixels that entered a window), taken out: for an
    /// executor that repairs them itself. Nothing is left scheduled.
    pub fn take_dues(&mut self) -> Vec<(u32, layer::Due)> {
        let todo = std::mem::take(&mut self.lay_todo);
        todo.into_iter().map(|lid| (lid, self.layers.take_due(lid))).collect()
    }
    /// `update`, then `plan_eager`: for an executor that needs the whole schedule up front.
    pub fn update_eager(&mut self, u: &U) -> FrameStats {
        let mut fs = self.update(u);
        self.plan_eager();
        (fs.prefix_tiles, fs.blocks, fs.layer_px) = (self.plan.prefix_tiles, self.plan.blocks, self.plan.px);
        fs
    }
    /// Some record still depends on the clock: the host owes the scene a frame at the output's next
    /// refresh. False: nothing moves, and no frame is owed until a block word or a record is written.
    pub fn animating(&self) -> bool {
        self.cols.iter().any(|c| !c.active.is_empty()) || self.clocks.iter().any(|&(w, _, d)| self.block.w[w] < 32_768 && self.block.w[d] != 0)
    }
    /// Declare block word `word` derived: every `update` sets it to the Q15 progress of `Time` from word
    /// `start` over word `dur` ticks (0 before the start, 32,768 from the end on, and at once when `dur` is
    /// 0). Declare it broad (`Scene::broad`) when no geometry reads it: a fade is then the frame damaged
    /// whole on every frame it runs, and no record is visited. Draw under `Scene::block`.
    pub fn clock(&mut self, word: usize, start: usize, dur: usize) {
        assert!(word < 64 && start < 64 && dur < 64 && word != U_TIME);
        self.clocks.push((word, start, dur));
        self.first = true;
    }
    /// The block the last `update` ran under (the host's, with the derived words): what the executors
    /// draw the frame under.
    pub fn block(&self) -> &U {
        &self.block
    }
    /// Table `table`'s slots from 0 (packed RGBA8 words, as a colour operand holds them). The lanes are
    /// re-mounted (a record's opaque cover may depend on a table word) and the frame is redrawn: this is a
    /// definition, not a per-frame act; choosing between tables is a block word.
    pub fn set_palette(&mut self, table: usize, colors: &[u32]) {
        assert!(table < PAL_TABLES && colors.len() <= PAL_SLOTS, "palette table {table} with {} slots", colors.len());
        let at = self.pal_base + table * PAL_SLOTS;
        self.arena.words_mut()[at..at + colors.len()].copy_from_slice(colors);
        self.first = true;
    }
    /// The palette tables (`PAL_TABLES` of `PAL_SLOTS` words).
    pub fn palettes(&self) -> &[u32] {
        &self.arena.words()[self.pal_base..self.pal_base + PAL_WORDS]
    }
    /// The records in motion (the size of the active sets).
    pub fn active_records(&self) -> usize {
        self.cols.iter().map(|c| c.active.len()).sum()
    }
    fn fx<'a>(&'a self, pool: &'a [u32]) -> Fx<'a> {
        Fx { layers: &self.layers.list, of: &self.layers.of, pool, media: self.media.words(), fw: self.fw, fh: self.fh }
    }
    /// The part of tile `ti`'s list a job runs: entries `from..to`, started from the stored prefix of layer
    /// `init` (`layer::NONE`: from nothing). `upto`: the layer whose prefix the job computes (its entries
    /// beneath that layer's record), or `layer::NONE` for an output tile (all of them). The job starts at
    /// the topmost layer record in its range: a layer record flattens the accumulator over the whole tiles
    /// of its halo, which are the tiles it is listed in, so its stored prefix is the accumulator there.
    /// Depth therefore costs a job the records between two readers, not everything beneath. The last
    /// value: some layer record is in the range.
    pub fn seg(&self, ti: u32, upto: u32) -> (usize, usize, u32, bool) {
        let (ox, oy) = ((ti as usize % self.tx) as i32 * 32, (ti as usize / self.tx) as i32 * 32);
        let (cols, nrows) = ((self.fw as usize - ox as usize).min(32), (self.fh as usize - oy as usize).min(32));
        let list = self.bins.tile(ti as usize);
        let (mut from, mut to, mut init, mut marked) = (0, list.len(), layer::NONE, false);
        if self.layers.any() {
            if upto != layer::NONE {
                let zb = self.layers.list[upto as usize].z;
                to = list.partition_point(|&z| z < zb);
            }
            for j in (0..to).rev() {
                let lid = self.layers.lid(list[j]);
                if lid == layer::NONE {
                    continue;
                }
                marked = true;
                // the record is listed here, so the tile is one of its halo's whole tiles: it flattened every
                // pixel of it and its level 0 holds them
                let h = self.layers.list[lid as usize].lv[0].r;
                if !self.fx_nocache && h[0] <= ox && h[1] <= oy && h[2] >= ox + cols as i32 && h[3] >= oy + nrows as i32 {
                    (from, init) = (j, lid);
                    break;
                }
            }
        }
        (from, to, init, marked)
    }
    /// The tile program of tile `ti` into `t`'s accumulator: its entries beneath layer `upto`'s record (a
    /// prefix job), or all of them (`layer::NONE`: an output tile). It starts from the stored prefix of
    /// the topmost layer beneath whose record's window holds the whole tile, when there is one.
    fn run_tile(&self, u: &U, ti: u32, t: &mut Tile, pool: &[u32], upto: u32) {
        let (ox, oy) = ((ti as usize % self.tx) as i32 * 32, (ti as usize / self.tx) as i32 * 32);
        let (cols, nrows) = ((self.fw as usize - ox as usize).min(32), (self.fh as usize - oy as usize).min(32));
        if let Some(world) = self.world_frame() {
            debug_assert_eq!(upto, layer::NONE);
            let mut list = std::mem::take(&mut t.world_candidates);
            world.merged(ti as usize, self.tx, u, &mut list);
            t.begin(ox, oy, cols, nrows, false);
            t.cover_masks(&list, &[], false);
            t.prepare();
            let fx = self.fx(pool);
            self.runs(&list, |c, first, zs| (self.cols[c].desc.shade)(self.view(c), zs, first, &self.order, u, t, &fx));
            t.st.entries += list.len() as u64;
            t.world_candidates = list;
            return;
        }
        let (from, to, init, marked) = self.seg(ti, upto);
        let list = &self.bins.tile(ti as usize)[from..to];
        let flat = self.flat && !marked && upto == layer::NONE;
        t.begin(ox, oy, cols, nrows, flat);
        if flat {
            self.runs(list, |c, _, zs| (self.cols[c].desc.flat)(self.view(c), zs, &self.order, u, t));
        }
        t.cover_masks(list, &self.cover, self.cut && list.len() > 1);
        t.prepare();
        if init != layer::NONE {
            t.load(pool, &self.layers.list[init as usize].lv[0]);
        }
        let fx = self.fx(pool);
        let prof = *PROF_ON.get_or_init(|| std::env::var("BOUGH_PAINT_PROF").is_ok());
        if prof {
            self.runs(list, |c, first, zs| {
                let t0 = std::time::Instant::now();
                (self.cols[c].desc.shade)(self.view(c), zs, first, &self.order, u, t, &fx);
                let mut p = PROF.lock().unwrap();
                p[c].0 += t0.elapsed().as_nanos() as u64;
                p[c].1 += zs.len() as u64;
            });
        } else {
            self.runs(list, |c, first, zs| (self.cols[c].desc.shade)(self.view(c), zs, first, &self.order, u, t, &fx));
        }
        t.st.entries += list.len() as u64;
    }
    /// Repair what is due, layer by layer in paint order, and let damage stop where a repair changed
    /// nothing: a prefix tile whose every stored word came out as it was dirties no pyramid block, a block
    /// left byte-identical dirties nothing above it, and only the blocks a reader's levels really changed
    /// become output damage and events for the layers above. Exact (a comparison of words, no tolerance).
    /// Pixels that newly entered a window, and a fresh layer, count as changed whatever they hold.
    fn repair(&mut self, u: &U) -> Stats {
        let mut st = Stats::default();
        if self.lay_todo.is_empty() {
            return st;
        }
        let mut pool = std::mem::take(&mut self.layers.pool);
        let mut todo = std::mem::take(&mut self.lay_todo);
        let mut outs: Vec<[i32; 4]> = Vec::new();
        let prune = |same: bool, bx: i32| same || (cfg!(feature = "mutant_prune") && bx & 1 == 0);
        while let Some(lid) = todo.pop_first() {
            let due = self.layers.take_due(lid);
            outs.clear();
            {
                let s = &*self;
                let l = &s.layers.list[lid as usize];
                if l.lv[0].empty() {
                    continue;
                }
                let pw = pool.words_mut();
                let top = l.top as usize;
                let mut t = s.worker();
                // level 0: the tiles due; those that entered, or everything when fresh, are changed by rule
                let (mut d, mut forced) = (layer::Blocks::new(l.lv[0].r, 0), layer::Blocks::new(l.lv[0].r, 0));
                if due.fresh {
                    forced.all();
                } else {
                    due.entered[0].iter().for_each(|&r| forced.add_px(r));
                    if !cfg!(feature = "mutant_layer_event") {
                        due.pending.iter().for_each(|&ti| d.set((ti as usize % s.tx) as i32, (ti as usize / s.tx) as i32));
                    }
                }
                forced.each(|bx, by| d.set(bx, by));
                // what changed is kept as pixel boxes (of the words that really differ), not blocks: a block
                // is a tile at every level, so block-grain dilation grows a change by a tile ring per level
                // where its true footprint grows by a pixel or two of that level
                let ne = |r: &[i32; 4]| r[0] < r[2] && r[1] < r[3];
                let mut changed: Vec<[i32; 4]> = Vec::new();
                let mut tiles: Vec<(i32, i32)> = Vec::new();
                d.each(|bx, by| tiles.push((bx, by)));
                for &(bx, by) in &tiles {
                    let ti = by as u32 * s.tx as u32 + bx as u32;
                    s.run_tile(u, ti, &mut t, pw, lid);
                    let bb = t.store_layer(pw, &l.lv[0]);
                    if forced.has(bx, by) {
                        changed.push(layer::cut([bx << 5, by << 5, (bx + 1) << 5, (by + 1) << 5], l.lv[0].r));
                    } else if let Some(bb) = bb.filter(|_| !prune(false, bx)) {
                        changed.push(bb);
                    }
                }
                // Later tile programs restart at this checkpoint. The backdrop also composites its
                // input accumulator, including in the halo outside the ink window where it flattens.
                if !self.fx_nocache && !cfg!(feature = "mutant_prefix_lost") {
                    outs.extend_from_slice(&changed);
                }
                let mut read_changed: Vec<(u32, Vec<[i32; 4]>)> = Vec::new();
                if l.lvl == 0 {
                    read_changed.push((0, changed.clone()));
                }
                let mut aff: Vec<([i32; 4], bool)> = Vec::new();
                for k in 1..=top {
                    // the pixels a changed pixel beneath reaches, and what entered (changed by rule)
                    let r = l.lv[k].r;
                    aff.clear();
                    aff.extend(changed.iter().map(|c| (layer::cut(layer::up_px(*c), r), false)).filter(|a| ne(&a.0)));
                    if due.fresh {
                        aff.push((r, true));
                    } else {
                        aff.extend(due.entered[k].iter().map(|e| (layer::cut(*e, r), true)).filter(|a| ne(&a.0)));
                    }
                    let mut d = layer::Blocks::new(r, k as u32);
                    aff.iter().for_each(|a| d.add_px(a.0));
                    let mut blocks: Vec<(i32, i32)> = Vec::new();
                    d.each(|bx, by| blocks.push((bx, by)));
                    let bs = layer::bshift(k as u32);
                    changed.clear();
                    for &(bx, by) in &blocks {
                        let b = [bx << bs, by << bs, (bx + 1) << bs, (by + 1) << bs];
                        let (mut clip, mut fclip) = ([i32::MAX, i32::MAX, i32::MIN, i32::MIN], [i32::MAX, i32::MAX, i32::MIN, i32::MIN]);
                        let join = |a: &mut [i32; 4], c: [i32; 4]| *a = [a[0].min(c[0]), a[1].min(c[1]), a[2].max(c[2]), a[3].max(c[3])];
                        for (a, f) in &aff {
                            let c = layer::cut(*a, b);
                            if ne(&c) {
                                join(&mut clip, c);
                                if *f {
                                    join(&mut fclip, c);
                                }
                            }
                        }
                        t.st.blk[k] += 1;
                        let bb = layer::down_rect(pw, l, k as u32, s.fw, s.fh, clip);
                        t.st.blk_same[k] += bb.is_none() as u64;
                        if let Some(bb) = bb.filter(|_| !prune(false, bx)) {
                            join(&mut fclip, bb);
                        }
                        if ne(&fclip) {
                            changed.push(fclip);
                        }
                    }
                    if k as u32 == l.lvl || k == top {
                        read_changed.push((k as u32, changed.clone()));
                    }
                }
                for (k, rs) in &read_changed {
                    outs.extend(rs.iter().map(|r| layer::reach_px(l, *k, *r)).filter(ne));
                }
                st.add(&t.st);
            }
            let z = self.layers.list[lid as usize].z;
            for r in &outs {
                self.damage.add_vis([r[0] as i16, r[1] as i16, r[2] as i16, r[3] as i16], z, &self.bins.floor);
                Self::route(&self.bins, self.cert, &self.cover, &mut self.layers, &mut todo, *r, z, self.fx_nocache);
            }
        }
        self.layers.pool = pool;
        self.lay_todo = todo;
        st
    }
    /// Run the layers' stages of the frame `update` scheduled, on this thread: per layer in paint order, its
    /// prefix tiles, then its pyramid's dirty blocks level by level. Nothing to do (and nothing done) when
    /// no layer was reached by a change. Call between `update` and `shade`.
    pub fn prepare(&mut self, u: &U) -> Stats {
        if self.plan.full {
            self.plan = self.layers.full_plan(self.tx);
            self.lay_todo.clear();
        }
        if self.plan.work.is_empty() {
            return self.repair(u);
        }
        let mut pool = std::mem::take(&mut self.layers.pool);
        let pool_words = pool.words_mut();
        let st = {
            let s = &*self;
            let mut t = s.worker();
            for w in &s.plan.work {
                let l = &s.layers.list[w.lid as usize];
                for &ti in &w.prefix {
                    s.run_tile(u, ti, &mut t, pool_words, w.lid);
                    t.store_layer(pool_words, &l.lv[0]);
                }
                for (k, blocks) in &w.blocks {
                    for &(bx, by) in blocks {
                        t.st.blk[*k as usize] += 1;
                        t.st.blk_same[*k as usize] += layer::down(pool_words, l, *k, s.fw, s.fh, bx, by) as u64;
                    }
                }
            }
            t.st
        };
        self.layers.pool = pool;
        st
    }

    /// One worker's scratch: the 32 x 32 Q15 accumulator and the tile's occlusion masks. Keep one per core.
    pub fn worker(&self) -> Tile<'_> {
        Tile::new(&self.atlas, self.aw, self.tabs())
    }
    /// What a tile costs to shade, for cutting bands of equal work and for choosing an executor: its
    /// entries weighted by their archetype's cost class.
    pub fn tile_weight(&self, t: u32) -> u32 {
        self.bins.tile(t as usize).iter().map(|&z| self.cols[(self.order[z as usize] >> IDX_BITS) as usize].desc.cost.weight()).sum()
    }
    /// Shade `tiles` into the surface at `out` with this worker's scratch: the band interface. Reads the
    /// columns, the window lane, the bins and the tables; writes only the pixels of the tiles given.
    ///
    /// # Safety
    /// `out` is valid for `(fh - 1) * stride + fw` words, and no other thread reads or writes the pixels of
    /// these tiles during the call (tiles given to concurrent callers are disjoint).
    pub unsafe fn shade_into(&self, u: &U, tiles: &[u32], out: *mut u32, stride: usize, t: &mut Tile) {
        for &ti in tiles {
            self.run_tile(u, ti, t, self.layers.pool.words(), layer::NONE);
            let (ox, oy) = ((ti as usize % self.tx) * 32, (ti as usize / self.tx) * 32);
            let at = oy * stride + ox;
            // SAFETY: the tile's rows [oy, oy + nrows) x columns [ox, ox + cols) lie inside `out` (the
            // caller's contract), and nothing else touches them during the call.
            unsafe { t.store(out.add(at), stride) };
        }
    }
    /// Shade `tiles` (indices, any order, no duplicates) into `out` on the caller plus `extra` pool workers.
    pub fn shade(&self, u: &U, tiles: &[u32], out: &mut [u32], stride: usize, pool: &Pool, extra: usize) -> Stats {
        // admission, once per call: rows do not overlap, the surface holds the frame, and the tiles are a
        // set (bounded, no duplicates), which is the whole proof that workers write disjoint pixels
        assert!(stride >= self.fw as usize, "stride {stride} is less than the frame's width");
        assert!(out.len() >= (self.fh as usize - 1) * stride + self.fw as usize);
        let nt = self.tx * self.ty;
        let mut seen = vec![0u64; nt.div_ceil(64)];
        for &t in tiles {
            let t = t as usize;
            assert!(t < nt, "tile {t} of {nt}");
            assert!(seen[t / 64] >> (t % 64) & 1 == 0, "tile {t} is listed twice");
            seen[t / 64] |= 1 << (t % 64);
        }
        let optr = OutPtr(out.as_mut_ptr());
        let next = AtomicUsize::new(0);
        const CHUNK: usize = 8;
        let total = std::sync::Mutex::new(Stats::default());
        let work = |_id: usize| {
            let optr = &optr;
            let mut t = self.worker();
            loop {
                let a = next.fetch_add(CHUNK, Relaxed);
                if a >= tiles.len() {
                    break;
                }
                // SAFETY: `out` is exclusively borrowed for this call and covers the frame (asserted
                // above); each chunk of tiles is claimed by one worker (the atomic counter) and the tiles
                // are distinct.
                unsafe { self.shade_into(u, &tiles[a..(a + CHUNK).min(tiles.len())], optr.0, stride, &mut t) };
            }
            total.lock().unwrap().add(&t.st);
        };
        let extra = extra.min(tiles.len() / (2 * CHUNK));
        pool.run(extra, &work);
        total.into_inner().unwrap()
    }
    /// Maximal runs of one column in a tile's list.
    #[inline(always)]
    fn runs(&self, list: &[u32], mut f: impl FnMut(usize, usize, &[u32])) {
        let mut a = 0;
        while a < list.len() {
            let c = self.order[list[a] as usize] >> IDX_BITS;
            let mut b = a + 1;
            while b < list.len() && self.order[list[b] as usize] >> IDX_BITS == c {
                b += 1;
            }
            f(c as usize, a, &list[a..b]);
            a = b;
        }
    }

    /// A whole CPU frame: update, then shade what the update damaged. `out` holds the previous frame.
    pub fn frame(&mut self, u: &U, out: &mut [u32], stride: usize, pool: &Pool, extra: usize) -> FrameStats {
        let mut fs = self.update(u);
        let u = &self.block.clone();
        let st = self.prepare(u);
        let mut tiles = std::mem::take(&mut self.tiles);
        self.damage.tiles(&mut tiles);
        self.damage.clear();
        fs.tiles = tiles.len() as u32;
        fs.raster = self.shade(u, &tiles, out, stride, pool, extra);
        fs.raster.add(&st);
        self.tiles = tiles;
        fs
    }
    /// The tiles the last `frame` shaded.
    pub fn last_tiles(&self) -> &[u32] {
        &self.tiles
    }
    /// Forget what the surface shows: the next frame redraws everything.
    pub fn invalidate(&mut self) {
        self.first = true;
    }

    /// The scene under `u` as the generic kernel's commands, in paint order: the oracle's input, and the
    /// baseline's (what round 7 restaged on the CPU whenever state changed).
    pub fn stage(&self, u: &U) -> bough_kernel::stage::Scene {
        let mut s = bough_kernel::stage::Scene::default();
        for &o in &self.order {
            let c = (o >> IDX_BITS) as usize;
            (self.cols[c].desc.stage)(self.view(c), o & IDX_MASK, u, &mut s);
        }
        s
    }
    /// The archetypes' specification, pixel by pixel over every record (small frames only). A scene with
    /// layers is folded record by record over the whole frame instead, each backdrop record reading a plain
    /// mip chain of the frame beneath it: no tiles, no layer store, no damage (the retained path's oracle).
    pub fn spec(&self, u: &U, out: &mut [u32], stride: usize) {
        let none = |_: Cmd, _: i32, _: i32| Vec4 { x: 0, y: 0, z: 0, w: 0 };
        let media = self.media.words();
        if !self.layers.any() {
            for y in 0..self.fh as i32 {
                for x in 0..self.fw as i32 {
                    let mut acc = Vec4 { x: 0, y: 0, z: 0, w: 0 };
                    for &o in &self.order {
                        let c = (o >> IDX_BITS) as usize;
                        acc = (self.cols[c].desc.spec)(self.view(c), o & IDX_MASK, u, x, y, acc, &self.atlas, self.aw, self.tabs(), &none, media).0;
                    }
                    out[y as usize * stride + x as usize] = crate::ops::ops::x_store(acc);
                }
            }
            return;
        }
        use crate::ops::ops as x;
        let (fw, fh) = (self.fw as i32, self.fh as i32);
        let mut acc = vec![Vec4 { x: 0u32, y: 0, z: 0, w: 0 }; (fw * fh) as usize];
        for (z, &o) in self.order.iter().enumerate() {
            let c = (o >> IDX_BITS) as usize;
            let d = self.cols[c].desc;
            // the mip chain of what is beneath a backdrop record: level l + 1 from level l, whole frame
            let mut lev: Vec<(i32, i32, Vec<u32>)> = Vec::new();
            if d.layer && self.layers.lid(z as u32) != layer::NONE {
                lev.push((fw, fh, acc.iter().map(|a| x::x_store(*a)).collect()));
                for k in 1..layer::LEVELS as u32 {
                    let (sw, sh, src) = &lev[k as usize - 1];
                    let (dw, dh) = (x::bd_dim(self.fw, k), x::bd_dim(self.fh, k));
                    let mut dst = vec![0u32; (dw * dh) as usize];
                    for yy in 0..dh {
                        for xx in 0..dw {
                            let row = |j: i32| {
                                let sy = (2 * yy - 1 + j).clamp(0, sh - 1);
                                let t = |i: i32| src[(sy * sw + (2 * xx - 1 + i).clamp(0, sw - 1)) as usize];
                                x::dn_row(t(0), t(1), t(2), t(3))
                            };
                            dst[(yy * dw + xx) as usize] = x::dn_fin(row(0), row(1), row(2), row(3));
                        }
                    }
                    lev.push((dw, dh, dst));
                }
            }
            let read = |k: u32, px: i32, py: i32| {
                let (dw, dh, v) = &lev[k as usize];
                let (i, j) = (x::bd_i(px, k), x::bd_i(py, k));
                let t = |a: i32, b: i32| v[(b.clamp(0, dh - 1) * dw + a.clamp(0, dw - 1)) as usize];
                x::bd_bil(t(i, j), t(i + 1, j), t(i, j + 1), t(i + 1, j + 1), x::bd_f(px, k), x::bd_f(py, k))
            };
            let bd = |cm: Cmd, px: i32, py: i32| {
                let a = read(cm.rad.y as u32, px, py);
                if cm.rad.z == 0 { a } else { bough_kernel::k::mix4u(a, read(cm.rad.y as u32 + 1, px, py), cm.rad.z as u32) }
            };
            for y in 0..fh {
                for xx in 0..fw {
                    let a = &mut acc[(y * fw + xx) as usize];
                    *a = (d.spec)(self.view(c), o & IDX_MASK, u, xx, y, *a, &self.atlas, self.aw, self.tabs(), if lev.is_empty() { &none } else { &bd }, media).0;
                }
            }
        }
        for y in 0..fh {
            for xx in 0..fw {
                out[y as usize * stride + xx as usize] = x::x_store(acc[(y * fw + xx) as usize]);
            }
        }
    }
    /// The pick: the id of the topmost record that inks pixel (x, y), from the tile's bin (front to back).
    /// `NO_HIT` when nothing inks it or the topmost record carries no id.
    pub fn pick(&self, u: &U, x: i32, y: i32) -> u32 {
        if x < 0 || y < 0 || x >= self.fw as i32 || y >= self.fh as i32 {
            return NO_HIT;
        }
        let t = (y as usize >> 5) * self.tx + (x as usize >> 5);
        let mut candidates = Vec::new();
        let keys = if let Some(world) = self.world_frame() {
            world.merged(t, self.tx, u, &mut candidates);
            &candidates[..]
        } else {
            self.bins.tile(t)
        };
        for &z in keys.iter().rev() {
            let o = self.order[z as usize];
            let c = (o >> IDX_BITS) as usize;
            let d = self.cols[c].desc;
            let (_, ink) = (d.spec)(self.view(c), o & IDX_MASK, u, x, y, Vec4 { x: 0, y: 0, z: 0, w: 0 }, &self.atlas, self.aw, self.tabs(), &|_, _, _| Vec4 { x: 0, y: 0, z: 0, w: 0 }, self.media.words());
            if ink {
                return if d.hit { (d.pick)(self.view(c), o & IDX_MASK, u) } else { NO_HIT };
            }
        }
        NO_HIT
    }
}
