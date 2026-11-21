//! G7: the in-application overlay, and the behaviours ported from the clean-room GPUI inspector's tests
//! (`reactivity/crates/gpui/tests/inspector.rs`) in the typed core's terms. Style editors are not ported: the
//! owner decided against them, and a record's operand is a staged expression of state.
use bough_guibench_x::{rows::Rows, table::Table};
use bough_inspect::record::Command;
use bough_inspect::schema::{Key, Snapshot};
use bough_inspect::session::Session;
use bough_inspect::ui::Inspector;
use bough_inspect::{PropValue, Refusal};
use bough_inspect_app::Lab;
use bough_x::el::App;
use bough_x::event::Key as K;
use bough_x::soft::Soft;
fn rows(n: usize) -> Session<Lab<Rows>> {
    Session::new(Lab::new(Rows::new(n), 800, 600, 1))
}

/// 2. a node keeps its identity and reveals the value its state gave it
#[test]
fn identity_survives_a_state_change_and_shows_the_new_value() {
    let mut s = rows(2_000);
    let before = s.snapshot().clone();
    let row = before
        .nodes
        .iter()
        .find(|n| n.kind == "fill" && n.store == 2 && n.key.row == 3)
        .expect("row 3's fill");
    let key = row.key;
    let colour0 = row.prop("color").unwrap().value.clone();
    s.set("hot", &PropValue::Int(3)).unwrap();
    let after = s.snapshot();
    let same = after.find_key(&key).expect("the same entity is still live");
    assert_eq!(same.key, key);
    let colour1 = same.prop("color").unwrap().value.clone();
    assert_ne!(colour0, colour1, "the hovered row's fill changed colour");
    assert_eq!(same.text, row.text);
}

/// 3. a state write damages its dependents and runs no layout
#[test]
fn a_colour_write_skips_layout_and_touches_only_its_readers() {
    let mut s = rows(2_000);
    let before = s.snapshot().clone();
    let retables = before.counters.retables;
    let quiet: Vec<(Key, Vec<String>)> = before
        .nodes
        .iter()
        .filter(|n| !n.reads.contains(&1))
        .map(|n| (n.key, n.ops.iter().map(|o| o.value.text()).collect()))
        .collect();
    let o = s.set("hot", &PropValue::Int(3)).unwrap();
    assert!(o.damage_tiles > 0);
    let after = s.snapshot();
    assert_eq!(
        after.counters.retables, retables,
        "no store was made or re-tabled"
    );
    assert!(
        after.counters.evaluated_columns > 0,
        "the columns reading the word were differentiated"
    );
    for (key, vals) in quiet {
        if let Some(n) = after.find_key(&key) {
            assert_eq!(
                n.ops.iter().map(|o| o.value.text()).collect::<Vec<_>>(),
                vals,
                "a record that does not read the word kept its values"
            );
        }
    }
}

/// 4. an invalid edit is refused before anything changes
#[test]
fn refusals_leave_the_world_and_the_description_unchanged() {
    let mut s = rows(1_000);
    let before = s.snapshot().clone();
    let rev = before.app.revision;
    let cases: Vec<(Refusal, &str)> = vec![
        (
            s.set("scroll", &PropValue::Text("x".into())).unwrap_err(),
            "wrong-kind",
        ),
        (
            s.set("nope", &PropValue::Int(1)).unwrap_err(),
            "unknown-field",
        ),
        (
            s.set("scroll", &PropValue::Int(-5)).unwrap_err(),
            "rejected",
        ),
    ];
    for (r, code) in cases {
        assert_eq!(r.code(), code);
        assert!(!r.reason().is_empty());
    }
    let after = s.snapshot();
    assert_eq!(after.app.revision, rev);
    assert_eq!(after.digest(), before.digest());
}

/// 5. controls without a selection are inert
#[test]
fn without_a_selection_the_controls_are_inert() {
    let snap = rows(100).snapshot().clone();
    let mut insp = Inspector::new(snap);
    assert!(insp.selected.is_none());
    assert!(!insp.select_parent());
    assert!(insp.ancestors().is_empty());
    assert!(insp.jump_to_selected().is_none());
    assert!(insp.selected.is_none());
}

/// 6. the parent walk stops at the application root
#[test]
fn parent_walk_stops_at_the_root() {
    let snap = rows(2_000).snapshot().clone();
    let mut insp = Inspector::new(snap);
    let row = insp
        .snap
        .nodes
        .iter()
        .position(|n| n.kind == "fill" && n.store == 2)
        .unwrap();
    insp.select(row);
    let mut steps = 0;
    while insp.select_parent() {
        steps += 1;
        assert!(steps < 8, "the walk does not stop");
    }
    assert!(steps >= 1, "the body's list has a scope above it");
    let top = insp
        .selected_index
        .map(|i| insp.snap.nodes[i].store)
        .unwrap();
    let depth = insp.snap.stores[top].depth;
    assert!(depth <= 1, "the walk stopped below the root ({depth})");
}

