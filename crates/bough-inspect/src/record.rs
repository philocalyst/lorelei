//! Turns as data: a log of the commands a session ran, with what each one produced, and a replay that must
//! produce the same descriptions and the same pixels on a fresh application.
//!
//! `settle` is not recorded separately: every command commits through the application's own settle, exactly as a
//! host does. `tick` is a frame, not a turn (nothing is compared).
use crate::frame::Frame;
use crate::json::{self, Value as J};
use crate::schema::PropValue;
use crate::session::{Input, Outcome, Program, Refusal, Session};
use bough_x::event::{Button, Key, Mods};

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Pointer { x: i32, y: i32 },
    Down { x: i32, y: i32, button: Button },
    Up { x: i32, y: i32, button: Button },
    Click { x: i32, y: i32 },
    Key { key: Key, mods: Mods },
    Text { text: String },
    Wheel { x: i32, y: i32, dx: f32, dy: f32 },
    Tick { ms: u32 },
    Resize { w: i32, h: i32, scale: f32 },
    Set { field: String, value: PropValue },
    Settle,
}
impl Command {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        let coordinate = |v: i32| (-1_000_000..=1_000_000).contains(&v);
        match self {
            Self::Resize { w, h, scale }
                if !(1..=4096).contains(w)
                    || !(1..=4096).contains(h)
                    || i64::from(*w) * i64::from(*h) > 8_388_608
                    || !(1.0..=4.0).contains(scale) =>
            {
                Err("resize exceeds inspector size or scale limits")
            }
            Self::Pointer { x, y }
            | Self::Down { x, y, .. }
            | Self::Up { x, y, .. }
            | Self::Click { x, y }
            | Self::Wheel { x, y, .. }
                if !coordinate(*x) || !coordinate(*y) =>
            {
                Err("input coordinates outside inspector limits")
            }
            Self::Wheel { dx, dy, .. }
                if dx.abs() > 10_000.0
                    || dy.abs() > 10_000.0
                    || !dx.is_finite()
                    || !dy.is_finite() =>
            {
                Err("invalid wheel delta")
            }
            Self::Text { text } if text.len() > 64 * 1024 => Err("text exceeds inspector limit"),
            _ => Ok(()),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Command::Pointer { .. } => "pointer",
            Command::Down { .. } => "down",
            Command::Up { .. } => "up",
            Command::Click { .. } => "click",
            Command::Key { .. } => "key",
            Command::Text { .. } => "text",
            Command::Wheel { .. } => "wheel",
            Command::Tick { .. } => "tick",
            Command::Resize { .. } => "resize",
            Command::Set { .. } => "set",
            Command::Settle => "settle",
        }
    }
    pub fn input(&self) -> Option<Input> {
        Some(match *self {
            Command::Pointer { x, y } => Input::Pointer { x, y },
            Command::Down { x, y, button } => Input::Down { x, y, button },
            Command::Up { x, y, button } => Input::Up { x, y, button },
            Command::Click { x, y } => Input::Click { x, y },
            Command::Key { key, mods } => Input::Key { key, mods },
            Command::Text { ref text } => Input::Text { text: text.clone() },
            Command::Wheel { x, y, dx, dy } => Input::Wheel { x, y, dx, dy },
            Command::Tick { .. }
            | Command::Resize { .. }
            | Command::Set { .. }
            | Command::Settle => {
                return None;
            }
        })
    }
    pub fn to_json(&self) -> J {
        let o = match self {
            Command::Pointer { x, y } => J::obj().set("cmd", "pointer").set("x", *x).set("y", *y),
            Command::Down { x, y, button } => J::obj()
                .set("cmd", "down")
                .set("x", *x)
                .set("y", *y)
                .set("button", button_name(*button)),
            Command::Up { x, y, button } => J::obj()
                .set("cmd", "up")
                .set("x", *x)
                .set("y", *y)
                .set("button", button_name(*button)),
            Command::Click { x, y } => J::obj().set("cmd", "click").set("x", *x).set("y", *y),
            Command::Key { key, mods } => J::obj()
                .set("cmd", "key")
                .set("key", key_name(*key))
                .set("shift", mods.shift)
                .set("ctrl", mods.ctrl)
                .set("alt", mods.alt)
                .set("logo", mods.cmd),
            Command::Text { text } => J::obj().set("cmd", "text").set("text", text.as_str()),
            Command::Wheel { x, y, dx, dy } => J::obj()
                .set("cmd", "wheel")
                .set("x", *x)
                .set("y", *y)
                .set("dx", *dx)
                .set("dy", *dy),
            Command::Tick { ms } => J::obj().set("cmd", "tick").set("ms", *ms),
            Command::Resize { w, h, scale } => J::obj()
                .set("cmd", "resize")
                .set("w", *w)
                .set("h", *h)
                .set("scale", *scale),
            Command::Set { field, value } => J::obj()
                .set("cmd", "set")
                .set("field", field.as_str())
                .set("value", value.to_json()),
            Command::Settle => J::obj().set("cmd", "settle"),
        };
        o
    }
    pub fn from_json(v: &J) -> Result<Command, String> {
        let cmd = v
            .get("cmd")
            .and_then(J::as_str)
            .ok_or("a command needs a name")?;
        let i = |k: &str| {
            v.get(k)
                .and_then(J::as_i64)
                .and_then(|x| i32::try_from(x).ok())
        };
        let f = |k: &str| v.get(k).and_then(J::as_f32);
        let b = |k: &str| v.get(k).and_then(J::as_bool).unwrap_or(false);
        Ok(match cmd {
            "pointer" => Command::Pointer {
                x: i("x").ok_or("x")?,
                y: i("y").ok_or("y")?,
            },
            "down" => Command::Down {
                x: i("x").ok_or("x")?,
                y: i("y").ok_or("y")?,
                button: button_of(v.get("button").and_then(J::as_str)).ok_or("button")?,
            },
            "up" => Command::Up {
                x: i("x").ok_or("x")?,
                y: i("y").ok_or("y")?,
                button: button_of(v.get("button").and_then(J::as_str)).ok_or("button")?,
            },
            "click" => Command::Click {
                x: i("x").ok_or("x")?,
                y: i("y").ok_or("y")?,
            },
            "key" => Command::Key {
                key: key_of(v.get("key").and_then(J::as_str)).ok_or("key")?,
                mods: Mods {
                    shift: b("shift"),
                    ctrl: b("ctrl"),
                    alt: b("alt"),
                    cmd: b("logo"),
                },
            },
            "text" => Command::Text {
                text: v.get("text").and_then(J::as_str).ok_or("text")?.to_owned(),
            },
            "wheel" => Command::Wheel {
                x: i("x").ok_or("x")?,
                y: i("y").ok_or("y")?,
                dx: f("dx").ok_or("dx")?,
                dy: f("dy").ok_or("dy")?,
            },
            "tick" => Command::Tick {
                ms: v.get("ms").and_then(J::as_u32).ok_or("ms")?,
            },
            "resize" => Command::Resize {
                w: i("w").ok_or("w")?,
                h: i("h").ok_or("h")?,
                scale: f("scale").unwrap_or(1.0),
            },
            "set" => Command::Set {
                field: v
                    .get("field")
                    .and_then(J::as_str)
                    .ok_or("field")?
                    .to_owned(),
                value: PropValue::from_json(v.get("value").ok_or("value")?)?,
            },
            "settle" => Command::Settle,
            other => return Err(format!("unknown command {other:?}")),
        })
    }
}

