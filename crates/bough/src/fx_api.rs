//! Styles and motion without type parameters.
//!
//! This is the author's side of the paint lane's flat bank (`bough_paint::flat_scene`): a box's look is a
//! value ([`BoxStyle`]: a fill or a [`Gradient`], corner radius, a border, any number of drop and inset
//! shadows ([`ShadowSpec`]), a backdrop blur ([`BlurSpec`]), opacity), built with ordinary methods and
//! changed at run time; a motion is a value too ([`AnimSpec`]: a spring, an eased curve, keyframes). Nothing
//! here names a paint type: `style.shadow(a).shadow(b)` and `style.shadow(a)` are the same Rust type, and
//! a third shadow added tomorrow compiles nothing new.
//!
//! ```ignore
//! let card = BoxStyle::new()
//!     .gradient(Gradient::linear(135.0).stop(0.0, rgb(0x4f46e5)).stop(0.6, rgb(0x9333ea)).stop(1.0, rgb(0xec4899)))
//!     .rounded(16.0)
//!     .border(1.0, Color::rgba(255, 255, 255, 60))
//!     .shadow(ShadowSpec::drop(0.0, 12.0, 32.0, Color::rgba(0, 0, 0, 70)).spread(-6.0))
//!     .shadow(ShadowSpec::drop(0.0, 2.0, 4.0, Color::rgba(0, 0, 0, 50)))
//!     .shadow(ShadowSpec::inset(0.0, 1.0, 0.0, Color::rgba(255, 255, 255, 90)));
//! let b = scene.add_box([40.0, 40.0, 360.0, 240.0], &card)?;
//! motion.animate(&scene, b.body(), Prop::Dy, -8.0, AnimSpec::spring(SpringSpec::SNAPPY), now);
//! ```
//!
//! A box is lowered to a run of nodes of the flat bank in paint order: drop shadows (the last one given
//! deepest, as CSS), the backdrop, the fill, inset shadows, the border. [`StyledBox`] remembers the run.
use crate::color::Color;
pub use bough_paint::flat_scene::{DrawCmd, EffectKind, FlatConfig, FlatError, FlatScene, FlatStats, GroupId, Lut, Node, NodeId, Prop, RampGeo, Space};
pub use bough_paint::motion::{AnimBank, ClockNs, CubicBezier, Direction, Ease, Key, Repeat, SpringSpec, Timing, TrackId};
use std::collections::HashMap;

/// Where a gradient runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GradientKind {
    /// CSS `linear-gradient(<angle>)`: 0 is to the top, clockwise.
    Linear { angle_deg: f32 },
    /// An ellipse about `center` (fractions of the box). `radius`: pixels, or `None` for the farthest
    /// corner (CSS's default). `focal`: where the first stop sits, as an offset from the centre in radii.
    Radial { center: [f32; 2], radius: Option<[f32; 2]>, focal: [f32; 2] },
    /// CSS `conic-gradient(from <start> at <center>)`.
    Conic { center: [f32; 2], start_deg: f32 },
}

