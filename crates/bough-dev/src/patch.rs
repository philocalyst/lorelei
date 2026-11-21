//! Building and applying one view patch.
//!
//! A patch is a `cdylib` built by `rustc` from a source file that exports `hot_text` and `hot_state_token`.
//! [`build`] compiles it; [`apply`] asks the running application to load it through its inspector socket (the
//! load itself happens in the application's process, at its hot-call boundary). A refusal is a value: the loop
//! then takes the existing state-preserving restart path.
use bough_inspect::client::Client;
use bough_inspect::json::Value as J;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What happened to a patch.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// The application loaded it and re-described.
    Patched { revision: u32, digest: String },
    /// The application refused it; the world and the code are unchanged.
    Refused(String),
    /// The application could not be asked (no socket, a dead process).
    Unreachable(String),
}

impl Outcome {
    /// Whether the developer loop must fall back to the state-preserving restart.
    pub fn needs_restart(&self) -> bool {
        !matches!(self, Outcome::Patched { .. })
    }
}

impl std::fmt::Display for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Outcome::Patched { revision, digest } => {
                write!(f, "patched (revision {revision}, {digest})")
            }
            Outcome::Refused(e) => write!(f, "refused: {e}"),
            Outcome::Unreachable(e) => write!(f, "unreachable: {e}"),
        }
    }
}

/// Compile one patch source into a shared library. `rustc` is the toolchain in `PATH`; the patch is plain
/// `extern "C"` code with no dependency on the application's crates, so it links against the C runtime.
pub fn build(source: &Path, out: &Path) -> Result<(), String> {
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let tmp = out.with_extension("rlib.tmp");
    let status = Command::new(&rustc)
        .arg("--crate-type")
        .arg("cdylib")
        .arg("--edition")
        .arg("2021")
        .arg("-O")
        .arg("-C")
        .arg("panic=abort")
        .arg("-o")
        .arg(out)
        .arg(source)
        .output()
        .map_err(|e| format!("cannot run {}: {e}", PathBuf::from(&rustc).display()))?;
    let _ = std::fs::remove_file(tmp);
    if !status.status.success() {
        return Err(format!(
            "the patch did not compile:\n{}",
            String::from_utf8_lossy(&status.stderr)
        ));
    }
    Ok(())
}

/// Build one source file beside itself and apply it: the developer loop's whole patch step for a view whose
/// text crosses the hot-call boundary.
pub fn patch_source(client: &mut Client, source: &Path) -> Result<Outcome, String> {
    let out = source.with_extension("patch.dylib");
    build(source, &out)?;
    Ok(apply(client, &out))
}

/// Ask the running application to load `dylib` and re-describe. The application checks the patch's state token
/// before any of its code runs.
pub fn apply(client: &mut Client, dylib: &Path) -> Outcome {
    let p = J::obj().set("path", dylib.to_string_lossy().as_ref());
    match client.request("hotpatch", &p) {
        Ok(Ok(v)) => Outcome::Patched {
            revision: v.get("revision").and_then(J::as_u32).unwrap_or(0) as u32,
            digest: v.get("digest").and_then(J::as_str).unwrap_or("").to_owned(),
        },
        Ok(Err(e)) => Outcome::Refused(e.message),
        Err(e) => Outcome::Unreachable(e.to_string()),
    }
}