pub fn button_name(b: Button) -> &'static str {
    match b {
        Button::Primary => "primary",
        Button::Secondary => "secondary",
        Button::Middle => "middle",
    }
}
pub fn button_of(s: Option<&str>) -> Option<Button> {
    Some(match s? {
        "primary" | "left" => Button::Primary,
        "secondary" | "right" => Button::Secondary,
        "middle" => Button::Middle,
        _ => return None,
    })
}
pub fn key_name(k: Key) -> String {
    match k {
        Key::Char(c) => format!("char:{c}"),
        Key::Tab => "tab".into(),
        Key::Backspace => "backspace".into(),
        Key::Delete => "delete".into(),
        Key::Enter => "enter".into(),
        Key::Escape => "escape".into(),
        Key::Left => "left".into(),
        Key::Right => "right".into(),
        Key::Up => "up".into(),
        Key::Down => "down".into(),
        Key::Home => "home".into(),
        Key::End => "end".into(),
        Key::PageUp => "page-up".into(),
        Key::PageDown => "page-down".into(),
    }
}
pub fn key_of(s: Option<&str>) -> Option<Key> {
    let s = s?;
    Some(match s {
        "tab" => Key::Tab,
        "backspace" => Key::Backspace,
        "delete" => Key::Delete,
        "enter" | "return" => Key::Enter,
        "escape" | "esc" => Key::Escape,
        "left" => Key::Left,
        "right" => Key::Right,
        "up" => Key::Up,
        "down" => Key::Down,
        "home" => Key::Home,
        "end" => Key::End,
        "page-up" => Key::PageUp,
        "page-down" => Key::PageDown,
        other => Key::Char(
            other
                .strip_prefix("char:")
                .unwrap_or(other)
                .chars()
                .next()?,
        ),
    })
}

