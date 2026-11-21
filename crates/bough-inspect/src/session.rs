//! One inspected application: the state it was built with, the world, the last snapshot, and the commands an
//! inspector or an agent may run against it.
//!
//! The session owns the world and runs on the application's own thread: a world's stores are raw pointers into
//! the stores and must not move across threads, so the socket is pumped from the application's loop rather than
//! serving from another thread (see [`crate::socket`]).
use crate::damage::capture;
use crate::frame::Frame;
use crate::record::Command;
use crate::schema::Damage as DamageSnapshot;
use crate::schema::*;
use bough_ax::{Ax as AxProjection, Role};
use bough_x::event::{Button, Key as EKey, Mods};
use bough_x::expr::{APP_WORDS, U, U_ENV, U_FOCUS, U_PICK, U_PRESS, U_PRESS_AT, U_SCALE, U_TIME};
use bough_x::reflect::{ColInfo, Kind as SKind, Reflect};
use bough_x::sink::View;
use bough_x::store::{Backend, NO_WORD, POINTS, Path, Rect, Store as StoreRef};
use bough_x::world::World;
use std::time::Instant;

/// An input event an agent or a test can inject. The application translates it through its own input path
/// (`World::pointer`, its own wheel model), then the session settles.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    Pointer { x: i32, y: i32 },
    Down { x: i32, y: i32, button: Button },
    Up { x: i32, y: i32, button: Button },
    Click { x: i32, y: i32 },
    Key { key: EKey, mods: Mods },
    Text { text: String },
    Wheel { x: i32, y: i32, dx: f32, dy: f32 },
}

/// A typed state field the application exposes. The application's registration is the authority: `get` reads
/// the real field and `set` writes it through the same typed path a handler would (a lens or a `Signal`).
/// A setter must validate before writing, leave the app unchanged on `Err`, and return `Ok` only if
/// `get` will report the admitted value. Arbitrary application effects cannot be rolled back by a session.
pub struct FieldDef<V> {
    /// Stable field name used by tools and state files.
    pub name: &'static str,
    /// Canonical kind accepted by the setter and returned by the getter.
    pub kind: FieldKind,
    /// The block word it lands in, when it is a block field.
    pub word: Option<u32>,
    /// Read the current typed value without changing the application.
    pub get: fn(&V) -> PropValue,
    /// Apply an admitted value, or return a refusal before mutation. `None` marks a read-only field.
    pub set: Option<fn(&mut V, &PropValue) -> Result<(), Refusal>>,
}
impl<V> Clone for FieldDef<V> {
    fn clone(&self) -> Self {
        FieldDef {
            name: self.name,
            kind: self.kind,
            word: self.word,
            get: self.get,
            set: self.set,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Int,
    Float,
    Bool,
    Color,
    Text,
    Bits,
}
impl FieldKind {
    pub fn name(self) -> &'static str {
        match self {
            FieldKind::Int => "int",
            FieldKind::Float => "float",
            FieldKind::Bool => "bool",
            FieldKind::Color => "color",
            FieldKind::Text => "text",
            FieldKind::Bits => "bits",
        }
    }
    /// Whether a value of this kind may be written to a field of that kind. Int and Float interchange when the
    /// float is integral; Bits accepts any integer.
    pub fn admits(self, v: &PropValue) -> bool {
        self.canonical(v).is_some()
    }

    fn canonical(self, value: &PropValue) -> Option<PropValue> {
        match (self, value) {
            (Self::Int | Self::Bits, PropValue::Float(f))
                if f.fract() == 0.0
                    && (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&f64::from(*f)) =>
            {
                Some(PropValue::Int(*f as i32))
            }
            (Self::Float, PropValue::Int(n)) => Some(PropValue::Float(*n as f32)),
            (Self::Float, PropValue::Float(f)) if f.is_finite() => Some(value.clone()),
            (Self::Int | Self::Bits, PropValue::Int(_))
            | (Self::Bool, PropValue::Bool(_))
            | (Self::Color, PropValue::Color(_))
            | (Self::Text, PropValue::Text(_)) => Some(value.clone()),
            _ => None,
        }
    }
}

/// Why a mutation did not run. Every variant is a refusal *before* anything changed, or after the field's own
/// admission refused it; `code` is stable for a test to assert on.
#[derive(Clone, Debug, PartialEq)]
pub enum Refusal {
    UnknownField(String),
    ReadOnly {
        field: String,
        why: &'static str,
    },
    WrongKind {
        field: String,
        expected: &'static str,
        got: &'static str,
    },
    Rejected {
        field: String,
        why: &'static str,
    },
    UnknownProperty {
        node: usize,
        prop: String,
    },
    ReadOnlyProperty {
        node: usize,
        prop: String,
        why: &'static str,
    },
    ForeignNode,
    NotExposed,
}
impl Refusal {
    pub fn code(&self) -> &'static str {
        match self {
            Refusal::UnknownField(_) => "unknown-field",
            Refusal::ReadOnly { .. } => "read-only",
            Refusal::WrongKind { .. } => "wrong-kind",
            Refusal::Rejected { .. } => "rejected",
            Refusal::UnknownProperty { .. } => "unknown-property",
            Refusal::ReadOnlyProperty { .. } => "read-only-property",
            Refusal::ForeignNode => "foreign-node",
            Refusal::NotExposed => "not-exposed",
        }
    }
    pub fn reason(&self) -> String {
        match self {
            Refusal::UnknownField(f) => format!("no field {f:?} is registered for inspection"),
            Refusal::ReadOnly { field, why } => format!("field {field:?} is read-only: {why}"),
            Refusal::WrongKind {
                field,
                expected,
                got,
            } => format!("field {field:?} takes {expected}, not {got}"),
            Refusal::Rejected { field, why } => format!("field {field:?} refused the value: {why}"),
            Refusal::UnknownProperty { node, prop } => {
                format!("node {node} has no operand {prop:?}")
            }
            Refusal::ReadOnlyProperty { node, prop, why } => {
                format!("operand {prop:?} of node {node} is read-only: {why}")
            }
            Refusal::ForeignNode => {
                "the node does not belong to this session's last snapshot".to_owned()
            }
            Refusal::NotExposed => {
                "the entity is not materialised in the current window".to_owned()
            }
        }
    }
}

