//! Pages and the transitions between them.
//!
//! A page is a flat scene ([`crate::style`]) described once. The router keeps each page as a **retained group**
//! of one stage scene: the page is painted into its own surface when its content changes, at the grain of
//! that change, and otherwise not at all. A transition never asks a page anything. It moves, scales,
//! fades and blurs the one stage record that shows the page's surface (the blur reads the surface's
//! pyramid, which is retained under the page's content), so a frame of a transition is a few record
//! rewrites and the damage of the records that moved.
//!
//! Every moving quantity is a track of an animation bank driven by monotone time. Pushing or popping
//! while a transition is in flight starts nothing over: each page still on screen is **retargeted** from
//! the offset, scale, opacity and blur it has at that instant, and, on a spring, from the velocity it has.
//!
//! ```ignore
//! let mut router = PageRouter::new(1280, 800, RouterConfig::default())?;
//! let home = router.add_page(home_scene)?;
//! let detail = router.add_page(detail_scene)?;
//! router.show(home);
//! router.push(detail, PageTransition::slide(Dir::Left), now);
//! // ... a few frames later, before it has arrived:
//! router.pop(now);          // the detail page turns round where it is, at the speed it had
//! ```
use crate::color::Color;
use crate::motion::*;
use crate::style::*;
use bough_paint::flat_scene::surface_words;
use std::collections::HashMap;

/// What moves a transition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Drive {
    /// A spring: interrupted, it keeps its velocity.
    Spring(SpringSpec),
    /// An eased curve of a fixed duration: interrupted, it starts again from where it is.
    Curve { ease: Ease, dur_ms: u32 },
}
impl Drive {
    pub const SNAPPY: Drive = Drive::Spring(SpringSpec::SNAPPY);
    pub const GENTLE: Drive = Drive::Spring(SpringSpec::GENTLE);
    pub const BOUNCY: Drive = Drive::Spring(SpringSpec::BOUNCY);
    pub const fn ease(dur_ms: u32) -> Drive {
        Drive::Curve { ease: Ease::EMPHASIZED, dur_ms }
    }
}

/// The way the incoming page moves (its content's direction of travel).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// A pair of elements, one in the outgoing page and one in the incoming page, that are the same thing to
/// the eye: the keys they were declared under (`PageRouter::declare_hero`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeroMatch {
    pub from: u64,
    pub to: u64,
}
impl HeroMatch {
    /// The same key in both pages.
    pub const fn key(k: u64) -> HeroMatch {
        HeroMatch { from: k, to: k }
    }
}

/// An element a shared-element transition can carry from one page to another: where it is in its page,
/// how it looks, and the nodes that draw it there (they are hidden while it is in flight).
#[derive(Clone, Debug, PartialEq)]
pub struct Hero {
    pub rect: [f32; 4],
    pub radius: f32,
    pub color: Color,
    pub shadow: Option<ShadowSpec>,
    pub nodes: Vec<NodeId>,
}
impl Hero {
    /// The hero a styled box is: its rect, corner radius, solid colour and first drop shadow.
    pub fn of(scene: &FlatScene, b: &StyledBox) -> Hero {
        let shadow = b.part(Part::Shadow).last().and_then(|id| match scene.node(id).cmd {
            DrawCmd::Shadow { spread, dx, dy, blur, color, .. } => Some(ShadowSpec::drop(dx, dy, blur, Color(color)).spread(spread)),
            _ => None,
        });
        Hero { rect: b.rect, radius: scene.prop(b.body(), Prop::Radius), color: Color(scene.color(b.body())), shadow, nodes: b.ids().collect() }
    }
}