/// An N-stop gradient.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub kind: GradientKind,
    /// (position 0..1, colour), in any order.
    pub stops: Vec<(f32, Color)>,
}
impl Gradient {
    pub fn linear(angle_deg: f32) -> Gradient {
        Gradient { kind: GradientKind::Linear { angle_deg }, stops: Vec::new() }
    }
    /// A circle about the box's centre reaching its farthest corner.
    pub fn radial() -> Gradient {
        Gradient { kind: GradientKind::Radial { center: [0.5, 0.5], radius: None, focal: [0.0, 0.0] }, stops: Vec::new() }
    }
    pub fn conic(start_deg: f32) -> Gradient {
        Gradient { kind: GradientKind::Conic { center: [0.5, 0.5], start_deg }, stops: Vec::new() }
    }
    /// The centre of a radial or conic gradient, as fractions of the box.
    pub fn at(mut self, x: f32, y: f32) -> Gradient {
        match &mut self.kind {
            GradientKind::Radial { center, .. } | GradientKind::Conic { center, .. } => *center = [x, y],
            GradientKind::Linear { .. } => {}
        }
        self
    }
    /// A radial gradient's radii in pixels (an ellipse when they differ).
    pub fn radius(mut self, rx: f32, ry: f32) -> Gradient {
        if let GradientKind::Radial { radius, .. } = &mut self.kind {
            *radius = Some([rx, ry]);
        }
        self
    }
    /// A radial gradient's focal point: an offset from the centre in radii (inside the unit circle).
    pub fn focal(mut self, fx: f32, fy: f32) -> Gradient {
        if let GradientKind::Radial { focal, .. } = &mut self.kind {
            *focal = [fx, fy];
        }
        self
    }
    pub fn stop(mut self, t: f32, c: Color) -> Gradient {
        self.stops.push((t, c));
        self
    }
    pub fn stops(mut self, stops: &[(f32, Color)]) -> Gradient {
        self.stops.extend_from_slice(stops);
        self
    }
    /// The geometry for a box of `w` x `h` pixels.
    pub fn geo(&self, w: f32, h: f32) -> RampGeo {
        match self.kind {
            GradientKind::Linear { angle_deg } => RampGeo::Linear { angle_deg },
            GradientKind::Radial { center, radius, focal } => {
                let far = {
                    let (dx, dy) = (center[0].max(1.0 - center[0]) * w, center[1].max(1.0 - center[1]) * h);
                    (dx * dx + dy * dy).sqrt().max(1.0)
                };
                RampGeo::Radial { center, radius: radius.unwrap_or([far, far]), focal }
            }
            GradientKind::Conic { center, start_deg } => RampGeo::Conic { center, start_deg },
        }
    }
}

/// What fills a box.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Fill {
    #[default]
    None,
    Color(Color),
    Gradient(Gradient),
}

/// One CSS `box-shadow`: `blur` is the blur radius (two sigma), `spread` grows (or, inset, shrinks) the
/// shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowSpec {
    pub dx: f32,
    pub dy: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Color,
    pub inset: bool,
}
impl ShadowSpec {
    pub const fn drop(dx: f32, dy: f32, blur: f32, color: Color) -> ShadowSpec {
        ShadowSpec { dx, dy, blur, spread: 0.0, color, inset: false }
    }
    pub const fn inset(dx: f32, dy: f32, blur: f32, color: Color) -> ShadowSpec {
        ShadowSpec { dx, dy, blur, spread: 0.0, color, inset: true }
    }
    pub const fn spread(mut self, s: f32) -> ShadowSpec {
        self.spread = s;
        self
    }
}

/// A blur: of what is beneath the box (`backdrop-filter`), with the filter's other factors and a tint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlurSpec {
    /// A standard deviation in pixels (CSS `blur(<r>)`).
    pub radius: f32,
    pub brightness: f32,
    pub saturation: f32,
    pub contrast: f32,
    pub tint: Color,
}
impl BlurSpec {
    pub const fn backdrop(radius: f32) -> BlurSpec {
        BlurSpec { radius, brightness: 1.0, saturation: 1.0, contrast: 1.0, tint: Color(0) }
    }
    pub const fn tint(mut self, c: Color) -> BlurSpec {
        self.tint = c;
        self
    }
    pub const fn saturate(mut self, s: f32) -> BlurSpec {
        self.saturation = s;
        self
    }
    pub const fn brightness(mut self, b: f32) -> BlurSpec {
        self.brightness = b;
        self
    }
    pub const fn contrast(mut self, c: f32) -> BlurSpec {
        self.contrast = c;
        self
    }
}