/// What one command did. `damage_tiles` is the marked tile count after the command's commit (0: nothing moved).
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub command: String,
    pub revision: u32,
    pub damage_tiles: u32,
    pub digest: u64,
}

/// The application under inspection: what the session needs from it, and nothing about its UI.
pub trait Program {
    type V: View + 'static;
    type B: Backend;
    fn name(&self) -> &'static str;
    fn view(&self) -> &Self::V;
    fn view_mut(&mut self) -> &mut Self::V;
    fn world(&self) -> &World<Self::V, Self::B>;
    fn world_mut(&mut self) -> &mut World<Self::V, Self::B>;
    /// The application's own after-turn policy: the block compare (and a re-description when it is opted into).
    fn settle(&mut self);
    fn size(&self) -> (i32, i32);
    /// Paint a whole frame with the application's own raster.
    fn shot(&mut self) -> Frame;
    fn resize(&mut self, w: i32, h: i32, scale: f32);
    /// Inject one event through the application's own input path.
    fn input(&mut self, e: &Input);
    /// Advance the clock. The default is the world's frame tick.
    fn advance(&mut self, ms: u32) {
        self.world_mut().tick(ms);
    }
    fn fields(&self) -> &'static [FieldDef<Self::V>] {
        &[]
    }
    /// Re-read whatever the application keeps outside its process (assets, a theme file, configuration) and
    /// re-describe what depends on it. `false`: this program has nothing to reload.
    fn reload(&mut self) -> bool {
        false
    }
    /// A fresh application with the same configuration, for replay. `None`: this program cannot be rebuilt
    /// (a windowed app whose loop owns the only instance).
    fn fresh(&self) -> Option<Self>
    where
        Self: Sized,
    {
        None
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct CacheKey {
    revision: u32,
    shape: u64,
    input: u64,
    shown: U,
    damage: u32,
}

/// Owns one program and its derived inspector state on the application thread.
pub struct Session<P: Program> {
    app: P,
    refl: Option<Reflect<P::V, P::B>>,
    ax: AxProjection<P::V, P::B>,
    cached: Option<Snapshot>,
    key: Option<CacheKey>,
    last_damage: DamageSnapshot,
    pointer: Option<(i32, i32)>,
    commands: u64,
    frames: u64,
    last_command: String,
    last_settle_us: u64,
    last_paint_us: u64,
    /// A recording session keeps the commands it ran, to replay them on a fresh app.
    log: Option<crate::record::Log>,
    log_overflow: bool,
    /// The block as of the last command (to name what moved) and the words that moved then.
    last_shown: U,
    last_change: u64,
    /// The world the reflection and the accessibility projection were learned from. A host that replaces its
    /// world (a resize in the benchmark adapter) starts both again.
    world: Option<bough_x::world::WorldId>,
}

impl<P: Program> Session<P> {
    /// Start inspecting a mounted program.
    pub fn new(app: P) -> Session<P> {
        let last_shown = app.world().shown;
        let last_damage = capture(&app.world().tiles, &app.world().shown);
        let world = Some(app.world().id());
        Session {
            app,
            refl: None,
            ax: AxProjection::new(),
            cached: None,
            key: None,
            last_damage,
            pointer: None,
            commands: 0,
            frames: 0,
            last_command: String::new(),
            last_settle_us: 0,
            last_paint_us: 0,
            log: None,
            log_overflow: false,
            last_shown,
            last_change: 0,
            world,
        }
    }
    pub fn app(&self) -> &P {
        &self.app
    }
    pub fn app_mut(&mut self) -> &mut P {
        &mut self.app
    }
    pub fn fields(&self) -> &'static [FieldDef<P::V>] {
        self.app.fields()
    }
    /// The last snapshot, rebuilt only when the world, the block or the damage moved.
    pub fn snapshot(&mut self) -> &Snapshot {
        let world = self.app.world();
        let key = CacheKey {
            revision: world.revision(),
            shape: world.root().structure_revision(),
            input: world.input_revision(),
            shown: world.shown,
            damage: self.last_damage.tiles,
        };
        let reuse = !cfg!(feature = "mutant_inspector_cache")
            && self.key == Some(key)
            && self.cached.is_some();
        if !reuse {
            let snap = self.build();
            self.cached = Some(snap);
            self.key = Some(key);
        }
        self.cached.as_ref().expect("a snapshot was just built")
    }
    /// Drop the cached snapshot (after a command that changed something the key does not name).
    pub fn invalidate(&mut self) {
        self.key = None;
    }

    /// Commit after state was written: the app's settle, then the damage that commit produced is captured before
    /// any painter can consume it.
    pub fn commit(&mut self) -> u32 {
        let t = Instant::now();
        self.app.settle();
        self.last_settle_us = t.elapsed().as_micros() as u64;
        self.capture();
        self.key = None;
        self.last_damage.tiles
    }
    fn capture(&mut self) {
        let world = self.app.world();
        self.last_damage = capture(&world.tiles, &world.shown);
        self.last_change = self.last_shown.diff(&world.shown, 64);
        self.last_shown = world.shown;
        self.commands += 1;
    }
    /// Paint a frame and consume its damage, as a host does after presenting.
    pub fn shot(&mut self) -> Frame {
        let t = Instant::now();
        let f = self.app.shot();
        self.last_paint_us = t.elapsed().as_micros() as u64;
        self.frames += 1;
        let world = self.app.world_mut();
        world.tiles.clear();
        self.key = None;
        f
    }
    /// The damage of the last commit, before the last paint consumed it.
    pub fn damage(&self) -> &DamageSnapshot {
        &self.last_damage
    }

    /// Run one command, then settle (when it changes state). Returns what it did.
    pub fn run(&mut self, c: &Command) -> Outcome {
        if let Some(e) = c.input() {
            if let Input::Pointer { x, y }
            | Input::Down { x, y, .. }
            | Input::Up { x, y, .. }
            | Input::Click { x, y }
            | Input::Wheel { x, y, .. } = e
            {
                self.pointer = Some((x, y));
            }
            self.app.input(&e);
        }
        match c {
            Command::Tick { ms } => {
                self.app.advance(*ms);
                self.capture();
                self.key = None;
            }
            Command::Resize { w, h, scale } => {
                self.app.resize(*w, *h, *scale);
                self.capture();
                self.key = None;
            }
            Command::Set { field, value } => {
                // the refusal is returned by `set`; a command log records it and replay asserts it the same way
                let _ = self.set(field, value);
            }
            Command::Settle => {
                self.commit();
            }
            _ => {
                self.commit();
            }
        }
        self.last_command = c.name().to_owned();
        let outcome = Outcome {
            command: c.name().to_owned(),
            revision: self.app.world().revision(),
            damage_tiles: self.last_damage.tiles,
            digest: self.snapshot().digest(),
        };
        if !matches!(c, Command::Set { .. }) {
            self.observe_command(|| c.clone(), None);
        }
        outcome
    }

    /// Write a registered state field through its typed setter, commit, and check that the write landed.
    pub fn set(&mut self, field: &str, value: &PropValue) -> Result<Outcome, Refusal> {
        let result = self.set_inner(field, value);
        self.observe_command(
            || Command::Set {
                field: field.to_owned(),
                value: value.clone(),
            },
            result.as_ref().err(),
        );
        result
    }

    fn set_inner(&mut self, field: &str, value: &PropValue) -> Result<Outcome, Refusal> {
        let def = self
            .app
            .fields()
            .iter()
            .find(|f| f.name == field)
            .ok_or_else(|| Refusal::UnknownField(field.to_owned()))?;
        let set = match def.set {
            Some(s) => s,
            None => {
                return Err(Refusal::ReadOnly {
                    field: field.to_owned(),
                    why: "a derived value: its sources are the writable fields",
                });
            }
        };
        let Some(expected) = def.kind.canonical(value) else {
            return Err(Refusal::WrongKind {
                field: field.to_owned(),
                expected: def.kind.name(),
                got: value.kind(),
            });
        };
        #[cfg(feature = "mutant_accept")]
        {
            // the negative control of G5: the typed admission is bypassed and an invalid write is accepted
            if let Some(f) = def.set {
                let _ = f(self.app.view_mut(), value);
                self.commit();
                return Ok(Outcome {
                    command: "set".to_owned(),
                    revision: self.app.world().revision(),
                    damage_tiles: self.last_damage.tiles,
                    digest: self.snapshot().digest(),
                });
            }
        }
        let get = def.get;
        let before = get(self.app.view());
        if before == expected {
            return Err(Refusal::Rejected {
                field: field.to_owned(),
                why: "the field already has that value",
            });
        }
        set(self.app.view_mut(), &expected).map_err(|e| match e {
            Refusal::Rejected { why, .. } => Refusal::Rejected {
                field: field.to_owned(),
                why,
            },
            other => other,
        })?;
        self.last_command = "set".to_owned();
        self.commit();
        let after = get(self.app.view());
        if after != expected {
            return Err(Refusal::Rejected {
                field: field.to_owned(),
                why: "the view did not read back the value that was written",
            });
        }
        let changed = before != after;
        Ok(Outcome {
            command: format!("set {field}"),
            revision: self.app.world().revision(),
            damage_tiles: if changed { self.last_damage.tiles } else { 0 },
            digest: self.snapshot().digest(),
        })
    }

    /// The state fields, read now.
    pub fn state(&self) -> Vec<(&'static str, PropValue)> {
        self.app
            .fields()
            .iter()
            .map(|f| (f.name, (f.get)(self.app.view())))
            .collect()
    }
    /// Restore a state snapshot (the dev server's restart fallback), returning the refusals.
    pub fn restore(&mut self, state: &[(&'static str, PropValue)]) -> Vec<Refusal> {
        let mut out = Vec::new();
        for (name, v) in state {
            if let Err(e) = self.set(name, v) {
                out.push(e);
            }
        }
        out
    }

    /// Every live record under device point (x, y), topmost first, as `(node index, key)`.
    pub fn stack(&mut self, x: i32, y: i32) -> Vec<(usize, Key)> {
        let snap = self.snapshot();
        let p = Rect {
            x0: x,
            y0: y,
            x1: x.saturating_add(1),
            y1: y.saturating_add(1),
        };
        let mut v: Vec<(usize, Key)> = snap
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| {
                let r = Rect {
                    x0: n.screen.x0.max(n.clip.x0),
                    y0: n.screen.y0.max(n.clip.y0),
                    x1: n.screen.x1.min(n.clip.x1),
                    y1: n.screen.y1.min(n.clip.y1),
                };
                r.hits(p)
            })
            .map(|(i, n)| (i, n.key))
            .collect();
        v.reverse();
        v
    }
    /// Replay a log against a fresh instance of this program, when it can make one.
    pub fn replay(&self, log: &crate::record::Log) -> Option<crate::record::Replay> {
        let fresh = self.app.fresh()?;
        Some(crate::record::replay(log, || fresh))
    }

    /// Ask the application to re-read its own inputs, then commit. The dev server's hot path for a change that
    /// is not Rust code.
    pub fn reload(&mut self) -> bool {
        let ok = self.app.reload();
        self.last_command = "reload".to_owned();
        self.commit();
        ok
    }

    /// Capture a replayable log from a pristine application instance.
    pub fn start_recording(&mut self) -> Result<(), &'static str> {
        if self.log.is_some() {
            return Err("a recording is already active");
        }
        if self.commands != 0 || self.app.fresh().is_none() {
            return Err("recording requires a fresh, replayable application");
        }
        self.log_overflow = false;
        self.log = Some(crate::record::Log::new(self));
        Ok(())
    }
    /// Whether the session is capturing turns.
    pub fn recording(&self) -> bool {
        self.log.is_some()
    }
    /// Finish and return the complete replay log.
    pub fn stop_recording(&mut self) -> Result<crate::record::Log, &'static str> {
        let log = self.log.take().ok_or("no recording is active")?;
        if self.log_overflow {
            return Err("recording exceeded the step limit");
        }
        Ok(log)
    }
    fn observe_command(&mut self, command: impl FnOnce() -> Command, refusal: Option<&Refusal>) {
        if let Some(mut log) = self.log.take() {
            if log.steps.len() < crate::record::MAX_LOG_STEPS {
                log.steps
                    .push(crate::record::observe(self, command(), refusal));
            } else {
                self.log_overflow = true;
            }
            self.log = Some(log);
        }
    }
    /// Return current turn counters, retained sizes and the most recent CPU timing samples.
    pub fn counters(&mut self) -> Counters {
        let world = self.app.world();
        let (records, pages, bytes) = world.root().stats();
        let d = world.delta_stats;
        Counters {
            visited_incidences: d.visited_incidences,
            evaluated_columns: d.evaluated_columns,
            dense_fallbacks: d.dense_fallbacks,
            retables: world.retables,
            revision: world.revision(),
            records,
            pages,
            bytes,
            commands: self.commands,
            frames: self.frames,
            last_command: self.last_command.clone(),
            last_settle_us: self.last_settle_us,
            last_paint_us: self.last_paint_us,
            changed: bits(self.last_change),
        }
    }
    /// The accessibility projection up to date with the world. Safe to call more than once: the projection
    /// compares and does nothing when nothing moved.
    pub fn ax(&mut self) -> &bough_ax::Tree {
        let app = &mut self.app;
        self.ax.update(app.world(), app.view());
        self.ax.tree()
    }

    // ---- the snapshot ------------------------------------------------------------------------------------

    fn build(&mut self) -> Snapshot {
        let app = &self.app;
        let world = app.world();
        let view = app.view();
        let shown = &world.shown;
        let root = world.root();
        if self.world != Some(world.id()) {
            // a host replaced its world (a resize): both projections were learned from the old one
            self.world = Some(world.id());
            self.refl = None;
            self.ax = AxProjection::new();
        }
        if !self.refl.as_ref().is_some_and(|r| r.current(root)) {
            self.refl = Some(Reflect::learn(world, view));
        }
        let refl = self.refl.as_ref().expect("reflection learned above");
        let mut snap = Snapshot::new();
        snap.app = AppInfo {
            name: app.name().to_owned(),
            view: core::any::type_name::<P::V>(),
            world: {
                // the world id's inner value, for a stable name across commands
                let id = world.id();
                // `WorldId` has no accessor: its debug form is stable for the world's lifetime
                format!("{id:?}")
                    .trim_start_matches("WorldId(")
                    .trim_end_matches(')')
                    .parse()
                    .unwrap_or(0)
            },
            w: app.size().0,
            h: app.size().1,
            scale: f32::from_bits(shown.w[U_SCALE]),
            revision: world.revision(),
            input_revision: world.input_revision(),
            shape: root.structure_revision(),
        };
        // stores
        let mut stores = Vec::new();
        let mut index = Vec::new();
        let mut path = Path::default();
        walk_store(root, &refl.root, &mut path, &mut stores, &mut index, shown);
        snap.stores = stores;
        // lists
        snap.lists = snap_lists(root, refl, &mut index);
        // nodes
        snap.nodes = nodes_of(refl, root, shown, &index);
        // words and the columns that read them
        let mut readers = [0u32; 64];
        for s in &snap.stores {
            for c in &s.cols {
                for &w in &c.reads {
                    if (w as usize) < 64 {
                        readers[w as usize] += 1;
                    }
                }
            }
        }
        let mut fields = Vec::new();
        for f in app.fields() {
            let value = (f.get)(view);
            fields.push(Field {
                name: f.name,
                kind: f.kind.name(),
                value,
                word: f.word,
                writable: f.set.is_some(),
            });
        }
        snap.fields = fields;
        snap.words = words(shown, &readers, &snap.fields);
        // pointers: the places the world holds, named by the nodes of this snapshot
        let pick = place_key(world, &index, &snap.nodes, shown.w[U_PICK]);
        let press = place_key(world, &index, &snap.nodes, shown.w[U_PRESS]);
        let focus = place_key(world, &index, &snap.nodes, shown.w[U_FOCUS]);
        snap.pointers = Pointers {
            pointer: self.pointer,
            pick,
            press,
            focus,
        };
        snap.damage = self.last_damage.clone();
        snap.counters = {
            let (records, pages, bytes) = root.stats();
            let d = world.delta_stats;
            Counters {
                visited_incidences: d.visited_incidences,
                evaluated_columns: d.evaluated_columns,
                dense_fallbacks: d.dense_fallbacks,
                retables: world.retables,
                revision: world.revision(),
                records,
                pages,
                bytes,
                commands: self.commands,
                frames: self.frames,
                last_command: self.last_command.clone(),
                last_settle_us: self.last_settle_us,
                last_paint_us: self.last_paint_us,
                changed: bits(self.last_change),
            }
        };
        // accessibility (its own projection, kept and deltaed)
        self.ax.update(world, view);
        let tree = self.ax.tree();
        snap.ax = Some(ax_snapshot(tree, self.ax.projected));
        snap
    }
}