/// How one page gives way to another.
#[derive(Clone, Debug, PartialEq)]
pub enum PageTransition {
    /// At once.
    None,
    /// The incoming page fades in over the outgoing one.
    CrossFade { drive: Drive },
    /// The incoming page slides in from the edge opposite `dir`; the outgoing one moves the same way by
    /// `parallax` of its size (1: pushed out whole; 0.3: the depth of a navigation stack; 0: it stays).
    Slide { dir: Dir, parallax: f32, drive: Drive },
    /// The incoming page grows from `from_scale` as it fades in; the outgoing one fades out past it.
    ScaleFade { from_scale: f32, drive: Drive },
    /// The incoming page comes into focus: from `max_blur_px` (a standard deviation) and `from_scale`,
    /// fading in, while the outgoing one blurs away.
    BlurReveal { max_blur_px: f32, from_scale: f32, drive: Drive },
    /// Matched elements travel from where they are in the outgoing page to where they are in the incoming
    /// one, changing size, corner radius, colour and shadow on the way; everything else cross-fades.
    SharedElement { keys: Vec<HeroMatch>, drive: Drive },
}
impl PageTransition {
    pub const fn cross_fade() -> PageTransition {
        PageTransition::CrossFade { drive: Drive::GENTLE }
    }
    pub const fn slide(dir: Dir) -> PageTransition {
        PageTransition::Slide { dir, parallax: 0.3, drive: Drive::SNAPPY }
    }
    pub const fn scale_fade() -> PageTransition {
        PageTransition::ScaleFade { from_scale: 0.92, drive: Drive::SNAPPY }
    }
    pub const fn blur_reveal() -> PageTransition {
        PageTransition::BlurReveal { max_blur_px: 24.0, from_scale: 1.06, drive: Drive::GENTLE }
    }
    pub fn shared(keys: &[HeroMatch]) -> PageTransition {
        PageTransition::SharedElement { keys: keys.to_vec(), drive: Drive::SNAPPY }
    }
    fn drive(&self) -> Option<Drive> {
        match self {
            PageTransition::None => None,
            PageTransition::CrossFade { drive } | PageTransition::Slide { drive, .. } | PageTransition::ScaleFade { drive, .. } | PageTransition::BlurReveal { drive, .. } | PageTransition::SharedElement { drive, .. } => Some(*drive),
        }
    }
    /// Where the incoming page starts, and where the outgoing page ends, in a `w` x `h` stage.
    fn ends(&self, w: f32, h: f32) -> (LayerState, LayerState) {
        let id = LayerState::IDENTITY;
        match *self {
            PageTransition::None => (id, id),
            PageTransition::CrossFade { .. } | PageTransition::SharedElement { .. } => (LayerState { opacity: 0.0, ..id }, id),
            PageTransition::Slide { dir, parallax, .. } => {
                let (ux, uy) = match dir {
                    Dir::Left => (-1.0, 0.0),
                    Dir::Right => (1.0, 0.0),
                    Dir::Up => (0.0, -1.0),
                    Dir::Down => (0.0, 1.0),
                };
                (LayerState { dx: -ux * w, dy: -uy * h, ..id }, LayerState { dx: ux * w * parallax, dy: uy * h * parallax, ..id })
            }
            PageTransition::ScaleFade { from_scale, .. } => (LayerState { scale: from_scale, opacity: 0.0, ..id }, LayerState { scale: 2.0 - from_scale, opacity: 0.0, ..id }),
            PageTransition::BlurReveal { max_blur_px, from_scale, .. } => (LayerState { blur: max_blur_px, scale: from_scale, opacity: 0.0, ..id }, LayerState { blur: max_blur_px, opacity: 0.0, ..id }),
        }
    }
}

/// Where a page's layer is: an offset (px), a scale about the stage's centre, an opacity and a blur (a
/// standard deviation, px).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerState {
    pub dx: f32,
    pub dy: f32,
    pub scale: f32,
    pub opacity: f32,
    pub blur: f32,
}
impl LayerState {
    pub const IDENTITY: LayerState = LayerState { dx: 0.0, dy: 0.0, scale: 1.0, opacity: 1.0, blur: 0.0 };
    fn get(&self, k: usize) -> f32 {
        [self.dx, self.dy, self.scale, self.opacity, self.blur][k]
    }
}
const LAYER_PROPS: [Prop; 5] = [Prop::Dx, Prop::Dy, Prop::Scale, Prop::Opacity, Prop::Blur];
/// A hero overlay's tracks: its rect, corner radius, colour, and its shadow's blur, offset and alpha.
const HERO_TRACKS: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PageId(pub u32);

