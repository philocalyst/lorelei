//! The two GPUI-rework inspector tests with no direct port, in lorelei's terms. The mapping is named in each
//! test's doc comment; what does not translate is stated, not faked.
use bough_guibench_dataflow::rows::Rows;
use bough_inspect::PropValue;
use bough_inspect::record::Command;
use bough_inspect::schema::Snapshot;
use bough_inspect::session::Session;
use bough_inspect::ui::Inspector;
use bough_inspect_app::Lab;

fn rows(n: usize) -> Session<Lab<Rows>> {
    Session::new(Lab::new(Rows::new(n), 800, 600, 1))
}

/// Port of `reactivity/crates/gpui/tests/multi_slot_inspector.rs`
/// (`one_source_two_slot_transfer_cannot_publish_ahead_of_inspector_caption`).
///
/// GPUI: one `Source` (a hover word) feeds a left and a right box; changing it in "exact" mode (through the
/// inspector's source admission) and in "coarse" mode (the model's own update) must produce the same
/// description and pixels, the inspector's caption must not be relabelled by a candidate the presentation has
/// not acknowledged, and the two slots must both show the new value.
///
/// Lorelei: one block word (`hot`) feeds every row's fill (two of them are checked here); "exact" is
/// `Session::set` through typed admission, "coarse" is the app's own input path (a pointer entering row 3),
/// and the "caption" is the inspector snapshot: a snapshot taken before the write keeps the old value, and the
/// next snapshot (and the inspector built from it) shows the new colours in both slots. There is no
/// presentation token or failed-candidate state in the typed core, so the "never relabel an older failed
/// candidate" half has no direct analogue; the equivalent assertion is that an older `Snapshot` value is a
/// value and cannot be rewritten by a later turn (checked below).
#[test]
fn one_source_two_slots_and_the_inspector_caption() {
    let mut exact = rows(2_000);
    let mut coarse = rows(2_000);
    // Start with the other slot hot so one write must update both slots.
    exact.set("hot", &PropValue::Int(7)).unwrap();
    coarse.set("hot", &PropValue::Int(7)).unwrap();
    let old_snapshot: Snapshot = exact.snapshot().clone();
    let left_key = old_snapshot
        .nodes
        .iter()
        .find(|n| n.store == 2 && n.kind == "fill" && n.key.row == 3)
        .expect("row 3's fill")
        .key;
    let right_key = old_snapshot
        .nodes
        .iter()
        .find(|n| n.store == 2 && n.kind == "fill" && n.key.row == 7)
        .expect("row 7's fill")
        .key;
    let colour = |s: &mut Session<Lab<Rows>>, k: bough_inspect::schema::Key| {
        s.snapshot()
            .find_key(&k)
            .and_then(|n| n.prop("color"))
            .map(|p| p.value.clone())
    };
    let before_left = colour(&mut exact, left_key).unwrap();
    let before_right = colour(&mut exact, right_key).unwrap();
    assert_ne!(before_right, before_left);
    let before_commands = old_snapshot.counters.commands;
    let before_retables = old_snapshot.counters.retables;

    // exact: the inspector's typed admission
    exact.set("hot", &PropValue::Int(3)).unwrap();
    // coarse: the app's own hover path, no inspection admission in the write
    coarse.run(&Command::Pointer {
        x: 100,
        y: 3 * 32 + 10,
    });
    assert_eq!(
        coarse
            .snapshot()
            .fields
            .iter()
            .find(|f| f.name == "hot")
            .unwrap()
            .value
            .text(),
        "3"
    );

    // the older snapshot was a value: the write did not relabel it (no pre-ACK relabelling)
    assert_eq!(
        old_snapshot
            .find_key(&left_key)
            .and_then(|n| n.prop("color"))
            .map(|p| p.value.clone()),
        Some(before_left.clone())
    );

    let a = exact.snapshot().clone();
    let b = coarse.snapshot().clone();
    let new_left = colour(&mut exact, left_key).unwrap();
    assert_eq!(new_left, before_right, "the first slot becomes hot");
    assert_eq!(
        colour(&mut exact, right_key).unwrap(),
        before_left,
        "the old hot slot clears"
    );
    assert_eq!(a.counters.commands, before_commands + 1);
    assert_eq!(a.counters.retables, before_retables);
    assert!(a.damage.tiles > 0);
    // the two write paths describe the same world and paint the same pixels. (The whole description also
    // carries the pointer's own words, which only the coarse path had: the records are the comparison here,
    // the analogue of GPUI zeroing every node's `target`.)
    let records = |snap: &Snapshot| {
        let mut v: Vec<(u32, u32, Vec<String>)> = snap
            .nodes
            .iter()
            .filter(|n| n.store == 2)
            .map(|n| {
                (
                    n.key.row,
                    n.key.pos,
                    n.ops.iter().map(|o| o.value.text()).collect(),
                )
            })
            .collect();
        v.sort();
        v
    };
    assert_eq!(records(&a), records(&b), "exact and coarse records agree");
    assert_eq!(
        a.counters.retables, b.counters.retables,
        "no store was made by either path"
    );
    let (fa, fb) = (exact.shot(), coarse.shot());
    assert!(fa.px == fb.px, "exact and coarse pixels agree");

    let mut insp = Inspector::new(old_snapshot);
    let left = insp
        .snap
        .nodes
        .iter()
        .position(|n| n.key == left_key)
        .unwrap();
    insp.select(left);
    assert!(insp.details().iter().any(|d| {
        d.label
            .starts_with(&format!("color = {}", before_left.text()))
    }));
    insp.set_snapshot(a);
    assert!(
        insp.details()
            .iter()
            .any(|d| d.label.starts_with(&format!("color = {}", new_left.text())))
    );
    assert!(!insp.details().iter().any(|d| {
        d.label
            .starts_with(&format!("color = {}", before_left.text()))
    }));
}