fn place_key(
    world: &World<impl View, impl Backend>,
    index: &[(*const StoreRef, usize)],
    nodes: &[Node],
    place: u32,
) -> Option<Key> {
    if place == bough_x::event::NONE {
        return None;
    }
    let (st, col, rec) = world.at_place(place)?;
    let store = index
        .iter()
        .find(|&&(s, _)| core::ptr::eq(s, st))
        .map(|&(_, i)| i)?;
    nodes
        .iter()
        .find(|n| n.store == store && n.key.col == col && n.key.rec == rec)
        .map(|n| n.key)
}

fn snap_lists(
    root: &StoreRef,
    refl: &Reflect<impl View, impl Backend>,
    index: &mut [(*const StoreRef, usize)],
) -> Vec<List> {
    let mut out = Vec::new();
    for (path, info, loc) in refl.lists() {
        let Some(st) = root.at(&path) else { continue };
        let store = index
            .iter()
            .find(|&&(s, _)| core::ptr::eq(s, st))
            .map(|&(_, i)| i)
            .unwrap_or(0);
        let scroll = (st.space.scroll != NO_WORD).then(|| (st.space.scroll & 63) as u32);
        out.push(List {
            store,
            path: path.at,
            depth: path.n,
            loc: Some(Loc::of(loc)),
            n: info.n,
            row_h: info.row_h,
            x: info.x,
            y: info.y,
            shape: info.shape.iter().map(|&r| R::of(r)).collect(),
            lo: st.lo,
            hi: st.hi,
            cap: st.cap,
            seen: st.seen,
            extent: st.geo.1 as i64 + st.n as i64 * st.geo.2 as i64,
            scroll,
        });
    }
    out
}

