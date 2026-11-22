//! The Model Context Protocol server over a running lorelei application.
//!
//! The tool surface follows `fakeland-mcp` where that is good (a tree, a find, a pick, driving input, screenshots,
//! waits, a scenario player) and goes further where an in-process inspector knows more: source locations on every
//! record, the staged expression behind every operand and the words it reads, dependencies of a word, why the
//! last turn repainted what it did, and typed state that can be written through the application's own admission.
//!
//! Transport is the MCP JSON-RPC (newline-delimited over stdio, or HTTP POST at `/mcp`); the application is reached
//! through the inspector socket ([`bough_inspect::client`]). The protocol handler is transport-free, so a test
//! drives it without a process.
use bough_inspect::client::{self, Client};
use bough_inspect::json::{self, Value as J};
use std::io::{self, BufRead, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The protocol revision this server speaks (MCP 2025-06-18). A client's revision is echoed when it is one of the
/// revisions with the same message shapes; otherwise this one is offered.
pub const PROTOCOL: &str = "2025-06-18";
const KNOWN: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "lorelei (bough) inspector. `tree` is the live element tree: every exposed record with \
its kind, source location (file:line:col), typed operands and the staged expression each came from, the block \
words it reads, rects, damage, focus/hover/press, stores, lists, the accessibility projection and counters. \
`find` selects by kind, text, type, role or state; `node` reads one; `pick` is the hit stack under a point in \
device pixels; `deps` names the records and spaces a state word moves; `why_repaint` explains the last turn. \
Drive with `pointer_move`, `click`, `pointer_down`/`pointer_up`, `key`, `type_text`, `wheel`, `tick`, `resize`. \
`state` lists the typed fields; `set_field` writes one through the application's own admission (record operands \
are read-only: they are staged expressions of state). `screenshot` returns a PNG; `diff_golden` compares one. \
`record_start`/`record_stop`/`replay` make a session deterministic. Every answer is JSON in a text block.";

/// The MCP handler: one inspector connection, lazily opened and reopened after a refusal at the transport.
pub struct Mcp {
    socket: PathBuf,
    client: Option<Client>,
    socket_stamp: Option<(u64, u64, i64, i64)>,
    protocol: String,
    /// A `shutdown` request: the transports stop.
    closed: bool,
}
impl Mcp {
    pub fn new(socket: impl Into<PathBuf>) -> Mcp {
        Mcp {
            socket: socket.into(),
            client: None,
            socket_stamp: None,
            protocol: PROTOCOL.to_owned(),
            closed: false,
        }
    }
    pub fn closed(&self) -> bool {
        self.closed
    }
    fn client(&mut self) -> io::Result<&mut Client> {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::symlink_metadata(&self.socket)?;
        let stamp = (
            metadata.dev(),
            metadata.ino(),
            metadata.ctime(),
            metadata.ctime_nsec(),
        );
        if self.socket_stamp != Some(stamp) {
            self.client = None;
        }
        if self.client.is_none() {
            self.client = Some(Client::open(&self.socket)?);
            self.socket_stamp = Some(stamp);
        }
        Ok(self.client.as_mut().unwrap())
    }
    /// Reconnect before dispatch when a new app has bound the path. Never retry a dispatched turn.
    fn call(&mut self, method: &str, params: &J) -> Result<J, String> {
        let r = self
            .client()
            .map_err(|e| format!("the application's inspector socket is not open: {e}"))?
            .request(method, params);
        match r {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) => Err(format!("{}: {}", e.code, e.message)),
            Err(e) => {
                self.client = None;
                Err(format!("the inspector connection failed: {e}"))
            }
        }
    }
    /// One JSON-RPC message. `None` for a notification (no response).
    pub fn handle(&mut self, msg: &J) -> Option<J> {
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(J::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(J::Null);
        if id.is_none() && method.starts_with("notifications/") {
            return None;
        }
        let id = id.unwrap_or(J::Null);
        let result = match method {
            "initialize" => {
                if let Some(v) = params.get("protocolVersion").and_then(J::as_str)
                    && KNOWN.contains(&v)
                {
                    self.protocol = v.to_owned();
                }
                Ok(J::obj()
                    .set("protocolVersion", self.protocol.as_str())
                    .set(
                        "capabilities",
                        J::obj()
                            .set("tools", J::obj().set("listChanged", false))
                            .set(
                                "resources",
                                J::obj().set("subscribe", false).set("listChanged", false),
                            ),
                    )
                    .set(
                        "serverInfo",
                        J::obj().set("name", "bough-mcp").set("version", VERSION),
                    )
                    .set("instructions", INSTRUCTIONS))
            }
            "ping" => Ok(J::obj()),
            "tools/list" => Ok(J::obj().set("tools", tools())),
            "tools/call" => self.tools_call(&params),
            "resources/list" => Ok(J::obj().set(
                "resources",
                vec![
                    resource(
                        "lorelei://tree",
                        "tree",
                        "The live element tree (JSON)",
                        "application/json",
                    ),
                    resource(
                        "lorelei://counters",
                        "counters",
                        "Per-turn counters and timings",
                        "application/json",
                    ),
                    resource(
                        "lorelei://screenshot",
                        "screenshot",
                        "The current frame (PNG)",
                        "image/png",
                    ),
                    resource(
                        "bough://tree",
                        "tree",
                        "The live element tree (JSON)",
                        "application/json",
                    ),
                    resource(
                        "bough://damage",
                        "damage",
                        "The latest damage map and tile stats (JSON)",
                        "application/json",
                    ),
                    resource(
                        "bough://effects",
                        "effects",
                        "The active effects inventory (JSON)",
                        "application/json",
                    ),
                    resource(
                        "bough://animations",
                        "animations",
                        "Active animations and transitions (JSON)",
                        "application/json",
                    ),
                    resource(
                        "bough://profile",
                        "profile",
                        "The latest frame profile (JSON)",
                        "application/json",
                    ),
                    resource(
                        "bough://screenshot.png",
                        "screenshot",
                        "The current frame (PNG)",
                        "image/png",
                    ),
                ],
            )),
            "resources/read" => self.resources_read(&params),
            "prompts/list" => Ok(J::obj().set("prompts", Vec::<J>::new())),
            "shutdown" => {
                // ask the application to stop serving, then close the connection
                let _ = self.call("shutdown", &J::obj());
                self.client = None;
                self.closed = true;
                Ok(J::obj())
            }
            other => Err(format!("-32601: no method {other:?}")),
        };
        Some(match result {
            Ok(v) => J::obj()
                .set("jsonrpc", "2.0")
                .set("id", id)
                .set("result", v),
            Err(e) => {
                let (code, message) = match e.split_once(':') {
                    Some((c, m)) if c.starts_with('-') => {
                        (c.parse::<i64>().unwrap_or(-32603), m.trim().to_owned())
                    }
                    _ => (-32603, e),
                };
                J::obj()
                    .set("jsonrpc", "2.0")
                    .set("id", id)
                    .set("error", J::obj().set("code", code).set("message", message))
            }
        })
    }

    fn tools_call(&mut self, p: &J) -> Result<J, String> {
        let name = p
            .get("name")
            .and_then(J::as_str)
            .ok_or("-32602: tools/call needs a name")?;
        let a = p.get("arguments").cloned().unwrap_or(J::Null);
        let (text, image, is_error) = match self.run_tool(name, &a) {
            Ok((text, image)) => (text, image, false),
            Err(e) => (e, None, true),
        };
        let mut content = vec![J::obj().set("type", "text").set("text", text)];
        if let Some(png) = image {
            content.push(
                J::obj()
                    .set("type", "image")
                    .set("data", png)
                    .set("mimeType", "image/png"),
            );
        }
        Ok(J::obj().set("content", content).set("isError", is_error))
    }

    /// Run a tool: `(text, optional PNG base64)`. A refusal is an error result, not an RPC error (MCP's shape).
    fn run_tool(&mut self, name: &str, a: &J) -> Result<(String, Option<String>), String> {
        let j = |v: J| json::to_string(&v);
        match name {
            "hello" | "app_info" => Ok((j(self.call("hello", &J::obj())?), None)),
            "tree" => {
                let mut p = J::obj();
                for k in ["nodes", "ax", "damage", "limit"] {
                    if let Some(v) = a.get(k) {
                        p.put(k, v.clone());
                    }
                }
                Ok((j(self.call("tree", &p)?), None))
            }
            "find" => Ok((j(self.call("find", a)?), None)),
            "node" => Ok((j(self.call("node", a)?), None)),
            "pick" => {
                let p = J::obj().set("x", req_i(a, "x")?).set("y", req_i(a, "y")?);
                Ok((j(self.call("pick", &p)?), None))
            }
            "deps" => Ok((j(self.call("deps", a)?), None)),
            "why_repaint" | "why" => Ok((j(self.call("why", &J::obj())?), None)),
            "counters" | "profile" => Ok((j(self.call("counters", &J::obj())?), None)),
            "state" => Ok((j(self.call("state", &J::obj())?), None)),
            "set_field" => {
                let field = a.get("field").and_then(J::as_str).ok_or("-32602: field")?;
                let value = a.get("value").cloned().ok_or("-32602: value")?;
                let p = J::obj().set("field", field).set("value", value);
                Ok((j(self.call("set", &p)?), None))
            }
            "pointer_move" | "hover" => {
                let p = J::obj().set("x", req_i(a, "x")?).set("y", req_i(a, "y")?);
                Ok((j(self.call("pointer", &p)?), None))
            }
            "click" => {
                let p = J::obj().set("x", req_i(a, "x")?).set("y", req_i(a, "y")?);
                Ok((j(self.call("click", &p)?), None))
            }
            "pointer_down" => {
                let p = J::obj()
                    .set("x", req_i(a, "x")?)
                    .set("y", req_i(a, "y")?)
                    .set(
                        "button",
                        a.get("button").cloned().unwrap_or(J::from("primary")),
                    );
                Ok((j(self.call("down", &p)?), None))
            }
            "pointer_up" => {
                let p = J::obj()
                    .set("x", req_i(a, "x")?)
                    .set("y", req_i(a, "y")?)
                    .set(
                        "button",
                        a.get("button").cloned().unwrap_or(J::from("primary")),
                    );
                Ok((j(self.call("up", &p)?), None))
            }
            "key" => {
                let mut p = J::obj().set("key", a.get("key").and_then(J::as_str).unwrap_or(""));
                for k in ["shift", "ctrl", "alt", "logo"] {
                    if let Some(v) = a.get(k) {
                        p.put(k, v.clone());
                    }
                }
                Ok((j(self.call("key", &p)?), None))
            }
            "type_text" | "text" => {
                let p = J::obj().set(
                    "text",
                    a.get("text").and_then(J::as_str).ok_or("-32602: text")?,
                );
                Ok((j(self.call("text", &p)?), None))
            }
            "wheel" => {
                let p = J::obj()
                    .set("x", a.get("x").cloned().unwrap_or(J::from(0)))
                    .set("y", a.get("y").cloned().unwrap_or(J::from(0)))
                    .set("dx", a.get("dx").cloned().unwrap_or(J::from(0.0)))
                    .set("dy", req_f(a, "dy")?);
                Ok((j(self.call("wheel", &p)?), None))
            }
            "tick" => {
                let p = J::obj().set("ms", a.get("ms").cloned().unwrap_or(J::from(16)));
                Ok((j(self.call("tick", &p)?), None))
            }
            "resize" => {
                let p = J::obj()
                    .set("w", req_i(a, "w")?)
                    .set("h", req_i(a, "h")?)
                    .set("scale", a.get("scale").cloned().unwrap_or(J::from(1.0)));
                Ok((j(self.call("resize", &p)?), None))
            }
            "screenshot" => {
                let mut p = J::obj();
                if a.get("raw").and_then(J::as_bool).unwrap_or(false) {
                    p.put("raw", true);
                }
                if let Some(n) = a.get("node") {
                    p.put("node", n.clone());
                }
                let v = self.call("shot", &p)?;
                let png = v.get("png").and_then(J::as_str).unwrap_or("").to_owned();
                let mut summary = J::obj()
                    .set("w", v.get("w").cloned().unwrap_or(J::Null))
                    .set("h", v.get("h").cloned().unwrap_or(J::Null))
                    .set("digest", v.get("digest").cloned().unwrap_or(J::Null))
                    .set("bytes", v.get("bytes").cloned().unwrap_or(J::Null));
                if let (Some(raw), Some(format)) = (v.get("raw"), v.get("format")) {
                    summary.put("raw", raw.clone());
                    summary.put("format", format.clone());
                }
                Ok((j(summary), Some(png)))
            }
            "diff_golden" | "diff" => {
                let p = J::obj().set(
                    "golden",
                    a.get("golden")
                        .and_then(J::as_str)
                        .ok_or("-32602: golden (base64 PNG)")?,
                );
                Ok((j(self.call("diff", &p)?), None))
            }
            "reload" => Ok((j(self.call("reload", &J::obj())?), None)),
            "record_start" => Ok((j(self.call("record_start", &J::obj())?), None)),
            "record_stop" => Ok((j(self.call("record_stop", &J::obj())?), None)),
            "replay" => {
                let p = J::obj().set("log", a.get("log").cloned().ok_or("-32602: log")?);
                Ok((j(self.call("replay", &p)?), None))
            }
            "wait_for_idle" => Ok((j(self.call("wait_for_idle", &J::obj())?), None)),
            "damage_map" => {
                let mut p = J::obj();
                if let Some(v) = a.get("include_heatmap_png") {
                    p.put("include_heatmap_png", v.clone());
                }
                Ok((j(self.call("damage_map", &p)?), None))
            }
            "effects_inspect" => {
                let mut p = J::obj();
                for k in ["node", "node_id", "selector"] {
                    if let Some(v) = a.get(k) {
                        p.put(k, v.clone());
                    }
                }
                Ok((j(self.call("effects_inspect", &p)?), None))
            }
            "animations" => Ok((j(self.call("animations", &J::obj())?), None)),
            "scrub_animation" => {
                let mut p = J::obj();
                for k in ["op", "action", "ms", "time_ms", "id"] {
                    if let Some(v) = a.get(k) {
                        p.put(k, v.clone());
                    }
                }
                Ok((j(self.call("scrub_animation", &p)?), None))
            }
            "profile_frame" => Ok((j(self.call("profile_frame", &J::obj())?), None)),
            "assert_selector" => Ok((j(self.call("assert_selector", a)?), None)),
            "batch" => {
                let mut p = J::obj();
                p.put("steps", a.get("steps").cloned().ok_or("-32602: steps")?);
                if let Some(v) = a.get("stop_on_error") {
                    p.put("stop_on_error", v.clone());
                }
                Ok((j(self.call("batch", &p)?), None))
            }
            "quit" => {
                self.client = None;
                self.closed = true;
                Ok((j(J::obj().set("bye", true)), None))
            }
            other => Err(format!("-32602: no tool {other:?} (tools/list names them)")),
        }
    }

    fn resources_read(&mut self, p: &J) -> Result<J, String> {
        let uri = p.get("uri").and_then(J::as_str).ok_or("-32602: uri")?;
        let (body, mime) = match uri {
            "lorelei://tree" => (
                J::obj().set("text", self.call("tree", &J::obj())?.pipe(json::to_string)),
                "application/json",
            ),
            "lorelei://counters" => (
                J::obj().set(
                    "text",
                    self.call("counters", &J::obj())?.pipe(json::to_string),
                ),
                "application/json",
            ),
            "lorelei://screenshot" => {
                let v = self.call("shot", &J::obj())?;
                let png = v.get("png").and_then(J::as_str).unwrap_or("").to_owned();
                (J::obj().set("blob", png), "image/png")
            }
            "bough://tree" => (
                J::obj().set("text", self.call("tree", &J::obj())?.pipe(json::to_string)),
                "application/json",
            ),
            "bough://damage" => (
                J::obj().set(
                    "text",
                    self.call("damage_map", &J::obj())?.pipe(json::to_string),
                ),
                "application/json",
            ),
            "bough://effects" => (
                J::obj().set(
                    "text",
                    self.call("effects_inspect", &J::obj())?.pipe(json::to_string),
                ),
                "application/json",
            ),
            "bough://animations" => (
                J::obj().set(
                    "text",
                    self.call("animations", &J::obj())?.pipe(json::to_string),
                ),
                "application/json",
            ),
            "bough://profile" => (
                J::obj().set(
                    "text",
                    self.call("profile_frame", &J::obj())?.pipe(json::to_string),
                ),
                "application/json",
            ),
            "bough://screenshot.png" => {
                let v = self.call("shot", &J::obj())?;
                let png = v.get("png").and_then(J::as_str).unwrap_or("").to_owned();
                (J::obj().set("blob", png), "image/png")
            }
            u if u.starts_with("lorelei://node/") => {
                let i: i64 = u["lorelei://node/".len()..]
                    .parse()
                    .map_err(|_| "-32602: node index")?;
                let v = self.call("node", &J::obj().set("index", i))?;
                (
                    J::obj().set("text", json::to_string(&v)),
                    "application/json",
                )
            }
            other => return Err(format!("-32602: no resource {other:?}")),
        };
        #[allow(unused_mut)]
        let mut one = J::obj().set("uri", uri).set("mimeType", mime);
        if let Some(t) = body.get("text") {
            one.put("text", t.clone());
        }
        if let Some(b) = body.get("blob") {
            one.put("blob", b.clone());
        }
        Ok(J::obj().set("contents", vec![one]))
    }
}

trait Pipe: Sized {
    fn pipe<R>(self, f: impl FnOnce(&Self) -> R) -> R {
        f(&self)
    }
}
impl Pipe for J {}

fn req_i(a: &J, k: &str) -> Result<J, String> {
    a.get(k).cloned().ok_or_else(|| format!("-32602: {k}"))
}
fn req_f(a: &J, k: &str) -> Result<J, String> {
    a.get(k).cloned().ok_or_else(|| format!("-32602: {k}"))
}

fn resource(uri: &str, name: &str, description: &str, mime: &str) -> J {
    J::obj()
        .set("uri", uri)
        .set("name", name)
        .set("description", description)
        .set("mimeType", mime)
}

fn schema(props: &[(&str, &str, &str)], required: &[&str]) -> J {
    let mut p = J::obj();
    for (name, kind, desc) in props {
        p.put(name, J::obj().set("type", *kind).set("description", *desc));
    }
    J::obj().set("type", "object").set("properties", p).set(
        "required",
        required
            .iter()
            .map(|&r| J::Str(r.to_owned()))
            .collect::<Vec<_>>(),
    )
}

/// A schema whose properties are written out (nested selectors and values must not be a bare `object`).
fn schema_raw(props: Vec<(&str, J)>, required: &[&str]) -> J {
    let mut p = J::obj();
    for (name, v) in props {
        p.put(name, v);
    }
    J::obj().set("type", "object").set("properties", p).set(
        "required",
        required
            .iter()
            .map(|&r| J::Str(r.to_owned()))
            .collect::<Vec<_>>(),
    )
}

fn int_array(max: i64, desc: &str) -> J {
    J::obj()
        .set("type", "array")
        .set("items", J::obj().set("type", "integer"))
        .set("maxItems", max)
        .set("description", desc)
}

/// One admitted shape of a typed field value.
fn field_value_schema(kind: &str, member: &str, value: J) -> J {
    schema_raw(
        vec![("kind", J::obj().set("const", kind)), (member, value)],
        &["kind", member],
    )
    .set("additionalProperties", false)
}

fn typed_value_schema() -> J {
    let ty = |name| J::obj().set("type", name);
    J::obj().set(
        "oneOf",
        vec![
            field_value_schema(
                "int",
                "value",
                ty("integer")
                    .set("minimum", i32::MIN)
                    .set("maximum", i32::MAX),
            ),
            field_value_schema("float", "value", ty("number")),
            field_value_schema("bool", "value", ty("boolean")),
            field_value_schema(
                "color",
                "rgba",
                ty("integer").set("minimum", 0).set("maximum", u32::MAX),
            ),
            field_value_schema("text", "text", ty("string")),
        ],
    )
}

fn replay_log_schema() -> J {
    let ty = |name| J::obj().set("type", name);
    let unsigned = || ty("integer").set("minimum", 0).set("maximum", u32::MAX);
    let digest = || ty("string").set("pattern", "^[0-9a-fA-F]{16}$");
    let point = || vec![("x", ty("integer")), ("y", ty("integer"))];
    let variants = [
        ("pointer", point(), vec!["x", "y"]),
        (
            "down",
            [point(), vec![("button", ty("string"))]].concat(),
            vec!["x", "y", "button"],
        ),
        (
            "up",
            [point(), vec![("button", ty("string"))]].concat(),
            vec!["x", "y", "button"],
        ),
        ("click", point(), vec!["x", "y"]),
        (
            "key",
            vec![
                ("key", ty("string")),
                ("ctrl", ty("boolean")),
                ("shift", ty("boolean")),
                ("alt", ty("boolean")),
                ("logo", ty("boolean")),
            ],
            vec!["key"],
        ),
        ("text", vec![("text", ty("string"))], vec!["text"]),
        (
            "wheel",
            [point(), vec![("dx", ty("number")), ("dy", ty("number"))]].concat(),
            vec!["x", "y", "dx", "dy"],
        ),
        ("tick", vec![("ms", unsigned())], vec!["ms"]),
        (
            "resize",
            vec![
                ("w", ty("integer")),
                ("h", ty("integer")),
                ("scale", ty("number")),
            ],
            vec!["w", "h"],
        ),
        (
            "set",
            vec![("field", ty("string")), ("value", typed_value_schema())],
            vec!["field", "value"],
        ),
        ("settle", vec![], vec![]),
    ];
    let steps: Vec<J> = variants
        .into_iter()
        .map(|(cmd, mut props, mut required)| {
            props.extend([
                ("cmd", J::obj().set("const", cmd)),
                ("digest", digest()),
                ("pixels", digest()),
                ("damage", unsigned()),
                ("revision", unsigned()),
                (
                    "refused",
                    schema_raw(
                        vec![("code", ty("string")), ("reason", ty("string"))],
                        &["code", "reason"],
                    )
                    .set("additionalProperties", false),
                ),
            ]);
            required.extend(["cmd", "digest", "pixels", "damage", "revision"]);
            schema_raw(props, &required).set("additionalProperties", false)
        })
        .collect();
    schema_raw(
        vec![
            (
                "schema",
                ty("integer").set(
                    "enum",
                    vec![
                        J::from(bough_inspect::record::LEGACY_LOG_SCHEMA_VERSION),
                        J::from(bough_inspect::record::LOG_SCHEMA_VERSION),
                    ],
                ),
            ),
            ("app", ty("string")),
            (
                "size",
                schema_raw(
                    vec![
                        ("w", ty("integer")),
                        ("h", ty("integer")),
                        ("scale", ty("number")),
                    ],
                    &["w", "h", "scale"],
                )
                .set("additionalProperties", false),
            ),
            (
                "steps",
                ty("array")
                    .set("maxItems", 1024)
                    .set("items", J::obj().set("oneOf", steps)),
            ),
            ("count", unsigned()),
        ],
        &["schema", "app", "size", "steps"],
    )
    .set("additionalProperties", false)
}

fn batch_schema() -> J {
    let step = schema_raw(
        vec![
            (
                "method",
                J::obj()
                    .set("type", "string")
                    .set("description", "The RPC method name."),
            ),
            (
                "params",
                J::obj().set("description", "The method's parameters (any JSON object)."),
            ),
        ],
        &["method"],
    );
    schema_raw(
        vec![
            (
                "steps",
                J::obj()
                    .set("type", "array")
                    .set("maxItems", 256)
                    .set("items", step),
            ),
            (
                "stop_on_error",
                J::obj()
                    .set("type", "boolean")
                    .set("description", "Stop after the first failing step."),
            ),
        ],
        &["steps"],
    )
}

fn tool(name: &str, description: &str, input: J) -> J {
    J::obj()
        .set("name", name)
        .set("description", description)
        .set("inputSchema", input)
}

/// The tool list. Kept in one place so `tools/list` and the tests agree.
pub fn tools() -> Vec<J> {
    let no = || schema(&[], &[]);
    vec![
        tool(
            "hello",
            "The application's inspector: schema version, app name, methods, capabilities.",
            no(),
        ),
        tool(
            "tree",
            "The live element tree: stores, lists, nodes (kind, source location, typed operands and the expression behind each, the block words it reads, rects, damage), pointers, counters and the accessibility projection.",
            schema(
                &[
                    ("nodes", "boolean", "Include records (default true)."),
                    (
                        "ax",
                        "boolean",
                        "Include the accessibility projection (default true).",
                    ),
                    (
                        "damage",
                        "boolean",
                        "Include the damage bit rows (default true).",
                    ),
                    ("limit", "integer", "At most this many records."),
                ],
                &[],
            ),
        ),
        tool(
            "find",
            "Select records by kind, text, type name, accessibility role, container store, or state (focused, hovered, pressed, visible, has a handler).",
            schema(
                &[
                    (
                        "kind",
                        "string",
                        "The paint kind: fill, text, label, num, outline, ...",
                    ),
                    ("text", "string", "Substring of the record's content text."),
                    ("ty", "string", "Substring of the record's Rust type."),
                    (
                        "role",
                        "string",
                        "Accessibility role (button, list, text-input, ...).",
                    ),
                    (
                        "within",
                        "integer",
                        "A store index; the search is its subtree.",
                    ),
                    ("focused", "boolean", "The focused record."),
                    ("hovered", "boolean", "The record picked under the pointer."),
                    ("pressed", "boolean", "The record pressed."),
                    ("visible", "boolean", "On screen (inside its clip)."),
                    ("handler", "boolean", "Has any handler."),
                    ("ops", "boolean", "Include operands."),
                    ("limit", "integer", "At most this many (default 64)."),
                ],
                &[],
            ),
        ),
        tool(
            "node",
            "One record by index or by its entity (path, row, pos), with the stores it lies in.",
            schema_raw(
                vec![
                    (
                        "index",
                        J::obj()
                            .set("type", "integer")
                            .set("description", "Node index in the last tree."),
                    ),
                    (
                        "path",
                        int_array(6, "Store path (child indices from the root)."),
                    ),
                    (
                        "row",
                        J::obj().set("type", "integer").set(
                            "description",
                            "Row of a list, or the row call of a fixed shape.",
                        ),
                    ),
                    (
                        "pos",
                        J::obj()
                            .set("type", "integer")
                            .set("description", "Position in the row's shape."),
                    ),
                ],
                &[],
            ),
        ),
        tool(
            "pick",
            "The records under a device-pixel point, topmost first, with whether each hears the pointer.",
            schema(
                &[("x", "integer", "Device x."), ("y", "integer", "Device y.")],
                &["x", "y"],
            ),
        ),
        tool(
            "deps",
            "What a state word moves: the records whose operands read it, the columns, and the spaces whose scroll word it is.",
            schema(
                &[
                    ("word", "integer", "Block word index."),
                    ("field", "string", "A registered field's name."),
                ],
                &[],
            ),
        ),
        tool(
            "why_repaint",
            "Why the last turn repainted: the command, the block words that changed, the records reading them, the damage and the delta counters.",
            no(),
        ),
        tool(
            "counters",
            "Per-turn counters: evaluated columns, visited incidences, retables, retained records/pages/bytes, damage, last command timings.",
            no(),
        ),
        tool(
            "state",
            "The typed state fields the application exposes, with values and whether each is writable.",
            no(),
        ),
        tool(
            "set_field",
            "Write a typed state field through the application's own admission; refused with a reason (read-only, wrong kind, out of range, unknown). Record operands are read-only.",
            schema_raw(
                vec![
                    (
                        "field",
                        J::obj()
                            .set("type", "string")
                            .set("description", "Field name from `state`."),
                    ),
                    ("value", typed_value_schema()),
                ],
                &["field", "value"],
            ),
        ),
        tool(
            "pointer_move",
            "Move the pointer (hover) to a device-pixel point.",
            schema(&[("x", "integer", "x"), ("y", "integer", "y")], &["x", "y"]),
        ),
        tool(
            "click",
            "Press and release the primary button at a point.",
            schema(&[("x", "integer", "x"), ("y", "integer", "y")], &["x", "y"]),
        ),
        tool(
            "pointer_down",
            "Press a button at a point.",
            schema(
                &[
                    ("x", "integer", "x"),
                    ("y", "integer", "y"),
                    ("button", "string", "primary, secondary, middle"),
                ],
                &["x", "y"],
            ),
        ),
        tool(
            "pointer_up",
            "Release a button at a point.",
            schema(
                &[
                    ("x", "integer", "x"),
                    ("y", "integer", "y"),
                    ("button", "string", "primary, secondary, middle"),
                ],
                &["x", "y"],
            ),
        ),
        tool(
            "key",
            "Press a key, optionally with modifiers (tab, enter, escape, up, down, delete, home, end, page-up, page-down, char:x).",
            schema(
                &[
                    ("key", "string", "key name"),
                    ("shift", "boolean", "shift"),
                    ("ctrl", "boolean", "ctrl"),
                    ("alt", "boolean", "alt"),
                    ("logo", "boolean", "logo"),
                ],
                &["key"],
            ),
        ),
        tool(
            "type_text",
            "Type text into the focused text handler.",
            schema(&[("text", "string", "text")], &["text"]),
        ),
        tool(
            "wheel",
            "Turn the wheel at a point (lines).",
            schema(
                &[
                    ("x", "integer", "x"),
                    ("y", "integer", "y"),
                    ("dx", "number", "horizontal lines"),
                    ("dy", "number", "vertical lines"),
                ],
                &["dy"],
            ),
        ),
        tool(
            "tick",
            "Advance the clock (a frame, not a turn).",
            schema(&[("ms", "integer", "milliseconds")], &[]),
        ),
        tool(
            "resize",
            "Resize the window (a remount in the benchmark adapter).",
            schema(
                &[
                    ("w", "integer", "width"),
                    ("h", "integer", "height"),
                    ("scale", "number", "device pixels per point"),
                ],
                &["w", "h"],
            ),
        ),
        tool(
            "screenshot",
            "Paint the frame and return it as a PNG image, with its size and digest; with `node`, the part of the frame that element covers.",
            schema(
                &[
                    ("node", "integer", "Node index: a screenshot by element."),
                    ("raw", "boolean", "Also return raw RGBA (base64)."),
                ],
                &[],
            ),
        ),
        tool(
            "diff_golden",
            "Paint the frame and diff it against a golden PNG (base64): differing pixels, bounding box, maximum channel delta.",
            schema(&[("golden", "string", "base64 PNG")], &["golden"]),
        ),
        tool(
            "reload",
            "Ask the application to re-read its own inputs (assets, theme, configuration) and re-describe; state is kept.",
            no(),
        ),
        tool(
            "record_start",
            "Start recording commands for a deterministic replay.",
            no(),
        ),
        tool(
            "record_stop",
            "Stop recording and return a replay log with per-step description and pixel digests.",
            no(),
        ),
        tool(
            "replay",
            "Replay a recorded log on a fresh application of the same kind; reports the first description or pixel that differs.",
            schema_raw(vec![("log", replay_log_schema())], &["log"]),
        ),
        tool(
            "wait_for_idle",
            "Finish a headless frame at the current clock. Reports its revision, pixel digest and whether motion needs another tick.",
            no(),
        ),
        tool(
            "damage_map",
            "The live damage map: damaged rects, tile bitmap, repair percentage, per-tile heat, scroll blit, occlusion and backdrop-blur counts; with include_heatmap_png, a colour-coded overlay (encoded off-thread).",
            schema(
                &[(
                    "include_heatmap_png",
                    "boolean",
                    "Also return the colour-coded heatmap PNG (base64).",
                )],
                &[],
            ),
        ),
        tool(
            "effects_inspect",
            "Active effects for a node or the whole scene: shadows, blurs, gradients and isolated opacity groups.",
            schema(
                &[
                    ("node", "integer", "Node index; omit for the whole scene."),
                    ("node_id", "integer", "Node index (alias)."),
                    ("selector", "string", "Selector (reserved)."),
                ],
                &[],
            ),
        ),
        tool(
            "animations",
            "All live animations and page transitions (id, node, property, kind, elapsed/duration, value, velocity, settled, phase).",
            no(),
        ),
        tool(
            "scrub_animation",
            "Deterministically scrub an animation or page transition (set_time_ms, advance_ms, pause, resume, seek_id).",
            schema(
                &[
                    ("op", "string", "set_time_ms | advance_ms | pause | resume | seek_id."),
                    ("ms", "number", "Milliseconds for set_time_ms/advance_ms."),
                    ("id", "string", "Animation id for seek_id."),
                ],
                &[],
            ),
        ),
        tool(
            "profile_frame",
            "A structured per-phase frame profile: input/eval/layout/damage/paint/submit timings, warm allocation, dirty nodes and tiles, executor.",
            no(),
        ),
        tool(
            "assert_selector",
            "Assert a selector's match count, text, bounding box, visibility or style; returns pass/fail and structured diagnostics.",
            schema(
                &[
                    ("kind", "string", "Paint kind."),
                    ("text", "string", "Substring of content text."),
                    ("ty", "string", "Substring of the record's Rust type."),
                    ("within", "integer", "Store subtree."),
                    ("visible", "boolean", "On screen."),
                    ("min_count", "integer", "At least this many matches."),
                    ("max_count", "integer", "At most this many matches."),
                    ("exact_count", "integer", "Exactly this many matches."),
                    ("text_contains", "string", "The first match's text contains this."),
                    ("text_equals", "string", "The first match's text equals this."),
                    ("x_min", "integer", "Bounds x minimum."),
                    ("x_max", "integer", "Bounds x maximum."),
                    ("y_min", "integer", "Bounds y minimum."),
                    ("y_max", "integer", "Bounds y maximum."),
                    ("w_min", "integer", "Bounds width minimum."),
                    ("w_max", "integer", "Bounds width maximum."),
                    ("h_min", "integer", "Bounds height minimum."),
                    ("h_max", "integer", "Bounds height maximum."),
                    ("style", "string", "Operand name to check."),
                    ("style_equals", "string", "Expected operand value text."),
                ],
                &[],
            ),
        ),
        tool(
            "batch",
            "Run a sequence of RPC commands in one round-trip, with per-step results and stop_on_error.",
            batch_schema(),
        ),
    ]
}

// ---- transports ----------------------------------------------------------------------------------------------

/// Serve newline-delimited JSON-RPC over any reader/writer pair (MCP's stdio transport).
pub fn serve_stream<R: BufRead, W: Write>(
    mcp: &mut Mcp,
    mut input: R,
    mut out: W,
) -> io::Result<()> {
    pump_stream(mcp, &mut input, |reply| {
        writeln!(out, "{}", json::to_string(&reply))?;
        out.flush()
    })
}

/// Encode and write stdio replies on a worker. The 64-reply queue applies backpressure when full.
pub fn serve_stream_owned<R: BufRead + Send + 'static, W: Write + Send + 'static>(
    mcp: &mut Mcp,
    mut input: R,
    out: W,
) -> io::Result<()> {
    #[cfg(feature = "mutant-inline-writer")]
    return serve_stream(mcp, input, out);

    #[cfg(not(feature = "mutant-inline-writer"))]
    serve_stream_worker(mcp, &mut input, out)
}

#[cfg(not(feature = "mutant-inline-writer"))]
fn serve_stream_worker<R: BufRead, W: Write + Send + 'static>(
    mcp: &mut Mcp,
    input: &mut R,
    out: W,
) -> io::Result<()> {
    use std::sync::mpsc;
    const OUT_QUEUE: usize = 64;
    let (tx, rx) = mpsc::sync_channel::<J>(OUT_QUEUE);
    let writer = std::thread::spawn(move || -> io::Result<()> {
        let mut out = out;
        for reply in rx {
            writeln!(out, "{}", json::to_string(&reply))?;
            out.flush()?;
        }
        Ok(())
    });
    let result = pump_stream(mcp, input, |reply| {
        tx.send(reply)
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "the writer stopped"))
    });
    drop(tx);
    match writer.join() {
        Ok(Ok(())) => result,
        Ok(Err(e)) => Err(e),
        Err(_) => Err(io::Error::new(
            io::ErrorKind::Other,
            "the writer thread panicked",
        )),
    }
}