#[derive(Clone, Copy, Debug)]
pub struct RouterConfig {
    /// Pages the router can hold at once (each keeps a surface of the stage's size).
    pub max_pages: usize,
    /// Pages that can be on screen at once (two for a plain transition; more when transitions are
    /// interrupted by further ones before they end).
    pub layers: usize,
    /// Shared elements one transition can carry.
    pub heroes: usize,
    /// Keep each page's blur pyramid (a third more memory per page): needed for `BlurReveal`.
    pub blur: bool,
    /// What shows where no page does.
    pub background: Color,
}
impl Default for RouterConfig {
    fn default() -> Self {
        RouterConfig { max_pages: 4, layers: 4, heroes: 4, blur: true, background: Color::rgba(0, 0, 0, 255) }
    }
}

struct Page {
    group: GroupId,
    tracks: [TrackId; 5],
    /// The stage slot that shows it.
    slot: Option<usize>,
    heroes: HashMap<u64, Hero>,
}
#[derive(Clone, Copy, Debug)]
struct Layer {
    page: PageId,
    /// It is on its way out: hidden when the motion ends.
    exiting: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Flight {
    /// The page the element is leaving and the one it is heading for, and its key in each.
    a: PageId,
    b: PageId,
    ka: u64,
    kb: u64,
}
struct HeroSlot {
    shadow: NodeId,
    body: NodeId,
    tracks: [TrackId; HERO_TRACKS],
    flight: Option<Flight>,
}

/// What a frame of the router did.
#[derive(Clone, Copy, Debug, Default)]
pub struct RouterStats {
    pub flat: FlatStats,
    /// A frame is owed at the output's next refresh.
    pub animating: bool,
    /// Pages on screen.
    pub layers: usize,
}

const TAG_PAGE: u64 = 1 << 62;
const TAG_HERO: u64 = 1 << 61;

/// Pages, the stack of them, and the transitions in flight. See the module's description.
pub struct PageRouter {
    stage: FlatScene,
    bank: AnimBank,
    size: (u32, u32),
    top: u32,
    pages: Vec<Option<Page>>,
    /// The navigation stack: each page with the transition that brought it (a pop plays it backwards).
    stack: Vec<(PageId, PageTransition)>,
    /// The pages on screen, bottom to top.
    layers: Vec<Layer>,
    slots: Vec<NodeId>,
    heroes: Vec<HeroSlot>,
    now: ClockNs,
}

impl PageRouter {
    /// A router whose pages are `w` x `h` pixels (at most 2047 a side).
    pub fn new(w: u32, h: u32, cfg: RouterConfig) -> Result<PageRouter, FlatError> {
        let top = if cfg.blur { 5 } else { 0 };
        let (layers, heroes) = (cfg.layers.max(2), cfg.heroes);
        let fc = FlatConfig::empty()
            .with(EffectKind::Solid, Space::Fixed, 1)
            .with(EffectKind::Group, Space::Fixed, layers)
            .with(EffectKind::Shadow, Space::Fixed, heroes)
            .with(EffectKind::RRect, Space::Fixed, heroes)
            .media(cfg.max_pages * (surface_words(w, h, top) + 16) + 64);
        let mut stage = FlatScene::new(w, h, fc);
        let mut bank = AnimBank::new();
        let full = [0.0, 0.0, w as f32, h as f32];
        stage.push(Node::new(full, DrawCmd::Solid { color: cfg.background.0 }))?;
        // every record the router will ever use is mounted now, hidden: a transition mounts nothing
        let mut slots = Vec::with_capacity(layers);
        for _ in 0..layers {
            slots.push(stage.push(Node::new(full, DrawCmd::Group { radius: 0.0, blur: 0.0, group: GroupId(u32::MAX) }).hidden())?);
        }
        let mut hs = Vec::with_capacity(heroes);
        for i in 0..heroes {
            let shadow = stage.push(Node::new([0.0; 4], DrawCmd::Shadow { radius: 0.0, spread: 0.0, dx: 0.0, dy: 0.0, blur: 0.0, color: 0 }).hidden())?;
            let body = stage.push(Node::new([0.0; 4], DrawCmd::RRect { radius: 0.0, color: 0 }).hidden())?;
            let tracks: [TrackId; HERO_TRACKS] = std::array::from_fn(|k| bank.track(TAG_HERO | (i as u64) << 8 | k as u64, 0.0));
            for (k, &t) in tracks.iter().enumerate() {
                bank.set_rest(t, if (5..9).contains(&k) || k == 11 { 0.25 } else { 1.0 / 64.0 });
            }
            hs.push(HeroSlot { shadow, body, tracks, flight: None });
        }
        Ok(PageRouter { stage, bank, size: (w, h), top, pages: Vec::new(), stack: Vec::new(), layers: Vec::new(), slots, heroes: hs, now: ClockNs(0) })
    }
    /// The stage: the scene an executor draws (`FlatScene::frame`, `flat_scene::FlatGpu::frame`).
    pub fn stage(&self) -> &FlatScene {
        &self.stage
    }
    pub fn stage_mut(&mut self) -> &mut FlatScene {
        &mut self.stage
    }
    /// The animation bank (to pause or scrub a transition: `AnimBank::scrub_group(0, ms)`).
    pub fn bank(&self) -> &AnimBank {
        &self.bank
    }
    pub fn bank_mut(&mut self) -> &mut AnimBank {
        &mut self.bank
    }

