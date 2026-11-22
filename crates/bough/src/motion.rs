//! Motion without type parameters.
//!
//! A motion is a value ([`AnimSpec`]: a spring, an eased curve, keyframes) sent to one scalar property of a
//! node of a flat scene; [`Motion`] is the animation bank (`bough_paint::motion`) with a track made on demand
//! for every (node, property) that was ever animated. Time is monotone ([`ClockNs`]); a spring that is
//! retargeted keeps the velocity it has.
//!
//! ```ignore
//! let b = scene.add_box([40.0, 40.0, 360.0, 240.0], &card)?;
//! motion.animate(&scene, b.body(), Prop::Dy, -8.0, AnimSpec::spring(SpringSpec::SNAPPY), now);
//! // each frame:
//! let owed = motion.tick(&mut scene, now);
//! ```
use crate::style::{FlatScene, NodeId, Prop, StyledBox};
pub use bough_paint::motion::{AnimBank, ClockNs, CubicBezier, Direction, Ease, Key, Repeat, SpringSpec, Timing, TrackId};
use std::collections::HashMap;

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
