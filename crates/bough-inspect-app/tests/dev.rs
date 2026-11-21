//! The dev server's fallback against a real app: snapshot the typed state, stop the application, start a fresh
//! binary, write the state back through the inspector's admission, and compare the descriptions. Also the hot
//! path that is not a rebuild: `reload` changes nothing and costs no restart.
use bough_dev::Running;
use bough_dev::hot::{self, AppSpec};
use bough_inspect::client::Client;
use bough_inspect::json::Value as J;
use std::time::Duration;

fn spec(tag: &str, args: &[&str]) -> AppSpec {
    let socket = Client::short_path(std::env::temp_dir(), tag);
    let _ = std::fs::remove_file(&socket);
    let mut s = AppSpec::new(env!("CARGO_BIN_EXE_bough-inspect-app"), socket);
    s.args = args.iter().map(|a| (*a).to_owned()).collect();
    s
}

#[test]
fn a_restart_keeps_the_typed_state_and_the_description() {
    let s = spec("devrestart", &["--scenario", "rows", "--rows", "800"]);
    let mut running = Running::start(&s, Duration::from_secs(15)).expect("start");
    {
        let c = running.client().unwrap();
        let _ = c.request(
            "set",
            &J::obj()
                .set("field", "scroll")
                .set("value", J::obj().set("kind", "int").set("value", 320)),
        );
        let _ = c.request(
            "set",
            &J::obj()
                .set("field", "hot")
                .set("value", J::obj().set("kind", "int").set("value", 9)),
        );
    }
    let before_state = {
        let c = running.client().unwrap();
        hot::state(c).unwrap()
    };
    let mut mcp = bough_mcp::Mcp::new(&s.socket);
    let call = J::obj()
        .set("jsonrpc", "2.0")
        .set("id", 1)
        .set("method", "tools/call")
        .set("params", J::obj().set("name", "state"));
    let before_mcp = mcp.handle(&call).unwrap().get("result").cloned().unwrap();
    assert_eq!(before_mcp.get("isError").and_then(J::as_bool), Some(false));
    let before_digest = {
        let c = running.client().unwrap();
        hot::tree_digest(c).unwrap()
    };
    // the same binary started again with the same arguments
    let r =
        hot::restart_preserving_state(&s, &mut running, Duration::from_secs(15)).expect("restart");
    assert_eq!(r.fields, before_state.len());
    assert_eq!(r.restored, r.fields, "refusals: {:?}", r.refusals);
    assert!(r.refusals.is_empty());
    assert_eq!(r.before, before_digest);
    assert!(
        r.matched,
        "the description after the restart differs: {} != {}",
        r.before, r.after
    );
    // the fields read back as they were
    let after = {
        let c = running.client().unwrap();
        hot::state(c).unwrap()
    };
    assert_eq!(before_state, after);
    let after_mcp = mcp.handle(&call).unwrap().get("result").cloned().unwrap();
    assert_eq!(
        before_mcp, after_mcp,
        "first call through the same MCP endpoint after restart"
    );
    running.stop();
}

#[test]
fn a_reload_keeps_the_description() {
    let s = spec("devreload", &["--scenario", "rows", "--rows", "300"]);
    let mut running = Running::start(&s, Duration::from_secs(15)).expect("start");
    {
        let c = running.client().unwrap();
        let _ = c.request(
            "set",
            &J::obj()
                .set("field", "scroll")
                .set("value", J::obj().set("kind", "int").set("value", 64)),
        );
    }
    let before = {
        let c = running.client().unwrap();
        (hot::tree_digest(c).unwrap(), hot::state(c).unwrap())
    };
    {
        let c = running.client().unwrap();
        let r = c.request("reload", &J::obj()).unwrap().unwrap();
        assert_eq!(r.get("reloaded").and_then(J::as_bool), Some(true));
    }
    let after = {
        let c = running.client().unwrap();
        (hot::tree_digest(c).unwrap(), hot::state(c).unwrap())
    };
    assert_eq!(
        before, after,
        "a reload re-reads inputs and keeps state and description"
    );
    running.stop();
}