    /// Give the router a page: a flat scene of the stage's size. It is painted into its own retained
    /// surface when it is first shown and whenever its content changes after that.
    pub fn add_page(&mut self, content: FlatScene) -> Result<PageId, FlatError> {
        assert_eq!(content.size(), self.size, "a page is the stage's size");
        let group = self.stage.group(content, self.top)?;
        let id = self.pages.iter().position(Option::is_none).unwrap_or(self.pages.len());
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        let tracks: [TrackId; 5] = std::array::from_fn(|k| self.bank.track(TAG_PAGE | (id as u64) << 8 | k as u64, LayerState::IDENTITY.get(k)));
        for (k, &t) in tracks.iter().enumerate() {
            self.bank.set_rest(t, rest_of(LAYER_PROPS[k]));
        }
        // a page well outside the stage cannot be seen (half a stage of margin for a scaled or blurred
        // one): its motion parks there
        self.bank.set_window(tracks[0], Some((-1.5 * w, 1.5 * w)));
        self.bank.set_window(tracks[1], Some((-1.5 * h, 1.5 * h)));
        let page = Page { group, tracks, slot: None, heroes: HashMap::new() };
        if id == self.pages.len() {
            self.pages.push(Some(page));
        } else {
            self.pages[id] = Some(page);
        }
        Ok(PageId(id as u32))
    }
    /// Take a page back (it must be neither on screen nor in the stack).
    pub fn remove_page(&mut self, id: PageId) -> Option<FlatScene> {
        if self.layers.iter().any(|l| l.page == id) || self.stack.iter().any(|s| s.0 == id) {
            return None;
        }
        let p = self.pages.get_mut(id.0 as usize)?.take()?;
        self.stage.drop_group(p.group)
    }
    fn page(&self, id: PageId) -> &Page {
        self.pages[id.0 as usize].as_ref().expect("a live page")
    }
    /// A page's own scene: change its content here, at any time (in flight too: the page's surface is
    /// repainted where it changed).
    pub fn page_mut(&mut self, id: PageId) -> &mut FlatScene {
        let g = self.page(id).group;
        self.stage.child_mut(g)
    }
    pub fn page_scene(&self, id: PageId) -> &FlatScene {
        self.stage.child(self.page(id).group)
    }
    /// Declare an element of a page that shared-element transitions can carry, under `key`.
    pub fn declare_hero(&mut self, page: PageId, key: u64, hero: Hero) {
        self.pages[page.0 as usize].as_mut().expect("a live page").heroes.insert(key, hero);
    }

