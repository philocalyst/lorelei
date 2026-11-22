//! The watcher: a content scan, so a test can drive it without waiting for a platform event; an edit with an
//! old mtime is still found, and the classifier routes each change to a hot-patch action.
use bough_dev::watch::{Kind, classify};
use bough_dev::{Change, Watcher};
use std::path::PathBuf;

fn root(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bough-watch-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_scan_finds_adds_edits_and_removes() {
    let d = root("scan");
    let mut w = Watcher::new([d.clone()]);
    assert!(w.scan().is_empty(), "the first scan is the baseline");
    std::fs::write(d.join("a.rs"), "fn a() {}").unwrap();
    let c = w.scan();
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].kind, Kind::LayoutOrTreeRebuild);
    assert!(w.scan().is_empty(), "nothing changed");
    // the same length and mtime: only content tells
    let p = d.join("a.rs");
    let mtime = std::fs::metadata(&p).unwrap().modified().unwrap();
    std::fs::write(&p, "fn b() {}").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&p)
        .unwrap()
        .set_modified(mtime)
        .unwrap();
    let c = w.scan();
    assert_eq!(
        c.len(),
        1,
        "a content change with an old mtime is found: {c:?}"
    );
    std::fs::write(d.join("Cargo.toml"), "[package]").unwrap();
    std::fs::write(d.join("theme.json"), "{}").unwrap();
    let c = w.scan();
    assert_eq!(c.len(), 2);
    assert_eq!(Watcher::worst(&c), Some(Kind::LayoutOrTreeRebuild));
    std::fs::remove_file(d.join("a.rs")).unwrap();
    let c = w.scan();
    assert!(c.iter().any(|c| c.kind == Kind::LayoutOrTreeRebuild), "{c:?}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn classification_routes_style_fx_asset_and_rebuild() {
    use std::path::Path;
    // structural Rust and build manifests
    assert_eq!(classify(Path::new("view.rs")), Kind::LayoutOrTreeRebuild);
    assert_eq!(classify(Path::new("Cargo.toml")), Kind::LayoutOrTreeRebuild);
    assert_eq!(classify(Path::new("Cargo.lock")), Kind::LayoutOrTreeRebuild);
    // shaders and style/token/theme files are a fast hot patch
    assert_eq!(classify(Path::new("s.wgsl")), Kind::StyleOrFxPatch);
    assert_eq!(classify(Path::new("s.metal")), Kind::StyleOrFxPatch);
    assert_eq!(classify(Path::new("s.glsl")), Kind::StyleOrFxPatch);
    assert_eq!(classify(Path::new("s.spv")), Kind::StyleOrFxPatch);
    assert_eq!(classify(Path::new("theme.json")), Kind::StyleOrFxPatch);
    assert_eq!(classify(Path::new("tokens.yaml")), Kind::StyleOrFxPatch);
    assert_eq!(classify(Path::new("palette.json")), Kind::StyleOrFxPatch);
    assert_eq!(classify(Path::new("styles.css")), Kind::StyleOrFxPatch);
    // fonts, images and SVG are an asset reload
    assert_eq!(classify(Path::new("font.ttf")), Kind::AssetReload);
    assert_eq!(classify(Path::new("font.otf")), Kind::AssetReload);
    assert_eq!(classify(Path::new("logo.png")), Kind::AssetReload);
    assert_eq!(classify(Path::new("icon.svg")), Kind::AssetReload);
    assert_eq!(classify(Path::new("photo.jpg")), Kind::AssetReload);
    // docs, tests, swap and hidden files are ignored
    assert_eq!(classify(Path::new("README.md")), Kind::Ignore);
    assert_eq!(classify(Path::new("notes.txt")), Kind::Ignore);
    assert_eq!(classify(Path::new("file.rs~")), Kind::Ignore);
    assert_eq!(classify(Path::new("file.rs.swp")), Kind::Ignore);
    assert_eq!(classify(Path::new(".DS_Store")), Kind::Ignore);
    assert_eq!(classify(Path::new("tests/foo.rs")), Kind::Ignore);
    assert_eq!(classify(Path::new("docs/guide.md")), Kind::Ignore);
    assert_eq!(classify(Path::new("target/debug/x")), Kind::Ignore);
}

#[test]
fn worst_picks_the_most_expensive_change() {
    use bough_dev::watch::{Kind, Watcher};
    use std::path::PathBuf;
    let changes = |kind: Kind| Change { path: PathBuf::from("x"), kind };
    assert_eq!(Watcher::worst(&[]), None);
    assert_eq!(
        Watcher::worst(&[changes(Kind::Ignore), changes(Kind::AssetReload)]),
        Some(Kind::AssetReload)
    );
    assert_eq!(
        Watcher::worst(&[
            changes(Kind::Ignore),
            changes(Kind::StyleOrFxPatch),
            changes(Kind::AssetReload),
        ]),
        Some(Kind::StyleOrFxPatch)
    );
    assert_eq!(
        Watcher::worst(&[
            changes(Kind::Ignore),
            changes(Kind::AssetReload),
            changes(Kind::LayoutOrTreeRebuild),
        ]),
        Some(Kind::LayoutOrTreeRebuild)
    );
}

#[test]
fn rapid_edits_coalesce_into_one_change() {
    let d = root("debounce");
    let mut w = Watcher::new([d.clone()]);
    assert!(w.scan().is_empty(), "the first scan is the baseline");
    let p = d.join("view.rs");
    for i in 0..5 {
        std::fs::write(&p, format!("fn v{i}() {{}}")).unwrap();
    }
    let c = w.scan();
    assert_eq!(
        c.len(),
        1,
        "five writes in one scan window coalesce into one change: {c:?}"
    );
    assert_eq!(c[0].kind, Kind::LayoutOrTreeRebuild);
    let _ = std::fs::remove_dir_all(&d);
}
