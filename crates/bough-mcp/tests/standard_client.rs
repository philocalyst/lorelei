//! TASK 3: a standard MCP client (the official Rust SDK, `rmcp` 3.10) drives the server against the real
//! `Rows` app over stdio, and the HTTP transport accepts the same client. Every advertised tool is called and
//! every advertised resource is read; the SDK parses the schemas and the results, so a shape the hand-written
//! JSON tests tolerated is a failure here.
#![cfg(target_os = "macos")]

use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ReadResourceRequestParams},
    transport::{ConfigureCommandExt, TokioChildProcess},
};
use serde_json::{Map, Value, json};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{
    OnceLock,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::time::Duration;

fn target_dir() -> PathBuf {
    // The test executable lives under <target>/debug/{deps,build/..}/...; walk up to the directory that
    // holds the workspace's built binaries.
    let exe = std::env::current_exe().unwrap();
    for a in exe.ancestors().skip(1).take(8) {
        if a.join("bough-inspect-app").is_file() || a.join("bough-mcp").is_file() {
            return a.to_path_buf();
        }
    }
    panic!("cannot find the target directory from {}", exe.display());
}

fn app_bin() -> PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let target = target_dir();
        let p = target.join("bough-inspect-app");
        if p.is_file() {
            return p;
        }
        // A standalone SDK test may not have the app binary built by the outer Cargo invocation.
        let build = target.join("mcp-sdk-app");
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let output = Command::new(cargo)
            .args([
                "build",
                "--offline",
                "--locked",
                "-j",
                "2",
                "-p",
                "bough-inspect-app",
                "--manifest-path",
            ])
            .arg(manifest)
            .arg("--target-dir")
            .arg(&build)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "build inspector fixture: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        build.join("debug/bough-inspect-app")
    })
    .clone()
}

fn readiness(child: &mut Child) -> String {
    let stdout = child.stdout.take().expect("piped stdout");
    let (send, recv) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
        let _ = send.send(result);
    });
    match recv.recv_timeout(Duration::from_secs(20)) {
        Ok(Ok(line)) if !line.is_empty() => line.trim_end().to_owned(),
        result => {
            let _ = child.kill();
            let _ = child.wait();
            panic!("child did not provide readiness: {result:?}");
        }
    }
}

struct App {
    child: Child,
    socket: PathBuf,
}
impl Drop for App {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
    }
}

