//! Building and applying one view patch.
//!
//! Cargo builds a patch `cdylib` from source exporting `hot_text` and `HOT_STATE_TOKEN`.
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

/// Compile trusted patch source using an isolated Cargo manifest and publish the artifact at `out`.
pub fn build(source: &Path, out: &Path) -> Result<(), String> {
    let source = source.canonicalize().map_err(|e| e.to_string())?;
    let source = source.to_str().ok_or("patch source path must be UTF-8")?;
    let parent = out.parent().ok_or("patch destination must have a parent")?;
    let stage = parent.join(format!(
        ".cargo-patch-{}-{}",
        std::process::id(),
        next_patch()
    ));
    std::fs::create_dir(&stage).map_err(|e| e.to_string())?;
    struct Stage(PathBuf);
    impl Drop for Stage {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let stage = Stage(stage);
    let manifest = stage.0.join("Cargo.toml");
    let source = bough_inspect::json::to_string(&J::from(source));
    std::fs::write(&manifest, format!("[workspace]\n[package]\nname = \"bough_view_patch\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[lib]\npath = {source}\ncrate-type = [\"cdylib\"]\n[profile.release]\npanic = \"abort\"\n"))
        .map_err(|e| e.to_string())?;
    let target = stage.0.join("target");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let result = Command::new(&cargo)
        .args(["build", "--offline", "--release", "--manifest-path"])
        .arg(&manifest)
        .arg("--target-dir")
        .arg(&target)
        .output()
        .map_err(|e| format!("cannot run Cargo: {e}"))?;
    if !result.status.success() {
        return Err(format!(
            "Cargo patch build failed:\n{}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    let artifact = target.join("release").join(format!(
        "{}bough_view_patch{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    ));
    std::fs::rename(artifact, out).map_err(|e| format!("publish patch artifact: {e}"))
}

fn next_patch() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Build one source file and load its newly linked artifact. Every build gets a distinct loader path.
pub fn patch_source(client: &mut Client, source: &Path) -> Result<Outcome, String> {
    let source = source.canonicalize().map_err(|e| e.to_string())?;
    let out = source.with_extension(format!(
        "patch-{}-{}{}",
        std::process::id(),
        next_patch(),
        std::env::consts::DLL_SUFFIX
    ));
    build(&source, &out)?;
    Ok(apply(client, &out))
}

/// Apply a patch to a managed app, restoring its registered fields through a restart on refusal.
pub fn patch_or_restart(
    running: &mut crate::hot::Running,
    spec: &crate::hot::AppSpec,
    source: &Path,
    timeout: std::time::Duration,
) -> Result<(Outcome, Option<crate::hot::RestartReport>), String> {
    let outcome = patch_source(running.client()?, source)?;
    let restart = if matches!(outcome, Outcome::Refused(_)) {
        Some(crate::hot::restart_preserving_state(
            spec, running, timeout,
        )?)
    } else {
        None
    };
    Ok((outcome, restart))
}

/// Ask the running application to load `dylib` and re-describe. The application checks the patch's state token
/// before installing the boundary function.
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
