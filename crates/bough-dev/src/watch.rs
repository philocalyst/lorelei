//! A polling source watcher: no platform file-event dependency, and a missed notification cannot lose an edit
//! (a scan compares content, not only mtime). The gpui-ce CLI's watcher (upstream `watch.rs`) is the model; this
//! one trades its `notify` backend for a deterministic scan so a test can drive it without sleeping on an event.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// Docs, tests, target directories and swap files: nothing to do.
    Ignore,
    /// A font, image or SVG: invalidate the running application's asset cache (no rebuild, no restart).
    AssetReload,
    /// Colours, shadows, gradients, tokens, theme or shaders: a fast in-process hot patch.
    StyleOrFxPatch,
    /// Structural Rust view/component changes: an incremental rebuild and a state-preserving restart.
    LayoutOrTreeRebuild,
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
    /// The most expensive change in a batch (highest priority), ignoring nothing-to-do edits.
    pub fn worst(changes: &[Change]) -> Option<Kind> {
        changes
            .iter()
            .map(|c| c.kind)
            .filter(|k| *k != Kind::Ignore)
            .max()
    }
}

/// Classify one changed path into a hot-patch action. The decision is by extension and, for style/token
/// files, by the file's name; directories named `tests` or `docs` are never a rebuild.
pub fn classify(p: &Path) -> Kind {
    if p.components().any(|c| {
        matches!(
            c.as_os_str().to_str(),
            Some("tests" | "test" | "docs" | "doc" | "target" | "fixtures")
        )
    }) {
        return Kind::Ignore;
    }
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.ends_with('~')
        || name.ends_with(".swp")
        || name.ends_with(".swo")
        || (name.starts_with('#') && name.ends_with('#'))
        || name.starts_with('.')
    {
        return Kind::Ignore;
    }
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "md" | "rst" | "txt" | "adoc" | "markdown" => Kind::Ignore,
        "swp" | "swo" | "tmp" | "bak" | "orig" | "rej" | "log" => Kind::Ignore,
        "rs" => Kind::LayoutOrTreeRebuild,
        "toml" | "lock" => Kind::LayoutOrTreeRebuild,
        "wgsl" | "metal" | "glsl" | "spv" | "vert" | "frag" | "comp" => Kind::StyleOrFxPatch,
        "css" | "scss" | "less" | "sass" => Kind::StyleOrFxPatch,
        "json" | "yaml" | "yml" => {
            let lower = name.to_ascii_lowercase();
            if lower.contains("theme")
                || lower.contains("token")
                || lower.contains("palette")
                || lower.contains("color")
                || lower.contains("style")
            {
                Kind::StyleOrFxPatch
            } else {
                Kind::AssetReload
            }
        }
        "ttf" | "otf" | "woff" | "woff2" | "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico"
        | "svg" => Kind::AssetReload,
        _ => Kind::AssetReload,
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
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
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