    /// The page on top of the stack.
    pub fn current(&self) -> Option<PageId> {
        self.stack.last().map(|s| s.0)
    }
    pub fn depth(&self) -> usize {
        self.stack.len()
    }
    /// Pages on screen, bottom to top.
    pub fn visible(&self) -> Vec<PageId> {
        self.layers.iter().map(|l| l.page).collect()
    }
    /// Where a page's layer is (as of the last tick), if it is on screen.
    pub fn layer_state(&self, id: PageId) -> Option<LayerState> {
        self.layers.iter().any(|l| l.page == id).then(|| {
            let t = self.page(id).tracks;
            LayerState { dx: self.bank.get(t[0]), dy: self.bank.get(t[1]), scale: self.bank.get(t[2]), opacity: self.bank.get(t[3]), blur: self.bank.get(t[4]) }
        })
    }
    /// A page's layer at any instant: (state, its rate of change per second). A pure function of time.
    pub fn layer_at(&self, id: PageId, at: ClockNs) -> (LayerState, [f64; 5]) {
        let t = self.page(id).tracks;
        let s = |k: usize| self.bank.sample(t[k], at);
        (LayerState { dx: s(0), dy: s(1), scale: s(2), opacity: s(3), blur: s(4) }, std::array::from_fn(|k| self.bank.velocity(t[k], at)))
    }
    /// A hero overlay at any instant: its rect, radius, colour channels and shadow, and their rates.
    pub fn hero_at(&self, slot: usize, at: ClockNs) -> Option<([f32; HERO_TRACKS], [f64; HERO_TRACKS])> {
        let h = self.heroes.get(slot)?;
        h.flight?;
        Some((std::array::from_fn(|k| self.bank.sample(h.tracks[k], at)), std::array::from_fn(|k| self.bank.velocity(h.tracks[k], at))))
    }
    /// Some transition is still moving.
    pub fn in_flight(&self) -> bool {
        self.moving(self.now)
    }
    fn moving(&self, now: ClockNs) -> bool {
        let live = |t: &TrackId| self.bank.end(*t).is_some_and(|e| e > now) || self.bank.paused(*t);
        self.layers.iter().any(|l| self.page(l.page).tracks.iter().any(live)) || self.heroes.iter().any(|h| h.flight.is_some() && h.tracks.iter().any(live))
    }

    // ---- navigation ----

    /// Make `page` the only page, at once.
    pub fn show(&mut self, page: PageId) {
        self.land();
        self.stack.clear();
        self.stack.push((page, PageTransition::None));
        for l in std::mem::take(&mut self.layers) {
            self.pages[l.page.0 as usize].as_mut().unwrap().slot = None;
        }
        self.place(page, LayerState::IDENTITY, None);
        self.send(page, LayerState::IDENTITY, None, self.now);
        self.sync_slots();
    }
    /// Put `page` on top of the stack, arriving by `t`. In flight or not: whatever is on screen goes on
    /// from where it is.
    pub fn push(&mut self, page: PageId, t: PageTransition, now: ClockNs) {
        let from = self.current();
        self.stack.push((page, t.clone()));
        self.begin(from, page, &t, false, now);
    }
    /// Take the top page off the stack: it leaves the way it came, backwards. Returns it.
    pub fn pop(&mut self, now: ClockNs) -> Option<PageId> {
        if self.stack.len() < 2 {
            return None;
        }
        let (popped, t) = self.stack.pop().unwrap();
        let to = self.stack.last().unwrap().0;
        self.begin(Some(popped), to, &t, true, now);
        Some(popped)
    }
    /// Replace the top page by `page`, arriving by `t`.
    pub fn replace(&mut self, page: PageId, t: PageTransition, now: ClockNs) {
        let from = self.stack.pop().map(|s| s.0);
        self.stack.push((page, t.clone()));
        self.begin(from, page, &t, false, now);
    }