/// 7. selection of a removed or foreign entity is refused / cleared; leaving the window is not removal
#[test]
fn selection_of_removed_and_foreign_entities() {
    // leaving exposure keeps the selection
    let mut s = rows(2_000);
    let snap = s.snapshot().clone();
    let mut insp = Inspector::new(snap);
    let row = insp
        .snap
        .nodes
        .iter()
        .position(|n| n.kind == "text" && n.key.row == 3 && n.store == 2)
        .unwrap();
    insp.select(row);
    let key = insp.selected.unwrap();
    s.set("scroll", &PropValue::Int(64 * 100)).unwrap();
    insp.set_snapshot(s.snapshot().clone());
    assert_eq!(
        insp.selected,
        Some(key),
        "a row that left the window is still the same row"
    );
    assert!(!insp.stale);

    // a row the list no longer has is gone
    let mut t = Session::new(Lab::new(Table::new(40, None), 800, 600, 1));
    let chosen = t
        .snapshot()
        .nodes
        .iter()
        .filter(|n| n.store == 2 && n.kind == "fill")
        .map(|n| n.key.row)
        .max()
        .unwrap();
    t.set("selected", &PropValue::Int(chosen as i32)).unwrap();
    let snap = t.snapshot().clone();
    let mut ti = Inspector::new(snap);
    let last = ti
        .snap
        .nodes
        .iter()
        .position(|n| n.store == 2 && n.key.row == chosen)
        .unwrap();
    ti.select(last);
    let _key = ti.selected.unwrap();
    // F8 clears the relation: every row is gone, so the selected entity is no longer part of the view
    t.run(&Command::Key {
        key: K::Char('\u{f70b}'),
        mods: Default::default(),
    });
    ti.set_snapshot(t.snapshot().clone());
    assert!(ti.stale, "the selected row was removed from the relation");
    assert!(ti.selected.is_none());

    // a key from another application is not this application's
    let foreign = Key {
        path: [9, 9, 0, 0, 0, 0],
        depth: 2,
        row: 0,
        pos: 0,
        col: 0,
        rec: 0,
    };
    let mut third = Inspector::new(rows(10).snapshot().clone());
    assert!(!third.select_key(foreign));
    assert!(third.stale);
}

/// 8. geometry is the same candidate twice, at one revision, with no follow-up turn
#[test]
fn geometry_is_stable_and_needs_no_followup_frame() {
    let mut s = rows(1_000);
    let a = s.snapshot().clone();
    let node = a.nodes.iter().find(|n| n.store == 2).unwrap();
    let key = node.key;
    let once = node.screen;
    s.set("scroll", &PropValue::Int(64)).unwrap();
    let b = s.snapshot().clone();
    let commands = b.counters.commands;
    let c = s.snapshot().clone();
    assert_eq!(
        c.counters.commands, commands,
        "reading a snapshot runs no command"
    );
    assert_eq!(c.app.revision, b.app.revision);
    let moved = c.find_key(&key);
    if let Some(n) = moved {
        assert_eq!(
            n.screen.y0,
            once.y0 - 64,
            "the row moved exactly with the space"
        );
    }
    // screen = rect + the space's origin
    let st = &c.stores[2];
    for n in c.nodes.iter().filter(|n| n.store == 2) {
        assert_eq!(n.screen.x0, n.rect.x0 + st.origin.0);
        assert_eq!(n.screen.y0, n.rect.y0 + st.origin.1);
    }
}

/// 9. every registered typed field reads, writes and reads back (and read-only ones refuse)
#[test]
fn every_registered_field_is_typed() {
    let mut s = rows(1_000);
    for (field, value) in [("scroll", 128), ("hot", 7)] {
        let o = s.set(field, &PropValue::Int(value)).expect("writable");
        assert!(o.revision > 0);
        let snap = s.snapshot();
        let f = snap.fields.iter().find(|f| f.name == field).unwrap();
        assert_eq!(f.value.text(), value.to_string());
        assert!(f.writable);
        assert_eq!(f.word.is_some(), true);
    }
    // back to where it was
    s.set("scroll", &PropValue::Int(0)).unwrap();
    s.set("hot", &PropValue::Int(0)).unwrap();
    // a read-only field
    let mut t = Session::new(Lab::new(Table::new(100, None), 800, 600, 1));
    let r = t.set("len", &PropValue::Int(5)).unwrap_err();
    assert_eq!(r.code(), "read-only");
}

