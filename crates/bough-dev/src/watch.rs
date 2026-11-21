//! A polling source watcher: no platform file-event dependency, and a missed notification cannot lose an edit
//! (a scan compares content, not only mtime). The gpui-ce CLI's watcher (upstream `watch.rs`) is the model; this
//! one trades its `notify` backend for a deterministic scan so a test can drive it without sleeping on an event.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Rust source: a patch needs a compile and a restart with state.
    Source,
    /// Manifest, lockfile, build script, shader or a local dependency: a full rebuild.
    Config,
    /// An asset (theme, JSON, image): the running application can re-read it without a rebuild.
    Asset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub mtime: SystemTime,
    pub len: u64,
    pub hash: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub path: PathBuf,
    pub kind: Kind,
}

pub struct Watcher {
    roots: Vec<PathBuf>,
    seen: BTreeMap<PathBuf, Stamp>,
    /// The first scan establishes the baseline and reports nothing.
    primed: bool,
}
impl Watcher {
    pub fn new(roots: impl IntoIterator<Item = PathBuf>) -> Watcher {
        Watcher {
            roots: roots.into_iter().collect(),
            seen: BTreeMap::new(),
            primed: false,
        }
    }
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }
    /// Every file under the roots that changed, was added or was removed since the last scan.
    pub fn scan(&mut self) -> Vec<Change> {
        let mut now = BTreeMap::new();
        for root in &self.roots {
            collect(root, &mut now);
        }
        let mut out = Vec::new();
        if self.primed {
            for (p, s) in &now {
                match self.seen.get(p) {
                    Some(old) if old == s => {}
                    _ => out.push(Change {
                        path: p.clone(),
                        kind: classify(p),
                    }),
                }
            }
            for p in self.seen.keys() {
                if !now.contains_key(p) {
                    out.push(Change {
                        path: p.clone(),
                        kind: classify(p),
                    });
                }
            }
        }
        self.seen = now;
        self.primed = true;
        out.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }
    /// The worst change in a batch: a config change rebuilds, a source change compiles, an asset only reloads.
    pub fn worst(changes: &[Change]) -> Option<Kind> {
        let mut w = None;
        for c in changes {
            w = Some(match (w, c.kind) {
                (None, k) => k,
                (Some(Kind::Config), _) | (_, Kind::Config) => Kind::Config,
                (Some(Kind::Source), _) | (_, Kind::Source) => Kind::Source,
                _ => Kind::Asset,
            });
        }
        w
    }
}

pub fn classify(p: &Path) -> Kind {
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "rs" => Kind::Source,
        "toml" | "lock" | "wgsl" | "metal" | "glsl" | "spv" => Kind::Config,
        _ => Kind::Asset,
    }
}

fn skip(p: &Path) -> bool {
    p.components().any(|c| {
        matches!(
            c.as_os_str().to_str(),
            Some("target" | ".git" | "node_modules")
        )
    })
}

fn collect(dir: &Path, out: &mut BTreeMap<PathBuf, Stamp>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if skip(&p) {
            continue;
        }
        let Ok(md) = e.metadata() else { continue };
        if md.is_dir() {
            collect(&p, out);
        } else if md.is_file() {
            if let Some(s) = stamp(&p, &md) {
                out.insert(p, s);
            }
        }
    }
}

fn stamp(p: &Path, md: &std::fs::Metadata) -> Option<Stamp> {
    let len = md.len();
    if len > 16 * 1024 * 1024 {
        // a huge file (an image, a database): mtime and length are enough
        return Some(Stamp {
            mtime: md.modified().ok()?,
            len,
            hash: 0,
        });
    }
    let bytes = std::fs::read(p).ok()?;
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ b as u64).wrapping_mul(0x100_0000_01b3)
    });
    Some(Stamp {
        mtime: md.modified().ok()?,
        len,
        hash,
    })
}
