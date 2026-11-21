//! The MCP handler against a fake inspector: message shapes, the tool list, resources, the image content, the
//! refusal as a tool error, the stdio loop and the HTTP transport. The real apps are driven in
//! `bough-inspect-app/tests/mcp_e2e.rs`; this file holds the protocol still.
use bough_inspect::json::{self, Value as J};
use bough_inspect::png;
use bough_inspect::socket::{read_frame, write_frame};
use bough_mcp::{Mcp, serve_http, serve_stream};
use std::io::{Cursor, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;

/// A fake inspector that answers the methods the MCP server uses, and records the last request.
fn fake_app(tag: &str) -> (PathBuf, std::thread::JoinHandle<()>) {
    let path = std::env::temp_dir().join(format!("bm-{}-{tag}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let l = UnixListener::bind(&path).unwrap();
    let p = path.clone();
    let h = std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        loop {
            let frame = match read_frame(&mut s) {
                Ok(f) => f,
                Err(_) => break,
            };
            let req = json::parse(std::str::from_utf8(&frame).unwrap()).unwrap();
            let id = req.get("id").cloned().unwrap_or(J::Null);
            let method = req.get("method").and_then(J::as_str).unwrap_or("");
            let reply = match method {
                "hello" => J::obj().set("result", J::obj().set("schema", 1).set("app", "fake")),
                "tree" => J::obj().set(
                    "result",
                    J::obj()
                        .set("schema", 1)
                        .set("nodes", vec![J::obj().set("kind", "fill")]),
                ),
                "counters" => J::obj().set("result", J::obj().set("revision", 3)),
                "shot" => {
                    let png = png::encode(1, 1, &[0xff00_00ff]);
                    J::obj().set(
                        "result",
                        J::obj()
                            .set("w", 1)
                            .set("h", 1)
                            .set("digest", "00")
                            .set("bytes", png.len())
                            .set("png", json::base64_encode(&png)),
                    )
                }
                "set" => {
                    let field = req
                        .get("params")
                        .and_then(|p| p.get("field"))
                        .and_then(J::as_str)
                        .unwrap_or("");
                    if field == "ro" {
                        J::obj().set(
                            "error",
                            J::obj()
                                .set("code", -32010)
                                .set("message", "field \"ro\" is read-only: derived")
                                .set("data", J::obj().set("code", "read-only")),
                        )
                    } else {
                        J::obj().set("result", J::obj().set("command", "set"))
                    }
                }
                "shutdown" => {
                    let _ = write_frame(
                        &mut s,
                        json::to_string(
                            &J::obj()
                                .set("jsonrpc", "2.0")
                                .set("id", id)
                                .set("result", J::obj()),
                        )
                        .as_bytes(),
                    );
                    break;
                }
                _ => J::obj().set("result", J::obj().set("echo", method)),
            };
            let mut resp = J::obj().set("jsonrpc", "2.0").set("id", id);
            if let Some(e) = reply.get("error") {
                resp.put("error", e.clone());
            } else {
                resp.put("result", reply.get("result").cloned().unwrap_or(J::Null));
            }
            write_frame(&mut s, json::to_string(&resp).as_bytes()).unwrap();
        }
        let _ = std::fs::remove_file(&p);
    });
    (path, h)
}

fn rpc(mcp: &mut Mcp, id: i64, method: &str, params: J) -> J {
    mcp.handle(
        &J::obj()
            .set("jsonrpc", "2.0")
            .set("id", id)
            .set("method", method)
            .set("params", params),
    )
    .expect("a response")
}

#[test]
fn protocol_shapes_and_tools() {
    let (path, h) = fake_app("shapes");
    let mut m = Mcp::new(&path);
    let init = rpc(
        &mut m,
        1,
        "initialize",
        J::obj()
            .set("protocolVersion", "2025-06-18")
            .set("capabilities", J::obj()),
    );
    assert_eq!(init.get("jsonrpc").and_then(J::as_str), Some("2.0"));
    let r = init.get("result").unwrap();
    assert_eq!(
        r.get("protocolVersion").and_then(J::as_str),
        Some("2025-06-18")
    );
    assert_eq!(
        r.get("serverInfo").unwrap().get("name").and_then(J::as_str),
        Some("bough-mcp")
    );

    assert!(
        m.handle(
            &J::obj()
                .set("jsonrpc", "2.0")
                .set("method", "notifications/initialized")
        )
        .is_none()
    );
    assert_eq!(
        rpc(&mut m, 2, "ping", J::obj()).get("result"),
        Some(&J::obj())
    );

    let tools = rpc(&mut m, 3, "tools/list", J::obj());
    let list = tools.get("result").unwrap().get("tools").unwrap().arr();
    let names: Vec<&str> = list
        .iter()
        .map(|t| t.get("name").and_then(J::as_str).unwrap())
        .collect();
    for need in [
        "tree",
        "find",
        "node",
        "pick",
        "deps",
        "why_repaint",
        "set_field",
        "screenshot",
        "diff_golden",
        "replay",
        "key",
        "type_text",
    ] {
        assert!(names.contains(&need), "{need} is missing from {names:?}");
    }
    assert!(names.len() >= 20, "{} tools", names.len());
    for t in list {
        assert!(
            t.get("inputSchema")
                .unwrap()
                .get("type")
                .and_then(J::as_str)
                == Some("object")
        );
    }

    // a tool call is JSON in a text block
    let tree = rpc(
        &mut m,
        4,
        "tools/call",
        J::obj().set("name", "tree").set("arguments", J::obj()),
    );
    let content = tree.get("result").unwrap().get("content").unwrap().arr();
    let body: J = json::parse(content[0].get("text").and_then(J::as_str).unwrap()).unwrap();
    assert_eq!(body.get("nodes").unwrap().arr().len(), 1);

    // a refusal is a tool error with the reason
    let bad = rpc(
        &mut m,
        5,
        "tools/call",
        J::obj().set("name", "set_field").set(
            "arguments",
            J::obj()
                .set("field", "ro")
                .set("value", J::obj().set("kind", "int").set("value", 1)),
        ),
    );
    let rr = bad.get("result").unwrap();
    assert_eq!(rr.get("isError").and_then(J::as_bool), Some(true));
    let text = rr.get("content").unwrap().arr()[0]
        .get("text")
        .and_then(J::as_str)
        .unwrap();
    assert!(text.contains("read-only"), "{text}");

    // the image content of a screenshot
    let shot = rpc(
        &mut m,
        6,
        "tools/call",
        J::obj()
            .set("name", "screenshot")
            .set("arguments", J::obj()),
    );
    let content = shot.get("result").unwrap().get("content").unwrap().arr();
    assert_eq!(content.len(), 2);
    assert_eq!(content[1].get("type").and_then(J::as_str), Some("image"));
    assert_eq!(
        content[1].get("mimeType").and_then(J::as_str),
        Some("image/png")
    );
    let bytes = json::base64_decode(content[1].get("data").and_then(J::as_str).unwrap()).unwrap();
    assert!(png::decode(&bytes).is_ok());

    // resources
    let rs = rpc(&mut m, 7, "resources/list", J::obj());
    let uris: Vec<&str> = rs
        .get("result")
        .unwrap()
        .get("resources")
        .unwrap()
        .arr()
        .iter()
        .map(|r| r.get("uri").and_then(J::as_str).unwrap())
        .collect();
    assert!(uris.contains(&"lorelei://tree"));
    assert!(uris.contains(&"lorelei://screenshot"));
    let read = rpc(
        &mut m,
        8,
        "resources/read",
        J::obj().set("uri", "lorelei://tree"),
    );
    let c = &read.get("result").unwrap().get("contents").unwrap().arr()[0];
    assert_eq!(
        c.get("mimeType").and_then(J::as_str),
        Some("application/json")
    );
    assert!(c.get("text").and_then(J::as_str).unwrap().contains("nodes"));
    let shotr = rpc(
        &mut m,
        9,
        "resources/read",
        J::obj().set("uri", "lorelei://screenshot"),
    );
    assert_eq!(
        shotr.get("result").unwrap().get("contents").unwrap().arr()[0]
            .get("mimeType")
            .and_then(J::as_str),
        Some("image/png")
    );

    // an unknown method is an RPC error, not a result
    let bad = rpc(&mut m, 10, "no/such", J::obj());
    assert_eq!(
        bad.get("error").unwrap().get("code").and_then(J::as_i64),
        Some(-32601)
    );

    rpc(&mut m, 11, "shutdown", J::obj());
    drop(m);
    h.join().unwrap();
}

#[test]
fn stdio_loop_is_newline_framed() {
    let (path, h) = fake_app("stdio");
    let mut m = Mcp::new(&path);
    let input = Cursor::new(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n\
         {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{}}\n\
         \n\
         {\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"hello\",\"arguments\":{}}}\n\
         {\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"ping\",\"params\":{}}\n",
    );
    let mut out = Vec::new();
    serve_stream(&mut m, input, &mut out).unwrap();
    let text = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 4, "{text}");
    for l in lines {
        let v = json::parse(l).unwrap();
        assert_eq!(v.get("jsonrpc").and_then(J::as_str), Some("2.0"));
    }
    drop(m);
    h.join().unwrap();
}

#[test]
fn http_post_is_streamable_http() {
    let (path, h) = fake_app("http");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let t = std::thread::spawn(move || {
        let mut m = Mcp::new(path);
        // one request only, then the test stops the listener by dropping it in the client thread? serve_http loops;
        // it ends when the listener is closed by the process, so this thread is detached after the assertions.
        let _ = serve_http(&mut m, listener);
    });
    let body = json::to_string(
        &J::obj()
            .set("jsonrpc", "2.0")
            .set("id", 1)
            .set("method", "initialize")
            .set("params", J::obj()),
    );
    let req = format!(
        "POST /mcp HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.write_all(req.as_bytes()).unwrap();
    let mut resp = String::new();
    use std::io::Read;
    s.read_to_string(&mut resp).unwrap();
    assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
    assert!(
        resp.to_ascii_lowercase()
            .contains("content-type: application/json")
    );
    let body = resp.split("\r\n\r\n").nth(1).unwrap();
    assert!(body.contains("\"serverInfo\""));
    // a tool call that reaches the app (and opens the inspector connection)
    let body = json::to_string(
        &J::obj()
            .set("jsonrpc", "2.0")
            .set("id", 3)
            .set("method", "tools/call")
            .set(
                "params",
                J::obj().set("name", "tree").set("arguments", J::obj()),
            ),
    );
    let req = format!(
        "POST /mcp HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.write_all(req.as_bytes()).unwrap();
    let mut resp = String::new();
    s.read_to_string(&mut resp).unwrap();
    assert!(resp.contains("nodes"), "{resp}");
    // GET is refused
    let req = "GET /mcp HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n";
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.write_all(req.as_bytes()).unwrap();
    let mut resp = String::new();
    s.read_to_string(&mut resp).unwrap();
    assert!(resp.starts_with("HTTP/1.1 405"), "{resp}");
    // closing the MCP connection lets the fake app leave, so the thread can be joined
    let body = json::to_string(
        &J::obj()
            .set("jsonrpc", "2.0")
            .set("id", 2)
            .set("method", "shutdown")
            .set("params", J::obj()),
    );
    let req = format!(
        "POST /mcp HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.write_all(req.as_bytes()).unwrap();
    let mut resp = String::new();
    s.read_to_string(&mut resp).unwrap();
    assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
    h.join().unwrap();
    drop(t);
}
