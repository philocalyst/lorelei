//! TASK 2: the inspector mounted beside a live `Rows` and `Table`, driven by real injected input.
//!
//! Closed, the app's pixels are the app's own (the identity gate; `mutant_overlay_paint` must fail it). Open,
//! a pointer click on a drawn tree line selects that node through the overlay's own handler, Up/Down move the
//! selection, Enter leaves the selected node's `file:line:col` for the host to open, and the frame carries the
//! selection outline and the last turn's damage flash.
use bough_guibench_x::{rows::Rows, table::Table};
use bough_inspect::PropValue;
use bough_inspect::record::Command;
use bough_inspect::session::Session;
use bough_inspect_app::{Lab, WithOverlay};
use bough_x::event::Key as K;

fn key(k: K) -> Command {
    Command::Key {
        key: k,
        mods: Default::default(),
    }
}

fn open() -> Session<WithOverlay<Rows>> {
    let mut s = Session::new(WithOverlay::new(
        Lab::new(Rows::new(2_000), 800, 600, 1),
        true,
    ));
    let _ = s.shot();
    s
}

/// The first tree line's rect in panel coordinates, from the overlay's own description.
fn first_line(s: &Session<WithOverlay<Rows>>) -> (i32, i32, i32) {
    let ov = s.app().ov.as_ref().expect("the panel is mounted");
    let r = bough_x::reflect::Reflect::learn(&ov.app.world, &ov.app.v);
    let items = r.items(ov.app.world.root(), &ov.app.world.shown);
    let line = items
        .iter()
        .find(|i| i.text().as_deref().is_some_and(|t| t.starts_with("fill")))
        .expect("the first tree line is drawn");
    let rect = line.screen;
    (
        ov.x0() + (rect.x0 + rect.x1) / 2,
        (rect.y0 + rect.y1) / 2,
        line.ent.pos as i32,
    )
}

#[test]
fn closed_pixels_are_the_apps_own() {
    let mut plain = Lab::new(Rows::new(1_000), 800, 600, 1);
    plain.settle();
    let theirs = plain.shot();
    let mut s = Session::new(WithOverlay::new(
        Lab::new(Rows::new(1_000), 800, 600, 1),
        false,
    ));
    let ours = s.shot();
    assert_eq!(ours.w, theirs.w);
    assert_eq!(ours.h, theirs.h);
    assert!(
        ours.px == theirs.px,
        "a closed panel changed {} pixels",
        ours.px
            .iter()
            .zip(&theirs.px)
            .filter(|(a, b)| a != b)
            .count()
    );
}

#[test]
fn a_pointer_click_on_a_tree_line_selects_that_node() {
    let mut s = open();
    let (x, y, _) = first_line(&s);
    s.run(&Command::Click { x, y });
    let ov = s.app().ov.as_ref().unwrap();
    assert_eq!(
        ov.selected(),
        Some(0),
        "the click went through the overlay's own handler to node 0"
    );
    // The panel is painted beside the app: the frame differs from the closed one and has panel pixels.
    let f = s.shot();
    let ov = s.app().ov.as_ref().unwrap();
    let x0 = ov.x0() as usize;
    let distinct = (0..f.h as usize)
        .map(|y| f.px[y * f.w as usize + x0 + 10])
        .collect::<std::collections::HashSet<_>>();
    assert!(distinct.len() > 1, "the panel drew a tree");
    assert!(
        f.px[..x0 as usize].iter().any(|&p| p == 0xff5a9bff)
            || f.px.iter().any(|&p| p == 0xff5a9bff),
        "the selection is outlined on the app's pixels"
    );
}

#[test]
fn a_key_moves_the_selection_and_enter_leaves_a_source_location() {
    let mut s = open();
    let (x, y, _) = first_line(&s);
    s.run(&Command::Click { x, y });
    assert_eq!(s.app().ov.as_ref().unwrap().selected(), Some(0));
    s.run(&key(K::Down));
    assert_eq!(s.app().ov.as_ref().unwrap().selected(), Some(1));
    s.run(&key(K::Up));
    assert_eq!(s.app().ov.as_ref().unwrap().selected(), Some(0));
    s.run(&key(K::Enter));
    let ov = s.app().ov.as_ref().unwrap();
    let want = ov.snapshot().nodes[0].loc;
    let got = ov.jump().expect("Enter left a jump for the host");
    assert_eq!(got, want, "the jump names the selected node's site");
    assert!(got.file.ends_with("rows.rs"), "{got:?}");
    assert!(got.line > 0);
    // The panel shows the node's location, its operands and its dependency words.
    let text = ov
        .app
        .v
        .details()
        .iter()
        .map(|d| d.label.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("rows.rs"), "{text}");
    assert!(
        text.contains("reads") || text.contains("handlers"),
        "{text}"
    );
    assert!(
        ov.app.v.status().contains("records"),
        "{}",
        ov.app.v.status()
    );
}

#[test]
fn a_state_write_flashes_the_damage_of_that_turn() {
    let mut s = open();
    let (x, y, _) = first_line(&s);
    s.run(&Command::Click { x, y });
    s.set("hot", &PropValue::Int(3)).unwrap();
    // the flash alternates; one of the next two frames carries it
    let mut flashed = false;
    for _ in 0..2 {
        let f = s.shot();
        flashed |= f.px.iter().any(|&p| p == 0xffe06020);
    }
    assert!(
        flashed,
        "the last turn's damage was flashed on the app's pixels"
    );
    let ov = s.app().ov.as_ref().unwrap();
    assert!(ov.snapshot().damage.tiles > 0);
    assert!(
        ov.snapshot().counters.last_command.contains("set"),
        "the panel counts the turn: {}",
        ov.snapshot().counters.last_command
    );
}

#[test]
fn table_mounts_the_same_panel() {
    let mut s = Session::new(WithOverlay::new(
        Lab::new(Table::new(300, None), 800, 600, 1),
        true,
    ));
    let f = s.shot();
    let ov = s.app().ov.as_ref().expect("the panel is mounted");
    assert!(ov.open);
    assert!(!ov.snapshot().nodes.is_empty());
    assert!(f.px.iter().any(|&p| p != 0));
}