/// Walk the store tree in pre-order, keeping the reflection's description of each store.
fn walk_store(
    st: &StoreRef,
    info: &bough_x::reflect::StoreInfo,
    path: &mut Path,
    out: &mut Vec<Store>,
    index: &mut Vec<(*const StoreRef, usize)>,
    shown: &U,
) {
    let me = out.len();
    index.push((st, me));
    let (kind, loc, list) = match &info.kind {
        SKind::Root => ("root", info.loc, None),
        SKind::Scope => ("scope", info.loc, None),
        SKind::List(l) => ("list", info.loc, Some(l.clone())),
    };
    let _ = list;
    let (records, pages, bytes) = own_stats(st);
    let mut cols = Vec::new();
    for (j, c) in info.cols.iter().enumerate() {
        let Some(c) = c else { continue };
        cols.push(col_of(st, j as u32, c));
    }
    let mut subs = Vec::new();
    out.push(Store {
        index: me,
        path: path.at,
        depth: path.n,
        kind,
        loc: loc.map(Loc::of),
        clip: R::of(st.space.clip),
        origin: st.space.origin(shown),
        scroll: (st.space.scroll != NO_WORD).then(|| (st.space.scroll & 63) as u32),
        cap: st.cap,
        lo: st.lo,
        hi: st.hi,
        n: st.n,
        geo: st.src.is_some().then_some(st.geo),
        seen: st.seen,
        moved: st
            .moved
            .iter()
            .map(|e| match *e {
                bough_x::sink::Edit::Insert(at) => ("insert", at),
                bough_x::sink::Edit::Remove(at) => ("remove", at),
                bough_x::sink::Edit::Update(at) => ("update", at),
                bough_x::sink::Edit::Shift(at) => ("shift", at),
            })
            .collect(),
        moved_ok: st.moved_ok,
        cols,
        records,
        pages,
        bytes,
        subs: Vec::new(),
    });
    for (q, (s, i)) in st.subs.iter().zip(&info.subs).enumerate() {
        path.at[path.n as usize] = q as u16;
        path.n += 1;
        walk_store(s, i, path, out, index, shown);
        path.n -= 1;
        subs.push(
            index
                .iter()
                .find(|&&(p, _)| core::ptr::eq(p, s))
                .map_or(0, |&(_, i)| i),
        );
        let _ = i;
    }
    out[me].subs = subs;
}

