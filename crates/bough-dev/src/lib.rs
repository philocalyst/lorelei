//! The lorelei developer loop: watch sources, rebuild, reload or restart with typed state preserved, and serve
//! the MCP endpoint. Forked from the gpui-ce CLI (`gpui-ce/cli` @ 6774184, Apache-2.0): see `NOTICE` and
//! `README.md` for what is inherited and what is different.
pub mod hot;
#[cfg(feature = "hot-view")]
pub mod hotview;
pub mod inspect;
#[cfg(feature = "hot-view")]
pub mod patch;
pub mod watch;

use bough_inspect::json::{self, Value as J};
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;

pub use hot::{AppSpec, Rebuild, RestartReport, Running};
pub use watch::{Change, Kind, Watcher};

/// Serve the MCP endpoint at `addr` from this process, over the application's socket. Returns the bound address
/// and a handle to stop it (dropping the handle does not close the listener; the process ending does).
pub fn serve_mcp_http(
    socket: impl Into<std::path::PathBuf>,
    addr: &str,
) -> Result<std::net::SocketAddr, String> {
    let listener = TcpListener::bind(addr).map_err(|e| format!("cannot bind {addr}: {e}"))?;
    let at = listener.local_addr().map_err(|e| e.to_string())?;
    let socket = socket.into();
    std::thread::Builder::new()
        .name("bough-mcp-http".into())
        .spawn(move || {
            let mut m = bough_mcp::Mcp::new(socket);
            if let Err(e) = bough_mcp::serve_http(&mut m, listener) {
                eprintln!("bough-dev: mcp http: {e}");
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(at)
}

/// Serve MCP over stdio in this process (the `bough-dev mcp` command).
pub fn serve_mcp_stdio(socket: impl Into<std::path::PathBuf>) -> Result<(), String> {
    let mut m = bough_mcp::Mcp::new(socket);
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    bough_mcp::serve_stream(&mut m, std::io::BufReader::new(stdin.lock()), stdout.lock())
        .map_err(|e| e.to_string())
}

/// The typed state of a running application (a `state` file's contents).
pub fn read_state_file(path: &Path) -> Result<Vec<(String, J)>, String> {
    hot::read_state(path)
}
pub fn write_state_file(path: &Path, st: &[(String, J)]) -> Result<(), String> {
    hot::write_state(path, st)
}

/// The one-line summary of a state file (for a log).
pub fn state_line(st: &[(String, J)]) -> String {
    st.iter()
        .map(|(f, v)| format!("{f}={}", inspect::value_text(v)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A description of a batch for the log.
pub fn describe(changes: &[Change]) -> String {
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

/// A state file as JSON (for `state --json`).
pub fn state_json(st: &[(String, J)]) -> String {
    let arr: Vec<J> = st
        .iter()
        .map(|(f, v)| J::obj().set("field", f.as_str()).set("value", v.clone()))
        .collect();
    json::to_string(&J::Arr(arr))
}

/// Build an `Arc<str>` without failing on a poisoned mutex: not used by the loop, kept for a host that wants a
/// shared label.
pub fn shared_label(s: &str) -> Arc<str> {
    Arc::from(s)
}
