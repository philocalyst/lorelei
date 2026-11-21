//! TASK 3 hardening: nested schemas are real schemas, a slow reader does not hold the turn, random and
//! truncated input is refused without a panic or unbounded memory, and a stale socket is reclaimed.
use bough_inspect::json::{self, Value as J};
use bough_inspect::socket::{Server, read_frame, write_frame};
use bough_mcp::{Mcp, serve_stream, serve_stream_owned};
use std::io::{Cursor, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn temp_socket(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("bm-h-{}-{tag}.sock", std::process::id()))
}

/// A fake inspector that answers minimally and records the methods it saw.
fn fake_app(tag: &str, seen: Arc<Mutex<Vec<String>>>) -> (PathBuf, std::thread::JoinHandle<()>) {
    let path = temp_socket(tag);
    let _ = std::fs::remove_file(&path);
    let l = UnixListener::bind(&path).unwrap();
    let p = path.clone();
    let h = std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        while let Ok(frame) = read_frame(&mut s) {
            let req = json::parse(std::str::from_utf8(&frame).unwrap()).unwrap();
            let id = req.get("id").cloned().unwrap_or(J::Null);
            let method = req
                .get("method")
                .and_then(J::as_str)
                .unwrap_or("")
                .to_owned();
            seen.lock().unwrap().push(method.clone());
            let result = match method.as_str() {
                "hello" => J::obj().set("schema", 1).set("app", "fake"),
                "shot" => {
                    let png = bough_inspect::png::encode(1, 1, &[0xff00_00ff]);
                    J::obj()
                        .set("w", 1)
                        .set("h", 1)
                        .set("digest", "00")
                        .set("bytes", png.len())
                        .set("png", json::base64_encode(&png))
                }
                "shutdown" => {
                    let resp = J::obj()
                        .set("jsonrpc", "2.0")
                        .set("id", id.clone())
                        .set("result", J::obj());
                    let _ = write_frame(&mut s, json::to_string(&resp).as_bytes());
                    break;
                }
                _ => J::obj().set("echo", method.clone()),
            };
            let resp = J::obj()
                .set("jsonrpc", "2.0")
                .set("id", id)
                .set("result", result);
            write_frame(&mut s, json::to_string(&resp).as_bytes()).unwrap();
        }
        let _ = std::fs::remove_file(&p);
    });
    (path, h)
}

fn rss_kb() -> i64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or(0)
}

// ---- schemas -------------------------------------------------------------------------------------------------

fn members<'a>(v: &'a J) -> &'a [(String, J)] {
    match v {
        J::Obj(m) => m,
        _ => &[],
    }
}

fn walk_schema(v: &J, depth: usize, path: &str, problems: &mut Vec<String>) {
    if let Some(t) = v.get("type").and_then(J::as_str) {
        if t == "object" && depth > 0 {
            let props = v.get("properties").map(members).unwrap_or(&[]);
            if props.is_empty() {
                problems.push(format!("{path}: a nested object without properties"));
            }
        }
        if t == "array" && v.get("items").is_none() {
            problems.push(format!("{path}: an array without items"));
        }
    }
    for (k, x) in v.get("properties").map(members).unwrap_or(&[]) {
        walk_schema(x, depth + 1, &format!("{path}.{k}"), problems);
    }
    if let Some(J::Arr(one)) = v.get("oneOf") {
        for (i, x) in one.iter().enumerate() {
            walk_schema(x, depth + 1, &format!("{path}|oneOf[{i}]"), problems);
        }
    }
    if let Some(items) = v.get("items") {
        walk_schema(items, depth + 1, &format!("{path}[]"), problems);
    }
}

