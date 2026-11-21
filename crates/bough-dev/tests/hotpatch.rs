//! A trusted Cargo-built patch changes live view code and rejects a real state declaration change.
#![cfg(feature = "hot-view")]
use bough_dev::hot::{AppSpec, Running};
use bough_dev::patch::{self, Outcome};
use bough_inspect::client::Client;
use bough_inspect::json::Value as J;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_patch(source: &Path, text: &str, level: &str) {
    let declaration = include_str!("../fixtures/demo_state.rs");
    std::fs::write(
        source,
        format!(
            r#"{declaration}
        demo_state!({level});
        #[unsafe(no_mangle)]
        pub static HOT_STATE_TOKEN: u64 = STATE_TOKEN;
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn hot_text(out: *mut u8, cap: u32) -> u32 {{
            let bytes = b"{text}";
            let len = bytes.len().min(cap as usize);
            // SAFETY: the hot-call contract grants access to cap writable bytes.
            unsafe {{ core::ptr::copy_nonoverlapping(bytes.as_ptr(), out, len) }};
            len as u32
        }}
    "#
        ),
    )
    .unwrap();
}

fn request(client: &mut Client, method: &str, params: J) -> J {
    client.request(method, &params).unwrap().unwrap()
}

fn tree(client: &mut Client) -> J {
    request(client, "tree", J::obj().set("limit", 16))
}

fn shows(tree: &J, text: &str) -> bool {
    tree.get("nodes")
        .unwrap()
        .arr()
        .iter()
        .any(|node| node.get("text").and_then(J::as_str) == Some(text))
}

fn world(tree: &J) -> i64 {
    tree.get("app")
        .unwrap()
        .get("world")
        .and_then(J::as_i64)
        .unwrap()
}

fn pixels(client: &mut Client) -> String {
    request(client, "shot", J::obj())
        .get("digest")
        .and_then(J::as_str)
        .unwrap()
        .to_owned()
}

#[test]
fn a_view_patch_changes_the_text_in_the_same_process_and_world() {
    let dir = Scratch(std::env::temp_dir().join(format!("bi-hot-{}", std::process::id())));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir.0)
        .unwrap();
    let mut spec = AppSpec::new(env!("CARGO_BIN_EXE_hotdemo"), dir.0.join("app.sock"));
    spec.args = vec!["--patch-dir".into(), dir.0.display().to_string()];
    let timeout = Duration::from_secs(10);
    let mut running = Running::start(&spec, timeout).unwrap();
    let pid = running.child.id();
    let client = running.client().unwrap();
    request(
        client,
        "set",
        J::obj()
            .set("field", "level")
            .set("value", J::obj().set("kind", "int").set("value", 27)),
    );
    let before = tree(client);
    let world_before = world(&before);
    assert!(shows(&before, "Demo A") && shows(&before, "Level 27"));
    let pixels_before = pixels(client);
    let source = dir.0.join("view.rs");
    write_patch(&source, "Demo B", "i32");
    let (patch, restart) = patch::patch_or_restart(&mut running, &spec, &source, timeout).unwrap();
    assert!(
        matches!(patch, Outcome::Patched { .. }) && restart.is_none(),
        "{patch}"
    );
    assert_eq!(running.child.id(), pid);
    assert!(running.child.try_wait().unwrap().is_none());
    let client = running.client().unwrap();
    let after = tree(client);
    assert!(
        shows(&after, "Demo B"),
        "the running view shows the new literal"
    );
    assert!(shows(&after, "Level 27"));
    assert_eq!(world(&after), world_before);
    assert_ne!(pixels(client), pixels_before);

    // Build a second artifact from the same edited file. A loader cache keyed by pathname fails this check.
    write_patch(&source, "Demo C", "i32");
    let second = patch::patch_source(client, &source).unwrap();
    assert!(matches!(second, Outcome::Patched { .. }));
    let second_tree = tree(client);
    assert!(shows(&second_tree, "Demo C") && shows(&second_tree, "Level 27"));
    assert_eq!(world(&second_tree), world_before);

    // Change the actual type of the patch's field, preserving its size and alignment.
    write_patch(&source, "Demo D", "u32");
    let refused = patch::patch_source(client, &source).unwrap();
    assert!(
        matches!(&refused, Outcome::Refused(reason) if reason.contains("state type changed")),
        "{refused}"
    );
    assert!(shows(&tree(client), "Demo C"));
    let (refused, restart) =
        patch::patch_or_restart(&mut running, &spec, &source, timeout).unwrap();
    assert!(refused.needs_restart());
    let restarted = restart.expect("the fallback actually restarts the managed app");
    assert_eq!((restarted.restored, restarted.fields), (1, 1));
    assert!(restarted.refusals.is_empty());
    let new_pid = running.child.id();
    assert_ne!(new_pid, pid);
    let fresh = tree(running.client().unwrap());
    assert!(shows(&fresh, "Demo A") && shows(&fresh, "Level 27"));
    assert_eq!(
        std::fs::read_dir(&dir.0)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "dylib"))
            .count(),
        4
    );
    println!(
        "Cargo patched PID {pid}, World {world_before}, level 27. After a state type refusal the new PID {new_pid} restored 1/1 fields."
    );
}