/// The protocol loop over newline-delimited JSON, with the emit step supplied by the transport (so a
/// transport can hand an owned JSON value to its writer).
fn pump_stream<R: BufRead, F: FnMut(J) -> io::Result<()>>(
    mcp: &mut Mcp,
    input: &mut R,
    mut emit: F,
) -> io::Result<()> {
    let mut line = String::new();
    loop {
        line.clear();
        use std::io::Read;
        let limit = bough_inspect::socket::MAX_FRAME;
        let n = input
            .by_ref()
            .take((limit + 2) as u64)
            .read_line(&mut line)?;
        if n > limit + 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "stdio message exceeds frame limit",
            ));
        }
        if n == 0 {
            return Ok(());
        }
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let msg = match json::parse(t) {
            Ok(v) => v,
            Err(e) => {
                let reply = J::obj().set("jsonrpc", "2.0").set("id", J::Null).set(
                    "error",
                    J::obj().set("code", -32700).set("message", format!("{e}")),
                );
                emit(reply)?;
                continue;
            }
        };
        match msg {
            J::Arr(batch) => {
                let mut replies = Vec::new();
                for m in &batch {
                    if let Some(r) = mcp.handle(m) {
                        replies.push(r);
                    }
                }
                if !replies.is_empty() {
                    emit(J::Arr(replies))?;
                }
            }
            m => {
                if let Some(r) = mcp.handle(&m) {
                    emit(r)?;
                }
            }
        }
    }
}

