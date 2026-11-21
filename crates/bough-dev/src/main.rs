//! `bough-dev`: the developer loop for a lorelei (bough) application.
//!
//! ```text
//! bough-dev run --app target/debug/bough-inspect-app --scenario rows --rows 10000
//! bough-dev inspect tree --socket /tmp/b.sock
//! bough-dev mcp --socket /tmp/b.sock --http 127.0.0.1:7777
//! ```
use bough_dev::hot::{self, AppSpec, Rebuild, Running};
use bough_dev::{Kind, Watcher};
use bough_inspect::client::Client;
use std::path::PathBuf;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match run(&args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("bough-dev: {e}");
            1
        }
    };
    std::process::exit(code);
}

struct Opts {
    socket: Option<PathBuf>,
    app: Option<PathBuf>,
    app_args: Vec<String>,
    roots: Vec<PathBuf>,
    http: Option<String>,
    once: bool,
    interval_ms: u64,
    timeout_ms: u64,
    package: Option<String>,
    bin: Option<String>,
    example: Option<String>,
    features: Vec<String>,
    all_features: bool,
    no_default_features: bool,
    offline: bool,
    file: Option<PathBuf>,
    source: Option<PathBuf>,
}
fn parse(rest: &[String]) -> Result<Opts, String> {
    let mut o = Opts {
        socket: None,
        app: None,
        app_args: Vec::new(),
        roots: Vec::new(),
        http: None,
        once: false,
        interval_ms: 250,
        timeout_ms: 15_000,
        package: None,
        bin: None,
        example: None,
        features: Vec::new(),
        all_features: false,
        no_default_features: false,
        offline: false,
        file: None,
        source: None,
    };
    let mut i = 0;
    while i < rest.len() {
        let a = &rest[i];
        let mut val = |name: &str| -> Result<String, String> {
            i += 1;
            rest.get(i)
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match a.as_str() {
            "--socket" => o.socket = Some(PathBuf::from(val("--socket")?)),
            "--app" => o.app = Some(PathBuf::from(val("--app")?)),
            "--app-arg" => o.app_args.push(val("--app-arg")?),
            "--watch" => o.roots.push(PathBuf::from(val("--watch")?)),
            "--http" => o.http = Some(val("--http")?),
            "--once" => o.once = true,
            "--interval-ms" => {
                o.interval_ms = val("--interval-ms")?.parse().map_err(|_| "interval")?
            }
            "--timeout-ms" => o.timeout_ms = val("--timeout-ms")?.parse().map_err(|_| "timeout")?,
            "-p" | "--package" => o.package = Some(val("--package")?),
            "--bin" => o.bin = Some(val("--bin")?),
            "--example" => o.example = Some(val("--example")?),
            "--features" => o.features.extend(
                val("--features")?
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
            ),
            "--all-features" => o.all_features = true,
            "--no-default-features" => o.no_default_features = true,
            "--offline" => o.offline = true,
            "--file" => o.file = Some(PathBuf::from(val("--file")?)),
            "--source" => o.source = Some(PathBuf::from(val("--source")?)),
            "--" => {
                o.app_args.extend(rest[i + 1..].iter().cloned());
                i = rest.len();
            }
            other => return Err(format!("unknown option {other:?}")),
        }
        i += 1;
    }
    Ok(o)
}

fn socket_of(o: &Opts) -> Result<PathBuf, String> {
    o.socket
        .clone()
        .or_else(|| std::env::var_os("BOUGH_INSPECT_SOCKET").map(PathBuf::from))
        .ok_or_else(|| "no socket: pass --socket or set BOUGH_INSPECT_SOCKET".to_owned())
}

fn spec_of(o: &Opts) -> Result<AppSpec, String> {
    let socket = match &o.socket {
        Some(s) => s.clone(),
        None => hot::default_socket("run"),
    };
    let program = o
        .app
        .clone()
        .unwrap_or_else(|| PathBuf::from("bough-inspect-app"));
    let mut spec = AppSpec::new(program, socket);
    spec.args = if o.app_args.is_empty() {
        vec![
            "--scenario".into(),
            "rows".into(),
            "--rows".into(),
            "1000".into(),
        ]
    } else {
        o.app_args.clone()
    };
    Ok(spec)
}

/// `inspect` takes a subcommand and then its own arguments, so it is parsed apart from the option grammar.
fn inspect_cmd(args: &[String]) -> Result<(), String> {
    let mut socket = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--socket" {
            i += 1;
            socket = Some(PathBuf::from(
                args.get(i).ok_or("--socket needs a value")?.clone(),
            ));
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    let socket = socket
        .or_else(|| std::env::var_os("BOUGH_INSPECT_SOCKET").map(PathBuf::from))
        .ok_or("no socket: pass --socket or set BOUGH_INSPECT_SOCKET")?;
    bough_dev::inspect::run(&socket, &rest)
}

fn run(args: &[String]) -> Result<(), String> {
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    if cmd == "inspect" {
        return inspect_cmd(&args[1..]);
    }
    let o = parse(&args[1..])?;
    match cmd {
        "run" | "watch" => dev(&o),
        "rebuild" => rebuild(&o),
        "restart" => restart(&o),
        "reload" => {
            let socket = socket_of(&o)?;
            let mut c = Client::open(&socket).map_err(|e| format!("{}: {e}", socket.display()))?;
            let v = c
                .request("reload", &bough_inspect::json::Value::obj())
                .map_err(|e| e.to_string())?
                .map_err(|e| e.message)?;
            println!("{}", bough_inspect::json::to_string(&v));
            Ok(())
        }
        "state" => {
            let socket = socket_of(&o)?;
            let mut c = Client::open(&socket).map_err(|e| format!("{}: {e}", socket.display()))?;
            let st = hot::state(&mut c)?;
            if let Some(f) = &o.file {
                hot::write_state(f, &st)?;
                println!("{} fields written to {}", st.len(), f.display());
            } else {
                println!("{}", bough_dev::state_json(&st));
            }
            Ok(())
        }
        "restore" => {
            let socket = socket_of(&o)?;
            let file = o.file.clone().ok_or("restore needs --file")?;
            let st = hot::read_state(&file)?;
            let mut c = Client::open(&socket).map_err(|e| format!("{}: {e}", socket.display()))?;
            let (ok, bad) = hot::restore(&mut c, &st);
            for b in &bad {
                eprintln!("refused: {b}");
            }
            println!("{ok}/{} fields restored", st.len());
            if bad.is_empty() {
                Ok(())
            } else {
                Err(format!("{} refused", bad.len()))
            }
        }
        "hotpatch" => {
            let socket = socket_of(&o)?;
            let source = o
                .source
                .clone()
                .ok_or("hotpatch needs --source FILE (the patched view code)")?;
            let mut c = Client::open(&socket).map_err(|e| format!("{}: {e}", socket.display()))?;
            let out = bough_dev::patch::patch_source(&mut c, &source)?;
            println!("{out}");
            if out.needs_restart() {
                Err("the patch was not applied: rebuild and restart with state".to_owned())
            } else {
                Ok(())
            }
        }
        "inspect" => unreachable!("inspect is parsed apart"),
        "mcp" => {
            let socket = socket_of(&o)?;
            match &o.http {
                Some(addr) => {
                    // on this thread: a `shutdown` request closes the server and the process exits
                    let listener = std::net::TcpListener::bind(addr.as_str())
                        .map_err(|e| format!("cannot bind {addr}: {e}"))?;
                    println!(
                        "listening {}",
                        listener.local_addr().map_err(|e| e.to_string())?
                    );
                    let mut m = bough_mcp::Mcp::new(socket);
                    bough_mcp::serve_http(&mut m, listener).map_err(|e| e.to_string())
                }
                None => bough_dev::serve_mcp_stdio(socket),
            }
        }
        "version" | "--version" | "-V" => {
            println!("bough-dev {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "help" | "--help" | "-h" => {
            println!(
                "bough-dev <command>\n\
                 \n\
                 run       start the app, watch sources, rebuild and restart with state (--once: one change)\n\
                 watch     alias of run\n\
                 rebuild   cargo build the package, then restart the running app with its state preserved\n\
                 restart   stop the running app, start --app, restore its typed state, compare descriptions\n\
                 reload    ask the running app to re-read its own inputs (no rebuild)\n\
                 hotpatch  build --source into a patch dylib and load it in the running app (same PID/world)\n\
                 state     print or save the typed state (--file)\n\
                 restore   write a saved state back through the app's admission (--file)\n\
                 inspect   tree|node|find|pick|deps|why|counters|state|set|click|key|type|tick|shot|diff|reload|stop\n\
                 mcp       serve MCP over stdio, or --http ADDR\n\
                 version   print the version\n\
                 \n\
                 options: --socket PATH --app PROG --app-arg A --watch ROOT --http ADDR --once --interval-ms N\n\
                          --source FILE\n\
                          -p PKG --bin BIN --example EX --features a,b --all-features --no-default-features"
            );
            Ok(())
        }
        other => Err(format!("no command {other:?} (try help)")),
    }
}

fn dev(o: &Opts) -> Result<(), String> {
    let spec = spec_of(o)?;
    let mut running = Running::start(&spec, Duration::from_millis(o.timeout_ms))?;
    println!(
        "app ready: {} (socket {}){}",
        spec.program.display(),
        spec.socket.display(),
        if running.ready() {
            ""
        } else {
            " (no ready line)"
        }
    );
    if let Some(addr) = &o.http {
        let at = bough_dev::serve_mcp_http(spec.socket.clone(), addr)?;
        println!("mcp listening {at}");
    }
    let roots = if o.roots.is_empty() {
        vec![
            PathBuf::from("crates/bough-inspect-app/src"),
            PathBuf::from("comparisons/bough-guibench-x/src"),
            PathBuf::from("crates/bough-inspect/src"),
        ]
    } else {
        o.roots.clone()
    };
    let mut w = Watcher::new(roots.clone());
    let _ = w.scan();
    let package = o
        .package
        .clone()
        .unwrap_or_else(|| "bough-inspect-app".into());
    loop {
        std::thread::sleep(Duration::from_millis(o.interval_ms));
        let changes = w.scan();
        if changes.is_empty() {
            continue;
        }
        println!("change: {}", bough_dev::describe(&changes));
        match Watcher::worst(&changes) {
            Some(Kind::Asset) => {
                let c = running.client()?;
                match c.request("reload", &bough_inspect::json::Value::obj()) {
                    Ok(Ok(v)) => println!("reloaded: {}", bough_inspect::json::to_string(&v)),
                    Ok(Err(e)) => eprintln!("reload refused: {}", e.message),
                    Err(e) => eprintln!("reload: {e}"),
                }
            }
            Some(_) => {
                let mut rb = Rebuild::new(&package);
                rb.bin = o.bin.clone();
                rb.example = o.example.clone();
                rb.features = o.features.clone();
                rb.all_features = o.all_features;
                rb.no_default_features = o.no_default_features;
                rb.offline = o.offline;
                match rb.run() {
                    Ok(()) => match hot::restart_preserving_state(
                        &spec,
                        &mut running,
                        Duration::from_millis(o.timeout_ms),
                    ) {
                        Ok(r) => println!("{}", r.line()),
                        Err(e) => eprintln!("restart: {e}"),
                    },
                    Err(e) => {
                        // the old app and its state stay; the next valid save retries
                        eprintln!("build failed; the running app is kept:\n{e}");
                    }
                }
            }
            None => {}
        }
        if o.once {
            return Ok(());
        }
    }
}

fn rebuild(o: &Opts) -> Result<(), String> {
    let socket = socket_of(o)?;
    let package = o
        .package
        .clone()
        .unwrap_or_else(|| "bough-inspect-app".into());
    let mut rb = Rebuild::new(&package);
    rb.bin = o.bin.clone();
    rb.example = o.example.clone();
    rb.features = o.features.clone();
    rb.all_features = o.all_features;
    rb.no_default_features = o.no_default_features;
    rb.offline = o.offline;
    println!("building {} {}...", rb.package, rb.args().join(" "));
    rb.run()?;
    // the new binary is the same program with the same arguments; the socket is the contract
    let program = o
        .app
        .clone()
        .ok_or("rebuild needs --app (the new binary)")?;
    let mut spec = AppSpec::new(program, socket.clone());
    spec.args = o.app_args.clone();
    restart_between(&socket, &spec, Duration::from_millis(o.timeout_ms))
}

fn restart(o: &Opts) -> Result<(), String> {
    let socket = socket_of(o)?;
    let program = o.app.clone().ok_or("restart needs --app")?;
    let mut spec = AppSpec::new(program, socket.clone());
    spec.args = o.app_args.clone();
    restart_between(&socket, &spec, Duration::from_millis(o.timeout_ms))
}

/// Snapshot the state of the app at `socket`, stop it, start `spec`, restore, reload and compare. This is the
/// fallback a hot patch refuses; `run` and `rebuild` both land here for a Rust change.
fn restart_between(socket: &PathBuf, spec: &AppSpec, timeout: Duration) -> Result<(), String> {
    let (st, before) = {
        let mut c = Client::open(socket).map_err(|e| format!("{}: {e}", socket.display()))?;
        (hot::state(&mut c)?, hot::tree_digest(&mut c)?)
    };
    {
        let mut c = Client::open(socket).map_err(|e| e.to_string())?;
        let _ = c.request("shutdown", &bough_inspect::json::Value::obj());
    }
    // wait for the old socket to go (the app removes it on a clean exit)
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while Client::open(socket).is_ok() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut running = Running::start(spec, timeout)?;
    let (restored, refusals) = {
        let c = running.client()?;
        hot::restore(c, &st)
    };
    let after = {
        let c = running.client()?;
        let _ = c.request("reload", &bough_inspect::json::Value::obj());
        hot::tree_digest(c)?
    };
    let r = hot::RestartReport {
        fields: st.len(),
        restored,
        refusals,
        before: before.clone(),
        after: after.clone(),
        matched: before == after,
    };
    println!("{}", r.line());
    running.stop();
    if r.matched {
        Ok(())
    } else {
        Err("the restored description differs from the one before the restart".into())
    }
}
