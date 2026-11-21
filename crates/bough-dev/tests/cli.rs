//! The `bough-dev` terminal client against a fake inspector: every `inspect` subcommand maps to one request and
//! prints what it got, and a diff that differs exits non-zero.
use bough_inspect::json::{self, Value as J};
use bough_inspect::png;
use bough_inspect::socket::{read_frame, write_frame};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};

fn fake(tag: &str) -> (PathBuf, std::thread::JoinHandle<()>) {
    let path = std::env::temp_dir().join(format!("bd-{}-{tag}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let l = UnixListener::bind(&path).unwrap();
    let p = path.clone();
    let h = std::thread::spawn(move || {
        // each `inspect` command is its own process and its own connection
        'accept: loop {
            let Ok((mut s, _)) = l.accept() else { break };
            loop {
                let f = match read_frame(&mut s) {
                    Ok(f) => f,
                    Err(_) => continue 'accept,
                };
                let req = json::parse(std::str::from_utf8(&f).unwrap()).unwrap();
                let id = req.get("id").cloned().unwrap_or(J::Null);
                let method = req.get("method").and_then(J::as_str).unwrap_or("");
                let result = match method {
                    "hello" => J::obj().set("schema", 1).set("app", "fake"),
                    "tree" => J::obj()
                        .set("digest", "abc")
                        .set("app", J::obj().set("name", "fake").set("w", 8).set("h", 8).set("revision", 1).set("scale", 1.0))
                        .set("counters", J::obj().set("retained", J::obj().set("records", 1)))
                        .set("damage", J::obj().set("tiles", 0))
                        .set("nodes", vec![J::obj().set("kind", "fill").set("depth", 0).set("text", "hi").set("loc", J::obj().set("file", "a.rs").set("line", 1).set("col", 1)).set("screen", J::obj().set("x", 0).set("y", 0).set("w", 8).set("h", 8)).set("handlers", Vec::<J>::new())]),
                    "find" => J::obj().set("total", 1).set("nodes", vec![J::obj().set("index", 0).set("kind", "fill").set("text", "hi").set("loc", J::obj().set("file", "a.rs").set("line", 1))]),
                    "state" => J::obj().set("fields", vec![J::obj().set("name", "scroll").set("kind", "int").set("value", J::obj().set("kind", "int").set("value", 0)).set("word", 0).set("writable", true)]),
                    "set" => J::obj().set("command", "set"),
                    "shot" => {
                        let png = png::encode(2, 2, &[0xff00_0000; 4]);
                        J::obj().set("w", 2).set("h", 2).set("digest", "00").set("png", json::base64_encode(&png))
                    }
                    "diff" => {
                        let want = req.get("params").and_then(|p| p.get("golden")).and_then(J::as_str).unwrap_or("");
                        let bytes = json::base64_decode(want).unwrap_or_default();
                        let same = png::decode(&bytes).is_ok();
                        J::obj().set("match", same).set("differing", if same { 0 } else { 1 })
                    }
                    "reload" | "click" | "key" | "text" | "tick" | "resize" | "pointer" => J::obj().set("command", method),
                    "shutdown" => {
                        let r = J::obj().set("jsonrpc", "2.0").set("id", id).set("result", J::obj());
                        let _ = write_frame(&mut s, json::to_string(&r).as_bytes());
                        break 'accept;
                    }
                    _ => J::obj(),
                };
                let resp = J::obj().set("jsonrpc", "2.0").set("id", id).set("result", result);
                if write_frame(&mut s, json::to_string(&resp).as_bytes()).is_err() {
                    continue 'accept;
                }
            }
        }
        let _ = std::fs::remove_file(&p);
    });
    (path, h)
}

fn dev(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bough-dev"))
        .args(args)
        .output()
        .expect("run bough-dev")
}

#[test]
fn inspect_subcommands() {
    let (path, h) = fake("inspect");
    let s = path.to_str().unwrap();
    let tree = dev(&["inspect", "tree", "--socket", s]);
    assert!(tree.status.success(), "{}", String::from_utf8_lossy(&tree.stderr));
    let text = String::from_utf8_lossy(&tree.stdout);
    assert!(text.contains("fake") && text.contains("fill") && text.contains("a.rs"), "{text}");

    let find = dev(&["inspect", "find", "--kind", "fill", "--socket", s]);
    assert!(String::from_utf8_lossy(&find.stdout).contains("1 total"));

    let state = dev(&["inspect", "state", "--socket", s]);
    assert!(String::from_utf8_lossy(&state.stdout).contains("scroll"));

    let set = dev(&["inspect", "set", "scroll", "int", "320", "--socket", s]);
    assert!(set.status.success(), "{}", String::from_utf8_lossy(&set.stderr));

    let out = std::env::temp_dir().join(format!("bd-shot-{}.png", std::process::id()));
    let shot = dev(&["inspect", "shot", out.to_str().unwrap(), "--socket", s]);
    assert!(shot.status.success(), "{}", String::from_utf8_lossy(&shot.stderr));
    assert!(png::decode(&std::fs::read(&out).unwrap()).is_ok());
    let diff = dev(&["inspect", "diff", out.to_str().unwrap(), "--socket", s]);
    assert!(diff.status.success(), "{}", String::from_utf8_lossy(&diff.stderr));
    // a file that is not a PNG is not a matching golden
    std::fs::write(&out, b"not a png").unwrap();
    let diff = dev(&["inspect", "diff", out.to_str().unwrap(), "--socket", s]);
    assert!(!diff.status.success());
    let _ = std::fs::remove_file(&out);

    let stop = dev(&["inspect", "stop", "--socket", s]);
    assert!(stop.status.success(), "{}", String::from_utf8_lossy(&stop.stderr));
    h.join().unwrap();
}

#[test]
fn mcp_http_from_the_dev_binary() {
    let (path, h) = fake("mcp");
    let mut child: Child = Command::new(env!("CARGO_BIN_EXE_bough-dev"))
        .args(["mcp", "--socket", path.to_str().unwrap(), "--http", "127.0.0.1:0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
    assert!(line.starts_with("listening "), "{line:?}");
    let addr = line.trim_start_matches("listening ").trim().to_owned();
    let body = json::to_string(&J::obj().set("jsonrpc", "2.0").set("id", 1).set("method", "initialize").set("params", J::obj()));
    let req = format!("POST /mcp HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
    let mut s = std::net::TcpStream::connect(&addr).unwrap();
    s.write_all(req.as_bytes()).unwrap();
    let mut resp = String::new();
    use std::io::Read;
    s.read_to_string(&mut resp).unwrap();
    assert!(resp.starts_with("HTTP/1.1 200") && resp.contains("bough-mcp"), "{resp}");
    // the endpoint is the same handler `bough-mcp` serves (its shutdown path is tested there); it is enough
    // here that this process bound it and answered, so the child is stopped rather than asked to leave
    let _ = child.kill();
    let _ = child.wait();
    // the fake is left in `accept` for a client that will not come; the process ends with the test
    drop(h);
    let _ = std::fs::remove_file(&path);
}