fn start_app(scenario: &str) -> App {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let socket = std::env::temp_dir().join(format!(
        "bm-std-{}-{}.sock",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut child = Command::new(app_bin())
        .args([
            "--scenario",
            scenario,
            "--rows",
            "400",
            "--socket",
            socket.to_str().unwrap(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the app");
    assert!(readiness(&mut child).starts_with("ready "));
    App { child, socket }
}

fn tool_args(
    name: &str,
    log: &mut Option<Value>,
    golden: &mut Option<String>,
) -> Map<String, Value> {
    let mut m = Map::new();
    match name {
        "tree" => {
            m.insert("limit".into(), json!(64));
        }
        "find" => {
            m.insert("kind".into(), json!("text"));
            m.insert("limit".into(), json!(8));
            m.insert("ops".into(), json!(true));
        }
        "node" => {
            m.insert("index".into(), json!(0));
        }
        "pick" => {
            m.insert("x".into(), json!(20));
            m.insert("y".into(), json!(20));
        }
        "deps" => {
            m.insert("field".into(), json!("hot"));
        }
        "set_field" => {
            m.insert("field".into(), json!("hot"));
            m.insert("value".into(), json!({"kind": "int", "value": 2}));
        }
        "pointer_move" | "click" => {
            m.insert("x".into(), json!(20));
            m.insert("y".into(), json!(2 * 32 + 10));
        }
        "pointer_down" | "pointer_up" => {
            m.insert("x".into(), json!(20));
            m.insert("y".into(), json!(2 * 32 + 10));
            m.insert("button".into(), json!("primary"));
        }
        "key" => {
            m.insert("key".into(), json!("down"));
        }
        "type_text" => {
            m.insert("text".into(), json!("x"));
        }
        "wheel" => {
            m.insert("x".into(), json!(20));
            m.insert("y".into(), json!(20));
            m.insert("dy".into(), json!(2));
        }
        "tick" => {
            m.insert("ms".into(), json!(16));
        }
        "resize" => {
            m.insert("w".into(), json!(800));
            m.insert("h".into(), json!(600));
            m.insert("scale".into(), json!(1));
        }
        "screenshot" => {
            m.insert("node".into(), json!(0));
        }
        "diff_golden" => {
            if let Some(g) = golden {
                m.insert("golden".into(), json!(g));
            }
        }
        "replay" => {
            if let Some(l) = log {
                m.insert("log".into(), l.clone());
            }
        }
        "damage_map" => {
            m.insert("include_heatmap_png".into(), json!(true));
        }
        "effects_inspect" => {
            m.insert("node".into(), json!(0));
        }
        "scrub_animation" => {
            m.insert("op".into(), json!("resume"));
        }
        "assert_selector" => {
            m.insert("kind".into(), json!("text"));
        }
        "batch" => {
            m.insert(
                "steps".into(),
                json!([{"method": "counters", "params": {}}]),
            );
        }
        _ => {}
    }
    m
}

fn text_of(r: &rmcp::model::CallToolResult) -> String {
    use rmcp::model::ContentBlock;
    let mut s = String::new();
    for c in &r.content {
        if let ContentBlock::Text(t) = c {
            s.push_str(&t.text);
        }
    }
    s
}

fn image_of(r: &rmcp::model::CallToolResult) -> Option<String> {
    use rmcp::model::ContentBlock;
    r.content.iter().find_map(|c| match c {
        ContentBlock::Image(i) => Some(i.data.clone()),
        _ => None,
    })
}

async fn record_and_replay(
    recorder: &rmcp::service::RunningService<rmcp::service::RoleClient, ()>,
) {
    let start = recorder
        .call_tool(CallToolRequestParams::new("record_start").with_arguments(Map::new()))
        .await
        .expect("record_start");
    assert_ne!(start.is_error, Some(true), "{}", text_of(&start));
    for (name, args) in [
        ("pointer_move", json!({"x": 20, "y": 20})),
        ("click", json!({"x": 20, "y": 20})),
        ("tick", json!({"ms": 16})),
    ] {
        let r = recorder
            .call_tool(
                CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone()),
            )
            .await
            .unwrap();
        assert_ne!(r.is_error, Some(true), "{name}: {}", text_of(&r));
    }
    let stop = recorder
        .call_tool(CallToolRequestParams::new("record_stop").with_arguments(Map::new()))
        .await
        .expect("record_stop");
    assert_ne!(stop.is_error, Some(true), "{}", text_of(&stop));
    let log: Value = serde_json::from_str(&text_of(&stop)).expect("the log is JSON");
    assert!(log.get("steps").is_some(), "{log}");
    let replay = recorder
        .call_tool(
            CallToolRequestParams::new("replay")
                .with_arguments(json!({"log": log}).as_object().unwrap().clone()),
        )
        .await
        .expect("replay");
    assert_ne!(replay.is_error, Some(true), "{}", text_of(&replay));
    let result: Value = serde_json::from_str(&text_of(&replay)).unwrap();
    assert_eq!(result["match"], true, "{result}");
}

#[test]
fn a_standard_client_drives_every_tool_and_resource_over_stdio() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async move {
        let app = start_app("rows");
        let transport = TokioChildProcess::new(
            tokio::process::Command::new(env!("CARGO_BIN_EXE_bough-mcp")).configure(|c| {
                c.arg("--socket").arg(&app.socket);
            }),
        )
        .expect("spawn the MCP server");
        let client = ().serve(transport).await.expect("initialize");
        let info = client.peer_info().expect("server info");
        assert_eq!(info.server_info.as_ref().unwrap().name, "bough-mcp");

        let tools = client.list_all_tools().await.expect("tools/list");
        assert!(tools.len() >= 26, "{} tools", tools.len());
        for t in &tools {
            assert!(t.input_schema.get("type").is_some(), "{} schema", t.name);
        }

        let mut log = None;
        let mut golden = None;
        let mut fails = Vec::new();
        for t in &tools {
            let args = tool_args(&t.name, &mut log, &mut golden);
            match client
                .call_tool(CallToolRequestParams::new(t.name.to_string()).with_arguments(args))
                .await
            {
                Ok(r) => {
                    if r.is_error == Some(true) {
                        let text = text_of(&r);
                        // record/replay need a pristine application; this session has already run tools.
                        let expected =
                            matches!(t.name.as_ref(), "record_start" | "record_stop" | "replay")
                                && (text.contains("fresh, replayable")
                                    || text.contains("no recording is active")
                                    || text.ends_with(": log"));
                        if !expected {
                            fails.push(format!("{}: {}", t.name, text));
                        }
                    }
                    match t.name.as_ref() {
                        "record_stop" => {
                            if let Ok(v) = serde_json::from_str::<Value>(&text_of(&r)) {
                                log = Some(v);
                            }
                        }
                        "screenshot" => {
                            golden = image_of(&r);
                        }
                        _ => {}
                    }
                }
                Err(e) => fails.push(format!("{}: {e}", t.name)),
            }
        }
        assert!(fails.is_empty(), "{fails:#?}");

        // record -> replay on a pristine application: the tool refuses an app that has already run commands,
        // so this is a second app and a second client whose first calls are the recording ones.
        let pristine = start_app("table");
        let transport = TokioChildProcess::new(
            tokio::process::Command::new(env!("CARGO_BIN_EXE_bough-mcp")).configure(|c| {
                c.arg("--socket").arg(&pristine.socket);
            }),
        )
        .unwrap();
        let recorder = ().serve(transport).await.expect("pristine initialize");
        record_and_replay(&recorder).await;
        recorder.cancel().await.ok();
        drop(pristine);

        let resources = client.list_all_resources().await.expect("resources/list");
        assert!(resources.len() >= 3, "{} resources", resources.len());
        for r in &resources {
            let read = client
                .read_resource(ReadResourceRequestParams::new(r.uri.clone()))
                .await
                .expect("resources/read");
            assert!(!read.contents.is_empty(), "{} has no contents", r.uri);
        }

        // A reconnect: stop the server, start another against the same app, and read the same state.
        let before = text_of(
            &client
                .call_tool(CallToolRequestParams::new("state").with_arguments(Map::new()))
                .await
                .unwrap(),
        );
        client.cancel().await.ok();
        let transport = TokioChildProcess::new(
            tokio::process::Command::new(env!("CARGO_BIN_EXE_bough-mcp")).configure(|c| {
                c.arg("--socket").arg(&app.socket);
            }),
        )
        .unwrap();
        let again = ().serve(transport).await.expect("second initialize");
        let after = text_of(
            &again
                .call_tool(CallToolRequestParams::new("state").with_arguments(Map::new()))
                .await
                .unwrap(),
        );
        assert_eq!(before, after, "a new session sees the same state");
        again.cancel().await.ok();
    });
}

#[test]
fn the_http_transport_accepts_the_same_client() {
    use rmcp::transport::StreamableHttpClientTransport;
    let app = start_app("table");
    let mut child = Command::new(env!("CARGO_BIN_EXE_bough-mcp"))
        .args([
            "--socket",
            app.socket.to_str().unwrap(),
            "--http",
            "127.0.0.1:0",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the HTTP server");
    let ready = readiness(&mut child);
    let address = ready
        .strip_prefix("listening ")
        .expect("HTTP listener address")
        .to_owned();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async move {
        let transport = StreamableHttpClientTransport::from_uri(format!("http://{address}/mcp"));
        let client = ().serve(transport).await.expect("HTTP initialize");
        let tools = client.list_all_tools().await.expect("HTTP tools/list");
        assert!(tools.len() >= 26);
        record_and_replay(&client).await;
        let mut log = None;
        let mut golden = None;
        let mut fails = Vec::new();
        for t in &tools {
            let mut args = tool_args(&t.name, &mut log, &mut golden);
            if matches!(t.name.as_ref(), "deps" | "set_field") {
                args.insert("field".into(), json!("selected"));
            }
            match client
                .call_tool(CallToolRequestParams::new(t.name.to_string()).with_arguments(args))
                .await
            {
                Ok(r) => {
                    if r.is_error == Some(true) {
                        let text = text_of(&r);
                        // record/replay need a pristine application; this session has already run tools.
                        let expected =
                            matches!(t.name.as_ref(), "record_start" | "record_stop" | "replay")
                                && (text.contains("fresh, replayable")
                                    || text.contains("no recording is active")
                                    || text.ends_with(": log"));
                        if !expected {
                            fails.push(format!("{}: {}", t.name, text));
                        }
                    }
                    match t.name.as_ref() {
                        "record_stop" => {
                            if let Ok(v) = serde_json::from_str::<Value>(&text_of(&r)) {
                                log = Some(v);
                            }
                        }
                        "screenshot" => {
                            golden = image_of(&r);
                        }
                        _ => {}
                    }
                }
                Err(e) => fails.push(format!("{}: {e}", t.name)),
            }
        }
        assert!(fails.is_empty(), "{fails:#?}");

        let resources = client
            .list_all_resources()
            .await
            .expect("HTTP resources/list");
        assert!(resources.len() >= 9, "{} resources", resources.len());
        for resource in resources {
            let read = client
                .read_resource(rmcp::model::ReadResourceRequestParams::new(resource.uri))
                .await
                .unwrap();
            assert!(!read.contents.is_empty());
        }
        client.cancel().await.ok();
    });
    let _ = child.kill();
    let _ = child.wait();
}