    /// Put `page` on screen at `start` if it is not there: above everything, or just beneath `under`.
    fn place(&mut self, page: PageId, start: LayerState, under: Option<PageId>) {
        if let Some(l) = self.layers.iter_mut().find(|l| l.page == page) {
            l.exiting = false;
            return;
        }
        if self.layers.len() == self.slots.len() {
            // more pages on screen than the stage has layers for: the oldest one on its way out goes now
            let k = self.layers.iter().position(|l| l.exiting).unwrap_or(0);
            let gone = self.layers.remove(k);
            self.pages[gone.page.0 as usize].as_mut().unwrap().slot = None;
        }
        let at = under.and_then(|u| self.layers.iter().position(|l| l.page == u)).unwrap_or(self.layers.len());
        self.layers.insert(at, Layer { page, exiting: false });
        let tracks = self.page(page).tracks;
        for (k, &t) in tracks.iter().enumerate() {
            self.bank.set(t, start.get(k));
        }
    }
    /// Head a page's layer for `to`.
    fn send(&mut self, page: PageId, to: LayerState, drive: Option<Drive>, now: ClockNs) {
        let tracks = self.page(page).tracks;
        for (k, &t) in tracks.iter().enumerate() {
            match drive {
                None => self.bank.set(t, to.get(k)),
                Some(Drive::Spring(s)) => self.bank.spring_to(t, s, to.get(k), now),
                Some(Drive::Curve { ease, dur_ms }) => self.bank.tween_to(t, to.get(k), ease, Timing::once(dur_ms), now),
            }
        }
    }
    fn begin(&mut self, from: Option<PageId>, to: PageId, t: &PageTransition, reverse: bool, now: ClockNs) {
        self.now = now;
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        let (enter, exit) = t.ends(w, h);
        let drive = t.drive();
        // forwards the incoming page arrives on top from `enter` and the outgoing one heads for `exit`;
        // backwards the page revealed comes back from `exit` beneath, and the popped one leaves by `enter`
        let (start, leave, under) = if reverse { (exit, enter, from) } else { (enter, exit, None) };
        self.place(to, start, under);
        if !reverse {
            // an arriving page is on top, even if it was on screen already (on its way out)
            let k = self.layers.iter().position(|l| l.page == to).unwrap();
            let l = self.layers.remove(k);
            self.layers.push(l);
        }
        self.send(to, LayerState::IDENTITY, drive, now);
        if let Some(f) = from.filter(|&f| f != to) {
            if let Some(l) = self.layers.iter_mut().find(|l| l.page == f) {
                l.exiting = true;
                self.send(f, leave, drive, now);
            }
        }
        // shared elements
        let keys: &[HeroMatch] = match t {
            PageTransition::SharedElement { keys, .. } => keys,
            _ => &[],
        };
        let wanted: Vec<Flight> = match from {
            Some(f) if f != to => keys.iter().map(|m| if reverse { Flight { a: f, b: to, ka: m.to, kb: m.from } } else { Flight { a: f, b: to, ka: m.from, kb: m.to } }).collect(),
            _ => Vec::new(),
        };
        // an element in the air goes on from where it is when this transition wants it (the same flight,
        // the same flight turned round, or onward from the page it was heading for); one already heading
        // for this transition's page flies on; any other lands at once
        let continues = |fl: Flight, w: &Flight| *w == fl || (w.a == fl.b && w.b == fl.a && w.ka == fl.kb && w.kb == fl.ka) || (w.a == fl.b && w.ka == fl.kb);
        for i in 0..self.heroes.len() {
            if let Some(fl) = self.heroes[i].flight {
                if !wanted.iter().any(|w| continues(fl, w)) && fl.b != to {
                    self.land_one(i);
                }
            }
        }
        for want in wanted {
            let (Some(ha), Some(hb)) = (self.page(want.a).heroes.get(&want.ka).cloned(), self.page(want.b).heroes.get(&want.kb).cloned()) else { continue };
            let slot = match self.heroes.iter().position(|h| h.flight.is_some_and(|fl| continues(fl, &want))) {
                Some(i) => {
                    // already in the air: it goes on from where it is. The pages it now flies between stop
                    // drawing it; a page it no longer has to do with draws it again
                    let fl = self.heroes[i].flight.unwrap();
                    for (p, k) in [(fl.a, fl.ka), (fl.b, fl.kb)] {
                        if p != want.a && p != want.b {
                            self.show_hero(p, k, true);
                        }
                    }
                    self.show_hero(want.a, want.ka, false);
                    self.show_hero(want.b, want.kb, false);
                    i
                }
                None => {
                    let Some(i) = self.heroes.iter().position(|h| h.flight.is_none()) else { continue };
                    // it starts where the outgoing page shows it now
                    let at = self.layer_at(want.a, now).0;
                    let (cx, cy) = (w * 0.5, h * 0.5);
                    let map = |x: f32, y: f32| (cx + (x - cx) * at.scale + at.dx, cy + (y - cy) * at.scale + at.dy);
                    let ((x0, y0), (x1, y1)) = (map(ha.rect[0], ha.rect[1]), map(ha.rect[2], ha.rect[3]));
                    let start = hero_values(&Hero { rect: [x0, y0, x1, y1], radius: ha.radius * at.scale, ..ha.clone() });
                    let tracks = self.heroes[i].tracks;
                    for (k, &tr) in tracks.iter().enumerate() {
                        self.bank.set(tr, start[k]);
                    }
                    let (sh, body) = (self.heroes[i].shadow, self.heroes[i].body);
                    let shadow = hb.shadow.or(ha.shadow);
                    self.stage.set(sh, DrawCmd::Shadow { radius: ha.radius, spread: shadow.map_or(0.0, |s| s.spread), dx: shadow.map_or(0.0, |s| s.dx), dy: start[10], blur: start[9], color: shadow.map_or(0, |s| s.color.0 & 0x00ff_ffff) });
                    self.stage.show(sh, shadow.is_some());
                    self.stage.show(body, true);
                    // the pages stop drawing the element themselves: one repaint of its tiles in each
                    self.show_hero(want.a, want.ka, false);
                    self.show_hero(want.b, want.kb, false);
                    i
                }
            };
            self.heroes[slot].flight = Some(want);
            let target = hero_values(&hb);
            let tracks = self.heroes[slot].tracks;
            for (k, &tr) in tracks.iter().enumerate() {
                match drive {
                    None => self.bank.set(tr, target[k]),
                    Some(Drive::Spring(s)) => self.bank.spring_to(tr, s, target[k], now),
                    Some(Drive::Curve { ease, dur_ms }) => self.bank.tween_to(tr, target[k], ease, Timing::once(dur_ms), now),
                }
            }
        }
        self.sync_slots();
    }
    /// Whether a page draws its element `key` itself.
    fn show_hero(&mut self, p: PageId, key: u64, visible: bool) {
        let Some(page) = self.pages.get(p.0 as usize).and_then(Option::as_ref) else { return };
        let Some(hero) = page.heroes.get(&key) else { return };
        let child = self.stage.child_mut(page.group);
        for &n in &hero.nodes {
            child.show(n, visible);
        }
    }
    /// A hero overlay's flight is over: the pages draw the element themselves again.
    fn land_one(&mut self, i: usize) {
        let Some(fl) = self.heroes[i].flight.take() else { return };
        let (sh, body) = (self.heroes[i].shadow, self.heroes[i].body);
        self.stage.show(sh, false);
        self.stage.show(body, false);
        self.show_hero(fl.a, fl.ka, true);
        self.show_hero(fl.b, fl.kb, true);
    }
    fn land(&mut self) {
        for i in 0..self.heroes.len() {
            self.land_one(i);
        }
    }
    /// Bring the stage's layer records to the pages on screen, bottom to top.
    fn sync_slots(&mut self) {
        for i in 0..self.slots.len() {
            let node = self.slots[i];
            match self.layers.get(i).copied() {
                Some(l) => {
                    let p = self.pages[l.page.0 as usize].as_mut().unwrap();
                    p.slot = Some(i);
                    let (g, t) = (p.group, p.tracks);
                    let v: [f32; 5] = std::array::from_fn(|k| self.bank.get(t[k]));
                    self.stage.set(node, DrawCmd::Group { radius: 0.0, blur: v[4], group: g });
                    self.stage.set_xf(node, v[0].round(), v[1].round(), v[2]);
                    self.stage.set_opacity(node, v[3]);
                    self.stage.show(node, true);
                }
                None => self.stage.show(node, false),
            }
        }
    }

