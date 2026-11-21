//! The inspector over the real comparison apps: smoke tests first, then the gates. Every test mounts a
//! `bough-guibench-x` scenario through `bough-inspect-app`'s host; nothing here writes a second scene.
use bough_guibench_x::rows::Rows;
use bough_inspect::session::Session;
use bough_inspect_app::Lab;

fn rows(n: usize) -> Session<Lab<Rows>> {
    let lab = Lab::new(Rows::new(n), 800, 600, 1);
    Session::new(lab)
}

#[test]
fn a_snapshot_names_the_tree() {
    let mut s = rows(1_000);
    let snap = s.snapshot();
    assert!(!snap.nodes.is_empty(), "records exist");
    assert!(!snap.stores.is_empty(), "stores exist");
    assert!(!snap.lists.is_empty(), "the rows are a list");
    for n in &snap.nodes {
        assert!(!n.kind.is_empty());
        assert!(n.loc.file.ends_with(".rs"), "{}", n.loc.short());
        assert!(n.loc.line > 0);
    }
    assert!(
        snap.ax.is_some(),
        "the accessibility projection is published"
    );
    println!(
        "rows=1000 nodes={} stores={} lists={} ax={} records={}",
        snap.nodes.len(),
        snap.stores.len(),
        snap.lists.len(),
        snap.ax.as_ref().unwrap().nodes.len(),
        snap.counters.records
    );
    if std::env::var("DUMP").is_ok() {
        println!("{}", snap.json_string());
    }
}

#[test]
fn a_pointer_hovers_and_the_pick_names_the_record() {
    let mut s = rows(1_000);
    let before = s.snapshot().damage.tiles;
    assert!(before > 0, "the mount damaged the window ({before})");
    let o = s.run(&bough_inspect::record::Command::Pointer {
        x: 100,
        y: 5 * 32 + 10,
    });
    assert_eq!(o.damage_tiles > 0, true, "a hover damages two rows");
    let snap = s.snapshot();
    let pick = snap.pointers.pick.expect("something is under the pointer");
    let n = snap.find_key(&pick).expect("the pick is a node");
    assert!(
        !n.handlers.is_empty(),
        "the picked record hears the pointer: {:?}",
        n.handlers
    );
    let hot = snap.fields.iter().find(|f| f.name == "hot").unwrap();
    assert_eq!(hot.value.text(), "5", "the hover word followed the pointer");
    let hit = s.stack(100, 5 * 32 + 10);
    assert!(!hit.is_empty());
}

#[test]
fn a_typed_field_write_damages_exactly_its_readers() {
    let mut s = rows(1_000);
    // the scroll word is read by a space, not by a column: scrolling moves the whole store
    assert!(
        s.snapshot().stores.iter().any(|st| st.scroll == Some(0)),
        "the body's space reads the scroll word"
    );
    let o = s
        .set("scroll", &bough_inspect::PropValue::Int(64))
        .expect("scroll is writable");
    assert!(o.damage_tiles > 0, "a scroll changes the window");
    let after = s.snapshot();
    assert_eq!(
        after
            .fields
            .iter()
            .find(|f| f.name == "scroll")
            .unwrap()
            .value
            .text(),
        "64"
    );
    assert_eq!(after.lists[0].scroll, Some(0));
    // the refusal probes
    assert_eq!(
        s.set("scroll", &bough_inspect::PropValue::Text("x".into()))
            .err()
            .unwrap()
            .code(),
        "wrong-kind"
    );
    assert_eq!(
        s.set("nope", &bough_inspect::PropValue::Int(1))
            .err()
            .unwrap()
            .code(),
        "unknown-field"
    );
    assert_eq!(
        s.set("scroll", &bough_inspect::PropValue::Int(-1))
            .err()
            .unwrap()
            .code(),
        "rejected"
    );
}

#[test]
fn rows_source_points_at_the_real_description_call_site() {
    const SOURCE: &str = include_str!("../../../comparisons/bough-guibench-x/src/rows.rs");
    let mut session = rows(100);
    let snapshot = session.snapshot();
    let label = snapshot
        .nodes
        .iter()
        .find(|n| n.text.as_deref() == Some("Row 00002"))
        .unwrap();
    assert!(
        label
            .loc
            .file
            .ends_with("comparisons/bough-guibench-x/src/rows.rs")
    );
    let source = SOURCE.lines().nth(label.loc.line as usize - 1).unwrap();
    assert!(
        source.trim_start().starts_with("s.list(&List"),
        "{} pointed at {source:?}",
        label.loc.short()
    );
    // A row describes several leaves. The description location currently names
    // the call they share, so agent navigation should land on that call.
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|n| n.kind == "fill" && n.key.row == label.key.row && n.loc == label.loc)
    );
}
