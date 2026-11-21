//! G4: a recorded script replays on a fresh application of the same kind, pixel-exact and node-exact, for two
//! real apps. The script mixes every command kind the socket serves.
use bough_guibench_x::{rows::Rows, table::Table};
use bough_inspect::record::{self, Command};
use bough_inspect::session::Session;
use bough_inspect_app::Lab;
use bough_x::event::Key;

fn rows_script() -> Vec<Command> {
    vec![
        Command::Pointer { x: 100, y: 100 },
        Command::Pointer { x: 120, y: 132 },
        Command::Click { x: 120, y: 132 },
        Command::Pointer { x: 50, y: 300 },
        Command::Set { field: "scroll".into(), value: bough_inspect::PropValue::Int(320) },
        Command::Pointer { x: 400, y: 40 },
        Command::Set { field: "hot".into(), value: bough_inspect::PropValue::Int(7) },
        Command::Wheel { x: 400, y: 300, dx: 0.0, dy: 3.0 },
        Command::Tick { ms: 16 },
        Command::Tick { ms: 160 },
        Command::Pointer { x: 700, y: 500 },
        Command::Resize { w: 640, h: 480, scale: 1.0 },
        Command::Pointer { x: 300, y: 200 },
        Command::Click { x: 300, y: 200 },
        Command::Resize { w: 800, h: 600, scale: 1.0 },
        Command::Settle,
    ]
}

#[test]
fn rows_replay_is_pixel_and_node_exact() {
    let mut a = Session::new(Lab::new(Rows::new(500), 800, 600, 1));
    let log = record::record(&mut a, rows_script());
    assert_eq!(log.steps.len(), 16);
    assert!(log.steps.iter().any(|s| s.pixels != 0));
    match record::replay(&log, || Lab::new(Rows::new(500), 800, 600, 1)) {
        record::Replay::Match { steps } => assert_eq!(steps, 16),
        other => panic!("replay diverged: {other:?}"),
    }
}

#[test]
fn table_replay_is_pixel_and_node_exact() {
    let script = vec![
        Command::Pointer { x: 200, y: 120 },
        Command::Click { x: 200, y: 120 },
        Command::Key { key: Key::Down, mods: Default::default() },
        Command::Key { key: Key::Delete, mods: Default::default() },
        Command::Set { field: "selected".into(), value: bough_inspect::PropValue::Int(3) },
        Command::Set { field: "scroll".into(), value: bough_inspect::PropValue::Int(64) },
        Command::Wheel { x: 100, y: 300, dx: 0.0, dy: -2.0 },
        Command::Tick { ms: 33 },
        Command::Pointer { x: 500, y: 400 },
    ];
    let mut a = Session::new(Lab::new(Table::new(300, None), 800, 600, 1));
    let log = record::record(&mut a, script);
    match record::replay(&log, || Lab::new(Table::new(300, None), 800, 600, 1)) {
        record::Replay::Match { steps } => assert_eq!(steps, 9),
        other => panic!("replay diverged: {other:?}"),
    }
}