impl PropValue {
    pub fn to_json(&self) -> J {
        match self {
            PropValue::Int(i) => J::obj().set("kind", "int").set("value", *i),
            PropValue::Float(f) => J::obj().set("kind", "float").set("value", *f),
            PropValue::Bool(b) => J::obj().set("kind", "bool").set("value", *b),
            PropValue::Color(c) => J::obj().set("kind", "color").set("rgba", *c),
            PropValue::Text(t) => J::obj().set("kind", "text").set("text", t.as_str()),
            other => J::obj().set("kind", other.kind()).set("text", other.text()),
        }
    }
    pub fn from_json(v: &J) -> Result<PropValue, String> {
        let kind = v.get("kind").and_then(J::as_str).unwrap_or("int");
        Ok(match kind {
            "int" => PropValue::Int(
                v.get("value")
                    .and_then(J::as_i64)
                    .and_then(|n| i32::try_from(n).ok())
                    .ok_or("int value outside signed 32-bit range")?,
            ),
            "float" => PropValue::Float(
                v.get("value")
                    .and_then(J::as_f32)
                    .filter(|x| x.is_finite())
                    .ok_or("finite float value")?,
            ),
            "bool" => PropValue::Bool(v.get("value").and_then(J::as_bool).ok_or("bool value")?),
            "color" => PropValue::Color(v.get("rgba").and_then(J::as_u32).ok_or("colour value")?),
            "text" => PropValue::Text(
                v.get("text")
                    .and_then(J::as_str)
                    .ok_or("text value")?
                    .to_owned(),
            ),
            other => return Err(format!("not a settable value kind: {other}")),
        })
    }
}

/// What one command produced, including volatile damage and revision diagnostics.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub command: Command,
    pub digest: u64,
    pub pixels: u64,
    pub damage: u32,
    pub revision: u32,
    /// A `set` that was refused, as `(code, reason)`.
    pub refused: Option<(String, String)>,
}

pub(crate) const MAX_LOG_STEPS: usize = 1024;
/// Log versions are independent of the live snapshot and RPC schema version.
pub const LOG_SCHEMA_VERSION: u32 = 2;
pub const LEGACY_LOG_SCHEMA_VERSION: u32 = 1;

