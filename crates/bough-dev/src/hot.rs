//! The hot path and its fallback.
//!
//! What is real here: an application is started, its typed state is read through the inspector's admission,
//! it is stopped, a rebuilt binary is started, and the state is written back through the same admission — then
//! the two descriptions are compared by digest. A change that is not code (a theme, an asset) goes through the
//! application's own `reload` instead: no rebuild, no restart, state untouched.
//!
//! What is not: patching Rust functions in a running process (the dylib/jump-table route Subsecond takes in the
//! gpui-ce CLI). The unit here is the view description; a changed `view()` is new code, and new code needs a
//! new binary. The dev server therefore classifies a Rust change as a *state-preserving restart* and says so.
use bough_inspect::client::Client;
use bough_inspect::json::{self, Value as J};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// How to start the application under development.
#[derive(Clone, Debug)]
pub struct AppSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub socket: PathBuf,
    pub cwd: Option<PathBuf>,
    /// The arguments the app is given for the inspector socket (`--socket <path>` by default).
    pub socket_args: Vec<String>,
}
impl AppSpec {
    pub fn new(program: impl Into<PathBuf>, socket: impl Into<PathBuf>) -> AppSpec {
        AppSpec {
            program: program.into(),
            args: Vec::new(),
            socket: socket.into(),
            cwd: None,
            socket_args: Vec::new(),
        }
    }
    fn command(&self) -> Command {
        let mut c = Command::new(&self.program);
        c.args(&self.args);
        if self.socket_args.is_empty() {
            c.arg("--socket").arg(&self.socket);
        } else {
            for a in &self.socket_args {
                c.arg(a.replace("{socket}", &self.socket.display().to_string()));
            }
        }
        if let Some(d) = &self.cwd {
            c.current_dir(d);
        }
        c
    }
}

