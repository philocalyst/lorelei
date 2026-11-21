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
    protocol: String,
    /// A `shutdown` request: the transports stop.
    closed: bool,
}
impl Mcp {
    pub fn new(socket: impl Into<PathBuf>) -> Mcp {
        Mcp {
            socket: socket.into(),
            client: None,
            protocol: PROTOCOL.to_owned(),
            closed: false,
        }
    }
    pub fn closed(&self) -> bool {
        self.closed
    }
    fn client(&mut self) -> io::Result<&mut Client> {
        if self.client.is_none() {
            self.client = Some(Client::open(&self.socket)?);
        }
        Ok(self.client.as_mut().unwrap())
    }
    /// Call the inspector. A dead connection is reopened once (an app restart keeps the same socket path).
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
                            .set("resources", J::obj().set("subscribe", false).set("listChanged", false)),
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
                    resource("lorelei://tree", "tree", "The live element tree (JSON)", "application/json"),
                    resource("lorelei://counters", "counters", "Per-turn counters and timings", "application/json"),
                    resource("lorelei://screenshot", "screenshot", "The current frame (PNG)", "image/png"),
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
            Ok(v) => J::obj().set("jsonrpc", "2.0").set("id", id).set("result", v),
            Err(e) => {
                let (code, message) = match e.split_once(':') {
                    Some((c, m)) if c.starts_with('-') => (
                        c.parse::<i64>().unwrap_or(-32603),
                        m.trim().to_owned(),
                    ),
                    _ => (-32603, e),
                };
                J::obj().set("jsonrpc", "2.0").set("id", id).set(
                    "error",
                    J::obj().set("code", code).set("message", message),
                )
            }
        })
    }

    fn tools_call(&mut self, p: &J) -> Result<J, String> {
        let name = p.get("name").and_then(J::as_str).ok_or("-32602: tools/call needs a name")?;
        let a = p.get("arguments").cloned().unwrap_or(J::Null);
        let (text, image) = self.run_tool(name, &a)?;
        // an admission's refusal is a tool error (the model can correct the call); everything else is a result
        let is_error = text.starts_with("-320") || text.starts_with("-326");
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
                let p = J::obj()
                    .set("x", req_i(a, "x")?)
                    .set("y", req_i(a, "y")?);
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
                match self.call("set", &p) {
                    Ok(v) => Ok((j(v), None)),
                    Err(e) => Ok((e, None)), // the admission's reason, as the tool's answer
                }
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
                    .set("button", a.get("button").cloned().unwrap_or(J::from("primary")));
                Ok((j(self.call("down", &p)?), None))
            }
            "pointer_up" => {
                let p = J::obj()
                    .set("x", req_i(a, "x")?)
                    .set("y", req_i(a, "y")?)
                    .set("button", a.get("button").cloned().unwrap_or(J::from("primary")));
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
                let v = self.call("shot", &p)?;
                let png = v.get("png").and_then(J::as_str).unwrap_or("").to_owned();
                let summary = J::obj()
                    .set("w", v.get("w").cloned().unwrap_or(J::Null))
                    .set("h", v.get("h").cloned().unwrap_or(J::Null))
                    .set("digest", v.get("digest").cloned().unwrap_or(J::Null))
                    .set("bytes", v.get("bytes").cloned().unwrap_or(J::Null));
                Ok((j(summary), Some(png)))
            }
            "diff_golden" | "diff" => {
                let p = J::obj().set(
                    "golden",
                    a.get("golden").and_then(J::as_str).ok_or("-32602: golden (base64 PNG)")?,
                );
                Ok((j(self.call("diff", &p)?), None))
            }
            "record_start" => Ok((j(self.call("record_start", &J::obj())?), None)),
            "record_stop" => Ok((j(self.call("record_stop", &J::obj())?), None)),
            "replay" => {
                let p = J::obj().set(
                    "log",
                    a.get("log").cloned().ok_or("-32602: log")?,
                );
                Ok((j(self.call("replay", &p)?), None))
            }
            "wait_for_idle" => {
                // the inspector is synchronous: a command returns after its turn. Report the state that shows.
                let v = self.call("counters", &J::obj())?;
                Ok((
                    j(J::obj()
                        .set("idle", true)
                        .set("revision", v.get("revision").cloned().unwrap_or(J::Null))
                        .set("damage", v.get("damage_tiles").cloned().unwrap_or(J::Null))),
                    None,
                ))
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
                J::obj().set("text", self.call("counters", &J::obj())?.pipe(json::to_string)),
                "application/json",
            ),
            "lorelei://screenshot" => {
                let v = self.call("shot", &J::obj())?;
                let png = v.get("png").and_then(J::as_str).unwrap_or("").to_owned();
                (J::obj().set("blob", png), "image/png")
            }
            u if u.starts_with("lorelei://node/") => {
                let i: i64 = u["lorelei://node/".len()..]
                    .parse()
                    .map_err(|_| "-32602: node index")?;
                let v = self.call("node", &J::obj().set("index", i))?;
                (J::obj().set("text", json::to_string(&v)), "application/json")
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
        p.put(
            name,
            J::obj().set("type", *kind).set("description", *desc),
        );
    }
    J::obj()
        .set("type", "object")
        .set("properties", p)
        .set(
            "required",
            required.iter().map(|&r| J::Str(r.to_owned())).collect::<Vec<_>>(),
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
        tool("hello", "The application's inspector: schema version, app name, methods, capabilities.", no()),
        tool("tree", "The live element tree: stores, lists, nodes (kind, source location, typed operands and the expression behind each, the block words it reads, rects, damage), pointers, counters and the accessibility projection.", schema(&[
            ("nodes", "boolean", "Include records (default true)."),
            ("ax", "boolean", "Include the accessibility projection (default true)."),
            ("damage", "boolean", "Include the damage bit rows (default true)."),
            ("limit", "integer", "At most this many records."),
        ], &[])),
        tool("find", "Select records by kind, text, type name, accessibility role, container store, or state (focused, hovered, pressed, visible, has a handler).", schema(&[
            ("kind", "string", "The paint kind: fill, text, label, num, outline, ..."),
            ("text", "string", "Substring of the record's content text."),
            ("ty", "string", "Substring of the record's Rust type."),
            ("role", "string", "Accessibility role (button, list, text-input, ...)."),
            ("within", "integer", "A store index; the search is its subtree."),
            ("focused", "boolean", "The focused record."),
            ("hovered", "boolean", "The record picked under the pointer."),
            ("pressed", "boolean", "The record pressed."),
            ("visible", "boolean", "On screen (inside its clip)."),
            ("handler", "boolean", "Has any handler."),
            ("ops", "boolean", "Include operands."),
            ("limit", "integer", "At most this many (default 64)."),
        ], &[])),
        tool("node", "One record by index or by its entity (path, row, pos), with the stores it lies in.", schema(&[
            ("index", "integer", "Node index in the last tree."),
            ("path", "array", "Store path (child indices from the root)."),
            ("row", "integer", "Row of a list, or the row call of a fixed shape."),
            ("pos", "integer", "Position in the row's shape."),
        ], &[])),
        tool("pick", "The records under a device-pixel point, topmost first, with whether each hears the pointer.", schema(&[("x", "integer", "Device x."), ("y", "integer", "Device y.")], &["x", "y"])),
        tool("deps", "What a state word moves: the records whose operands read it, the columns, and the spaces whose scroll word it is.", schema(&[("word", "integer", "Block word index."), ("field", "string", "A registered field's name.")], &[])),
        tool("why_repaint", "Why the last turn repainted: the command, the block words that changed, the records reading them, the damage and the delta counters.", no()),
        tool("counters", "Per-turn counters: evaluated columns, visited incidences, retables, retained records/pages/bytes, damage, last command timings.", no()),
        tool("state", "The typed state fields the application exposes, with values and whether each is writable.", no()),
        tool("set_field", "Write a typed state field through the application's own admission; refused with a reason (read-only, wrong kind, out of range, unknown). Record operands are read-only.", schema(&[
            ("field", "string", "Field name from `state`."),
            ("value", "object", "{\"kind\":\"int\"|\"float\"|\"bool\"|\"color\"|\"text\", ...}"),
        ], &["field", "value"])),
        tool("pointer_move", "Move the pointer (hover) to a device-pixel point.", schema(&[("x", "integer", "x"), ("y", "integer", "y")], &["x", "y"])),
        tool("click", "Press and release the primary button at a point.", schema(&[("x", "integer", "x"), ("y", "integer", "y")], &["x", "y"])),
        tool("pointer_down", "Press a button at a point.", schema(&[("x", "integer", "x"), ("y", "integer", "y"), ("button", "string", "primary, secondary, middle")], &["x", "y"])),
        tool("pointer_up", "Release a button at a point.", schema(&[("x", "integer", "x"), ("y", "integer", "y"), ("button", "string", "primary, secondary, middle")], &["x", "y"])),
        tool("key", "Press a key, optionally with modifiers (tab, enter, escape, up, down, delete, home, end, page-up, page-down, char:x).", schema(&[("key", "string", "key name"), ("shift", "boolean", "shift"), ("ctrl", "boolean", "ctrl"), ("alt", "boolean", "alt"), ("logo", "boolean", "logo")], &["key"])),
        tool("type_text", "Type text into the focused text handler.", schema(&[("text", "string", "text")], &["text"])),
        tool("wheel", "Turn the wheel at a point (lines).", schema(&[("x", "integer", "x"), ("y", "integer", "y"), ("dx", "number", "horizontal lines"), ("dy", "number", "vertical lines")], &["dy"])),
        tool("tick", "Advance the clock (a frame, not a turn).", schema(&[("ms", "integer", "milliseconds")], &[])),
        tool("resize", "Resize the window (a remount in the benchmark adapter).", schema(&[("w", "integer", "width"), ("h", "integer", "height"), ("scale", "number", "device pixels per point")], &["w", "h"])),
        tool("screenshot", "Paint the whole frame and return it as a PNG image, with its size and digest.", schema(&[("raw", "boolean", "Also return raw RGBA (base64).")], &[])),
        tool("diff_golden", "Paint the frame and diff it against a golden PNG (base64): differing pixels, bounding box, maximum channel delta.", schema(&[("golden", "string", "base64 PNG")], &["golden"])),
        tool("record_start", "Start recording commands for a deterministic replay.", no()),
        tool("record_stop", "Stop recording and return the commands.", no()),
        tool("replay", "Replay a recorded log on a fresh application of the same kind; reports the first description or pixel that differs.", schema(&[("log", "object", "the log from record_stop, plus digests")], &["log"])),
        tool("wait_for_idle", "The inspector answers after each command's turn; reports that nothing is pending and the current revision and damage.", no()),
    ]
}