/// A box's look. Every part is optional and every method may be called in any order, any number of times
/// (a later fill replaces an earlier one; shadows accumulate, the first on top as in CSS).
#[derive(Clone, Debug, PartialEq)]
pub struct BoxStyle {
    pub fill: Fill,
    pub radius: f32,
    pub border: Option<(f32, Color)>,
    pub shadows: Vec<ShadowSpec>,
    pub backdrop: Option<BlurSpec>,
    pub opacity: f32,
    pub clip: Option<[f32; 4]>,
    pub space: Space,
}
impl Default for BoxStyle {
    fn default() -> Self {
        BoxStyle { fill: Fill::None, radius: 0.0, border: None, shadows: Vec::new(), backdrop: None, opacity: 1.0, clip: None, space: Space::Fixed }
    }
}
impl BoxStyle {
    pub fn new() -> BoxStyle {
        BoxStyle::default()
    }
    pub fn bg(mut self, c: Color) -> BoxStyle {
        self.fill = Fill::Color(c);
        self
    }
    pub fn gradient(mut self, g: Gradient) -> BoxStyle {
        self.fill = Fill::Gradient(g);
        self
    }
    pub fn rounded(mut self, r: f32) -> BoxStyle {
        self.radius = r;
        self
    }
    pub fn border(mut self, width: f32, c: Color) -> BoxStyle {
        self.border = Some((width, c));
        self
    }
    pub fn shadow(mut self, s: ShadowSpec) -> BoxStyle {
        self.shadows.push(s);
        self
    }
    pub fn backdrop_blur(mut self, b: BlurSpec) -> BoxStyle {
        self.backdrop = Some(b);
        self
    }
    pub fn opacity(mut self, o: f32) -> BoxStyle {
        self.opacity = o;
        self
    }
    /// Cut to a rect (pixels).
    pub fn clip(mut self, k: [f32; 4]) -> BoxStyle {
        self.clip = Some(k);
        self
    }
    /// The box scrolls with the scene's pan.
    pub fn panned(mut self) -> BoxStyle {
        self.space = Space::Panned;
        self
    }
    /// The box's parts in paint order: (which part, its effect). Gradients' tables are made in `scene`.
    fn parts(&self, scene: &mut FlatScene, rect: [f32; 4]) -> Result<Vec<(Part, DrawCmd)>, FlatError> {
        let r = self.radius;
        let mut out = Vec::new();
        for s in self.shadows.iter().rev().filter(|s| !s.inset) {
            out.push((Part::Shadow, DrawCmd::Shadow { radius: r, spread: s.spread, dx: s.dx, dy: s.dy, blur: s.blur, color: s.color.0 }));
        }
        if let Some(b) = self.backdrop {
            out.push((Part::Backdrop, DrawCmd::Glass { radius: r, blur: b.radius, brightness: b.brightness, saturation: b.saturation, contrast: b.contrast, tint: b.tint.0 }));
        }
        match &self.fill {
            Fill::None => {}
            Fill::Color(c) => out.push((Part::Fill, if r > 0.0 { DrawCmd::RRect { radius: r, color: c.0 } } else { DrawCmd::Solid { color: c.0 } })),
            Fill::Gradient(g) => {
                let stops: Vec<(f32, u32)> = g.stops.iter().map(|&(t, c)| (t, c.0)).collect();
                let lut = scene.lut(&stops)?;
                out.push((Part::Fill, DrawCmd::Ramp { radius: r, geo: g.geo(rect[2] - rect[0], rect[3] - rect[1]), lut }));
            }
        }
        for s in self.shadows.iter().rev().filter(|s| s.inset) {
            out.push((Part::Inset, DrawCmd::Inset { radius: r, spread: s.spread, dx: s.dx, dy: s.dy, blur: s.blur, color: s.color.0 }));
        }
        if let Some((w, c)) = self.border {
            out.push((Part::Border, DrawCmd::Border { radius: r, width: w, color: c.0, fill: 0 }));
        }
        Ok(out)
    }
}

/// What a node of a styled box is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Shadow,
    Backdrop,
    Fill,
    Inset,
    Border,
}

/// A box in a scene: the run of nodes its style was lowered to.
#[derive(Clone, Debug, PartialEq)]
pub struct StyledBox {
    pub rect: [f32; 4],
    /// In paint order.
    pub nodes: Vec<(Part, NodeId)>,
}
impl StyledBox {
    /// The node that carries the box's body (its fill; failing that its first node): what an element-wise
    /// animation usually targets.
    pub fn body(&self) -> NodeId {
        self.nodes.iter().find(|(p, _)| *p == Part::Fill).or(self.nodes.first()).map(|n| n.1).expect("a styled box has a node")
    }
    pub fn part(&self, p: Part) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes.iter().filter(move |n| n.0 == p).map(|n| n.1)
    }
    pub fn ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes.iter().map(|n| n.1)
    }
}

