//! The wire: JSON-RPC 2.0 methods over one session. The socket and the MCP server both call [`handle`], so a
//! tool and a test drive the application through exactly the same admission.
use crate::json::{self, Value as J};
use crate::png;
use crate::record::{self, Command, Log};
use crate::schema::{Key, PropValue, SCHEMA_VERSION, Snapshot};
use crate::session::{Program, Refusal, Session};
use bough_x::event::{Button, Mods};

pub const PARSE_ERROR: i32 = -32700;
pub const INVALID_REQUEST: i32 = -32600;
pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INVALID_PARAMS: i32 = -32602;
pub const INTERNAL: i32 = -32603;
/// A refusal from the inspector's typed admission (`data` carries the stable code and the reason).
pub const REFUSED: i32 = -32010;

#[derive(Clone, Debug, PartialEq)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    pub data: J,
}
impl RpcError {
    pub fn new(code: i32, message: impl Into<String>) -> RpcError {
        RpcError {
            code,
            message: message.into(),
            data: J::Null,
        }
    }
    pub fn params(what: &str) -> RpcError {
        RpcError::new(INVALID_PARAMS, format!("invalid params: {what}"))
    }
    pub fn refusal(r: &Refusal) -> RpcError {
        RpcError {
            code: REFUSED,
            message: r.reason(),
            data: record::refusal_json(r),
        }
    }
    pub fn to_json(&self) -> J {
        J::obj()
            .set("code", self.code)
            .set("message", self.message.as_str())
            .set("data", self.data.clone())
    }
}

/// A request as the transports hand it over.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub id: J,
    pub method: String,
    pub params: J,
}
impl Request {
    pub fn from_json(v: &J) -> Result<Request, RpcError> {
        let id = v.get("id").cloned().unwrap_or(J::Null);
        let method = v
            .get("method")
            .and_then(J::as_str)
            .ok_or_else(|| RpcError::new(INVALID_REQUEST, "a request needs a method"))?
            .to_owned();
        Ok(Request {
            id,
            method,
            params: v.get("params").cloned().unwrap_or(J::Null),
        })
    }
    pub fn result(&self, result: J) -> J {
        J::obj()
            .set("jsonrpc", "2.0")
            .set("id", self.id.clone())
            .set("result", result)
    }
    pub fn error(&self, e: &RpcError) -> J {
        J::obj()
            .set("jsonrpc", "2.0")
            .set("id", self.id.clone())
            .set("error", e.to_json())
    }
}

/// What a dispatch did: a result, or a refusal/error, and whether the caller asked to stop.
pub struct Reply {
    pub result: Result<J, RpcError>,
    pub shutdown: bool,
}

fn ok(v: J) -> Reply {
    Reply {
        result: Ok(v),
        shutdown: false,
    }
}
fn fail(e: RpcError) -> Reply {
    Reply {
        result: Err(e),
        shutdown: false,
    }
}

/// The methods this build serves (a tool list, for `hello` and for a person).
pub const METHODS: &[&str] = &[
    "hello",
    "methods",
    "tree",
    "node",
    "find",
    "pick",
    "deps",
    "why",
    "counters",
    "state",
    "set",
    "pointer",
    "down",
    "up",
    "click",
    "key",
    "text",
    "wheel",
    "tick",
    "resize",
    "settle",
    "reload",
    "shot",
    "diff",
    "record_start",
    "record_stop",
    "replay",
    "shutdown",
];