#[test]
fn every_nested_schema_is_a_real_schema() {
    let mut problems = Vec::new();
    let tools = bough_mcp::tools();
    assert!(tools.len() >= 20, "{} tools", tools.len());
    for t in &tools {
        let name = t.get("name").and_then(J::as_str).unwrap();
        let input = t.get("inputSchema").expect("an input schema");
        assert_eq!(
            input.get("type").and_then(J::as_str),
            Some("object"),
            "{name}"
        );
        walk_schema(input, 0, name, &mut problems);
        // required names are properties
        let props = input.get("properties").map(members).unwrap_or(&[]);
        for r in input.get("required").map(J::arr).unwrap_or(&[]) {
            let r = r.as_str().unwrap_or("");
            assert!(
                props.iter().any(|(k, _)| k == r),
                "{name}: required {r} is not a property"
            );
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
    // the two formerly bare objects are now specific
    let tool = |n: &str| {
        tools
            .iter()
            .find(|t| t.get("name").and_then(J::as_str) == Some(n))
            .unwrap()
            .clone()
    };
    let v = tool("set_field").get("inputSchema").unwrap().clone();
    let val = v.get("properties").unwrap().get("value").unwrap();
    assert!(val.get("oneOf").is_some(), "set_field.value has no shapes");
    assert_eq!(
        val.get("type"),
        None,
        "set_field.value is not a bare object"
    );
    let log = tool("replay").get("inputSchema").unwrap().clone();
    let l = log.get("properties").unwrap().get("log").unwrap();
    assert!(
        l.get("properties").is_some(),
        "replay.log names its members"
    );
}

// ---- a slow reader -------------------------------------------------------------------------------------------

/// A writer that takes 10 ms per byte for its first 100 bytes (a client reading slowly), then drains.
struct Slow {
    inner: Arc<Mutex<Vec<u8>>>,
    written: Arc<AtomicUsize>,
}
impl Write for Slow {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        for &b in buf.iter().take(1) {
            if self.written.load(Ordering::Relaxed) < 100 {
                std::thread::sleep(Duration::from_millis(10));
            }
            self.inner.lock().unwrap().push(b);
            self.written.fetch_add(1, Ordering::Relaxed);
        }
        let _ = buf;
        Ok(1)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_slow_reader_does_not_hold_the_turn() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (path, app) = fake_app("slow", Arc::clone(&seen));
    let mut mcp = Mcp::new(&path);
    let input = Cursor::new(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n\
         {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"screenshot\",\"arguments\":{}}}\n\
         {\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"hello\",\"arguments\":{}}}\n\
         {\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"shutdown\",\"params\":{}}\n",
    );
    let inner = Arc::new(Mutex::new(Vec::new()));
    let written = Arc::new(AtomicUsize::new(0));
    let out = Slow {
        inner: Arc::clone(&inner),
        written: Arc::clone(&written),
    };
    let server = std::thread::spawn(move || serve_stream_owned(&mut mcp, input, out));
    // The turn must reach the third request while the first big reply is still being written slowly.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut reached = false;
    while Instant::now() < deadline {
        let methods = seen.lock().unwrap().clone();
        if methods.len() >= 3 {
            assert!(
                written.load(Ordering::Relaxed) < 100,
                "the server waited for the slow writer ({} bytes already)",
                written.load(Ordering::Relaxed)
            );
            reached = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(reached, "the request loop did not reach the third message");
    server.join().unwrap().expect("the server ended cleanly");
    app.join().unwrap();
    let text = String::from_utf8(inner.lock().unwrap().clone()).unwrap();
    assert!(
        text.contains("\"id\":3"),
        "the later reply was written: {text}"
    );
}

// ---- random and truncated input ------------------------------------------------------------------------------

fn run(input: String) -> std::io::Result<()> {
    let path = temp_socket("fuzz");
    let _ = std::fs::remove_file(&path);
    let mut mcp = Mcp::new(&path);
    let mut sink = std::io::sink();
    serve_stream(&mut mcp, Cursor::new(input), &mut sink)
}

#[test]
fn random_and_truncated_frames_are_refused_without_a_panic_or_unbounded_memory() {
    let before = rss_kb();
    // deterministic pseudo-random printable JSON-ish text and truncated requests
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    let mut big = String::new();
    for _ in 0..400 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let n = (x as usize) % 200 + 1;
        let mut h = x;
        big.push('{');
        for _ in 0..n {
            h = h.wrapping_mul(6364136223846793005).wrapping_add(1);
            let c = b" \t{}[]\":,abc0123456789\\\n"[((h >> 33) as usize) % 24];
            big.push(c as char);
        }
        big.push('\n');
    }
    let start = Instant::now();
    let r = run(big);
    assert!(r.is_ok() || r.is_err(), "it returned");
    assert!(
        start.elapsed() < Duration::from_secs(30),
        "{:?}",
        start.elapsed()
    );
    // truncated: no panic and a clean end
    for text in [
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/ca\n",
        "{\n",
        "\u{0}\u{1}\u{2}\n",
        &"a".repeat(65 * 1024 * 1024),
    ] {
        let _ = run(text.to_owned());
    }
    let after = rss_kb();
    assert!(
        after - before < 512 * 1024,
        "memory grew by {} KiB over the fuzz loop",
        after - before
    );
}

// ---- stale socket --------------------------------------------------------------------------------------------

#[test]
fn a_stale_socket_is_reclaimed_and_a_live_one_is_not() {
    let path = temp_socket("stale");
    let _ = std::fs::remove_file(&path);
    // a process that died leaves the file behind: bind, then drop the listener without unlinking
    let l = UnixListener::bind(&path).unwrap();
    drop(l);
    assert!(path.exists(), "the stale file is still there");
    // Another test may fork while this listener closes. Wait for any inherited
    // descriptor to close at exec before treating the socket as stale.
    let deadline = Instant::now() + Duration::from_secs(5);
    let server = loop {
        match Server::bind(&path) {
            Ok(server) => break server,
            Err(error)
                if error.kind() == std::io::ErrorKind::AddrInUse && Instant::now() < deadline =>
            {
                std::thread::yield_now();
            }
            Err(error) => panic!("a stale socket is reclaimed: {error}"),
        }
    };
    let live = Server::bind(&path);
    assert!(live.is_err(), "a live socket is not stolen");
    let client = UnixStream::connect(&path);
    assert!(client.is_ok(), "the reclaimed socket accepts a client");
    drop(client);
    drop(server);
    assert!(!path.exists(), "the server unlinked its own socket");
    // a non-socket path is left for bind to refuse
    std::fs::write(&path, "keep me").unwrap();
    assert!(Server::bind(&path).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep me");
    let _ = std::fs::remove_file(&path);
}