fn own_stats(st: &StoreRef) -> (usize, usize, usize) {
    let mut t = (0, 0, 0);
    for c in &st.cols {
        let live = bough_x::store::Live {
            cap: st.cap,
            rows: &st.row_live,
            nrec: c.nrec,
        };
        t.0 += (0..c.npages as usize)
            .map(|p| live.page(p).count_ones() as usize)
            .sum::<usize>();
        t.1 += c.npages as usize;
        t.2 += c.npages as usize * c.desc.page.size();
    }
    t
}

fn col_of(st: &StoreRef, j: u32, c: &ColInfo) -> Col {
    let raw = &st.cols[j as usize];
    let live = bough_x::store::Live {
        cap: st.cap,
        rows: &st.row_live,
        nrec: raw.nrec,
    };
    let records = (0..raw.npages as usize)
        .map(|p| live.page(p).count_ones() as usize)
        .sum();
    Col {
        index: j,
        kind: c.kind,
        ty: c.ty,
        stage: stage_name(c.stage),
        reads: bits(c.reads),
        moves: c.moves,
        timed: raw.desc.timed,
        npages: raw.npages,
        nrec: raw.nrec,
        sites: c.sites.clone(),
        records,
        bytes: raw.npages as usize * raw.desc.page.size(),
    }
}