pub fn handle<P: Program>(s: &mut Session<P>, method: &str, params: &J) -> Reply {
    match method {
        "hello" => ok(J::obj()
            .set("schema", SCHEMA_VERSION as i64)
            .set("app", s.app().name())
            .set(
                "methods",
                METHODS
                    .iter()
                    .map(|m| J::Str((*m).to_owned()))
                    .collect::<Vec<_>>(),
            )
            .set(
                "capabilities",
                vec![
                    J::Str("tree".into()),
                    J::Str("mutation".into()),
                    J::Str("record".into()),
                    J::Str("replay".into()),
                    J::Str("screenshot".into()),
                    J::Str("pixel-diff".into()),
                    J::Str("accessibility".into()),
                    J::Str("input".into()),
                ],
            )),
        "methods" => ok(J::obj().set(
            "methods",
            METHODS
                .iter()
                .map(|m| J::Str((*m).to_owned()))
                .collect::<Vec<_>>(),
        )),
        "tree" => ok(tree(s, params)),
        "node" => match node(s, params) {
            Ok(v) => ok(v),
            Err(e) => fail(e),
        },
        "find" => match find(s, params) {
            Ok(v) => ok(v),
            Err(e) => fail(e),
        },
        "pick" => {
            let (x, y) = match (int(params, "x"), int(params, "y")) {
                (Some(x), Some(y)) => (x, y),
                _ => return fail(RpcError::params("x and y")),
            };
            let hits = s.stack(x, y);
            let snap = s.snapshot();
            let v: Vec<J> = hits
                .iter()
                .map(|&(i, k)| {
                    let n = &snap.nodes[i];
                    J::obj()
                        .set("index", i)
                        .set("key", key_json(k))
                        .set("kind", n.kind)
                        .set("text", n.text.clone())
                        .set("screen", rect_json(n.screen))
                        .set("handler", !n.handlers.is_empty())
                })
                .collect();
            ok(J::obj().set("x", x).set("y", y).set("stack", v))
        }
        "deps" => match deps(s, params) {
            Ok(v) => ok(v),
            Err(e) => fail(e),
        },
        "why" => ok(why(s)),
        "counters" => {
            let c = s.counters();
            let mut j = c.json();
            j.put("damage_tiles", s.damage().tiles);
            j.put("damage_rects", s.damage().rects.len());
            ok(j)
        }
        "state" => {
            let snap = s.snapshot();
            ok(J::obj().set(
                "fields",
                snap.to_json()
                    .get("fields")
                    .cloned()
                    .unwrap_or(J::Arr(Vec::new())),
            ))
        }
        "set" => {
            let field = match params.get("field").and_then(J::as_str) {
                Some(f) => f,
                None => return fail(RpcError::params("field")),
            };
            let value = match params.get("value") {
                Some(v) => match PropValue::from_json(v) {
                    Ok(v) => v,
                    Err(e) => return fail(RpcError::params(&e)),
                },
                None => return fail(RpcError::params("value")),
            };
            match s.set(field, &value) {
                Ok(o) => ok(record::outcome_json(&o)),
                Err(r) => fail(RpcError::refusal(&r)),
            }
        }
        "pointer" | "down" | "up" | "click" | "key" | "text" | "wheel" | "tick" | "resize"
        | "settle" => {
            let c = match command(method, params) {
                Ok(c) => c,
                Err(e) => return fail(e),
            };
            if let Err(e) = c.validate() {
                return fail(RpcError::params(e));
            }
            let o = s.run(&c);
            ok(record::outcome_json(&o))
        }
        "reload" => {
            if s.recording() {
                return fail(RpcError::params(
                    "stop recording before reloading external inputs",
                ));
            }
            let reloaded = s.reload();
            let snap = s.snapshot();
            ok(J::obj()
                .set("reloaded", reloaded)
                .set("revision", snap.app.revision)
                .set("digest", J::Str(format!("{:016x}", snap.digest()))))
        }
        "shot" => match shot(s, params) {
            Ok(v) => ok(v),
            Err(e) => fail(e),
        },
        "diff" => match diff(s, params) {
            Ok(v) => ok(v),
            Err(e) => fail(e),
        },
        "record_start" => match s.start_recording() {
            Ok(()) => ok(J::obj().set("recording", true)),
            Err(e) => fail(RpcError::params(e)),
        },
        "record_stop" => match s.stop_recording() {
            Ok(log) => ok(log.to_json().set("count", log.steps.len())),
            Err(e) => fail(RpcError::params(e)),
        },
        "replay" => {
            let log = match params.get("log") {
                Some(v) => match Log::from_json(v) {
                    Ok(l) => l,
                    Err(e) => return fail(RpcError::params(&e)),
                },
                None => return fail(RpcError::params("log")),
            };
            match s.replay(&log) {
                Some(record::Replay::Match { steps }) => {
                    ok(J::obj().set("match", true).set("steps", steps))
                }
                Some(record::Replay::Mismatch {
                    step,
                    command,
                    what,
                    expected,
                    got,
                }) => ok(J::obj()
                    .set("match", false)
                    .set("step", step)
                    .set("command", command.name())
                    .set("what", what)
                    .set("expected", expected.as_str())
                    .set("got", got.as_str())),
                None => fail(RpcError::new(
                    INTERNAL,
                    "this program cannot be rebuilt for replay (no fresh instance)",
                )),
            }
        }
        "shutdown" => Reply {
            result: Ok(J::obj().set("bye", true)),
            shutdown: true,
        },
        other => fail(RpcError::new(
            METHOD_NOT_FOUND,
            format!("no method {other:?} (see `methods`)"),
        )),
    }
}

