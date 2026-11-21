//! G3: the agent/test socket end to end against two real apps. Each test spawns the app binary, speaks the
//! length-framed JSON-RPC protocol through `bough_inspect::client`, and checks the tree, a pick, input, a typed
//! mutation, a screenshot PNG and a pixel diff. The protocol's own refusals are checked in the first test.
use bough_inspect::client::Client;
use bough_inspect::json::{self, Value as J};
use bough_inspect::png;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct App {
    child: Child,
    path: PathBuf,
}
impl Drop for App {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.path);
    }
}

fn spawn(tag: &str, scenario: &str, rows: usize) -> App {
    let path = Client::short_path(std::env::temp_dir(), tag);
    let _ = std::fs::remove_file(&path);
    let mut child = Command::new(env!("CARGO_BIN_EXE_bough-inspect-app"))
        .args([
            "--scenario",
            scenario,
            "--rows",
            &rows.to_string(),
            "--socket",
            path.to_str().unwrap(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the app");
    let stdout = child.stdout.take().unwrap();
    let mut line = String::new();
    BufReader::new(stdout)
        .read_line(&mut line)
        .expect("the app says ready");
    assert!(
        line.starts_with("ready "),
        "the app did not become ready: {line:?}"
    );
    App { child, path }
}

fn call(c: &mut Client, method: &str, p: J) -> J {
    c.request(method, &p)
        .unwrap_or_else(|e| panic!("transport: {e}"))
        .unwrap_or_else(|e| panic!("{method} refused: {}", e.message))
}

fn refusable(c: &mut Client, method: &str, p: J) -> (i32, String) {
    match c.request(method, &p).expect("transport") {
        Ok(v) => panic!("{method} unexpectedly succeeded: {v:?}"),
        Err(e) => (e.code, e.message),
    }
}

#[test]
fn rows_over_the_socket() {
    let app = spawn("rows", "rows", 1_000);
    let mut c = Client::open(&app.path).expect("connect");

    let hello = call(&mut c, "hello", J::obj());
    assert_eq!(hello.get("schema").and_then(J::as_u32), Some(1));
    assert_eq!(hello.get("app").and_then(J::as_str), Some("rows"));

    let tree = call(&mut c, "tree", J::obj());
    let nodes = tree.get("nodes").unwrap().arr();
    assert!(!nodes.is_empty());
    let stores = tree.get("stores").unwrap().arr();
    assert_eq!(stores.len(), 3, "root, body scope, list");
    let lists = tree.get("lists").unwrap().arr();
    assert_eq!(lists[0].get("n").and_then(J::as_u32), Some(1_000));
    assert!(tree.get("ax").unwrap().get("nodes").unwrap().arr().len() > 10);
    assert!(
        nodes
            .iter()
            .all(|n| n.get("loc").unwrap().get("line").and_then(J::as_u32).unwrap() > 0)
    );

    // pick under a row: the pick names a record with a pointer handler
    let pick = call(&mut c, "pick", J::obj().set("x", 100).set("y", 5 * 32 + 10));
    let stack = pick.get("stack").unwrap().arr();
    assert!(!stack.is_empty());
    assert_eq!(stack[0].get("kind").and_then(J::as_str), Some("text"), "the label lies over the row");
    assert!(
        stack.iter().any(|n| n.get("handler").and_then(J::as_bool) == Some(true)),
        "the row's fill is under the label and hears the pointer"
    );

    // drive: hover, then read the world's hover state through the description
    call(&mut c, "pointer", J::obj().set("x", 100).set("y", 5 * 32 + 10));
    let tree = call(&mut c, "tree", J::obj().set("nodes", false));
    let pick = tree.get("pointers").unwrap().get("pick").unwrap();
    assert!(!pick.is_null(), "the pick is a node of the new snapshot");
    let hot = call(&mut c, "state", J::obj());
    let hot = hot
        .get("fields")
        .unwrap()
        .arr()
        .iter()
        .find(|f| f.get("name").and_then(J::as_str) == Some("hot"))
        .unwrap();
    assert_eq!(hot.get("value").unwrap().get("value").and_then(J::as_i64), Some(5));

    // find by text: Row 00042 is in the window only when scrolled there; scroll to it through a typed write
    call(&mut c, "set", J::obj().set("field", "scroll").set("value", J::obj().set("kind", "int").set("value", 42 * 32)));
    let found = call(
        &mut c,
        "find",
        J::obj().set("kind", "text").set("text", "Row 00042"),
    );
    assert_eq!(found.get("total").and_then(J::as_u32), Some(1), "{found:?}");
    let node = &found.get("nodes").unwrap().arr()[0];
    assert_eq!(node.get("text").and_then(J::as_str), Some("Row 00042"));

    // dependencies of the row's hover word
    let deps = call(&mut c, "deps", J::obj().set("field", "hot"));
    assert!(deps.get("nodes").unwrap().arr().len() > 5);
    assert_eq!(deps.get("name").and_then(J::as_str), Some("hot"));

    // why: the last command, the words it moved and the damage
    let why = call(&mut c, "why", J::obj());
    assert_eq!(why.get("command").and_then(J::as_str), Some("set"));
    assert!(!why.get("changed_words").unwrap().arr().is_empty());
    assert!(why.get("damage_tiles").and_then(J::as_u32).unwrap() > 0);

    // screenshot: a valid PNG of the frame, stable across two paints
    let shot = call(&mut c, "shot", J::obj());
    let bytes = json::base64_decode(shot.get("png").and_then(J::as_str).unwrap()).unwrap();
    let img = png::decode(&bytes).expect("the screenshot is a PNG");
    assert_eq!((img.w, img.h), (800, 600));
    assert_eq!(img.px.len(), 800 * 600);
    let shot2 = call(&mut c, "shot", J::obj());
    assert_eq!(
        shot.get("digest").and_then(J::as_str),
        shot2.get("digest").and_then(J::as_str),
        "two paints of one state are the same frame"
    );

    // diff: the frame against itself matches; one changed pixel does not
    let same = call(
        &mut c,
        "diff",
        J::obj().set("golden", shot.get("png").and_then(J::as_str).unwrap()),
    );
    assert_eq!(same.get("match").and_then(J::as_bool), Some(true));
    let mut px = img.px.clone();
    let at = (300 * 800 + 400) as usize;
    let (was, now) = (px[at], px[at] ^ 0x00ff_0000);
    px[at] = now;
    let want = (0..4)
        .map(|s| ((was >> (8 * s) & 255) as i64 - (now >> (8 * s) & 255) as i64).unsigned_abs())
        .max()
        .unwrap();
    let mutant = json::base64_encode(&png::encode(800, 600, &px));
    let d = call(&mut c, "diff", J::obj().set("golden", mutant));
    assert_eq!(d.get("match").and_then(J::as_bool), Some(false));
    assert_eq!(d.get("differing").and_then(J::as_u32), Some(1));
    assert_eq!(d.get("max_channel").and_then(J::as_u32), Some(want as u32));

    // counters
    let ctr = call(&mut c, "counters", J::obj());
    assert!(ctr.get("revision").and_then(J::as_u32).unwrap() > 0);
    assert!(ctr.get("retained").unwrap().get("records").and_then(J::as_u32).unwrap() < 200);

    // protocol refusals
    let (code, msg) = refusable(&mut c, "no-such-method", J::obj());
    assert_eq!(code, -32601, "{msg}");
    let (code, _) = refusable(
        &mut c,
        "set",
        J::obj().set("field", "hot").set("value", J::obj().set("kind", "text").set("text", "x")),
    );
    assert_eq!(code, -32010, "a refusal is distinguishable from a transport error");

    call(&mut c, "shutdown", J::obj());
    drop(c);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut app = app;
    loop {
        if let Some(st) = app.child.try_wait().unwrap() {
            assert!(st.success(), "the app exited with {st}");
            break;
        }
        assert!(Instant::now() < deadline, "the app did not exit after shutdown");
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = std::fs::remove_file(&app.path);
}

#[test]
fn table_over_the_socket() {
    let app = spawn("table", "table", 500);
    let mut c = Client::open(&app.path).expect("connect");
    assert_eq!(
        call(&mut c, "hello", J::obj()).get("app").and_then(J::as_str),
        Some("table")
    );
    let tree = call(&mut c, "tree", J::obj());
    assert_eq!(tree.get("app").unwrap().get("name").and_then(J::as_str), Some("table"));
    // click a row: the header of the table has a status text and rows are a list
    call(&mut c, "click", J::obj().set("x", 200).set("y", 120));
    let tree = call(&mut c, "tree", J::obj().set("nodes", false));
    assert!(!tree.get("pointers").unwrap().get("pick").unwrap().is_null());
    // a read-only field is refused with its reason
    let (code, msg) = refusable(
        &mut c,
        "set",
        J::obj().set("field", "len").set("value", J::obj().set("kind", "int").set("value", 1)),
    );
    assert_eq!(code, -32010);
    assert!(msg.contains("read-only"), "{msg}");
    // the frame is a PNG again
    let shot = call(&mut c, "shot", J::obj());
    let bytes = json::base64_decode(shot.get("png").and_then(J::as_str).unwrap()).unwrap();
    assert!(png::decode(&bytes).is_ok());
    call(&mut c, "shutdown", J::obj());
    let _ = app;
}

#[test]
fn handshake_is_enforced_and_a_second_client_is_refused() {
    let app = spawn("handshake", "rows", 100);
    // the first request must be hello
    {
        use bough_inspect::socket::{read_frame, write_frame};
        let mut s = std::os::unix::net::UnixStream::connect(&app.path).unwrap();
        let req = J::obj()
            .set("jsonrpc", "2.0")
            .set("id", 1)
            .set("method", "tree")
            .set("params", J::obj());
        write_frame(&mut s, json::to_string(&req).as_bytes()).unwrap();
        let reply = json::parse(std::str::from_utf8(&read_frame(&mut s).unwrap()).unwrap()).unwrap();
        assert_eq!(reply.get("error").unwrap().get("code").and_then(J::as_i64), Some(-32600));
    }
    // a client naming another schema is refused
    {
        use bough_inspect::socket::{read_frame, write_frame};
        let mut s = std::os::unix::net::UnixStream::connect(&app.path).unwrap();
        let req = J::obj()
            .set("jsonrpc", "2.0")
            .set("id", 1)
            .set("method", "hello")
            .set("params", J::obj().set("schema", 999));
        write_frame(&mut s, json::to_string(&req).as_bytes()).unwrap();
        let reply = json::parse(std::str::from_utf8(&read_frame(&mut s).unwrap()).unwrap()).unwrap();
        assert_eq!(reply.get("error").unwrap().get("code").and_then(J::as_i64), Some(-32600));
    }
    // a live connection owns the world: a second one is told so
    let mut c = Client::open(&app.path).expect("connect");
    let _ = call(&mut c, "tree", J::obj());
    {
        use bough_inspect::socket::{read_frame, write_frame};
        let mut s = std::os::unix::net::UnixStream::connect(&app.path).unwrap();
        let req = J::obj()
            .set("jsonrpc", "2.0")
            .set("id", 1)
            .set("method", "hello")
            .set("params", J::obj());
        // the server accepts only when it is pumped; ask the live client to make it pump
        write_frame(&mut s, json::to_string(&req).as_bytes()).unwrap();
        let _ = call(&mut c, "tree", J::obj());
        let reply = json::parse(std::str::from_utf8(&read_frame(&mut s).unwrap()).unwrap()).unwrap();
        assert_eq!(reply.get("error").unwrap().get("code").and_then(J::as_i64), Some(-32600));
    }
    call(&mut c, "shutdown", J::obj());
    let _ = app;
}