pub struct Running {
    pub child: Child,
    pub socket: PathBuf,
    client: Option<Client>,
    /// Readiness was announced on stdout (`ready <path>`), so the line has been consumed.
    ready: bool,
}
impl Running {
    /// Start the application and wait until its inspector answers (or its `ready` line arrives). The child's
    /// output is drained on helper threads: it can never block on a full pipe, and an early exit can say why.
    pub fn start(spec: &AppSpec, timeout: Duration) -> Result<Running, String> {
        use std::collections::VecDeque;
        use std::sync::{Arc, Mutex};
        use std::sync::mpsc;
        let mut child = spec
            .command()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot start {}: {e}", spec.program.display()))?;
        let (tx, rx) = mpsc::channel();
        if let Some(out) = child.stdout.take() {
            std::thread::spawn(move || {
                for line in BufReader::new(out).lines() {
                    match line {
                        Ok(l) => {
                            if tx.send(l).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }
        let tail: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));
        if let Some(err) = child.stderr.take() {
            let t = Arc::clone(&tail);
            std::thread::spawn(move || {
                let mut r = BufReader::new(err);
                let mut line = String::new();
                while r.read_line(&mut line).unwrap_or(0) > 0 {
                    let mut q = t.lock().unwrap_or_else(|e| e.into_inner());
                    q.push_back(line.trim_end().to_owned());
                    if q.len() > 40 {
                        q.pop_front();
                    }
                    line.clear();
                }
            });
        }
        let mut ready = false;
        let deadline = Instant::now() + timeout;
        while !ready && Instant::now() < deadline {
            if let Ok(line) = rx.recv_timeout(Duration::from_millis(20))
                && line.starts_with("ready ")
            {
                ready = true;
            }
            if let Ok(Some(st)) = child.try_wait() {
                std::thread::sleep(Duration::from_millis(20));
                let q = tail.lock().unwrap_or_else(|e| e.into_inner());
                return Err(format!(
                    "the application exited before it was ready ({st}):\n{}",
                    q.iter().cloned().collect::<Vec<_>>().join("\n")
                ));
            }
        }
        // whatever the readiness protocol, the socket is the contract
        let mut client = None;
        while Instant::now() < deadline {
            if let Ok(c) = Client::open(&spec.socket) {
                client = Some(c);
                break;
            }
            if let Ok(Some(st)) = child.try_wait() {
                let q = tail.lock().unwrap_or_else(|e| e.into_inner());
                return Err(format!(
                    "the application exited before its socket answered ({st}):\n{}",
                    q.iter().cloned().collect::<Vec<_>>().join("\n")
                ));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let client = client.ok_or_else(|| {
            let _ = child.kill();
            format!(
                "the application's inspector socket {} did not answer within {:?}",
                spec.socket.display(),
                timeout
            )
        })?;
        Ok(Running {
            child,
            socket: spec.socket.clone(),
            client: Some(client),
            ready,
        })
    }
    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn client(&mut self) -> Result<&mut Client, String> {
        self.client
            .as_mut()
            .ok_or_else(|| "no inspector connection".to_owned())
    }
    pub fn stop(&mut self) {
        if let Some(c) = self.client.as_mut() {
            let _ = c.request("shutdown", &J::obj());
        }
        self.client = None;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                _ => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    return;
                }
            }
        }
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The application's typed state, as `(field, value)`.
pub fn state(c: &mut Client) -> Result<Vec<(String, J)>, String> {
    let r = c
        .request("state", &J::obj())
        .map_err(|e| e.to_string())?
        .map_err(|e| e.message)?;
    Ok(r.get("fields")
        .map(J::arr)
        .unwrap_or(&[])
        .iter()
        .map(|f| {
            (
                f.get("name").and_then(J::as_str).unwrap_or("").to_owned(),
                f.get("value").cloned().unwrap_or(J::Null),
            )
        })
        .collect())
}

/// Write a state back through the inspector's typed admission. Returns `(written, refusals)`.
pub fn restore(c: &mut Client, st: &[(String, J)]) -> (usize, Vec<String>) {
    let mut ok = 0;
    let mut bad = Vec::new();
    for (field, value) in st {
        let p = J::obj().set("field", field.as_str()).set("value", value.clone());
        match c.request("set", &p) {
            Ok(Ok(_)) => ok += 1,
            Ok(Err(e)) => bad.push(format!("{field}: {}", e.message)),
            Err(e) => bad.push(format!("{field}: {e}")),
        }
    }
    (ok, bad)
}

/// The description's identity, as `tree` reports it.
pub fn tree_digest(c: &mut Client) -> Result<String, String> {
    let r = c
        .request("tree", &J::obj().set("nodes", false))
        .map_err(|e| e.to_string())?
        .map_err(|e| e.message)?;
    Ok(r.get("digest")
        .and_then(J::as_str)
        .unwrap_or("")
        .to_owned())
}

#[derive(Clone, Debug)]
pub struct RestartReport {
    pub fields: usize,
    pub restored: usize,
    pub refusals: Vec<String>,
    pub before: String,
    pub after: String,
    pub matched: bool,
}
impl RestartReport {
    pub fn line(&self) -> String {
        format!(
            "state {}/{} restored{}; description {} ({} -> {})",
            self.restored,
            self.fields,
            if self.refusals.is_empty() {
                String::new()
            } else {
                format!(" ({} refused: {})", self.refusals.len(), self.refusals.join("; "))
            },
            if self.matched { "matched" } else { "DIFFERED" },
            self.before,
            self.after
        )
    }
}

/// Stop the application, start a new binary and put the typed state back. The fallback the hot path refuses
/// (a Rust change) lands here; an asset change never needs it.
pub fn restart_preserving_state(
    spec: &AppSpec,
    old: &mut Running,
    timeout: Duration,
) -> Result<RestartReport, String> {
    let (st, before) = {
        let c = old.client()?;
        (state(c)?, tree_digest(c)?)
    };
    old.stop();
    let mut new = Running::start(spec, timeout)?;
    let (restored, refusals) = {
        let c = new.client()?;
        restore(c, &st)
    };
    let after = {
        let c = new.client()?;
        // a reload re-describes from the restored state (the lab's reload is a re-description)
        let _ = c.request("reload", &J::obj());
        tree_digest(c)?
    };
    let matched = before == after;
    let report = RestartReport {
        fields: st.len(),
        restored,
        refusals,
        before,
        after,
        matched,
    };
    *old = new;
    Ok(report)
}

/// `cargo build` the package, in `cwd`, with the given features and target. Returns the build's stderr on error.
pub struct Rebuild {
    pub package: String,
    pub bin: Option<String>,
    pub example: Option<String>,
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    pub profile: Option<String>,
    pub cwd: Option<PathBuf>,
    pub offline: bool,
}
impl Rebuild {
    pub fn new(package: &str) -> Rebuild {
        Rebuild {
            package: package.to_owned(),
            bin: None,
            example: None,
            features: Vec::new(),
            all_features: false,
            no_default_features: false,
            profile: None,
            cwd: None,
            offline: false,
        }
    }
    pub fn args(&self) -> Vec<String> {
        let mut a = vec!["build".to_owned(), "-p".to_owned(), self.package.clone()];
        if let Some(b) = &self.bin {
            a.push("--bin".into());
            a.push(b.clone());
        }
        if let Some(e) = &self.example {
            a.push("--example".into());
            a.push(e.clone());
        }
        if self.all_features {
            a.push("--all-features".into());
        }
        if self.no_default_features {
            a.push("--no-default-features".into());
        }
        for f in &self.features {
            a.push("--features".into());
            a.push(f.clone());
        }
        if let Some(p) = &self.profile {
            a.push("--profile".into());
            a.push(p.clone());
        }
        if self.offline {
            a.push("--offline".into());
        }
        a
    }
    pub fn run(&self) -> Result<(), String> {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut c = Command::new(cargo);
        c.args(self.args());
        if let Some(d) = &self.cwd {
            c.current_dir(d);
        }
        let out = c.output().map_err(|e| format!("cannot run cargo: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            let err = String::from_utf8_lossy(&out.stderr);
            let tail: Vec<&str> = err.lines().rev().take(30).collect();
            let tail: Vec<&str> = tail.into_iter().rev().collect();
            Err(tail.join("\n"))
        }
    }
}

/// The socket path for a development run (short enough for a Unix address).
pub fn default_socket(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir();
    let p = dir.join(format!("bough-dev-{}-{tag}.sock", std::process::id()));
    assert!(p.as_os_str().len() < 100, "socket path too long: {}", p.display());
    p
}

/// Whether a path is under one of the roots (the dev server's classification of a changed file).
pub fn under(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| path.starts_with(r))
}

/// A compact one-line description of a change, for the dev server's log.
pub fn describe(changes: &[super::watch::Change]) -> String {
    let mut by: Vec<String> = changes
        .iter()
        .take(8)
        .map(|c| format!("{:?}:{}", c.kind, c.path.display()))
        .collect();
    if changes.len() > 8 {
        by.push(format!("and {} more", changes.len() - 8));
    }
    by.join(", ")
}

/// Parse a state file (`[{"field":..., "value":...}, ...]`).
pub fn read_state(path: &Path) -> Result<Vec<(String, J)>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v = json::parse(&text).map_err(|e| e.to_string())?;
    Ok(v
        .arr()
        .iter()
        .map(|f| {
            (
                f.get("field").and_then(J::as_str).unwrap_or("").to_owned(),
                f.get("value").cloned().unwrap_or(J::Null),
            )
        })
        .collect())
}

pub fn write_state(path: &Path, st: &[(String, J)]) -> Result<(), String> {
    let arr: Vec<J> = st
        .iter()
        .map(|(f, v)| J::obj().set("field", f.as_str()).set("value", v.clone()))
        .collect();
    std::fs::write(path, json::to_string(&J::Arr(arr))).map_err(|e| format!("{}: {e}", path.display()))
}