fn int(p: &J, k: &str) -> Option<i32> {
    p.get(k)
        .and_then(J::as_i64)
        .and_then(|x| i32::try_from(x).ok())
}
fn flt(p: &J, k: &str) -> Option<f32> {
    p.get(k).and_then(J::as_f32)
}
fn flag(p: &J, k: &str, d: bool) -> bool {
    p.get(k).and_then(J::as_bool).unwrap_or(d)
}

fn command(method: &str, p: &J) -> Result<Command, RpcError> {
    let coordinate = |name| match int(p, name) {
        Some(value) => Ok(value),
        None if method == "wheel" && p.get(name).is_none() => Ok(0),
        None => Err(RpcError::params(name)),
    };
    let (x, y) = if matches!(method, "pointer" | "down" | "up" | "click" | "wheel") {
        (coordinate("x")?, coordinate("y")?)
    } else {
        (0, 0)
    };
    Ok(match method {
        "pointer" => Command::Pointer { x, y },
        "down" => Command::Down {
            x,
            y,
            button: record::button_of(p.get("button").and_then(J::as_str))
                .unwrap_or(Button::Primary),
        },
        "up" => Command::Up {
            x,
            y,
            button: record::button_of(p.get("button").and_then(J::as_str))
                .unwrap_or(Button::Primary),
        },
        "click" => Command::Click { x, y },
        "key" => Command::Key {
            key: record::key_of(p.get("key").and_then(J::as_str))
                .ok_or_else(|| RpcError::params("key"))?,
            mods: Mods {
                shift: flag(p, "shift", false),
                ctrl: flag(p, "ctrl", false),
                alt: flag(p, "alt", false),
                cmd: flag(p, "logo", false),
            },
        },
        "text" => Command::Text {
            text: p
                .get("text")
                .and_then(J::as_str)
                .ok_or_else(|| RpcError::params("text"))?
                .to_owned(),
        },
        "wheel" => Command::Wheel {
            x,
            y,
            dx: flt(p, "dx").unwrap_or(0.0),
            dy: flt(p, "dy").unwrap_or(0.0),
        },
        "tick" => Command::Tick {
            ms: p.get("ms").and_then(J::as_u32).unwrap_or(0),
        },
        "resize" => Command::Resize {
            w: int(p, "w").ok_or_else(|| RpcError::params("w"))?,
            h: int(p, "h").ok_or_else(|| RpcError::params("h"))?,
            scale: flt(p, "scale").unwrap_or(1.0),
        },
        "settle" => Command::Settle,
        _ => return Err(RpcError::new(METHOD_NOT_FOUND, method)),
    })
}

fn key_json(k: Key) -> J {
    let path: Vec<J> = k.path[..k.depth as usize]
        .iter()
        .map(|&q| J::Int(q as i64))
        .collect();
    J::obj()
        .set("path", path)
        .set("row", k.row)
        .set("pos", k.pos)
        .set("col", k.col)
        .set("rec", k.rec)
}
fn rect_json(r: crate::schema::R) -> J {
    J::obj()
        .set("x", r.x0)
        .set("y", r.y0)
        .set("w", r.w())
        .set("h", r.h())
}

