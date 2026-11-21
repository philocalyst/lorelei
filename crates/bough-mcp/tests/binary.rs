//! The `bough-mcp` binary: stdio and `--http`, against a fake inspector socket. Proves the process's argument
//! handling, framing and transports; the real apps are in `bough-inspect-app/tests/mcp_e2e.rs`.
use bough_inspect::json::{self, Value as J};
use bough_inspect::socket::{read_frame, write_frame};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn fake_app(tag: &str) -> (PathBuf, std::thread::JoinHandle<()>) {
    let path = std::env::temp_dir().join(format!("bm-bin-{}-{tag}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let l = UnixListener::bind(&path).unwrap();
    let p = path.clone();
    let h = std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        loop {
            let f = match read_frame(&mut s) {
                Ok(f) => f,
                Err(_) => break,
            };
            let req = json::parse(std::str::from_utf8(&f).unwrap()).unwrap();
            let id = req.get("id").cloned().unwrap_or(J::Null);
            let method = req.get("method").and_then(J::as_str).unwrap_or("");
            let resp = if method == "shutdown" {
                let r = J::obj()
                    .set("jsonrpc", "2.0")
                    .set("id", id)
                    .set("result", J::obj().set("bye", true));
                let _ = write_frame(&mut s, json::to_string(&r).as_bytes());
                break;
            } else {
                J::obj().set("jsonrpc", "2.0").set("id", id).set(
                    "result",
                    J::obj()
                        .set("schema", 1)
                        .set("app", "fake")
                        .set("revision", 1),
                )
            };
            write_frame(&mut s, json::to_string(&resp).as_bytes()).unwrap();
        }
        let _ = std::fs::remove_file(&p);
    });
    (path, h)
}

fn wait(child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(st) = child.try_wait().unwrap() {
            assert!(st.success(), "{st}");
            return;
        }
        assert!(Instant::now() < deadline, "bough-mcp did not exit");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn stdio_binary() {
    let (path, h) = fake_app("stdio");
    let mut child = Command::new(env!("CARGO_BIN_EXE_bough-mcp"))
        .args(["--socket", path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let msgs = [
        J::obj()
            .set("jsonrpc", "2.0")
            .set("id", 1)
            .set("method", "initialize")
            .set("params", J::obj()),
        J::obj()
            .set("jsonrpc", "2.0")
            .set("id", 2)
            .set("method", "tools/list")
            .set("params", J::obj()),
        J::obj()
            .set("jsonrpc", "2.0")
            .set("id", 3)
            .set("method", "tools/call")
            .set(
                "params",
                J::obj().set("name", "hello").set("arguments", J::obj()),
            ),
        J::obj()
            .set("jsonrpc", "2.0")
            .set("id", 4)
            .set("method", "shutdown")
            .set("params", J::obj()),
    ];
    for m in msgs {
        writeln!(stdin, "{}", json::to_string(&m)).unwrap();
        stdin.flush().unwrap();
    }
    drop(stdin);
    let mut replies = Vec::new();
    for _ in 0..4 {
        let mut line = String::new();
        assert!(
            out.read_line(&mut line).unwrap() > 0,
            "fewer replies than requests"
        );
        replies.push(json::parse(line.trim()).unwrap());
    }
    assert_eq!(replies[0].get("id").and_then(J::as_i64), Some(1));
    assert!(
        replies[0]
            .get("result")
            .unwrap()
            .get("serverInfo")
            .is_some()
    );
    assert_eq!(
        replies[1]
            .get("result")
            .unwrap()
            .get("tools")
            .unwrap()
            .arr()
            .len()
            >= 20,
        true
    );
    assert!(
        replies[2]
            .get("result")
            .unwrap()
            .get("content")
            .unwrap()
            .arr()[0]
            .get("text")
            .and_then(J::as_str)
            .unwrap()
            .contains("fake")
    );
    wait(&mut child);
    h.join().unwrap();
}

#[test]
fn http_binary() {
    let (path, h) = fake_app("http");
    let mut child = Command::new(env!("CARGO_BIN_EXE_bough-mcp"))
        .args(["--socket", path.to_str().unwrap(), "--http", "127.0.0.1:0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(line.starts_with("listening "), "{line:?}");
    let addr = line.trim_start_matches("listening ").trim().to_owned();
    let post = |body: &str| {
        let req = format!(
            "POST /mcp HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let mut s = std::net::TcpStream::connect(&addr).unwrap();
        s.write_all(req.as_bytes()).unwrap();
        let mut resp = String::new();
        s.read_to_string(&mut resp).unwrap();
        resp
    };
    let init = json::to_string(
        &J::obj()
            .set("jsonrpc", "2.0")
            .set("id", 1)
            .set("method", "initialize")
            .set("params", J::obj()),
    );
    let r = post(&init);
    assert!(r.starts_with("HTTP/1.1 200"), "{r}");
    assert!(r.contains("bough-mcp"), "{r}");
    let call = json::to_string(
        &J::obj()
            .set("jsonrpc", "2.0")
            .set("id", 2)
            .set("method", "tools/call")
            .set(
                "params",
                J::obj().set("name", "hello").set("arguments", J::obj()),
            ),
    );
    let r = post(&call);
    assert!(r.contains("\"content\""), "{r}");
    let sd = json::to_string(
        &J::obj()
            .set("jsonrpc", "2.0")
            .set("id", 3)
            .set("method", "shutdown")
            .set("params", J::obj()),
    );
    let _ = post(&sd);
    wait(&mut child);
    h.join().unwrap();
}
