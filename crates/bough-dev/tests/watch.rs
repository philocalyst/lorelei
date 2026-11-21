//! The watcher: a content scan, so a test can drive it without waiting for a platform event, and an edit with an
//! old mtime is still found.
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
    assert_eq!(c[0].kind, Kind::Source);
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
    assert_eq!(Watcher::worst(&c), Some(Kind::Config));
    std::fs::remove_file(d.join("a.rs")).unwrap();
    let c = w.scan();
    assert!(c.iter().any(|c| c.kind == Kind::Source), "{c:?}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn classification_is_by_extension() {
    assert_eq!(classify(std::path::Path::new("a.rs")), Kind::Source);
    assert_eq!(classify(std::path::Path::new("Cargo.toml")), Kind::Config);
    assert_eq!(classify(std::path::Path::new("Cargo.lock")), Kind::Config);
    assert_eq!(classify(std::path::Path::new("s.wgsl")), Kind::Config);
    assert_eq!(classify(std::path::Path::new("theme.json")), Kind::Asset);
    assert_eq!(classify(std::path::Path::new("logo.png")), Kind::Asset);
    assert_eq!(
        Watcher::worst(&[
            Change {
                path: "a".into(),
                kind: Kind::Asset
            },
            Change {
                path: "b".into(),
                kind: Kind::Source
            }
        ]),
        Some(Kind::Source)
    );
}