fn tree<P: Program>(s: &mut Session<P>, p: &J) -> J {
    let snap = s.snapshot();
    let mut j = snap.to_json();
    // the description's identity, for a client that compares two trees (the dev server's state-preserving restart)
    j.put("digest", J::Str(format!("{:016x}", snap.digest())));
    if !flag(p, "nodes", true) {
        j.put("nodes", J::Arr(Vec::new()));
    }
    if !flag(p, "ax", true) {
        j.put("ax", J::Null);
    }
    if !flag(p, "damage", true) {
        j.put("damage", J::obj().set("tiles", snap.damage.tiles as i64));
    }
    if let Some(limit) = int(p, "limit") {
        if let Some(J::Arr(nodes)) = j.get("nodes").cloned().as_ref() {
            let n = (limit.max(0) as usize).min(nodes.len());
            j.put("nodes", J::Arr(nodes[..n].to_vec()));
            j.put("nodes_total", nodes.len());
        }
    }
    j
}

fn node<P: Program>(s: &mut Session<P>, p: &J) -> Result<J, RpcError> {
    let snap = s.snapshot();
    let i = if let Some(i) = int(p, "index") {
        i as usize
    } else {
        let key = Key {
            path: {
                let mut a = [0u16; 6];
                for (j, x) in p
                    .get("path")
                    .map(|v| v.arr())
                    .unwrap_or(&[])
                    .iter()
                    .enumerate()
                    .take(6)
                {
                    a[j] = x.as_i64().unwrap_or(0) as u16;
                }
                a
            },
            depth: p.get("path").map(|v| v.arr().len() as u16).unwrap_or(0),
            row: p.get("row").and_then(J::as_u32).unwrap_or(0),
            pos: p.get("pos").and_then(J::as_u32).unwrap_or(0),
            col: 0,
            rec: 0,
        };
        snap.find_place(key.depth, key.path, key.row, key.pos)
            .map(|n| snap.nodes.iter().position(|x| x.key == n.key).unwrap_or(0))
            .ok_or_else(|| RpcError::new(INVALID_PARAMS, "no such entity in the current window"))?
    };
    let Some(n) = snap.node(i) else {
        return Err(RpcError::new(INVALID_PARAMS, "node index out of range"));
    };
    let stores: Vec<J> = snap
        .store_chain(n.store)
        .iter()
        .map(|&s| {
            let st = &snap.stores[s];
            J::obj()
                .set("index", st.index)
                .set("kind", st.kind)
                .set("loc", st.loc.map_or(J::Null, |l| l.json()))
        })
        .collect();
    Ok(J::obj()
        .set("index", i)
        .set("node", n.json())
        .set("stores", stores))
}

fn subtree(snap: &Snapshot, store: usize) -> Vec<usize> {
    let mut v = vec![store];
    let mut k = 0;
    while k < v.len() {
        for &s in &snap.stores[v[k]].subs {
            v.push(s);
        }
        k += 1;
    }
    v
}