/// Run an HTTP/1.1 server whose `POST /mcp` speaks JSON-RPC (the MCP streamable-HTTP transport, JSON responses).
/// This local JSON-only endpoint binds to loopback, rejects browser Origin headers and refuses GET with 405.
pub fn serve_http(mcp: &mut Mcp, listener: TcpListener) -> io::Result<()> {
    if !listener.local_addr()?.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the inspector HTTP listener must bind to loopback",
        ));
    }
    for conn in listener.incoming() {
        if mcp.closed() {
            return Ok(());
        }
        match conn {
            Ok(s) => {
                if let Err(e) = one_http(mcp, s) {
                    eprintln!("bough-mcp: http: {e}");
                }
            }
            Err(e) => eprintln!("bough-mcp: http accept: {e}"),
        }
        if mcp.closed() {
            return Ok(());
        }
    }
    Ok(())
}

fn one_http(mcp: &mut Mcp, mut s: TcpStream) -> io::Result<()> {
    const HEADER_LIMIT: usize = 16 * 1024;
    let timeout = Some(std::time::Duration::from_secs(10));
    s.set_read_timeout(timeout)?;
    s.set_write_timeout(timeout)?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let n = s.read(&mut chunk)?;
        if n == 0 {
            return Ok(());
        }
        let start = buf.len().saturating_sub(3);
        buf.extend_from_slice(&chunk[..n]);
        if let Some(p) = find(&buf[start..], b"\r\n\r\n") {
            let end = start + p + 4;
            if end > HEADER_LIMIT {
                return respond(&mut s, 431, "text/plain", b"headers too large");
            }
            break end;
        }
        if buf.len() > HEADER_LIMIT {
            return respond(&mut s, 431, "text/plain", b"headers too large");
        }
    };
    let Ok(head) = std::str::from_utf8(&buf[..head_end - 4]) else {
        return respond(&mut s, 400, "text/plain", b"invalid headers");
    };
    let mut lines = head.split("\r\n");
    let mut request = lines.next().unwrap_or("").split_whitespace();
    let (method, path) = (request.next().unwrap_or(""), request.next().unwrap_or(""));
    let mut len = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return respond(&mut s, 400, "text/plain", b"invalid header");
        };
        if name.eq_ignore_ascii_case("origin") {
            return respond(
                &mut s,
                403,
                "text/plain",
                b"browser origins are not allowed",
            );
        }
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return respond(&mut s, 400, "text/plain", b"expected a content length");
        }
        if name.eq_ignore_ascii_case("content-length") {
            let Ok(value) = value.trim().parse::<usize>() else {
                return respond(&mut s, 400, "text/plain", b"invalid content length");
            };
            if len.replace(value).is_some() {
                return respond(&mut s, 400, "text/plain", b"duplicate content length");
            }
        }
    }
    if method == "OPTIONS" {
        return respond(&mut s, 204, "text/plain", b"");
    }
    if method != "POST" || !(path == "/mcp" || path == "/") {
        return respond(&mut s, 405, "text/plain", b"method not allowed");
    }
    let Some(len) = len else {
        return respond(&mut s, 411, "text/plain", b"content length required");
    };
    if len > bough_inspect::socket::MAX_FRAME {
        return respond(&mut s, 413, "text/plain", b"body too large");
    }
    let end = head_end + len;
    while buf.len() < end {
        let next = chunk.len().min(end - buf.len());
        let n = s.read(&mut chunk[..next])?;
        if n == 0 {
            return respond(&mut s, 400, "text/plain", b"incomplete body");
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = &buf[head_end..end];
    let msg = match json::parse(core::str::from_utf8(body).unwrap_or("")) {
        Ok(v) => v,
        Err(e) => {
            let r = J::obj().set("jsonrpc", "2.0").set("id", J::Null).set(
                "error",
                J::obj().set("code", -32700).set("message", format!("{e}")),
            );
            return respond(
                &mut s,
                200,
                "application/json",
                json::to_string(&r).as_bytes(),
            );
        }
    };
    let reply = match msg {
        J::Arr(batch) => {
            let mut rs = Vec::new();
            for m in &batch {
                if let Some(r) = mcp.handle(m) {
                    rs.push(r);
                }
            }
            J::Arr(rs)
        }
        m => match mcp.handle(&m) {
            Some(r) => r,
            None => return respond(&mut s, 202, "application/json", b""),
        },
    };
    // The reply is already a value: its write to this client runs on its own thread, so a reader that
    // takes one byte per tick cannot hold up the accept loop or another client.
    respond_async(s, 200, "application/json", reply)
}

/// Slow readers in flight. The bound keeps a stalled set of connections from growing without limit.
static SLOW_WRITERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
const MAX_SLOW_WRITERS: usize = 64;

fn respond_async(mut s: TcpStream, code: u16, mime: &'static str, body: J) -> io::Result<()> {
    use std::sync::atomic::Ordering;
    if SLOW_WRITERS.fetch_add(1, Ordering::AcqRel) >= MAX_SLOW_WRITERS {
        SLOW_WRITERS.fetch_sub(1, Ordering::AcqRel);
        return respond(&mut s, 503, "text/plain", b"too many slow readers");
    }
    std::thread::spawn(move || {
        struct Done;
        impl Drop for Done {
            fn drop(&mut self) {
                SLOW_WRITERS.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
            }
        }
        let _done = Done;
        let body = json::to_string(&body);
        let _ = respond(&mut s, code, mime, body.as_bytes());
    });
    Ok(())
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

fn respond(s: &mut TcpStream, code: u16, mime: &str, body: &[u8]) -> io::Result<()> {
    let reason = match code {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        405 => "Method Not Allowed",
        411 => "Length Required",
        413 => "Content Too Large",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "OK",
    };
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    s.write_all(head.as_bytes())?;
    s.write_all(body)?;
    s.flush()
}

/// The inspector a `bough-mcp` process should attach to: `--socket`, or `BOUGH_INSPECT_SOCKET`.
pub fn socket_from_env() -> Option<PathBuf> {
    std::env::var_os("BOUGH_INSPECT_SOCKET").map(PathBuf::from)
}

pub use client::call;
