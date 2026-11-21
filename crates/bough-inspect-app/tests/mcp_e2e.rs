//! G6 against a real app: the MCP handler drives `bough-guibench-x`'s Rows over the socket, and every answer is
//! the live application's. The transport is tested in `bough-mcp`; this file holds the application still.
use bough_inspect::json::{self, Value as J};
use bough_inspect::png;
use bough_inspect::client::Client;
use bough_mcp::Mcp;
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
fn spawn(tag: &str) -> App {
    let path = Client::short_path(std::env::temp_dir(), tag);
    let _ = std::fs::remove_file(&path);
    let mut child = Command::new(env!("CARGO_BIN_EXE_bough-inspect-app"))
        .args(["--scenario", "rows", "--rows", "2000", "--socket", path.to_str().unwrap()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
    assert!(line.starts_with("ready "), "{line:?}");
    App { child, path }
}
fn call(m: &mut Mcp, id: i64, method: &str, p: J) -> J {
    m.handle(&J::obj().set("jsonrpc", "2.0").set("id", id).set("method", method).set("params", p))
        .expect("a response")
}
fn tool(m: &mut Mcp, id: i64, name: &str, a: J) -> (J, bool) {
    let r = call(m, id, "tools/call", J::obj().set("name", name).set("arguments", a));
    let res = r.get("result").cloned().expect("a result");
    let content = res.get("content").unwrap().arr();
    let text = content[0].get("text").and_then(J::as_str).unwrap_or("");
    let body = json::parse(text).unwrap_or(J::Str(text.to_owned()));
    (body, res.get("isError").and_then(J::as_bool).unwrap_or(false))
}

#[test]
fn mcp_drives_the_rows_app() {
    let app = spawn("mcpe2e");
    let mut m = Mcp::new(&app.path);
    let init = call(&mut m, 1, "initialize", J::obj().set("protocolVersion", "2025-06-18"));
    assert_eq!(init.get("result").unwrap().get("protocolVersion").and_then(J::as_str), Some("2025-06-18"));

    let (tree, err) = tool(&mut m, 2, "tree", J::obj().set("limit", 8));
    assert!(!err);
    assert_eq!(tree.get("app").unwrap().get("name").and_then(J::as_str), Some("rows"));
    assert_eq!(tree.get("nodes_total").and_then(J::as_u32).unwrap_or(0) > 0, true);

    // find a cell by its text; the text comes from the span kinds' reflection
    let (found, _) = tool(&mut m, 3, "find", J::obj().set("kind", "text").set("text", "Row 00003"));
    assert_eq!(found.get("total").and_then(J::as_u32), Some(1), "{found:?}");
    let i = found.get("nodes").unwrap().arr()[0].get("index").and_then(J::as_u32).unwrap();

    // a screenshot is a real PNG of the window
    let shot = call(&mut m, 4, "tools/call", J::obj().set("name", "screenshot").set("arguments", J::obj()));
    let content = shot.get("result").unwrap().get("content").unwrap().arr();
    assert_eq!(content.len(), 2);
    let bytes = json::base64_decode(content[1].get("data").and_then(J::as_str).unwrap()).unwrap();
    let img = png::decode(&bytes).unwrap();
    assert_eq!((img.w, img.h), (800, 600));

    // drive: click the found node's centre through the inspector's own find (device pixels)
    let cx = found.get("nodes").unwrap().arr()[0].get("screen").unwrap();
    let (x, y) = (
        cx.get("x").and_then(J::as_i64).unwrap() as i64 + cx.get("w").and_then(J::as_i64).unwrap() / 2,
        cx.get("y").and_then(J::as_i64).unwrap() as i64 + cx.get("h").and_then(J::as_i64).unwrap() / 2,
    );
    let (o, _) = tool(&mut m, 5, "click", J::obj().set("x", x).set("y", y));
    assert_eq!(o.get("command").and_then(J::as_str), Some("click"));

    // the typed field write, and the refusal of a read-only one
    let (o, err) = tool(&mut m, 6, "set_field", J::obj().set("field", "scroll").set("value", J::obj().set("kind", "int").set("value", 320)));
    assert!(!err, "{o:?}");
    assert_eq!(o.get("damage").and_then(J::as_u32).unwrap_or(0) > 0, true);
    let (why, _) = tool(&mut m, 7, "why_repaint", J::obj());
    assert_eq!(why.get("command").and_then(J::as_str), Some("set"));
    let (d, _) = tool(&mut m, 8, "deps", J::obj().set("word", 0));
    assert!(!d.get("stores").unwrap().arr().is_empty(), "the scroll word moves a space");

    // a record's operand is read-only: refused with the reason as a tool error
    let (o, err) = tool(&mut m, 9, "set_field", J::obj().set("field", "len").set("value", J::obj().set("kind", "int").set("value", 1)));
    assert!(err, "{o:?}");

    // diff against the frame as it is now: equal
    let now = call(&mut m, 10, "tools/call", J::obj().set("name", "screenshot").set("arguments", J::obj()));
    let now = now.get("result").unwrap().get("content").unwrap().arr()[1]
        .get("data")
        .and_then(J::as_str)
        .unwrap()
        .to_owned();
    let same = call(&mut m, 10, "tools/call", J::obj().set("name", "diff_golden").set("arguments", J::obj().set("golden", now)));
    let body = json::parse(same.get("result").unwrap().get("content").unwrap().arr()[0].get("text").and_then(J::as_str).unwrap()).unwrap();
    assert_eq!(body.get("match").and_then(J::as_bool), Some(true), "{body:?}");

    // node by index, with its stores
    let (n, _) = tool(&mut m, 11, "node", J::obj().set("index", i));
    assert!(n.get("node").unwrap().get("loc").unwrap().get("file").and_then(J::as_str).unwrap().ends_with("rows.rs"));

    call(&mut m, 12, "shutdown", J::obj());
    drop(m);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut app = app;
    loop {
        if let Some(st) = app.child.try_wait().unwrap() {
            assert!(st.success(), "{st}");
            break;
        }
        assert!(Instant::now() < deadline, "the app did not exit");
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = std::fs::remove_file(&app.path);
}