fn find<P: Program>(s: &mut Session<P>, p: &J) -> Result<J, RpcError> {
    let snap = s.snapshot();
    let kind = p.get("kind").and_then(J::as_str);
    let ty = p.get("ty").and_then(J::as_str);
    let text = p.get("text").and_then(J::as_str);
    let role = p.get("role").and_then(J::as_str);
    let within = int(p, "within").map(|s| subtree(snap, s as usize));
    let focused = flag(p, "focused", false);
    let hovered = flag(p, "hovered", false);
    let pressed = flag(p, "pressed", false);
    let visible = p.get("visible").and_then(J::as_bool);
    let handler = p.get("handler").and_then(J::as_bool);
    let limit = int(p, "limit").unwrap_or(64).max(0) as usize;
    // role searches the accessibility tree, which knows roles; everything else searches records
    let mut out: Vec<(usize, J)> = Vec::new();
    if let Some(role) = role {
        let Some(ax) = &snap.ax else {
            return Err(RpcError::new(INVALID_PARAMS, "no accessibility projection"));
        };
        for (i, n) in ax.nodes.iter().enumerate() {
            if n.role != role || !n.name.contains(text.unwrap_or("")) {
                continue;
            }
            out.push((
                i,
                J::obj()
                    .set("ax", i)
                    .set("role", n.role)
                    .set("name", n.name.as_str())
                    .set("bounds", rect_json(n.bounds))
                    .set("place", n.place.map_or(J::Null, |p| J::Int(p as i64))),
            ));
        }
    } else {
        for (i, n) in snap.nodes.iter().enumerate() {
            if let Some(k) = kind
                && n.kind != k
            {
                continue;
            }
            if let Some(t) = ty
                && !n.ty.contains(t)
            {
                continue;
            }
            if let Some(t) = text
                && !n.text.as_deref().is_some_and(|x| x.contains(t))
            {
                continue;
            }
            if let Some(w) = &within
                && !w.contains(&n.store)
            {
                continue;
            }
            if focused && snap.pointers.focus != Some(n.key) {
                continue;
            }
            if hovered && snap.pointers.pick != Some(n.key) {
                continue;
            }
            if pressed && snap.pointers.press != Some(n.key) {
                continue;
            }
            if let Some(v) = visible
                && n.visible != v
            {
                continue;
            }
            if let Some(h) = handler
                && (!n.handlers.is_empty()) != h
            {
                continue;
            }
            let mut o = J::obj()
                .set("index", i)
                .set("kind", n.kind)
                .set("text", n.text.clone())
                .set("loc", n.loc.json())
                .set("screen", rect_json(n.screen))
                .set("visible", n.visible)
                .set(
                    "handlers",
                    n.handlers
                        .iter()
                        .map(|&h| J::Str(h.to_owned()))
                        .collect::<Vec<_>>(),
                );
            if p.get("ops").and_then(J::as_bool).unwrap_or(false) {
                o = o.set("ops", n.json().get("ops").cloned().unwrap_or(J::Null));
            }
            out.push((i, o));
        }
    }
    let total = out.len();
    out.truncate(limit);
    Ok(J::obj()
        .set("total", total)
        .set("nodes", out.into_iter().map(|(_, j)| j).collect::<Vec<_>>()))
}

fn deps<P: Program>(s: &mut Session<P>, p: &J) -> Result<J, RpcError> {
    let word = match p.get("word").and_then(J::as_u32) {
        Some(w) => w,
        None => match p.get("field").and_then(J::as_str) {
            Some(f) => {
                let snap = s.snapshot();
                snap.fields
                    .iter()
                    .find(|x| x.name == f)
                    .and_then(|x| x.word)
                    .ok_or_else(|| {
                        RpcError::new(INVALID_PARAMS, format!("field {f:?} names no block word"))
                    })?
            }
            None => return Err(RpcError::params("word or field")),
        },
    };
    let snap = s.snapshot();
    let name = snap
        .words
        .iter()
        .find(|w| w.word == word)
        .and_then(|w| w.name);
    let nodes: Vec<J> = snap
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.reads.contains(&word))
        .map(|(i, n)| {
            J::obj()
                .set("index", i)
                .set("kind", n.kind)
                .set("loc", n.loc.json())
                .set("screen", rect_json(n.screen))
        })
        .collect();
    let cols: Vec<J> = snap
        .stores
        .iter()
        .flat_map(|st| {
            st.cols
                .iter()
                .filter(|c| c.reads.contains(&word))
                .map(move |c| {
                    J::obj()
                        .set("store", st.index)
                        .set("col", c.index)
                        .set("kind", c.kind)
                        .set("ty", c.ty)
                        .set("stage", c.stage)
                })
        })
        .collect();
    let stores: Vec<J> = snap
        .stores
        .iter()
        .filter(|st| st.scroll == Some(word))
        .map(|st| {
            J::obj()
                .set("store", st.index)
                .set("kind", st.kind)
                .set("clip", rect_json(st.clip))
        })
        .collect();
    Ok(J::obj()
        .set("word", word)
        .set("name", name)
        .set("stores", stores)
        .set(
            "value",
            snap.words
                .iter()
                .find(|w| w.word == word)
                .map_or(J::Null, |w| J::Int(w.value as i64)),
        )
        .set("nodes", nodes)
        .set("columns", cols))
}