/// 10. a refused mutation changes nothing; an accepted one is visible in exactly one turn
#[test]
fn admission_is_all_or_nothing() {
    let mut s = rows(1_000);
    let before = s.snapshot().clone();
    let _ = s.set("scroll", &PropValue::Int(-1));
    let _ = s.set("hot", &PropValue::Text("x".into()));
    assert_eq!(s.snapshot().digest(), before.digest());
    let o = s.set("hot", &PropValue::Int(4)).unwrap();
    assert_eq!(o.revision, s.snapshot().app.revision);
    assert_eq!(
        s.snapshot()
            .fields
            .iter()
            .find(|f| f.name == "hot")
            .unwrap()
            .value
            .text(),
        "4"
    );
}

/// 11. rects are reported in the store's space and on screen, at the scale the frame was made at
#[test]
fn geometry_is_reported_in_both_spaces_at_scale() {
    let mut s = Session::new(Lab::new(Rows::new(500), 800, 600, 2));
    let snap = s.snapshot().clone();
    assert_eq!(snap.app.scale, 2.0);
    assert_eq!(snap.lists[0].row_h, 64, "a row is 32 points at scale 2");
    let st = &snap.stores[2];
    for n in snap.nodes.iter().filter(|n| n.store == 2) {
        assert_eq!(n.screen.x0, n.rect.x0 + st.origin.0);
        assert_eq!(n.screen.y0, n.rect.y0 + st.origin.1);
    }
    let first = snap.nodes.iter().find(|n| n.store == 2).unwrap();
    let second = s.snapshot().find_key(&first.key).unwrap();
    assert_eq!(first.screen, second.screen);
}

/// The overlay as a view: it paints, clicking a tree line selects that element, and a source location is left
/// for the host to open.
#[test]
fn the_overlay_paints_selects_and_offers_a_jump() {
    const W: i32 = 1280;
    const H: i32 = 800;
    let mut s = rows(2_000);
    let snap = s.snapshot().clone();
    let insp = Inspector::new(snap);
    let app = App::<Inspector, Soft>::new(insp, W, H, 1.0);
    let mut app = app;
    // it paints: there are records, and the frame is not blank
    let (records, _, _) = app.world.root().stats();
    assert!(records > 30, "{records} overlay records");
    let mut soft = Soft::new(W, H);
    app.world.paint(&mut soft);
    assert!(soft.px.iter().any(|&p| p != 0));
    // click the first tree line: select node 0 (the root fill)
    let (node, rect) = {
        let r = bough_x::reflect::Reflect::learn(&app.world, &app.v);
        let items = r.items(app.world.root(), &app.world.shown);
        if std::env::var("DUMP").is_ok() {
            for i in &items {
                println!("{:?} {:?} {:?}", i.kind(), i.text(), i.screen);
            }
        }
        let n = items
            .iter()
            // reflection holds twelve bytes of a rended text: enough to name the first line
            .find(|i| i.text().as_deref().is_some_and(|t| t.contains("fill")))
            .expect("the first tree line is a label");
        (n.ent, n.screen)
    };
    let _ = node;
    app.world
        .click(&mut app.v, (rect.x0 + rect.x1) / 2, (rect.y0 + rect.y1) / 2);
    app.settle();
    assert!(
        app.v.selected_index.is_some(),
        "clicking a line selects an element"
    );
    // the details pane offers the source location; selecting it leaves a jump for the host
    let idx = app.v.selected_index.unwrap();
    let loc = app.v.snap.nodes[idx].loc;
    app.v.jump = Some(loc);
    assert_eq!(app.v.jump_to_selected(), Some(app.v.snap.nodes[idx].loc));
    // two paints of the same selection are the same frame
    let mut first = Soft::new(W, H);
    app.world.paint(&mut first);
    let mut again = Soft::new(W, H);
    app.world.paint(&mut again);
    assert_eq!(first.px, again.px);
}

/// The overlay keeps the selection when the snapshot is replaced and the entity is still there.
#[test]
fn the_overlay_keeps_selection_across_a_refresh() {
    let mut s = rows(2_000);
    let snap = s.snapshot().clone();
    let mut insp = Inspector::new(snap);
    let row = insp
        .snap
        .nodes
        .iter()
        .position(|n| n.store == 2 && n.kind == "text" && n.key.row == 2)
        .unwrap();
    insp.select(row);
    let key: Key = insp.selected.unwrap();
    s.set("hot", &PropValue::Int(2)).unwrap();
    let before_lines = insp.lines().len();
    insp.set_snapshot(s.snapshot().clone());
    assert_eq!(insp.selected, Some(key));
    assert_eq!(insp.lines().len(), before_lines);
    assert!(insp.details().iter().any(|d| d.label.contains("rows.rs")));
    assert!(insp.status().contains("rows"));
}

/// A `Snapshot` can be cloned and compared: the overlay's input is data, not a borrowed world.
#[test]
fn snapshots_are_values() {
    let mut s = rows(100);
    let a: Snapshot = s.snapshot().clone();
    let b = s.snapshot().clone();
    assert_eq!(a, b);
    assert_eq!(a.json_string(), b.json_string());
}
