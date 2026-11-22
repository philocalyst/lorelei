//! The flat paint bank: styles and effects as data.
//!
//! `paint.rs` composes a record's layers as a type (`Over<Shadow<..>, Border<..>>`): the right form for a
//! program whose archetypes are known when it is compiled, and the wrong one for a style that is a run-time
//! value (three shadows today, an inset and a conic gradient tomorrow). Here nothing an author touches has
//! a type parameter:
//!
//! * a **node** is a rect, a [`DrawCmd`] (one effect: a fill, a border, a shadow, an inset shadow, an
//!   N-stop gradient, a backdrop, an image, a retained group), an opacity, a clip and a small transform;
//! * a styled box is a run of nodes in paint order (shadows, then the fill, then the inset shadows, then
//!   the border): any number of each, decided when it is described;
//! * the bank keeps the nodes as parallel arrays and lowers each one onto **one of a closed set of
//!   archetypes** that exist once in the binary ([`EffectKind`] x [`Space`]), whatever the application
//!   draws. Every operand of those archetypes is a record word, so a change of any property is one record
//!   rewritten (`Scene::set`): damage where it was and where it is, and nothing else.
//!
//! Underneath it is the same scene as everything else: the tile rasteriser and its occlusion cut, the
//! retained shadow coverage patches, the layers' toroidal pyramids for backdrops, the declared pan, and
//! the Metal executor reading the same arena. Stacking is by records, so the cut between a box and its
//! own shadows is the bins' (an opaque fill hides the shadow beneath it at pixel grain) rather than a
//! type's.
//!
//! Groups ([`FlatScene::group`]) are child banks painted into retained surfaces (`layer::Surface`): group
//! opacity, element blur, rounded clips of a subtree and pages in transition are all one record over one
//! surface, repaired at the grain of the child's own damage.
use crate::expr::*;
use crate::layer::Surface;
use crate::ops::ops as x;
use crate::paint::*;
use crate::scene::{ColId, Desc, FrameStats, LANES, Scene};
use bough_kernel::cpu::Pool;

/// The block words of the pan (Q8) that [`Space::Panned`] nodes move by.
pub const PAN_X: usize = 0;
pub const PAN_Y: usize = 1;

/// What a node draws: the archetype it is lowered onto.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum EffectKind {
    Solid = 0,
    RRect = 1,
    Border = 2,
    Shadow = 3,
    Inset = 4,
    Ramp = 5,
    Glass = 6,
    Image = 7,
    Group = 8,
    Glyph = 9,
}
pub const KINDS: usize = 10;
impl EffectKind {
    pub const ALL: [EffectKind; KINDS] = [EffectKind::Solid, EffectKind::RRect, EffectKind::Border, EffectKind::Shadow, EffectKind::Inset, EffectKind::Ramp, EffectKind::Glass, EffectKind::Image, EffectKind::Group, EffectKind::Glyph];
}

/// Whether a node stays put or moves with the pan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Space {
    #[default]
    Fixed = 0,
    /// The node's box is translated by block words [`PAN_X`], [`PAN_Y`]: a scroll is two block words, the
    /// scene's pan index answers it, and no record is rewritten.
    Panned = 1,
}

/// A colour table's handle (`FlatScene::lut`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lut {
    pub handle: u32,
    pub opaque: bool,
}

/// Where a gradient's parameter runs. Fractions are of the node's box; lengths are pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RampGeo {
    /// CSS `linear-gradient(<angle>)`: 0 is to the top, clockwise; the line spans the box's corners.
    Linear { angle_deg: f32 },
    /// An ellipse about `center` with radii `radius` (px), from the `focal` point (an offset from the
    /// centre in radii, inside the unit circle) outwards.
    Radial { center: [f32; 2], radius: [f32; 2], focal: [f32; 2] },
    /// CSS `conic-gradient(from <start> at <center>)`: clockwise from 12 o'clock.
    Conic { center: [f32; 2], start_deg: f32 },
}

/// A group's handle (`FlatScene::group`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GroupId(pub u32);
/// A node's handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

/// One effect. Colours are straight RGBA8 with red in the low byte (`0xAABBGGRR`); lengths are pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DrawCmd {
    /// A box with hard edges (no antialiasing).
    Solid { color: u32 },
    RRect { radius: f32, color: u32 },
    /// A rounded box of colour `fill` with a border of `width` in `color` inside its edge.
    Border { radius: f32, width: f32, color: u32, fill: u32 },
    /// A drop shadow of the node's box (CSS `box-shadow`: `blur` is the blur radius, two sigma).
    Shadow { radius: f32, spread: f32, dx: f32, dy: f32, blur: f32, color: u32 },
    /// An inset shadow (CSS `box-shadow: inset`), cut to the node's rounded box.
    Inset { radius: f32, spread: f32, dx: f32, dy: f32, blur: f32, color: u32 },
    /// An N-stop gradient: its stops are a table (`FlatScene::lut`).
    Ramp { radius: f32, geo: RampGeo, lut: Lut },
    /// What is beneath the box, blurred by `blur` (a standard deviation) and filtered (1.0 each: none),
    /// under a translucent `tint`.
    Glass { radius: f32, blur: f32, brightness: f32, saturation: f32, contrast: f32, tint: u32 },
    /// A published image or video frame of the scene's media pool (`dims` = width | height << 16).
    Image { radius: f32, slot: u32, dims: u32 },
    /// A retained group, blurred by `blur` (a standard deviation in the parent's pixels; 0: sharp).
    Group { radius: f32, blur: f32, group: GroupId },
    /// One glyph: the node's rect is its texels' box, `texel` its first texel in the scene's atlas.
    Glyph { texel: u32, color: u32 },
}
impl DrawCmd {
    pub fn kind(&self) -> EffectKind {
        match self {
            DrawCmd::Solid { .. } => EffectKind::Solid,
            DrawCmd::RRect { .. } => EffectKind::RRect,
            DrawCmd::Border { .. } => EffectKind::Border,
            DrawCmd::Shadow { .. } => EffectKind::Shadow,
            DrawCmd::Inset { .. } => EffectKind::Inset,
            DrawCmd::Ramp { .. } => EffectKind::Ramp,
            DrawCmd::Glass { .. } => EffectKind::Glass,
            DrawCmd::Image { .. } => EffectKind::Image,
            DrawCmd::Group { .. } => EffectKind::Group,
            DrawCmd::Glyph { .. } => EffectKind::Glyph,
        }
    }
}

/// A node as it is described.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Node {
    /// x0, y0, x1, y1 in pixels.
    pub rect: [f32; 4],
    pub cmd: DrawCmd,
    pub opacity: f32,
    pub clip: Option<[f32; 4]>,
    pub space: Space,
    pub visible: bool,
}
impl Node {
    pub fn new(rect: [f32; 4], cmd: DrawCmd) -> Node {
        Node { rect, cmd, opacity: 1.0, clip: None, space: Space::Fixed, visible: true }
    }
    pub fn opacity(mut self, o: f32) -> Node {
        self.opacity = o;
        self
    }
    pub fn clip(mut self, k: [f32; 4]) -> Node {
        self.clip = Some(k);
        self
    }
    pub fn panned(mut self) -> Node {
        self.space = Space::Panned;
        self
    }
    pub fn hidden(mut self) -> Node {
        self.visible = false;
        self
    }
}