// ---- transports ----------------------------------------------------------------------------------------------

/// Serve newline-delimited JSON-RPC over any reader/writer pair (MCP's stdio transport).
pub fn serve_stream<R: BufRead, W: Write>(mcp: &mut Mcp, mut input: R, mut out: W) -> io::Result<()> {
    let mut line = String::new();
    loop {
        line.clear();
        let n = input.read_line(&mut line)?;
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
                let reply = J::obj()
                    .set("jsonrpc", "2.0")
                    .set("id", J::Null)
                    .set(
                        "error",
                        J::obj().set("code", -32700).set("message", format!("{e}")),
                    );
                writeln!(out, "{}", json::to_string(&reply))?;
                out.flush()?;
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
                    writeln!(out, "{}", json::to_string(&J::Arr(replies)))?;
                    out.flush()?;
                }
            }
            m => {
                if let Some(r) = mcp.handle(&m) {
                    writeln!(out, "{}", json::to_string(&r))?;
                    out.flush()?;
                }
            }
        }
    }
}

/// Run an HTTP/1.1 server whose `POST /mcp` speaks JSON-RPC (the MCP streamable-HTTP transport, JSON responses).
/// `GET` is refused with 405; a client that accepts only `text/event-stream` is answered with JSON anyway, which
/// the specification permits for a server that does not stream.
pub fn serve_http(mcp: &mut Mcp, listener: TcpListener) -> io::Result<()> {
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
    s.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let (head_end, len) = loop {
        let n = s.read(&mut chunk)?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(p) = find(&buf, b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..p]).to_string();
            let len = head
                .lines()
                .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                .unwrap_or(0);
            break (p + 4, len);
        }
        if buf.len() > 8 * 1024 * 1024 {
            return Ok(());
        }
    };
    while buf.len() < head_end + len {
        let n = s.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let first = head.lines().next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");
    if method == "OPTIONS" {
        return respond(&mut s, 204, "text/plain", b"");
    }
    if method != "POST" || !(path == "/mcp" || path == "/") {
        return respond(&mut s, 405, "text/plain", b"method not allowed");
    }
    let body = &buf[head_end..(head_end + len).min(buf.len())];
    let msg = match json::parse(core::str::from_utf8(body).unwrap_or("")) {
        Ok(v) => v,
        Err(e) => {
            let r = J::obj()
                .set("jsonrpc", "2.0")
                .set("id", J::Null)
                .set("error", J::obj().set("code", -32700).set("message", format!("{e}")));
            return respond(&mut s, 200, "application/json", json::to_string(&r).as_bytes());
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
    respond(&mut s, 200, "application/json", json::to_string(&reply).as_bytes())
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

fn respond(s: &mut TcpStream, code: u16, mime: &str, body: &[u8]) -> io::Result<()> {
    let reason = match code {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        405 => "Method Not Allowed",
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