fn why<P: Program>(s: &mut Session<P>) -> J {
    let snap = s.snapshot();
    let changed: Vec<J> = snap
        .counters
        .changed
        .iter()
        .map(|&w| {
            let word = snap.words.iter().find(|x| x.word == w);
            let readers: Vec<J> = snap
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| n.reads.contains(&w))
                .take(64)
                .map(|(i, n)| {
                    J::obj()
                        .set("index", i)
                        .set("kind", n.kind)
                        .set("screen", rect_json(n.screen))
                })
                .collect();
            J::obj()
                .set("word", w)
                .set("name", word.and_then(|x| x.name))
                .set("value", word.map_or(0, |x| x.value))
                .set("readers", readers)
        })
        .collect();
    J::obj()
        .set("command", snap.counters.last_command.as_str())
        .set("revision", snap.app.revision)
        .set("changed_words", changed)
        .set("damage_tiles", snap.damage.tiles)
        .set("damage", snap.damage.json())
        .set(
            "delta",
            snap.to_json().get("counters").cloned().unwrap_or(J::Null),
        )
}

fn shot<P: Program>(s: &mut Session<P>, p: &J) -> Result<J, RpcError> {
    let crop = if let Some(node) = p.get("node") {
        let index = node
            .as_usize()
            .ok_or_else(|| RpcError::params("node index"))?;
        let node = s
            .snapshot()
            .node(index)
            .ok_or_else(|| RpcError::params("node index out of range"))?;
        Some(node.screen.and(node.clip))
    } else {
        None
    };
    let full = s.shot();
    let count = i64::from(full.w) * i64::from(full.h);
    if full.w <= 0 || full.h <= 0 || count > 8_388_608 || count as usize != full.px.len() {
        return Err(RpcError::new(
            INTERNAL,
            "application returned an invalid screenshot buffer",
        ));
    }
    let f = if let Some(r) = crop {
        let (x0, y0) = (r.x0.max(0), r.y0.max(0));
        let (x1, y1) = (r.x1.min(full.w), r.y1.min(full.h));
        if x1 <= x0 || y1 <= y0 {
            return Err(RpcError::params("element has no visible pixels"));
        }
        let (w, h) = (x1 - x0, y1 - y0);
        let mut px = Vec::with_capacity((w * h) as usize);
        for y in y0..y1 {
            let a = (y * full.w + x0) as usize;
            px.extend_from_slice(&full.px[a..a + w as usize]);
        }
        crate::frame::Frame { w, h, px }
    } else {
        full
    };
    let png = f.png();
    let mut o = J::obj()
        .set("w", f.w)
        .set("h", f.h)
        .set("digest", J::Str(format!("{:016x}", f.digest())))
        .set("png", json::base64_encode(&png))
        .set("bytes", png.len());
    if flag(p, "raw", false) {
        let mut raw = Vec::with_capacity(f.px.len() * 4);
        for c in &f.px {
            let [a, r, g, b] = c.to_be_bytes();
            raw.extend_from_slice(&[r, g, b, a]);
        }
        o = o
            .set("raw", json::base64_encode(&raw))
            .set("format", "rgba8-le");
    }
    Ok(o)
}

fn diff<P: Program>(s: &mut Session<P>, p: &J) -> Result<J, RpcError> {
    let golden = p
        .get("golden")
        .and_then(J::as_str)
        .ok_or_else(|| RpcError::params("golden (base64 PNG)"))?;
    let bytes = json::base64_decode(golden).map_err(|e| RpcError::params(&e.to_string()))?;
    let want = png::decode(&bytes).map_err(|e| RpcError::params(&e.to_string()))?;
    let f = s.shot();
    if (want.w, want.h) != (f.w, f.h) {
        return Err(RpcError::new(
            INVALID_PARAMS,
            format!(
                "golden is {}x{}, the frame is {}x{}",
                want.w, want.h, f.w, f.h
            ),
        ));
    }
    let d = png::diff(&want, &f.to_image()).map_err(|e| RpcError::new(INTERNAL, e.to_string()))?;
    Ok(J::obj()
        .set("match", d.differing == 0)
        .set("differing", d.differing)
        .set("total", d.total)
        .set("max_channel", d.max_channel as i64)
        .set(
            "bbox",
            d.bbox.map_or(J::Null, |b| {
                J::obj()
                    .set("x0", b[0])
                    .set("y0", b[1])
                    .set("x1", b[2])
                    .set("y1", b[3])
            }),
        )
        .set("digest", J::Str(format!("{:016x}", f.digest()))))
}