/// A session's commands and what each produced.
#[derive(Clone, Debug, PartialEq)]
pub struct Log {
    pub schema: u32,
    pub app: String,
    pub w: i32,
    pub h: i32,
    pub scale: f32,
    pub steps: Vec<Step>,
}
impl Log {
    pub fn new(session: &mut Session<impl Program>) -> Log {
        let snap = session.snapshot();
        Log {
            schema: LOG_SCHEMA_VERSION,
            app: snap.app.name.clone(),
            w: snap.app.w,
            h: snap.app.h,
            scale: snap.app.scale,
            steps: Vec::new(),
        }
    }
    pub fn to_json(&self) -> J {
        J::obj()
            .set("schema", self.schema as i64)
            .set("app", self.app.as_str())
            .set(
                "size",
                J::obj()
                    .set("w", self.w)
                    .set("h", self.h)
                    .set("scale", self.scale),
            )
            .set(
                "steps",
                self.steps
                    .iter()
                    .map(|s| {
                        let mut o = s
                            .command
                            .to_json()
                            .set("digest", J::Str(format!("{:016x}", s.digest)))
                            .set("pixels", J::Str(format!("{:016x}", s.pixels)))
                            .set("damage", s.damage)
                            .set("revision", s.revision);
                        if let Some((code, reason)) = &s.refused {
                            o = o.set(
                                "refused",
                                J::obj()
                                    .set("code", code.as_str())
                                    .set("reason", reason.as_str()),
                            );
                        }
                        o
                    })
                    .collect::<Vec<_>>(),
            )
    }
    pub fn to_json_string(&self) -> String {
        json::to_string(&self.to_json())
    }
    pub fn from_json(v: &J) -> Result<Log, String> {
        let schema = v.get("schema").and_then(J::as_u32).ok_or("schema")?;
        if !matches!(schema, LEGACY_LOG_SCHEMA_VERSION | LOG_SCHEMA_VERSION) {
            return Err(format!(
                "log is schema {schema}, this build reads {LEGACY_LOG_SCHEMA_VERSION} and {LOG_SCHEMA_VERSION}"
            ));
        }
        let size = v.get("size").ok_or("size")?;
        let mut steps = Vec::new();
        let Some(J::Arr(entries)) = v.get("steps") else {
            return Err("steps must be an array".into());
        };
        if entries.len() > MAX_LOG_STEPS {
            return Err("recording exceeded the step limit".into());
        }
        for s in entries {
            let command = Command::from_json(s)?;
            command.validate()?;
            let refused = s.get("refused").map(|r| {
                (
                    r.get("code").and_then(J::as_str).unwrap_or("").to_owned(),
                    r.get("reason").and_then(J::as_str).unwrap_or("").to_owned(),
                )
            });
            steps.push(Step {
                command,
                digest: s
                    .get("digest")
                    .and_then(J::as_str)
                    .and_then(|h| u64::from_str_radix(h, 16).ok())
                    .ok_or("digest")?,
                pixels: s
                    .get("pixels")
                    .and_then(J::as_str)
                    .and_then(|h| u64::from_str_radix(h, 16).ok())
                    .ok_or("pixels")?,
                damage: s.get("damage").and_then(J::as_u32).unwrap_or(0),
                revision: s.get("revision").and_then(J::as_u32).unwrap_or(0),
                refused,
            });
        }
        Ok(Log {
            schema,
            app: v.get("app").and_then(J::as_str).unwrap_or("").to_owned(),
            w: size.get("w").and_then(J::as_i64).unwrap_or(0) as i32,
            h: size.get("h").and_then(J::as_i64).unwrap_or(0) as i32,
            scale: size.get("scale").and_then(J::as_f32).unwrap_or(1.0),
            steps,
        })
    }
}

/// One command recorded: run it, then take the description and the frame it produced.
pub fn record_step<P: Program>(s: &mut Session<P>, c: Command) -> Step {
    replay_step(s, c, LOG_SCHEMA_VERSION)
}

fn replay_step<P: Program>(s: &mut Session<P>, c: Command, schema: u32) -> Step {
    let refused = match &c {
        Command::Set { field, value } => s.set(field, value).err(),
        _ => {
            s.run(&c);
            None
        }
    };
    observe_step(s, c, refused.as_ref(), schema)
}

pub(crate) fn observe<P: Program>(
    s: &mut Session<P>,
    c: Command,
    refusal: Option<&Refusal>,
) -> Step {
    observe_step(s, c, refusal, LOG_SCHEMA_VERSION)
}