/// A scalar property of a node, for animation (`FlatScene::set_prop`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Prop {
    X0 = 0,
    Y0 = 1,
    X1 = 2,
    Y1 = 3,
    /// The transform: a translation (px) and a scale about the box's centre, applied to the box and to
    /// every length of its effect.
    Dx = 4,
    Dy = 5,
    Scale = 6,
    Opacity = 7,
    Radius = 8,
    Blur = 9,
    Spread = 10,
    /// A shadow's offset.
    OffX = 11,
    OffY = 12,
    /// A border's width.
    Width = 13,
    /// The four channels of the node's main colour, 0 ..= 255.
    Red = 14,
    Green = 15,
    Blue = 16,
    Alpha = 17,
}
impl Prop {
    pub const ALL: [Prop; 18] = [Prop::X0, Prop::Y0, Prop::X1, Prop::Y1, Prop::Dx, Prop::Dy, Prop::Scale, Prop::Opacity, Prop::Radius, Prop::Blur, Prop::Spread, Prop::OffX, Prop::OffY, Prop::Width, Prop::Red, Prop::Green, Prop::Blue, Prop::Alpha];
    /// A (node, property) pair as one word: an animation track's tag (`motion::AnimBank`).
    pub fn tag(self, node: NodeId) -> u64 {
        (node.0 as u64) << 8 | self as u64
    }
    pub fn untag(tag: u64) -> (NodeId, Prop) {
        (NodeId((tag >> 8) as u32), Prop::ALL[(tag & 255) as usize % Prop::ALL.len()])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlatError {
    /// The column of this kind and space is full (`FlatConfig`).
    Full(EffectKind, Space),
    /// The scene's table region or media pool has no room.
    NoRoom,
    /// The group's surface is larger than 2047 px a side.
    TooLarge,
}

/// Records each archetype can hold, per space.
#[derive(Clone, Copy, Debug)]
pub struct FlatConfig {
    pub caps: [[usize; KINDS]; 2],
    /// Words of the media pool (images, video frames, the surfaces of this scene's groups).
    pub media_words: usize,
}
impl Default for FlatConfig {
    fn default() -> Self {
        FlatConfig { caps: [[256, 256, 256, 256, 64, 64, 16, 64, 16, 0], [0; KINDS]], media_words: 0 }
    }
}
impl FlatConfig {
    /// No column at all: add the ones the scene needs with [`FlatConfig::with`].
    pub fn empty() -> Self {
        FlatConfig { caps: [[0; KINDS]; 2], media_words: 0 }
    }
    pub fn with(mut self, kind: EffectKind, space: Space, cap: usize) -> Self {
        self.caps[space as usize][kind as usize] = cap;
        self
    }
    /// Room for the surfaces of groups (`surface_words`) and published images.
    pub fn media(mut self, words: usize) -> Self {
        self.media_words = words;
        self
    }
}
/// Words of the media pool a `w` x `h` group surface with pyramid levels up to `top` takes.
pub fn surface_words(w: u32, h: u32, top: u32) -> usize {
    crate::media::HDR + x::mp_off(w as i32, h as i32, top + 1) as usize
}

// ---- the closed set of archetypes ---------------------------------------------------------------------

type PanB = Off<Lane, I<Uw<PAN_X>>, I<Uw<PAN_Y>>>;
type C = Pm<Rec>;
type L = I<Rec>;
pub type ASolid<B> = Clip<Solid<B, C>, RecRect>;
pub type ARRect<B> = Clip<RRect<B, L, C>, RecRect>;
pub type ABorder<B> = Clip<Border<B, L, L, C, C>, RecRect>;
pub type AShadow<B> = Clip<Shadow<Off<Grow<B, L>, L, L>, L, L, C, Tab>, RecRect>;
pub type AInset<B> = Clip<Inset<B, L, L, L, L, L, C>, RecRect>;
pub type ARamp<B> = Clip<Ramp<B, L, Rec, RecRect, RecRect, Rec, Rec, C>, RecRect>;
pub type AGlass<B> = Clip<Over<Backdrop<B, L, L, V3<Rec, Rec, Rec>>, RRect<B, L, C>>, RecRect>;
pub type AImage<B> = Clip<Image<B, L, Rec, Rec, Rec>, RecRect>;
pub type AGroup<B> = Clip<Mip<B, L, Rec, Rec, Rec, L>, RecRect>;
pub type AGlyph<B> = Clip<Glyph<B, Rec, C>, RecRect>;

/// A node resolved to integers: what its record's words are made from.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Low {
    Solid { c: u32 },
    RRect { r: i32, c: u32 },
    Border { r: i32, w: i32, c: u32, fill: u32 },
    Shadow { r: i32, spread: i32, dx: i32, dy: i32, blur: i32, c: u32 },
    Inset { r: i32, spread: i32, dx: i32, dy: i32, blur: i32, c: u32 },
    Ramp { r: i32, kind: u32, p: [i32; 4], q: [i32; 4], lut: u32, op: u32, cov: u32 },
    Glass { r: i32, blur: i32, f: [u32; 3], tint: u32 },
    Image { r: i32, slot: u32, dims: u32, op: u32 },
    Group { r: i32, slot: u32, dims: u32, op: u32, blur: i32 },
    Glyph { texel: u32, c: u32 },
}

fn pm(c: u32) -> C {
    Pm { c: Rec(c) }
}
fn len(v: i32) -> L {
    I { a: Rec(v as u32) }
}
fn put<P: Paint>(s: &mut Scene, col: Option<ColId<P>>, slot: Option<u32>, rect: [i32; 4], p: P) -> (u32, u32) {
    let col = col.expect("the node's column exists: it was admitted");
    match slot {
        Some(i) => (s.set(col, i as usize, rect, p), i),
        None => {
            let z = s.push(col, rect, p);
            (z, s.cols()[col.0].len as u32 - 1)
        }
    }
}

macro_rules! flat_cols {
    ($name:ident, $B:ty, $mk:expr, $scroll:literal) => {
        struct $name {
            solid: Option<ColId<ASolid<$B>>>,
            rrect: Option<ColId<ARRect<$B>>>,
            border: Option<ColId<ABorder<$B>>>,
            shadow: Option<ColId<AShadow<$B>>>,
            inset: Option<ColId<AInset<$B>>>,
            ramp: Option<ColId<ARamp<$B>>>,
            glass: Option<ColId<AGlass<$B>>>,
            image: Option<ColId<AImage<$B>>>,
            group: Option<ColId<AGroup<$B>>>,
            glyph: Option<ColId<AGlyph<$B>>>,
        }
        impl $name {
            fn descs() -> [&'static Desc; KINDS] {
                [Desc::of::<ASolid<$B>>(), Desc::of::<ARRect<$B>>(), Desc::of::<ABorder<$B>>(), Desc::of::<AShadow<$B>>(), Desc::of::<AInset<$B>>(), Desc::of::<ARamp<$B>>(), Desc::of::<AGlass<$B>>(), Desc::of::<AImage<$B>>(), Desc::of::<AGroup<$B>>(), Desc::of::<AGlyph<$B>>()]
            }
            fn new(s: &mut Scene, caps: &[usize; KINDS]) -> Self {
                macro_rules! col {
                    ($k:expr, $T:ty) => {{
                        if caps[$k as usize] == 0 {
                            None
                        } else {
                            let c = s.column::<$T>(caps[$k as usize]);
                            if $scroll {
                                s.scroll(c, PAN_X, PAN_Y);
                            }
                            Some(c)
                        }
                    }};
                }
                $name {
                    solid: col!(EffectKind::Solid, ASolid<$B>),
                    rrect: col!(EffectKind::RRect, ARRect<$B>),
                    border: col!(EffectKind::Border, ABorder<$B>),
                    shadow: col!(EffectKind::Shadow, AShadow<$B>),
                    inset: col!(EffectKind::Inset, AInset<$B>),
                    ramp: col!(EffectKind::Ramp, ARamp<$B>),
                    glass: col!(EffectKind::Glass, AGlass<$B>),
                    image: col!(EffectKind::Image, AImage<$B>),
                    group: col!(EffectKind::Group, AGroup<$B>),
                    glyph: col!(EffectKind::Glyph, AGlyph<$B>),
                }
            }
            fn col_index(&self, kind: EffectKind) -> Option<usize> {
                match kind {
                    EffectKind::Solid => self.solid.map(|c| c.0),
                    EffectKind::RRect => self.rrect.map(|c| c.0),
                    EffectKind::Border => self.border.map(|c| c.0),
                    EffectKind::Shadow => self.shadow.map(|c| c.0),
                    EffectKind::Inset => self.inset.map(|c| c.0),
                    EffectKind::Ramp => self.ramp.map(|c| c.0),
                    EffectKind::Glass => self.glass.map(|c| c.0),
                    EffectKind::Image => self.image.map(|c| c.0),
                    EffectKind::Group => self.group.map(|c| c.0),
                    EffectKind::Glyph => self.glyph.map(|c| c.0),
                }
            }
            /// Write a node's record (a new one when `slot` is `None`): its key and its index in its column.
            fn write(&self, s: &mut Scene, slot: Option<u32>, rect: [i32; 4], clip: [i32; 4], l: &Low) -> (u32, u32) {
                let (b, k): ($B, RecRect) = ($mk, rec_rect(clip));
                match *l {
                    Low::Solid { c } => put(s, self.solid, slot, rect, Clip { p: Solid { b, c: pm(c) }, k }),
                    Low::RRect { r, c } => put(s, self.rrect, slot, rect, Clip { p: RRect { b, r: len(r), c: pm(c) }, k }),
                    Low::Border { r, w, c, fill } => put(s, self.border, slot, rect, Clip { p: Border { b, r: len(r), w: len(w), c: pm(c), fill: pm(fill) }, k }),
                    Low::Shadow { r, spread, dx, dy, blur, c } => put(s, self.shadow, slot, rect, Clip { p: Shadow { b: Off { b: Grow { b, d: len(spread) }, dx: len(dx), dy: len(dy) }, r: len(r), blur: len(blur), c: pm(c), tab: Tab::NONE }, k }),
                    Low::Inset { r, spread, dx, dy, blur, c } => put(s, self.inset, slot, rect, Clip { p: Inset { b, r: len(r), spread: len(spread), dx: len(dx), dy: len(dy), blur: len(blur), c: pm(c) }, k }),
                    Low::Ramp { r, kind, p, q, lut, op, cov } => put(s, self.ramp, slot, rect, Clip { p: Ramp { b, r: len(r), kind: Rec(kind), p: rec_rect(p), q: rec_rect(q), lut: Rec(lut), op: Rec(op), cov: pm(cov) }, k }),
                    Low::Glass { r, blur, f, tint } => put(s, self.glass, slot, rect, Clip { p: Over(Backdrop { b, r: len(r), blur: len(blur), f: V3 { a: Rec(f[0]), b: Rec(f[1]), c: Rec(f[2]) } }, RRect { b, r: len(r), c: pm(tint) }), k }),
                    Low::Image { r, slot: sl, dims, op } => put(s, self.image, slot, rect, Clip { p: Video { b, r: len(r), slot: Rec(sl), dims: Rec(dims), op: Rec(op) }, k }),
                    Low::Group { r, slot: sl, dims, op, blur } => put(s, self.group, slot, rect, Clip { p: Mip { b, r: len(r), slot: Rec(sl), dims: Rec(dims), op: Rec(op), blur: len(blur) }, k }),
                    Low::Glyph { texel, c } => put(s, self.glyph, slot, rect, Clip { p: Glyph { b, texel: Rec(texel), c: pm(c) }, k }),
                }
            }
        }
    };
}
flat_cols!(FixCols, Lane, Lane, false);
flat_cols!(PanCols, PanB, Off { b: Lane, dx: I { a: Uw }, dy: I { a: Uw } }, true);

/// The archetypes of the flat bank, fixed then panned, in [`EffectKind`] order: the whole set of paint
/// types a flat scene can ever instantiate.
pub fn archetypes() -> [[&'static Desc; KINDS]; 2] {
    [FixCols::descs(), PanCols::descs()]
}

struct Group {
    flat: Box<FlatScene>,
    surf: Surface,
    /// The nodes that show it.
    nodes: Vec<u32>,
}

/// What a `prepare` did.
#[derive(Clone, Copy, Debug, Default)]
pub struct FlatStats {
    /// Records rewritten (nodes lowered), in this scene and its groups.
    pub lowered: u32,
    /// Groups asked, and of them those whose surface changed; the tiles they repainted and the pyramid
    /// blocks they recomputed.
    pub groups: u32,
    pub groups_changed: u32,
    pub group_tiles: u32,
    pub group_blocks: u32,
    /// Records the groups' own scenes visited.
    pub group_recs: u32,
    /// The scene was rebuilt (a structural change).
    pub rebuilt: bool,
    /// The CPU frame, when one was drawn (`FlatScene::frame`).
    pub frame: FrameStats,
}

/// A scene of flat nodes. See the module's description.
pub struct FlatScene {
    scene: Scene,
    fix: FixCols,
    pan: PanCols,
    caps: [[usize; KINDS]; 2],
    // ---- the bank: one entry per node, parallel ----
    cmd: Vec<DrawCmd>,
    rect: Vec<[f32; 4]>,
    xf: Vec<[f32; 3]>,
    opacity: Vec<f32>,
    clip: Vec<[i32; 4]>,
    space: Vec<Space>,
    visible: Vec<bool>,
    live: Vec<bool>,
    /// The node's index in its column (`u32::MAX`: not mounted) and its paint-order key.
    slot: Vec<u32>,
    key: Vec<u32>,
    /// The rect its record holds (Q8), for mapping a group's damage.
    shown: Vec<[i32; 4]>,
    /// What the node's record holds (rect, clip, effect, and whether it took tables): a lowering that would
    /// write the same words writes nothing.
    last: Vec<Option<([i32; 4], [i32; 4], Low, bool)>>,
    /// A shadow's geometry key at its last lowering, and the lowering's serial.
    geo: Vec<[i32; 6]>,
    stamp: Vec<u64>,
    dirty_mark: Vec<bool>,
    dirty: Vec<u32>,
    /// Shadows lowered while their geometry was changing: without tables, until they hold still.
    unsettled: Vec<u32>,
    /// Paint order (node ids).
    order: Vec<u32>,
    groups: Vec<Option<Group>>,
    changed: Vec<[i32; 4]>,
    u: U,
    serial: u64,
    rebuild_due: bool,
    /// Something was written since the last `prepare` (or is still owed): an idle scene is not asked.
    pending: bool,
    /// Bumped when records were mounted or the scene was rebuilt: an executor bound to the scene re-syncs.
    pub mount_epoch: u64,
}

fn q8(v: f32) -> i32 {
    (v as f64 * 256.0).round().clamp(-268_000_000.0, 268_000_000.0) as i32
}
fn q8pos(v: f32) -> i32 {
    q8(v).max(0)
}
fn q15(v: f32) -> u32 {
    (v.clamp(0.0, 1.0) as f64 * 32768.0).round() as u32
}
/// A straight colour under an opacity.
pub fn fade(c: u32, op: f32) -> u32 {
    let a = ((c >> 24) as f32 * op.clamp(0.0, 1.0)).round() as u32;
    c & 0x00ff_ffff | a << 24
}
const NO: u32 = u32::MAX;
const NOCLIP: [i32; 4] = [NO_CLIP.x, NO_CLIP.y, NO_CLIP.z, NO_CLIP.w];

impl FlatScene {
    /// A scene of `fw` x `fh` pixels with the columns `cfg` asks for.
    pub fn new(fw: u32, fh: u32, cfg: FlatConfig) -> FlatScene {
        let descs = archetypes();
        let mut words = 0;
        for sp in 0..2 {
            for k in 0..KINDS {
                if cfg.caps[sp][k] > 0 {
                    words += cfg.caps[sp][k].div_ceil(64) * 64 * (LANES + descs[sp][k].words);
                }
            }
        }
        let mut scene = Scene::new(fw, fh, words.max(64));
        scene.media = crate::media::Pool::new(cfg.media_words);
        let fix = FixCols::new(&mut scene, &cfg.caps[0]);
        let pan = PanCols::new(&mut scene, &cfg.caps[1]);
        if cfg.caps[1].iter().any(|&c| c > 0) {
            scene.uniform_pan(PAN_X, PAN_Y);
        }
        FlatScene {
            scene,
            fix,
            pan,
            caps: cfg.caps,
            cmd: Vec::new(),
            rect: Vec::new(),
            xf: Vec::new(),
            opacity: Vec::new(),
            clip: Vec::new(),
            space: Vec::new(),
            visible: Vec::new(),
            live: Vec::new(),
            slot: Vec::new(),
            key: Vec::new(),
            shown: Vec::new(),
            last: Vec::new(),
            geo: Vec::new(),
            stamp: Vec::new(),
            dirty_mark: Vec::new(),
            dirty: Vec::new(),
            unsettled: Vec::new(),
            order: Vec::new(),
            groups: Vec::new(),
            changed: Vec::new(),
            u: U::default(),
            serial: 0,
            rebuild_due: false,
            pending: true,
            mount_epoch: 0,
        }
    }
    pub fn size(&self) -> (u32, u32) {
        (self.scene.fw, self.scene.fh)
    }
    /// The scene underneath (for an executor, the oracle, the pick).
    pub fn scene(&self) -> &Scene {
        &self.scene
    }
    pub fn scene_mut(&mut self) -> &mut Scene {
        self.pending = true;
        &mut self.scene
    }
    /// The block the scene is drawn under (the pan words; the rest is the caller's).
    pub fn block(&self) -> &U {
        &self.u
    }
    pub fn block_mut(&mut self) -> &mut U {
        self.pending = true;
        &mut self.u
    }
    pub fn nodes(&self) -> usize {
        self.cmd.len()
    }
    /// Records the column of `kind` in `space` can hold, and holds.
    pub fn capacity(&self, kind: EffectKind, space: Space) -> (usize, usize) {
        let col = match space {
            Space::Fixed => self.fix.col_index(kind),
            Space::Panned => self.pan.col_index(kind),
        };
        (self.caps[space as usize][kind as usize], col.map_or(0, |c| self.scene.cols()[c].len))
    }
    /// The table for a gradient's stops (position 0..1, straight RGBA8): one per distinct ramp, shared.
    pub fn lut(&mut self, stops: &[(f32, u32)]) -> Result<Lut, FlatError> {
        let (handle, opaque) = self.scene.lut(stops).ok_or(FlatError::NoRoom)?;
        Ok(Lut { handle, opaque })
    }
    /// Scroll the panned nodes to (x, y) pixels.
    pub fn set_pan(&mut self, px: f32, py: f32) {
        (self.u.w[PAN_X], self.u.w[PAN_Y]) = (q8(px) as u32, q8(py) as u32);
        self.pending = true;
    }
    /// Something was written since the last `prepare`, here or in a group: `prepare` has work to look for.
    pub fn pending(&self) -> bool {
        self.pending
    }

    fn admit(&self, kind: EffectKind, space: Space) -> Result<(), FlatError> {
        let col = match space {
            Space::Fixed => self.fix.col_index(kind),
            Space::Panned => self.pan.col_index(kind),
        };
        match col {
            Some(c) if self.scene.cols()[c].len < self.scene.cols()[c].cap => Ok(()),
            _ => Err(FlatError::Full(kind, space)),
        }
    }
    /// Append a node on top of everything. (A mount: the next frame is drawn whole. For content that comes
    /// and goes, mount hidden nodes once and show them: `Node::hidden`, `FlatScene::show`.)
    pub fn push(&mut self, n: Node) -> Result<NodeId, FlatError> {
        self.admit(n.cmd.kind(), n.space)?;
        let id = self.cmd.len() as u32;
        self.cmd.push(n.cmd);
        self.rect.push(n.rect);
        self.xf.push([0.0, 0.0, 1.0]);
        self.opacity.push(n.opacity);
        self.clip.push(n.clip.map_or(NOCLIP, |k| [q8(k[0]), q8(k[1]), q8(k[2]), q8(k[3])]));
        self.space.push(n.space);
        self.visible.push(n.visible);
        self.live.push(true);
        self.slot.push(NO);
        self.key.push(NO);
        self.shown.push([0; 4]);
        self.last.push(None);
        self.geo.push([i32::MIN; 6]);
        self.stamp.push(0);
        self.dirty_mark.push(false);
        self.order.push(id);
        if let DrawCmd::Group { group, .. } = n.cmd {
            if let Some(g) = self.groups.get_mut(group.0 as usize).and_then(Option::as_mut) {
                g.nodes.push(id);
            }
        }
        // mount now, so that paint order is push order
        self.pending = true;
        self.serial += 1;
        self.lower(id, true);
        self.mount_epoch += 1;
        Ok(NodeId(id))
    }
    /// Insert a node beneath `before`. Structural: the scene is rebuilt at the next `prepare`.
    pub fn insert_before(&mut self, before: NodeId, n: Node) -> Result<NodeId, FlatError> {
        let id = self.push(n)?;
        self.order.pop();
        let at = self.order.iter().position(|&o| o == before.0).unwrap_or(self.order.len());
        self.order.insert(at, id.0);
        self.rebuild_due = true;
        self.pending = true;
        Ok(id)
    }
    /// Remove a node: it stops drawing at once (a record rewrite); its record is given back when the scene
    /// is next rebuilt (`FlatScene::compact`).
    pub fn remove(&mut self, id: NodeId) {
        let i = id.0 as usize;
        if !self.live[i] {
            return;
        }
        self.live[i] = false;
        self.visible[i] = false;
        if let DrawCmd::Group { group, .. } = self.cmd[i] {
            if let Some(g) = self.groups.get_mut(group.0 as usize).and_then(Option::as_mut) {
                g.nodes.retain(|&n| n != id.0);
            }
        }
        self.mark(id.0);
    }
    /// Rebuild the scene from the live nodes in paint order, at the next `prepare`: removed nodes' records
    /// are given back.
    pub fn compact(&mut self) {
        self.rebuild_due = true;
        self.pending = true;
    }

    #[inline]
    fn mark(&mut self, id: u32) {
        self.pending = true;
        if !self.dirty_mark[id as usize] {
            self.dirty_mark[id as usize] = true;
            self.dirty.push(id);
        }
    }
    pub fn node(&self, id: NodeId) -> Node {
        let i = id.0 as usize;
        Node { rect: self.rect[i], cmd: self.cmd[i], opacity: self.opacity[i], clip: (self.clip[i] != NOCLIP).then(|| self.clip[i].map(|v| v as f32 / 256.0)), space: self.space[i], visible: self.visible[i] }
    }
    /// Replace a node's effect by another of the same kind.
    pub fn set(&mut self, id: NodeId, cmd: DrawCmd) {
        let i = id.0 as usize;
        assert_eq!(cmd.kind(), self.cmd[i].kind(), "a node keeps its kind: remove it and insert another");
        if self.cmd[i] != cmd {
            if let (DrawCmd::Group { group: old, .. }, DrawCmd::Group { group: new, .. }) = (self.cmd[i], cmd) {
                if old != new {
                    if let Some(g) = self.groups.get_mut(old.0 as usize).and_then(Option::as_mut) {
                        g.nodes.retain(|&n| n != id.0);
                    }
                    if let Some(g) = self.groups.get_mut(new.0 as usize).and_then(Option::as_mut) {
                        g.nodes.push(id.0);
                    }
                }
            }
            self.cmd[i] = cmd;
            self.mark(id.0);
        }
    }
    pub fn set_rect(&mut self, id: NodeId, rect: [f32; 4]) {
        if self.rect[id.0 as usize] != rect {
            self.rect[id.0 as usize] = rect;
            self.mark(id.0);
        }
    }
    pub fn set_opacity(&mut self, id: NodeId, o: f32) {
        if self.opacity[id.0 as usize] != o {
            self.opacity[id.0 as usize] = o;
            self.mark(id.0);
        }
    }
    /// The node's transform: a translation (px) and a scale about its box's centre.
    pub fn set_xf(&mut self, id: NodeId, dx: f32, dy: f32, scale: f32) {
        if self.xf[id.0 as usize] != [dx, dy, scale] {
            self.xf[id.0 as usize] = [dx, dy, scale];
            self.mark(id.0);
        }
    }
    pub fn set_clip(&mut self, id: NodeId, clip: Option<[f32; 4]>) {
        let k = clip.map_or(NOCLIP, |k| [q8(k[0]), q8(k[1]), q8(k[2]), q8(k[3])]);
        if self.clip[id.0 as usize] != k {
            self.clip[id.0 as usize] = k;
            self.mark(id.0);
        }
    }
    pub fn show(&mut self, id: NodeId, visible: bool) {
        if self.live[id.0 as usize] && self.visible[id.0 as usize] != visible {
            self.visible[id.0 as usize] = visible;
            self.mark(id.0);
        }
    }
    /// The node's main colour (a ramp, an image and a group have none).
    pub fn set_color(&mut self, id: NodeId, c: u32) {
        let mut cmd = self.cmd[id.0 as usize];
        match &mut cmd {
            DrawCmd::Solid { color } | DrawCmd::RRect { color, .. } | DrawCmd::Border { color, .. } | DrawCmd::Shadow { color, .. } | DrawCmd::Inset { color, .. } | DrawCmd::Glyph { color, .. } => *color = c,
            DrawCmd::Glass { tint, .. } => *tint = c,
            DrawCmd::Ramp { .. } | DrawCmd::Image { .. } | DrawCmd::Group { .. } => {}
        }
        self.set(id, cmd);
    }
    pub fn color(&self, id: NodeId) -> u32 {
        match self.cmd[id.0 as usize] {
            DrawCmd::Solid { color } | DrawCmd::RRect { color, .. } | DrawCmd::Border { color, .. } | DrawCmd::Shadow { color, .. } | DrawCmd::Inset { color, .. } | DrawCmd::Glyph { color, .. } => color,
            DrawCmd::Glass { tint, .. } => tint,
            DrawCmd::Ramp { .. } | DrawCmd::Image { .. } | DrawCmd::Group { .. } => 0,
        }
    }
    /// One scalar property (what an animation track writes). A property the node's kind does not have is
    /// ignored.
    pub fn set_prop(&mut self, id: NodeId, p: Prop, v: f32) {
        let i = id.0 as usize;
        let before = (self.rect[i], self.xf[i], self.opacity[i], self.cmd[i]);
        match p {
            Prop::X0 | Prop::Y0 | Prop::X1 | Prop::Y1 => self.rect[i][p as usize] = v,
            Prop::Dx => self.xf[i][0] = v,
            Prop::Dy => self.xf[i][1] = v,
            Prop::Scale => self.xf[i][2] = v,
            Prop::Opacity => self.opacity[i] = v,
            Prop::Radius => match &mut self.cmd[i] {
                DrawCmd::RRect { radius, .. } | DrawCmd::Border { radius, .. } | DrawCmd::Shadow { radius, .. } | DrawCmd::Inset { radius, .. } | DrawCmd::Ramp { radius, .. } | DrawCmd::Glass { radius, .. } | DrawCmd::Image { radius, .. } | DrawCmd::Group { radius, .. } => *radius = v,
                _ => {}
            },
            Prop::Blur => match &mut self.cmd[i] {
                DrawCmd::Shadow { blur, .. } | DrawCmd::Inset { blur, .. } | DrawCmd::Glass { blur, .. } | DrawCmd::Group { blur, .. } => *blur = v,
                _ => {}
            },
            Prop::Spread => match &mut self.cmd[i] {
                DrawCmd::Shadow { spread, .. } | DrawCmd::Inset { spread, .. } => *spread = v,
                _ => {}
            },
            Prop::OffX => match &mut self.cmd[i] {
                DrawCmd::Shadow { dx, .. } | DrawCmd::Inset { dx, .. } => *dx = v,
                _ => {}
            },
            Prop::OffY => match &mut self.cmd[i] {
                DrawCmd::Shadow { dy, .. } | DrawCmd::Inset { dy, .. } => *dy = v,
                _ => {}
            },
            Prop::Width => {
                if let DrawCmd::Border { width, .. } = &mut self.cmd[i] {
                    *width = v;
                }
            }
            Prop::Red | Prop::Green | Prop::Blue | Prop::Alpha => {
                let sh = 8 * (p as u32 - Prop::Red as u32);
                let c = self.color(id) & !(255 << sh) | (v.round().clamp(0.0, 255.0) as u32) << sh;
                let cmd = &mut self.cmd[i];
                match cmd {
                    DrawCmd::Solid { color } | DrawCmd::RRect { color, .. } | DrawCmd::Border { color, .. } | DrawCmd::Shadow { color, .. } | DrawCmd::Inset { color, .. } | DrawCmd::Glyph { color, .. } => *color = c,
                    DrawCmd::Glass { tint, .. } => *tint = c,
                    _ => {}
                }
            }
        }
        if before != (self.rect[i], self.xf[i], self.opacity[i], self.cmd[i]) {
            self.mark(id.0);
        }
    }
    pub fn prop(&self, id: NodeId, p: Prop) -> f32 {
        let i = id.0 as usize;
        match p {
            Prop::X0 | Prop::Y0 | Prop::X1 | Prop::Y1 => self.rect[i][p as usize],
            Prop::Dx => self.xf[i][0],
            Prop::Dy => self.xf[i][1],
            Prop::Scale => self.xf[i][2],
            Prop::Opacity => self.opacity[i],
            Prop::Radius => match self.cmd[i] {
                DrawCmd::RRect { radius, .. } | DrawCmd::Border { radius, .. } | DrawCmd::Shadow { radius, .. } | DrawCmd::Inset { radius, .. } | DrawCmd::Ramp { radius, .. } | DrawCmd::Glass { radius, .. } | DrawCmd::Image { radius, .. } | DrawCmd::Group { radius, .. } => radius,
                _ => 0.0,
            },
            Prop::Blur => match self.cmd[i] {
                DrawCmd::Shadow { blur, .. } | DrawCmd::Inset { blur, .. } | DrawCmd::Glass { blur, .. } | DrawCmd::Group { blur, .. } => blur,
                _ => 0.0,
            },
            Prop::Spread => match self.cmd[i] {
                DrawCmd::Shadow { spread, .. } | DrawCmd::Inset { spread, .. } => spread,
                _ => 0.0,
            },
            Prop::OffX => match self.cmd[i] {
                DrawCmd::Shadow { dx, .. } | DrawCmd::Inset { dx, .. } => dx,
                _ => 0.0,
            },
            Prop::OffY => match self.cmd[i] {
                DrawCmd::Shadow { dy, .. } | DrawCmd::Inset { dy, .. } => dy,
                _ => 0.0,
            },
            Prop::Width => match self.cmd[i] {
                DrawCmd::Border { width, .. } => width,
                _ => 0.0,
            },
            Prop::Red | Prop::Green | Prop::Blue | Prop::Alpha => ((self.color(id) >> (8 * (p as u32 - Prop::Red as u32))) & 255) as f32,
        }
    }
    /// Apply a tagged value (`Prop::tag`): the sink of `motion::AnimBank::tick`.
    #[inline]
    pub fn apply(&mut self, tag: u64, v: f32) {
        let (id, p) = Prop::untag(tag);
        if (id.0 as usize) < self.cmd.len() {
            self.set_prop(id, p, v);
        }
    }

    // ---- groups ----

    /// Make `child` a group of this scene: it is painted into a retained surface of this scene's media
    /// pool with pyramid levels up to `top` (0 for a group that is never blurred, `ops::BD_LMAX` for any
    /// blur). Show it with a [`DrawCmd::Group`] node (or several).
    pub fn group(&mut self, child: FlatScene, top: u32) -> Result<GroupId, FlatError> {
        let (w, h) = child.size();
        if w > 2047 || h > 2047 {
            return Err(FlatError::TooLarge);
        }
        let surf = Surface::new(w, h, &mut self.scene.media, top.min(x::BD_LMAX)).ok_or(FlatError::NoRoom)?;
        let g = Group { flat: Box::new(child), surf, nodes: Vec::new() };
        self.pending = true;
        let id = match self.groups.iter().position(Option::is_none) {
            Some(i) => {
                self.groups[i] = Some(g);
                i
            }
            None => {
                self.groups.push(Some(g));
                self.groups.len() - 1
            }
        };
        Ok(GroupId(id as u32))
    }
    /// The group's own scene: describe and change its content here.
    pub fn child(&self, g: GroupId) -> &FlatScene {
        &self.groups[g.0 as usize].as_ref().expect("a live group").flat
    }
    pub fn child_mut(&mut self, g: GroupId) -> &mut FlatScene {
        self.pending = true;
        &mut self.groups[g.0 as usize].as_mut().expect("a live group").flat
    }
    /// The content revision of a group's surface (bumped by every repair that repainted a pixel).
    pub fn group_revision(&self, g: GroupId) -> u64 {
        self.groups[g.0 as usize].as_ref().expect("a live group").surf.revision
    }
    /// Drop a group no node shows any more; its surface is reused after the next frame.
    pub fn drop_group(&mut self, g: GroupId) -> Option<FlatScene> {
        self.pending = true;
        let grp = self.groups.get_mut(g.0 as usize)?.take()?;
        assert!(grp.nodes.is_empty(), "the group is still shown");
        grp.surf.release(&mut self.scene.media);
        Some(*grp.flat)
    }

    // ---- lowering ----

    /// The node's rect under its transform, and the transform's scale.
    fn resolved(&self, i: usize) -> ([f32; 4], f32) {
        let (r, [dx, dy, s]) = (self.rect[i], self.xf[i]);
        if s == 1.0 {
            return ([r[0] + dx, r[1] + dy, r[2] + dx, r[3] + dy], 1.0);
        }
        let (cx, cy) = ((r[0] + r[2]) * 0.5, (r[1] + r[3]) * 0.5);
        ([cx + (r[0] - cx) * s + dx, cy + (r[1] - cy) * s + dy, cx + (r[2] - cx) * s + dx, cy + (r[3] - cy) * s + dy], s)
    }
    fn low(&self, i: usize) -> ([i32; 4], Low) {
        let (r, s) = self.resolved(i);
        let op = self.opacity[i];
        let rect = [q8(r[0]), q8(r[1]), q8(r[2]), q8(r[3])];
        let (w, h) = ((r[2] - r[0]).max(0.0), (r[3] - r[1]).max(0.0));
        let low = match self.cmd[i] {
            DrawCmd::Solid { color } => Low::Solid { c: fade(color, op) },
            DrawCmd::RRect { radius, color } => Low::RRect { r: q8pos(radius * s), c: fade(color, op) },
            DrawCmd::Border { radius, width, color, fill } => Low::Border { r: q8pos(radius * s), w: q8pos(width * s), c: fade(color, op), fill: fade(fill, op) },
            DrawCmd::Shadow { radius, spread, dx, dy, blur, color } => {
                // CSS: a rounded box's shadow is rounded by its radius plus the spread; a square box's is square
                let rr = if radius > 0.0 { (radius + spread).max(0.0) } else { 0.0 };
                Low::Shadow { r: q8pos(rr * s), spread: q8(spread * s), dx: q8(dx * s), dy: q8(dy * s), blur: q8pos(blur * s), c: fade(color, op) }
            }
            DrawCmd::Inset { radius, spread, dx, dy, blur, color } => Low::Inset { r: q8pos(radius * s), spread: q8(spread * s), dx: q8(dx * s), dy: q8(dy * s), blur: q8pos(blur * s), c: fade(color, op) },
            DrawCmd::Ramp { radius, geo, lut } => {
                let opq = q15(op);
                let (kind, p, q) = ramp_geo(geo, w, h, s);
                Low::Ramp { r: q8pos(radius * s), kind, p, q, lut: lut.handle, op: opq, cov: if lut.opaque && opq == 32768 { 0xff00_0000 } else { 0 } }
            }
            DrawCmd::Glass { radius, blur, brightness, saturation, contrast, tint } => {
                let f = |v: f32| (v.clamp(0.0, 16.0) * 256.0).round() as u32;
                Low::Glass { r: q8pos(radius * s), blur: q8pos(blur * s), f: [f(brightness), f(saturation), f(contrast)], tint: fade(tint, op) }
            }
            DrawCmd::Image { radius, slot, dims } => Low::Image { r: q8pos(radius * s), slot, dims, op: q15(op) },
            DrawCmd::Group { radius, blur, group } => match self.groups.get(group.0 as usize).and_then(Option::as_ref) {
                Some(g) => {
                    // the blur is given in this scene's pixels; the surface is read in its own texels
                    let per = if w > 0.0 { g.surf.size().0 as f32 / w } else { 1.0 };
                    Low::Group { r: q8pos(radius * s), slot: g.surf.handle(), dims: g.surf.dimensions(), op: q15(op), blur: q8pos(blur * per) }
                }
                None => Low::Solid { c: 0 },
            },
            DrawCmd::Glyph { texel, color } => Low::Glyph { texel, c: fade(color, op) },
        };
        (rect, low)
    }
    /// Write node `id`'s record. `settled`: it may take tables (a shadow whose geometry is not changing).
    /// Returns whether a record was written (a change too small to alter a word writes nothing).
    fn lower(&mut self, id: u32, settled: bool) -> bool {
        let i = id as usize;
        let (mut rect, low) = self.low(i);
        let mut clip = self.clip[i];
        // admission (`Scene::push`): a box further than the world's reach, or wider than the window lane, is
        // not drawn rather than refused (an animation may overshoot anything)
        let lim = crate::scene::MAX_PX * 256 + 255;
        let fits = rect.iter().all(|v| v.abs() <= crate::scene::MAX_WORLD_PX * 256) && (rect[2] as i64 - rect[0] as i64).abs() <= lim as i64 && (rect[3] as i64 - rect[1] as i64).abs() <= lim as i64;
        if !self.visible[i] || !fits {
            // nothing: an empty box under an empty clip stages an empty window for every kind
            (rect, clip) = ([0; 4], [0; 4]);
        }
        let slot = (self.slot[i] != NO).then_some(self.slot[i]);
        let kind = self.cmd[i].kind();
        let low = if kind == EffectKind::Group && !matches!(low, Low::Group { .. }) { Low::Group { r: 0, slot: 0, dims: 1 | 1 << 16, op: 0, blur: 0 } } else { low };
        // a shadow keeps its tables only while its shape holds still: one whose size, phase, radius or blur
        // is being animated would otherwise mint a table per frame
        let mut tables = true;
        if let (Low::Shadow { r, spread, dx, dy, blur, .. }, true) = (low, self.visible[i] && fits) {
            let key = [rect[2] - rect[0], rect[3] - rect[1], (rect[0] - spread + dx) & 255, (rect[1] - spread + dy) & 255, r, blur ^ spread.rotate_left(16)];
            if slot.is_some() && key != self.geo[i] && !settled {
                tables = false;
                if !self.unsettled.contains(&id) {
                    self.unsettled.push(id);
                }
            }
            self.geo[i] = key;
        }
        let now = Some((rect, clip, low, tables));
        if slot.is_some() && self.last[i] == now {
            self.stamp[i] = self.serial;
            return false;
        }
        self.last[i] = now;
        let (was_tab, was_cov) = (self.scene.memoise, self.scene.memoise_coverage);
        if !tables {
            self.scene.memoise = false;
        }
        let (z, sl) = match self.space[i] {
            Space::Fixed => self.fix.write(&mut self.scene, slot, rect, clip, &low),
            Space::Panned => self.pan.write(&mut self.scene, slot, rect, clip, &low),
        };
        (self.scene.memoise, self.scene.memoise_coverage) = (was_tab, was_cov);
        (self.key[i], self.slot[i], self.shown[i], self.stamp[i]) = (z, sl, rect, self.serial);
        true
    }
    /// Lower every node that changed since the last flush. Returns the records rewritten.
    pub fn flush(&mut self) -> u32 {
        self.serial += 1;
        let mut n = 0;
        let mut dirty = std::mem::take(&mut self.dirty);
        for &id in &dirty {
            self.dirty_mark[id as usize] = false;
            n += self.lower(id, false) as u32;
        }
        dirty.clear();
        self.dirty = dirty;
        // shadows that held still for a frame take their tables back (one more rewrite, the same pixels)
        if !self.unsettled.is_empty() {
            let mut un = std::mem::take(&mut self.unsettled);
            un.retain(|&id| {
                if self.stamp[id as usize] == self.serial {
                    return true;
                }
                n += self.lower(id, true) as u32;
                false
            });
            self.unsettled = un;
        }
        n
    }
    fn rebuild(&mut self) {
        self.scene.clear_records();
        self.order.retain(|&id| self.live[id as usize]);
        self.serial += 1;
        self.unsettled.clear();
        for k in 0..self.order.len() {
            let id = self.order[k];
            self.slot[id as usize] = NO;
            self.last[id as usize] = None;
            self.geo[id as usize] = [i32::MIN; 6];
            self.lower(id, true);
        }
        for id in std::mem::take(&mut self.dirty) {
            self.dirty_mark[id as usize] = false;
        }
        self.rebuild_due = false;
        self.mount_epoch += 1;
    }
    /// Shadows whose tables are withheld because their shape is changing.
    pub fn unsettled(&self) -> usize {
        self.unsettled.len()
    }
    /// Something is still owed a frame although nothing was written: a shadow waiting for its tables.
    pub fn owes_frame(&self) -> bool {
        !self.unsettled.is_empty() || !self.dirty.is_empty() || self.rebuild_due || self.groups.iter().flatten().any(|g| g.flat.owes_frame())
    }

    /// Bring the scene to what was described: repair the groups' surfaces (each at the grain of its own
    /// damage, damaging this scene's records where their surfaces changed and nowhere else), then rewrite
    /// the records of the nodes that changed. After this the scene underneath is ready for an executor
    /// (`Scene::update` and so on); `FlatScene::frame` is this and a CPU frame.
    pub fn prepare(&mut self, workers: &Pool, extra: usize) -> FlatStats {
        let mut st = FlatStats::default();
        if !self.pending {
            return st;
        }
        if self.rebuild_due {
            self.rebuild();
            st.rebuilt = true;
        }
        for gi in 0..self.groups.len() {
            let Some(g) = self.groups[gi].as_mut() else { continue };
            // a group nothing was written in, whose surface has been painted, is not asked anything
            if !g.flat.pending && g.surf.revision != 0 {
                continue;
            }
            let cst = g.flat.prepare(workers, extra);
            self.changed.clear();
            let rf = g.surf.repair(&mut g.flat.scene, &g.flat.u, &mut self.scene.media, workers, extra, &mut self.changed);
            st.groups += 1 + cst.groups;
            st.lowered += cst.lowered;
            st.groups_changed += (rf.tiles != 0) as u32 + cst.groups_changed;
            st.group_tiles += rf.tiles + cst.group_tiles;
            st.group_blocks += rf.blocks + cst.group_blocks;
            st.group_recs += rf.child.recs_visited + cst.group_recs;
            if self.changed.is_empty() {
                continue;
            }
            let (sw, sh) = g.surf.size();
            for &n in &g.nodes {
                let i = n as usize;
                if !self.visible[i] || self.dirty_mark[i] || self.key[i] == NO {
                    continue;
                }
                let r = self.shown[i];
                let (ox, oy) = (r[0] as f64 / 256.0, r[1] as f64 / 256.0);
                let (kx, ky) = ((r[2] - r[0]) as f64 / 256.0 / sw as f64, (r[3] - r[1]) as f64 / 256.0 / sh as f64);
                let reach = match self.cmd[i] {
                    DrawCmd::Group { blur, .. } => {
                        let w = (r[2] - r[0]) as f32 / 256.0;
                        g.surf.reach(q8pos(blur * if w > 0.0 { sw as f32 / w } else { 1.0 })) as f64
                    }
                    _ => 1.0,
                };
                let (px, py) = if self.space[i] == Space::Panned { (self.u.w[PAN_X] as i32 as f64 / 256.0, self.u.w[PAN_Y] as i32 as f64 / 256.0) } else { (0.0, 0.0) };
                for c in &self.changed {
                    let rect = [(ox + px + (c[0] as f64 - reach) * kx).floor() as i32 - 1, (oy + py + (c[1] as f64 - reach) * ky).floor() as i32 - 1, (ox + px + (c[2] as f64 + reach) * kx).ceil() as i32 + 1, (oy + py + (c[3] as f64 + reach) * ky).ceil() as i32 + 1];
                    self.scene.damage_part(self.key[i], rect);
                }
            }
        }
        st.lowered += self.flush();
        self.pending = !self.unsettled.is_empty() || self.groups.iter().flatten().any(|g| g.flat.pending);
        st
    }
    /// `prepare`, then a CPU frame into `out` (which holds the previous frame).
    pub fn frame(&mut self, out: &mut [u32], stride: usize, workers: &Pool, extra: usize) -> FlatStats {
        let mut st = self.prepare(workers, extra);
        st.frame = self.scene.frame(&self.u, out, stride, workers, extra);
        self.scene.media.reclaim();
        st
    }
    /// The whole frame by the specification (`Scene::spec`: no tiles, no bins, no damage), after `prepare`.
    pub fn spec(&mut self, out: &mut [u32], stride: usize) {
        self.scene.spec(&self.u, out, stride);
    }
}

/// A gradient's geometry in a `w` x `h` px box as the words of `ops::mk_ramp`.
fn ramp_geo(geo: RampGeo, w: f32, h: f32, s: f32) -> (u32, [i32; 4], [i32; 4]) {
    const LIM: f64 = 8_388_607.0;
    match geo {
        RampGeo::Linear { angle_deg } => {
            let th = (angle_deg as f64).to_radians();
            let (dx, dy) = (th.sin(), -th.cos());
            let l = (w as f64 * dx.abs() + h as f64 * dy.abs()).max(1.0);
            let m = |d: f64| (d / l * 8_388_608.0).round().clamp(-LIM, LIM) as i32;
            (x::RAMP_LINEAR, [0, 0, m(dx), m(dy)], [0; 4])
        }
        RampGeo::Radial { center, radius, focal } => {
            let inv = |r: f32| (268_435_456.0 / ((r * s).max(0.25) as f64 * 256.0)).round().clamp(1.0, LIM) as i32;
            let (mut gx, mut gy) = (focal[0] as f64, focal[1] as f64);
            let n = (gx * gx + gy * gy).sqrt();
            if n > 0.97 {
                (gx, gy) = (gx * 0.97 / n, gy * 0.97 / n);
            }
            let (gx, gy) = ((gx * 4096.0).round() as i32, (gy * 4096.0).round() as i32);
            let a = (4096 - (gx * gx + gy * gy) / 4096).clamp(1, 4096);
            (x::RAMP_RADIAL, [q8((center[0] - 0.5) * w), q8((center[1] - 0.5) * h), inv(radius[0]), inv(radius[1])], [gx, gy, (268_435_456.0 / a as f64).round().clamp(1.0, LIM) as i32, a])
        }
        RampGeo::Conic { center, start_deg } => {
            let turn = ((start_deg as f64 / 360.0).rem_euclid(1.0) * 524_288.0).round() as i32 & 0x7FFFF;
            (x::RAMP_CONIC, [q8((center[0] - 0.5) * w), q8((center[1] - 0.5) * h), turn, 0], [0; 4])
        }
    }
}

/// A flat scene drawn by the Metal executor: `prepare` on the CPU (the groups' surfaces and the record
/// writes), then the staged frame on the GPU from the same arena and the same media pool.
#[cfg(target_os = "macos")]
pub struct FlatGpu {
    gpu: crate::metal::Gpu,
    epoch: u64,
    tiles: Vec<u32>,
    stride: usize,
    words: usize,
}
#[cfg(target_os = "macos")]
impl FlatGpu {
    /// `None` without a Metal device.
    ///
    /// # Safety
    /// As `metal::Gpu::new`: the executor binds `flat`'s arena and media pool without a copy. `flat` must
    /// outlive this value, and be written only between its calls.
    pub unsafe fn new(flat: &FlatScene, cap: usize) -> Option<FlatGpu> {
        let (fw, fh) = flat.size();
        let words = (fw * fh) as usize;
        // SAFETY: the caller's contract is this function's.
        let gpu = unsafe { crate::metal::Gpu::new(&flat.scene, words, cap) }?;
        Some(FlatGpu { gpu, epoch: flat.mount_epoch, tiles: Vec::new(), stride: fw as usize, words })
    }
    /// The Metal source of `flat`'s scene (what `new` compiles).
    pub fn source(flat: &FlatScene) -> String {
        let s = &flat.scene;
        let descs: Vec<&Desc> = s.cols().iter().map(|c| c.desc).collect();
        let lays: Vec<(usize, usize)> = s.cols().iter().map(|c| (c.base, c.desc.words)).collect();
        crate::metal::source_fx(&descs, &lays, s.tab_region().0, (s.fw, s.fh), s.media.words().len())
    }
    /// As `new`, with the source given (the gates pass mutated text).
    ///
    /// # Safety
    /// As `new`.
    pub unsafe fn with_source(flat: &FlatScene, cap: usize, src: &str) -> Option<FlatGpu> {
        let (fw, fh) = flat.size();
        let words = (fw * fh) as usize;
        // SAFETY: the caller's contract is this function's.
        let gpu = unsafe { crate::metal::Gpu::with_source(&flat.scene, words, cap, src) }?;
        Some(FlatGpu { gpu, epoch: flat.mount_epoch, tiles: Vec::new(), stride: fw as usize, words })
    }
    /// One frame: the surface is `out()`.
    pub fn frame(&mut self, flat: &mut FlatScene, workers: &Pool, extra: usize) -> FlatStats {
        let mut st = flat.prepare(workers, extra);
        if flat.mount_epoch != self.epoch {
            self.gpu.sync(&flat.scene);
            self.epoch = flat.mount_epoch;
        }
        let u = flat.u;
        st.frame = flat.scene.update_eager(&u);
        flat.scene.damage.tiles(&mut self.tiles);
        flat.scene.damage.clear();
        st.frame.tiles = self.tiles.len() as u32;
        let block = *flat.scene.block();
        self.gpu.staged(&flat.scene, &block, &self.tiles, self.stride);
        flat.scene.media.reclaim();
        st
    }
    pub fn out(&self) -> &[u32] {
        self.gpu.out(self.words)
    }
    pub fn gpu(&mut self) -> &mut crate::metal::Gpu {
        &mut self.gpu
    }
}