fn nodes_of(
    refl: &Reflect<impl View, impl Backend>,
    root: &StoreRef,
    shown: &U,
    index: &[(*const StoreRef, usize)],
) -> Vec<Node> {
    let mut out = Vec::new();
    for it in refl.items(root, shown) {
        let Some(&(_, store)) = index.iter().find(|&&(p, _)| {
            // the item's path names a store; find it by walking the root's tree
            let mut st = root;
            for &q in &it.ent.path.at[..it.ent.path.n as usize] {
                st = &st.subs[q as usize];
            }
            core::ptr::eq(p, st)
        }) else {
            continue;
        };
        let handlers: Vec<&'static str> = it.info.on.map_or_else(Vec::new, |o| {
            let mut v = Vec::new();
            for (bit, name) in [
                (bough_x::event::kind::HOVER, "hover"),
                (bough_x::event::kind::CLICK, "click"),
                (bough_x::event::kind::PRESS, "press"),
                (bough_x::event::kind::DRAG, "drag"),
                (bough_x::event::kind::WHEEL, "wheel"),
                (bough_x::event::kind::KEY, "key"),
                (bough_x::event::kind::TEXT, "text"),
                (bough_x::event::kind::FOCUS, "focus"),
                (bough_x::event::kind::FOCUSABLE, "focusable"),
            ] {
                if o.kinds & bit != 0 {
                    v.push(name);
                }
            }
            v
        });
        let ops = it
            .ops()
            .map(|o| Prop {
                name: o.name(),
                word: o.op.word as u32,
                words: o.op.words as u32,
                value: PropValue::of(o.val),
                stage: stage_name(o.stage()),
                expr: o.ty(),
                reads: bits(o.op.reads),
            })
            .collect();
        let off = {
            // the paint's own offset, as `snap` reports it
            let s = it.snap();
            s.off
        };
        out.push(Node {
            key: Key::of(it.ent, it.col, it.rec),
            store,
            kind: it.kind(),
            ty: it.info.ty,
            depth: it.ent.path.n,
            loc: node_loc(it.location()),
            rect: R::of(it.rect),
            screen: R::of(it.screen),
            clip: R::of(it.clip),
            visible: it.visible(),
            moving: it.info.moves,
            offset: off,
            handlers,
            handler_ty: it.info.on.map(|o| (o.ty)()),
            ops,
            reads: bits(it.info.reads),
            text: it.text(),
        });
    }
    out
}