/// Styled boxes in a flat scene.
pub trait SceneExt {
    /// Mount a box: one node per part of its style, in paint order, on top of everything.
    fn add_box(&mut self, rect: [f32; 4], style: &BoxStyle) -> Result<StyledBox, FlatError>;
    /// Bring a mounted box to `rect` and `style`. The style must have the parts the box was mounted with
    /// (the same number of shadows of each sort, a fill of the same sort): every value may differ.
    /// `Err(Restyle::Structure)`: nothing was changed; mount another box.
    fn restyle(&mut self, b: &mut StyledBox, rect: [f32; 4], style: &BoxStyle) -> Result<(), Restyle>;
    /// Move every node of a box by a transform (a translation in pixels, a scale about its centre).
    fn transform_box(&mut self, b: &StyledBox, dx: f32, dy: f32, scale: f32);
    fn show_box(&mut self, b: &StyledBox, visible: bool);
    fn fade_box(&mut self, b: &StyledBox, opacity: f32);
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Restyle {
    /// The style has other parts than the box was mounted with.
    Structure,
    Scene(FlatError),
}
impl SceneExt for FlatScene {
    fn add_box(&mut self, rect: [f32; 4], style: &BoxStyle) -> Result<StyledBox, FlatError> {
        let parts = style.parts(self, rect)?;
        let mut nodes = Vec::with_capacity(parts.len());
        for (part, cmd) in parts {
            let mut n = Node::new(rect, cmd).opacity(style.opacity);
            (n.clip, n.space) = (style.clip, style.space);
            nodes.push((part, self.push(n)?));
        }
        Ok(StyledBox { rect, nodes })
    }
    fn restyle(&mut self, b: &mut StyledBox, rect: [f32; 4], style: &BoxStyle) -> Result<(), Restyle> {
        let parts = style.parts(self, rect).map_err(Restyle::Scene)?;
        let same = parts.len() == b.nodes.len() && parts.iter().zip(&b.nodes).all(|(p, n)| p.0 == n.0 && p.1.kind() == self.node(n.1).cmd.kind());
        if !same {
            return Err(Restyle::Structure);
        }
        for ((_, cmd), &(_, id)) in parts.into_iter().zip(&b.nodes) {
            self.set(id, cmd);
            self.set_rect(id, rect);
            self.set_opacity(id, style.opacity);
            self.set_clip(id, style.clip);
        }
        b.rect = rect;
        Ok(())
    }
    fn transform_box(&mut self, b: &StyledBox, dx: f32, dy: f32, scale: f32) {
        for id in b.ids() {
            self.set_xf(id, dx, dy, scale);
        }
    }
    fn show_box(&mut self, b: &StyledBox, visible: bool) {
        for id in b.ids() {
            self.show(id, visible);
        }
    }
    fn fade_box(&mut self, b: &StyledBox, opacity: f32) {
        for id in b.ids() {
            self.set_opacity(id, opacity);
        }
    }
}

/// How close to its target a spring driving `prop` is at rest: half the smallest step that shows.
pub fn rest_of(prop: Prop) -> f32 {
    match prop {
        Prop::Scale => 1.0 / 8192.0,
        Prop::Opacity => 1.0 / 1024.0,
        Prop::Red | Prop::Green | Prop::Blue | Prop::Alpha => 0.25,
        _ => 1.0 / 64.0,
    }
}

/// How a property gets to its new value.
#[derive(Clone, Debug, PartialEq)]
pub enum AnimSpec {
    /// At once.
    Snap,
    /// A spring from the value **and velocity** the property has now.
    Spring(SpringSpec),
    /// An eased curve from the value the property has now.
    Curve { ease: Ease, timing: Timing },
    /// A keyframe track (its own values: the target is ignored).
    Keyframes { keys: Vec<Key>, timing: Timing },
}
impl AnimSpec {
    pub const fn spring(s: SpringSpec) -> AnimSpec {
        AnimSpec::Spring(s)
    }
    /// A spring by its response (seconds) and damping ratio.
    pub fn response(response: f32, damping_ratio: f32) -> AnimSpec {
        AnimSpec::Spring(SpringSpec::response(response, damping_ratio))
    }
    pub const fn ease(dur_ms: u32, ease: Ease) -> AnimSpec {
        AnimSpec::Curve { ease, timing: Timing::once(dur_ms) }
    }
    pub const fn bezier(dur_ms: u32, x1: f32, y1: f32, x2: f32, y2: f32) -> AnimSpec {
        AnimSpec::Curve { ease: Ease::Bezier(CubicBezier::new(x1, y1, x2, y2)), timing: Timing::once(dur_ms) }
    }
    pub fn keyframes(keys: &[Key], timing: Timing) -> AnimSpec {
        AnimSpec::Keyframes { keys: keys.to_vec(), timing }
    }
    /// The same, starting `ms` later (a stagger). A spring holds its value until then.
    pub fn delay(self, ms: u32) -> Delayed {
        Delayed { spec: self, delay_ms: ms }
    }
}
/// An [`AnimSpec`] that starts later.
#[derive(Clone, Debug, PartialEq)]
pub struct Delayed {
    pub spec: AnimSpec,
    pub delay_ms: u32,
}

/// The properties of a scene's nodes in motion: an animation bank whose tracks are made on demand, one
/// per (node, property) that was ever animated.
#[derive(Default)]
pub struct Motion {
    bank: AnimBank,
    tracks: HashMap<u64, TrackId>,
}
impl Motion {
    pub fn new() -> Motion {
        Motion::default()
    }
    pub fn bank(&self) -> &AnimBank {
        &self.bank
    }
    pub fn bank_mut(&mut self) -> &mut AnimBank {
        &mut self.bank
    }
    /// The track of a node's property (made at the property's present value the first time).
    pub fn track(&mut self, scene: &FlatScene, node: NodeId, prop: Prop) -> TrackId {
        let tag = prop.tag(node);
        *self.tracks.entry(tag).or_insert_with(|| {
            let id = self.bank.track(tag, scene.prop(node, prop));
            self.bank.set_rest(id, rest_of(prop));
            id
        })
    }
    /// Send a node's property to `to`.
    pub fn animate(&mut self, scene: &FlatScene, node: NodeId, prop: Prop, to: f32, spec: AnimSpec, now: ClockNs) -> TrackId {
        self.animate_delayed(scene, node, prop, to, Delayed { spec, delay_ms: 0 }, now)
    }
    pub fn animate_delayed(&mut self, scene: &FlatScene, node: NodeId, prop: Prop, to: f32, d: Delayed, now: ClockNs) -> TrackId {
        let id = self.track(scene, node, prop);
        match d.spec {
            AnimSpec::Snap => self.bank.set(id, to),
            AnimSpec::Spring(s) => self.bank.spring_to_at(id, s, to, now, now.after_ms(d.delay_ms as u64)),
            AnimSpec::Curve { ease, timing } => self.bank.tween_to(id, to, ease, timing.delay(timing.delay_ms + d.delay_ms), now),
            AnimSpec::Keyframes { keys, timing } => self.bank.keyframes(id, &keys, timing.delay(timing.delay_ms + d.delay_ms), now),
        }
        id
    }
    /// The same property of several nodes, each starting `step_ms` after the one before.
    pub fn stagger(&mut self, scene: &FlatScene, nodes: &[NodeId], prop: Prop, to: f32, spec: AnimSpec, step_ms: u32, now: ClockNs) {
        for (i, &n) in nodes.iter().enumerate() {
            self.animate_delayed(scene, n, prop, to, spec.clone().delay(step_ms * i as u32), now);
        }
    }
    /// A whole box: the same motion of the same property on every one of its nodes.
    pub fn animate_box(&mut self, scene: &FlatScene, b: &StyledBox, prop: Prop, to: f32, spec: AnimSpec, now: ClockNs) {
        for id in b.ids() {
            self.animate(scene, id, prop, to, spec.clone(), now);
        }
    }
    /// Bring every property in motion to `now` and write it into `scene`. Returns whether a frame is owed
    /// at the output's next refresh.
    pub fn tick(&mut self, scene: &mut FlatScene, now: ClockNs) -> bool {
        self.bank.tick(now, |tag, v| scene.apply(tag, v))
    }
    pub fn animating(&self) -> bool {
        self.bank.animating()
    }
}
