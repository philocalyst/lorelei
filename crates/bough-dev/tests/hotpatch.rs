//! The in-process view patch: a string literal in the demo view's source changes on disk, the dev loop builds a
//! patch dylib, the running application loads it and re-describes in the same process, world and state. A patch
//! compiled against another state type is refused and the loop falls back to the restart path
//! (`restart_preserving_state`, covered by `tests/dev.rs` and exercised again here as a plain restart).
use bough_dev::hot::{AppSpec, Running};
use bough_dev::{hotview, patch};
use bough_inspect::client::Client;
use bough_inspect::json::Value as J;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn target_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    for a in exe.ancestors().skip(1).take(8) {
        if a.join("hotdemo").is_file() {
            return a.to_path_buf();
        }
    }
    panic!("cannot find the target directory from {}", exe.display());
}

fn write_patch(dir: &std::path::Path, name: &str, text: &str, token: u64) -> PathBuf {
    let src = dir.join(format!("{name}.rs"));
    let source = format!(
        "#[no_mangle]\npub extern \"C\" fn hot_state_token() -> u64 {{ {token:#x} }}\n\
         #[no_mangle]\npub extern \"C\" fn hot_text(out: *mut u8, cap: u32) -> u32 {{\n\
         let s = b\"{text}\";\n\
         let n = s.len().min(cap as usize);\n\
         unsafe {{ core::ptr::copy_nonoverlapping(s.as_ptr(), out, n) }};\n\
         n as u32\n\
         }}\n"
    );
    std::fs::write(&src, source).unwrap();
    src
}

fn tree(c: &mut Client) -> J {
    bough_inspect::client::call(c, "tree", &J::obj().set("limit", 16)).expect("tree")
}

fn view_text(t: &J) -> String {
    t.get("nodes")
        .map(J::arr)
        .unwrap_or(&[])
        .iter()
        .filter_map(|n| n.get("text").and_then(J::as_str))
        .find(|s| s.starts_with("Demo "))
        .unwrap_or("(no text)")
        .to_owned()
}

fn world(t: &J) -> i64 {
    t.get("app")
        .unwrap()
        .get("world")
        .and_then(J::as_i64)
        .unwrap_or(-1)
}

struct App {
    child: Child,
    socket: PathBuf,
    path: PathBuf,
}
impl Drop for App {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
    }
}

fn start(dir: &std::path::Path) -> App {
    let socket = dir.join(format!("hot-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket);
    let path = target_dir().join("hotdemo");
    let mut child = Command::new(&path)
        .args(["--socket", socket.to_str().unwrap()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start the demo");
    let out = child.stdout.take().unwrap();
    let mut lines = BufReader::new(out).lines();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "the demo did not become ready");
        if let Some(Ok(l)) = lines.next()
            && l.starts_with("ready ")
        {
            break;
        }
    }
    App {
        child,
        socket,
        path,
    }
}

#[test]
fn a_view_patch_changes_the_text_in_the_same_process_and_world() {
    let dir = std::env::temp_dir().join(format!("bm-hot-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let app = start(&dir);
    let pid = app.child.id();
    let mut c = Client::open(&app.socket).unwrap();
    let before = tree(&mut c);
    assert_eq!(view_text(&before), "Demo A", "the built-in text");
    let w0 = world(&before);

    let v2 = write_patch(&dir, "v2", "Demo B", hotview::STATE_TOKEN);
    let outcome = patch::patch_source(&mut c, &v2).expect("the dev loop builds the patch");
    assert!(
        matches!(outcome, patch::Outcome::Patched { .. }),
        "{outcome}"
    );
    let after = tree(&mut c);
    assert_eq!(
        view_text(&after),
        "Demo B",
        "the running view shows the new literal"
    );
    assert_eq!(world(&after), w0, "the world is the same");
    assert_eq!(app.child.id(), pid, "the process is the same");
    assert!(!outcome.needs_restart());

    // A patch compiled against another state type is refused before any of its code runs.
    let bad = write_patch(&dir, "bad", "Demo C", hotview::STATE_TOKEN ^ 0x1000);
    let refused = patch::patch_source(&mut c, &bad).expect("the dev loop builds the patch");
    assert!(matches!(refused, patch::Outcome::Refused(_)), "{refused}");
    let text = refused.to_string();
    assert!(text.contains("state type"), "{text}");
    assert!(refused.needs_restart(), "the loop takes the restart path");
    let still = tree(&mut c);
    assert_eq!(view_text(&still), "Demo B", "a refusal changes nothing");
    assert_eq!(world(&still), w0);
    assert_eq!(app.child.id(), pid);

    // The fallback really is a restart: a new process starts with the source's built-in text.
    let spec = AppSpec::new(&app.path, dir.join("restart.sock"));
    c.request("shutdown", &J::obj()).ok();
    let mut running = Running::start(&spec, Duration::from_secs(10)).expect("restart");
    let new_pid = running.child.id();
    assert_ne!(new_pid, pid, "the fallback is a new process");
    let mut c2 = running.client().expect("the restarted app");
    let fresh = tree(&mut c2);
    assert_eq!(view_text(&fresh), "Demo A");
    running.stop();
    drop(app);
    let _ = std::fs::remove_file(spec.socket);
    let _ = std::fs::remove_dir_all(&dir);
}