    /// Bring every transition to `now`. Returns whether a frame is owed at the output's next refresh.
    pub fn tick(&mut self, now: ClockNs) -> bool {
        self.now = now;
        let (stage, pages, slots, heroes) = (&mut self.stage, &self.pages, &self.slots, &self.heroes);
        self.bank.tick(now, |tag, v| {
            let (i, k) = (((tag >> 8) & 0xffff_ffff) as usize, (tag & 255) as usize);
            if tag & TAG_PAGE != 0 {
                if let Some(slot) = pages.get(i).and_then(Option::as_ref).and_then(|p| p.slot) {
                    // a page moves by whole pixels: its surface is then shown as it is (no resampling, on
                    // either executor), which is also what keeps its text sharp on the way
                    let v = if k < 2 { v.round() } else { v };
                    stage.set_prop(slots[slot], LAYER_PROPS[k], v);
                }
            } else if tag & TAG_HERO != 0 {
                let h = &heroes[i];
                match k {
                    0..=3 => {
                        let p = [Prop::X0, Prop::Y0, Prop::X1, Prop::Y1][k];
                        stage.set_prop(h.body, p, v);
                        stage.set_prop(h.shadow, p, v);
                    }
                    4 => {
                        stage.set_prop(h.body, Prop::Radius, v);
                        stage.set_prop(h.shadow, Prop::Radius, v);
                    }
                    5..=8 => stage.set_prop(h.body, [Prop::Red, Prop::Green, Prop::Blue, Prop::Alpha][k - 5], v),
                    9 => stage.set_prop(h.shadow, Prop::Blur, v),
                    10 => stage.set_prop(h.shadow, Prop::OffY, v),
                    _ => stage.set_prop(h.shadow, Prop::Alpha, v),
                }
            }
        });
        let moving = self.moving(now);
        if !moving {
            // everything has arrived: the pages that were leaving are gone, the elements are their pages' again
            if self.heroes.iter().any(|h| h.flight.is_some()) {
                self.land();
            }
            if self.layers.iter().any(|l| l.exiting) {
                for l in self.layers.iter().filter(|l| l.exiting) {
                    self.pages[l.page.0 as usize].as_mut().unwrap().slot = None;
                }
                self.layers.retain(|l| !l.exiting);
                self.sync_slots();
            }
        }
        moving || self.stage.pending()
    }
    /// `tick`, then a CPU frame of the stage into `out` (which holds the previous frame).
    pub fn frame(&mut self, now: ClockNs, out: &mut [u32], stride: usize, workers: &bough_kernel::cpu::Pool, extra: usize) -> RouterStats {
        let animating = self.tick(now);
        let flat = self.stage.frame(out, stride, workers, extra);
        RouterStats { flat, animating: animating || self.stage.pending(), layers: self.layers.len() }
    }
}

/// A hero as the twelve values its overlay's tracks hold.
fn hero_values(h: &Hero) -> [f32; HERO_TRACKS] {
    let c = h.color.0;
    let (blur, dy, alpha) = h.shadow.map_or((0.0, 0.0, 0.0), |s| (s.blur, s.dy, (s.color.0 >> 24) as f32));
    [h.rect[0], h.rect[1], h.rect[2], h.rect[3], h.radius, (c & 255) as f32, ((c >> 8) & 255) as f32, ((c >> 16) & 255) as f32, (c >> 24) as f32, blur, dy, alpha]
}