/// Port of `reactivity/crates/gpui/tests/semantic_metadata.rs`, the cases that have an analogue:
/// `direct_name_and_description_share_one_read_and_promotion_without_visual_work` and
/// `metadata_extends_pending_complete_sheet_but_never_relabels_an_older_failed_candidate`.
///
/// GPUI publishes semantic name/description/value from the same source read and separates that publication
/// from presentation tokens: a repeated identical edit promotes nothing, a refused or failed visual candidate
/// never relabels the older published text, and the published node's identity is stable.
///
/// Lorelei publishes accessibility from the reflection itself (`bough-ax`), and a refused typed write changes
/// neither the world nor the snapshot nor the projection. The analogue is asserted here: a cell's published
/// name and the node's identity survive an unrelated visual change and a refused write; a repeated identical
/// write runs no command; and an accepted write publishes exactly one new value. Lorelei has no presentation
/// epochs or failed-render candidates, so the "deferred label across a failed canvas" half is not translatable
/// and is not claimed.
#[test]
fn semantic_publication_is_not_relabelled_by_an_unrelated_turn() {
    let mut s = rows(1_000);
    let cell_name = "Row 00003";
    let find_ax = |snap: &Snapshot| {
        snap.ax
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find(|n| n.name == cell_name)
            .cloned()
            .expect("the cell is published")
    };
    let first = s.snapshot().clone();
    let published = find_ax(&first);
    let node_key = first
        .nodes
        .iter()
        .find(|n| n.text.as_deref() == Some(cell_name))
        .expect("the cell has a record")
        .key;
    let rev = first.app.revision;

    // A refused write: nothing changes, including the publication and its identity.
    let e = s.set("scroll", &PropValue::Text("x".into())).unwrap_err();
    assert_eq!(e.code(), "wrong-kind");
    let after_refusal = s.snapshot().clone();
    assert_eq!(after_refusal.app.revision, rev);
    assert_eq!(find_ax(&after_refusal).name, published.name);
    assert_eq!(find_ax(&after_refusal).bounds, published.bounds);

    // An unrelated visual change: the row's fill moves, the semantic name and the cell's identity do not.
    let fill_key = first
        .nodes
        .iter()
        .find(|n| n.store == 2 && n.kind == "fill" && n.key.row == 3)
        .unwrap()
        .key;
    let fill_before = first
        .find_key(&fill_key)
        .unwrap()
        .prop("color")
        .unwrap()
        .value
        .clone();
    s.set("hot", &PropValue::Int(3)).unwrap();
    let after_visual = s.snapshot();
    assert_ne!(
        after_visual
            .find_key(&fill_key)
            .unwrap()
            .prop("color")
            .unwrap()
            .value,
        fill_before,
        "the visual change happened"
    );
    assert_eq!(find_ax(after_visual).name, published.name);
    assert_eq!(
        after_visual
            .find_key(&node_key)
            .expect("the cell is the same entity")
            .id,
        first.find_key(&node_key).unwrap().id
    );
    assert_eq!(
        after_visual
            .find_key(&node_key)
            .unwrap()
            .prop("text")
            .unwrap()
            .value,
        bough_inspect::schema::PropValue::Text(cell_name.into())
    );

    // A repeated identical write is a no-op turn: no command and no new snapshot.
    let commands = after_visual.counters.commands;
    let revision = after_visual.app.revision;
    assert_eq!(
        s.set("hot", &PropValue::Int(3)).unwrap_err().code(),
        "rejected"
    );
    let same = s.snapshot();
    assert_eq!(same.counters.commands, commands);
    assert_eq!(same.app.revision, revision);
    assert_eq!(find_ax(same).name, published.name);
}

/// The exposed list projection shares the current count and parent. Retained snapshots keep
/// their own exposure information after a scroll. Explicit published candidates need host ACKs.
#[test]
fn exposed_ax_rows_keep_list_count_and_parent_after_scroll() {
    let mut s = rows(1000);
    let old = s.snapshot().clone();
    let check = |snapshot: &Snapshot| {
        let ax = &snapshot.ax.as_ref().unwrap().nodes;
        let rows: Vec<_> = ax
            .iter()
            .filter_map(|node| {
                let index = node.row_index?;
                let parent = node.parent?;
                if !matches!(node.role, "list-item" | "row") {
                    return None;
                }
                assert_eq!(ax[parent].row_count, Some(1000));
                assert!(index < 1000);
                assert!(matches!(ax[parent].role, "list" | "grid"));
                Some(index)
            })
            .collect();
        assert!(!rows.is_empty());
        rows
    };
    let before = check(&old);
    s.set("scroll", &PropValue::Int(64 * 32)).unwrap();
    let after = check(s.snapshot());
    assert!(after.iter().min() > before.iter().min());
    assert_eq!(check(&old), before);
}