fn observe_step<P: Program>(
    s: &mut Session<P>,
    c: Command,
    refusal: Option<&Refusal>,
    schema: u32,
) -> Step {
    let refused = refusal.map(|e| (e.code().to_owned(), e.reason()));
    let snapshot = s.snapshot();
    let digest = if schema == LEGACY_LOG_SCHEMA_VERSION {
        snapshot.legacy_digest()
    } else {
        snapshot.digest()
    };
    let damage = s.damage().tiles;
    let revision = s.app().world().revision();
    let f: Frame = s.shot();
    Step {
        command: c,
        digest,
        pixels: f.digest(),
        damage,
        revision,
        refused,
    }
}

/// What replay found. The first mismatch names the step and the field that differed.
#[derive(Clone, Debug, PartialEq)]
pub enum Replay {
    Match {
        steps: usize,
    },
    Mismatch {
        step: usize,
        command: Command,
        what: &'static str,
        expected: String,
        got: String,
    },
}

/// Replay a log on a fresh application. Compare description, pixels and refusal code at each step.
pub fn replay<P: Program>(log: &Log, app: impl FnOnce() -> P) -> Replay {
    let mut s = Session::new(app());
    let fresh = Log::new(&mut s);
    if (log.app.as_str(), log.w, log.h, log.scale)
        != (fresh.app.as_str(), fresh.w, fresh.h, fresh.scale)
        && let Some(step) = log.steps.first()
    {
        return Replay::Mismatch {
            step: 0,
            command: step.command.clone(),
            what: "initial configuration",
            expected: format!("{} {}x{} @ {}", log.app, log.w, log.h, log.scale),
            got: format!("{} {}x{} @ {}", fresh.app, fresh.w, fresh.h, fresh.scale),
        };
    }
    for (i, want) in log.steps.iter().enumerate() {
        #[cfg(feature = "mutant_replay")]
        if i % 4 == 3 {
            // the negative control of G4: a replay silently skips every fourth command
            continue;
        }
        let got = replay_step(&mut s, want.command.clone(), log.schema);
        for (what, e, g) in [
            (
                "description",
                format!("{:016x}", want.digest),
                format!("{:016x}", got.digest),
            ),
            (
                "pixels",
                format!("{:016x}", want.pixels),
                format!("{:016x}", got.pixels),
            ),
        ] {
            if e != g {
                return Replay::Mismatch {
                    step: i,
                    command: want.command.clone(),
                    what,
                    expected: e,
                    got: g,
                };
            }
        }
        let refused = |r: &Option<(String, String)>| r.as_ref().map(|(c, _)| c.clone());
        if refused(&want.refused) != refused(&got.refused) {
            return Replay::Mismatch {
                step: i,
                command: want.command.clone(),
                what: "refusal",
                expected: refused(&want.refused).unwrap_or_default(),
                got: refused(&got.refused).unwrap_or_default(),
            };
        }
    }
    Replay::Match {
        steps: log.steps.len(),
    }
}

/// Refusals a replayed log asserted, for a test that wants the reasons too.
pub fn refusal_codes(log: &Log) -> Vec<Option<String>> {
    log.steps
        .iter()
        .map(|s| s.refused.as_ref().map(|(c, _)| c.clone()))
        .collect()
}

/// The outcome of a command as the socket reports it.
pub fn outcome_json(o: &Outcome) -> J {
    J::obj()
        .set("command", o.command.as_str())
        .set("revision", o.revision)
        .set("damage", o.damage_tiles)
        .set("digest", J::Str(format!("{:016x}", o.digest)))
}
pub fn refusal_json(r: &Refusal) -> J {
    J::obj().set("code", r.code()).set("reason", r.reason())
}

/// Run a script on a session and return the log it produced (the recording entry point for a test).
pub fn record<P: Program>(s: &mut Session<P>, cmds: impl IntoIterator<Item = Command>) -> Log {
    let mut log = Log::new(s);
    for c in cmds {
        log.steps.push(record_step(s, c));
    }
    log
}