fn node_loc(l: &'static core::panic::Location<'static>) -> Loc {
    #[cfg(feature = "mutant_drop_site")]
    {
        // the negative control of G2: every record claims the same empty site
        let _ = l;
        Loc {
            file: "",
            line: 0,
            col: 0,
        }
    }
    #[cfg(not(feature = "mutant_drop_site"))]
    Loc::of(l)
}

fn bits(m: u64) -> Vec<u32> {
    (0..64)
        .filter(|w| m >> w & 1 != 0)
        .map(|w| w as u32)
        .collect()
}

const CORE_NAMES: [(u32, &str); 9] = [
    (U_SCALE as u32, "scale"),
    (U_FOCUS as u32, "focus"),
    (U_PRESS as u32, "press"),
    (U_ENV as u32, "env"),
    (U_PRESS_AT as u32, "press_at"),
    (60, "ptr_y"),
    (61, "ptr_x"),
    (U_PICK as u32, "pick"),
    (U_TIME as u32, "time"),
];

fn words(shown: &U, readers: &[u32; 64], fields: &[Field]) -> Vec<Word> {
    let mut out = Vec::new();
    for w in 0..64u32 {
        if w >= APP_WORDS as u32
            && readers[w as usize] == 0
            && !CORE_NAMES.iter().any(|&(c, _)| c == w)
        {
            continue;
        }
        let field = fields.iter().find(|f| f.word == Some(w));
        let core = CORE_NAMES.iter().find(|&&(c, _)| c == w);
        out.push(Word {
            word: w,
            name: field.map(|f| f.name).or(core.map(|&(_, n)| n)),
            value: shown.w[w as usize],
            kind: field.map_or(if core.is_some() { "core" } else { "bits" }, |f| f.kind),
            readers: readers[w as usize],
        });
    }
    out
}

