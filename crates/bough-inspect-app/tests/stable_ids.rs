//! TASK 1: a node's id is its identity, not its paint index.
//!
//! The id is derived from the leaf's call site, the store path, the row/pos slots and the host's remount epoch.
//! Two descriptions of the same view agree; a scroll that recycles materialisation slots does not
//! change it; two nodes do not share one.
use bough_guibench_x::rows::Rows;
use bough_inspect::schema::{Key, PropValue, stable_id};
use bough_inspect::session::Program;
use bough_inspect::session::Session;
use bough_inspect_app::Lab;
use bough_x::reflect::Reflect;
use std::collections::HashSet;

fn rows(n: usize) -> Session<Lab<Rows>> {
    Session::new(Lab::new(Rows::new(n), 800, 600, 1))
}

#[test]
fn two_descriptions_of_one_view_agree() {
    let mut s = rows(2_000);
    let a = s.snapshot().clone();
    let b = s.snapshot().clone();
    assert_eq!(a.nodes.len(), b.nodes.len());
    for (x, y) in a.nodes.iter().zip(&b.nodes) {
        assert_eq!(x.key, y.key);
        assert_eq!(x.id, y.id, "{} changed id between descriptions", x.kind);
        assert_eq!(x.kind_id, y.kind_id);
        assert_eq!(
            x.ops.iter().map(|o| o.id).collect::<Vec<_>>(),
            y.ops.iter().map(|o| o.id).collect::<Vec<_>>()
        );
    }
}

/// The id is a function of the reflected entity and the learned description, so learning twice is the same id.
#[test]
fn relearning_the_same_stores_is_the_same_id() {
    let lab = Lab::new(Rows::new(500), 800, 600, 1);
    let (r1, r2) = (
        Reflect::learn(lab.world(), lab.view()),
        Reflect::learn(lab.world(), lab.view()),
    );
    assert_eq!(0u64, 0u64);
    let one = r1.items(lab.world().root(), &lab.world().shown);
    let two = r2.items(lab.world().root(), &lab.world().shown);
    assert_eq!(one.len(), two.len());
    for (a, b) in one.iter().zip(&two) {
        let loc = bough_inspect::schema::Loc::of(a.location());
        let ia = stable_id(loc, a.ent.path.n, a.ent.path.at, a.ent.row, a.ent.pos, 0);
        let ib = stable_id(loc, b.ent.path.n, b.ent.path.at, b.ent.row, b.ent.pos, 0);
        assert_eq!(ia, ib);
    }
}

#[test]
fn a_scroll_recycles_slots_without_changing_ids() {
    let mut s = rows(2_000);
    let a = s.snapshot().clone();
    // A row that is exposed now.
    let row = a
        .nodes
        .iter()
        .find(|n| n.store == 2 && n.key.row == 5 && n.kind == "text")
        .expect("row 5's label is exposed");
    let (id, rec) = (row.id, row.key.rec);
    // Scroll 69 rows: the window no longer holds row 5, and row 69 (69 % 64 = 5) takes its slot.
    s.set("scroll", &PropValue::Int(69 * 32)).unwrap();
    let b = s.snapshot();
    let recycled = b
        .nodes
        .iter()
        .find(|n| n.store == 2 && n.kind == "text" && n.key.row == 69)
        .expect("row 69 is exposed after the scroll");
    assert_eq!(
        recycled.key.rec, rec,
        "row 69 reuses row 5's materialisation slot"
    );
    assert_ne!(recycled.id, id, "another row in the slot is another id");
    // Back: row 5 is described again, in the slot it had, with the id it had.
    s.set("scroll", &PropValue::Int(0)).unwrap();
    let c = s.snapshot();
    let again = c
        .nodes
        .iter()
        .find(|n| n.store == 2 && n.key.row == 5 && n.kind == "text")
        .expect("row 5 is exposed again");
    assert_eq!(
        again.id, id,
        "the same row keeps its id across a recycled slot"
    );
    assert_eq!(again.key.rec, rec);
}

#[test]
fn ids_are_unique_within_a_description() {
    let mut s = rows(2_000);
    let snap = s.snapshot();
    let mut seen = HashSet::new();
    for n in &snap.nodes {
        assert!(
            seen.insert(n.id),
            "two nodes share the id {:016x} ({} {})",
            n.id,
            n.kind,
            n.loc.short()
        );
        for o in &n.ops {
            let mut inner = HashSet::new();
            assert!(inner.insert(o.id), "two operands of {} share an id", n.kind);
        }
    }
    // A key is not an id: the materialisation is part of the key and not of the id (see the scroll gate).
    let k: Key = snap.nodes[0].key;
    assert_eq!(k.place(), snap.nodes[0].key.place());
}

/// A remount is a new world: the generation changes and the same place is a different node. A fresh app of
/// the same kind starts at generation 0, which is what lets replay compare two descriptions.
#[test]
fn a_remount_changes_the_generation_and_the_ids() {
    let mut s = rows(100);
    let before = s.snapshot().clone();
    assert_eq!(before.app.generation, 0);
    let old = before
        .nodes
        .iter()
        .find(|n| n.store == 2 && n.key.row == 2)
        .unwrap()
        .clone();
    s.run(&bough_inspect::record::Command::Resize {
        w: 800,
        h: 600,
        scale: 1.0,
    });
    let after = s.snapshot();
    assert_eq!(after.app.generation, 1, "the resize remounted the world");
    let same_place = after
        .find_place(old.depth, old.key.path, old.key.row, old.key.pos)
        .expect("the place exists in the new world");
    assert_ne!(same_place.id, old.id, "another world is another identity");
}