fn role_name(r: Role) -> &'static str {
    match r {
        Role::Window => "window",
        Role::Group => "group",
        Role::ScrollView => "scroll-view",
        Role::List => "list",
        Role::ListItem => "list-item",
        Role::Grid => "grid",
        Role::Row => "row",
        Role::Cell => "cell",
        Role::ColumnHeader => "column-header",
        Role::Button => "button",
        Role::TextInput => "text-input",
        Role::StaticText => "static-text",
        Role::Splitter => "splitter",
    }
}

fn ax_snapshot(tree: &bough_ax::Tree, projected: usize) -> Ax {
    let order: Vec<bough_ax::NodeId> = tree.nodes.keys().copied().collect();
    let pos = |id: &bough_ax::NodeId| order.iter().position(|x| x == id);
    let mut nodes = Vec::new();
    for id in &order {
        let n = &tree.nodes[id];
        let children = n.children.iter().filter_map(&pos).collect();
        let parent = order.iter().find_map(|p| {
            tree.nodes
                .get(p)
                .filter(|p| p.children.contains(id))
                .and_then(|_| pos(p))
        });
        let mut actions = Vec::new();
        for (bit, name) in [
            (bough_ax::act::CLICK, "click"),
            (bough_ax::act::FOCUS, "focus"),
            (bough_ax::act::SET_VALUE, "set-value"),
            (bough_ax::act::SCROLL, "scroll"),
        ] {
            if n.actions & bit != 0 {
                actions.push(name);
            }
        }
        nodes.push(AxNode {
            role: role_name(n.role),
            name: n.name.clone(),
            value: n.value.clone(),
            bounds: R::of(n.bounds),
            children,
            parent,
            focusable: n.focusable,
            focused: n.focused,
            selected: n.selected,
            row_index: n.row_index,
            row_count: n.row_count,
            col_index: n.col_index,
            col_count: n.col_count,
            live: n.live != bough_ax::Live::Off,
            actions,
            loc: n.loc.map(Loc::of),
            place: n.place,
        });
    }
    Ax {
        root: tree.root.and_then(|r| pos(&r)),
        focus: tree.focus.and_then(|f| pos(&f)),
        nodes,
        projected,
    }
}

/// A `Rect` as the schema writes it (for tests and the overlay).
pub fn r_of(r: Rect) -> R {
    R::of(r)
}

/// Whether a scroll word is in points (the app's own offset) rather than device pixels.
pub fn scroll_points(st: &StoreRef) -> bool {
    st.space.scroll & POINTS != 0
}
