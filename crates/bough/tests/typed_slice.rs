//! Integration Card 4: the counter and the vertical slice on the typed path (`bough::typed`), under the four
//! oracles at ring ages 1..=4, with the count bands: what a hover, a wheel and a handler's turn describe.
#[path = "../examples/counter.rs"]
mod counter;
#[path = "../examples/slice.rs"]
mod slice;

use bough::input::{Button, Key, Mods};
use bough::lower::provenance;
use bough::raw::Config;
use bough::signal::Signal;
use bough::typed::App;
use slice::{ROW_H, Slice};

fn cfg(w: f32, h: f32, scale: f32, ring: usize) -> Config {
    let mut c = Config::new(w, h, scale, bough::ui::default_font());
    c.ring = ring;
    c
}
fn admitted<V: bough::builders::View>(v: V, c: Config) -> App<V> {
    match App::new(v, c) {
        Ok(a) => a,
        Err((_, why)) => panic!("not admitted to the typed path: {why:?}"),
    }
}
fn click<V: bough::builders::View>(app: &mut App<V>, x: f32, y: f32) {
    app.pointer_down(x, y, Button::Primary);
    app.pointer_up(x, y, Button::Primary);
}

#[test]
fn the_counter_counts_on_the_typed_path() {
    for ring in 1..=4 {
        let mut app = admitted(
            counter::Counter {
                count: Signal::new(0),
            },
            cfg(480.0, 320.0, 2.0, ring),
        );
        app.check()
            .unwrap_or_else(|e| panic!("ring {ring} mount: {e}"));
        let d0 = app.describes();
        // hover the button and leave it: a block word, no description
        app.pointer_move(107.0, 40.0);
        app.check()
            .unwrap_or_else(|e| panic!("ring {ring} hover: {e}"));
        let hovered = app.presented_pixels();
        app.pointer_move(400.0, 300.0);
        app.check()
            .unwrap_or_else(|e| panic!("ring {ring} leave: {e}"));
        assert_ne!(
            hovered,
            app.presented_pixels(),
            "the hover paint shows and goes"
        );
        assert_eq!(app.describes(), d0, "hover describes nothing");
        for k in 0..3 {
            click(&mut app, 107.0, 40.0);
            app.check()
                .unwrap_or_else(|e| panic!("ring {ring} click {k}: {e}"));
        }
        assert_eq!(*app.view().count, 3);
        assert_eq!(
            app.describes(),
            d0 + 3,
            "a handler's turn is one typed description"
        );
        if let Ok(dir) = std::env::var("BOUGH_LOWERED_PNG") {
            app.write_png(
                &std::path::Path::new(&dir).join(format!("typed-counter-ring{ring}.png")),
            )
            .unwrap();
        }
    }
    let p = provenance::counts();
    assert_eq!(
        (p.legacy(), p.describe),
        (0, 0),
        "no legacy pass and no `lower` description"
    );
}

const BUTTONS: [(f32, f32); 5] = [
    (257.0, 24.0),
    (331.0, 24.0),
    (419.0, 24.0),
    (507.0, 24.0),
    (592.0, 24.0),
];

#[test]
fn the_slice_holds_the_four_oracles_on_the_typed_path() {
    for ring in 1..=4 {
        let mut app = admitted(
            Slice::new(300),
            cfg(640.0, 480.0, if ring % 2 == 0 { 2.0 } else { 1.0 }, ring),
        );
        app.check()
            .unwrap_or_else(|e| panic!("ring {ring} mount: {e}"));
        let mut x = 0x2545_f491_4f6c_dd1du64 ^ ring as u64;
        let mut r = move |m: u64| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x % m
        };
        for step in 0..80 {
            let what = r(11);
            match what {
                0..=4 => click(&mut app, BUTTONS[what as usize].0, BUTTONS[what as usize].1),
                5 | 6 => app.wheel(320.0, 300.0, 0.0, r(7) as f32 - 3.0),
                7 | 8 => app.pointer_move(r(640) as f32, r(480) as f32),
                9 => {
                    click(&mut app, 320.0, 300.0);
                    app.key_down(
                        [Key::Down, Key::Up, Key::PageDown, Key::PageUp][r(4) as usize],
                        Mods::default(),
                    );
                }
                _ => app.turn(|s| {
                    let t = format!("Slice {}", s.items.len());
                    s.title.set(t);
                }),
            }
            app.check()
                .unwrap_or_else(|e| panic!("ring {ring} step {step} (op {what}): {e}"));
        }
    }
    let p = provenance::counts();
    assert_eq!((p.legacy(), p.describe), (0, 0));
}

#[test]
fn count_bands_on_the_typed_path() {
    let mut app = admitted(Slice::new(100_000), cfg(640.0, 480.0, 1.0, 1));
    app.check().unwrap();
    assert!(
        app.world_stats().0 <= 400 + 64,
        "{} record slots for 100,000 rows",
        app.world_stats().0
    );
    let d0 = app.describes();
    // hover across rows: nothing is described
    for k in 0..6 {
        app.pointer_move(320.0, 200.0 + k as f32 * ROW_H);
    }
    assert_eq!(app.describes(), d0, "hover over rows describes nothing");
    app.check().unwrap();
    // the wheel: the space moves by its word, the offset goes back to the signal, nothing fixed is described
    app.wheel(320.0, 300.0, 0.0, 3.0);
    assert!(
        *app.view().scroll > 0.0,
        "the wheel moved the view's offset: {}",
        *app.view().scroll
    );
    assert_eq!(app.describes(), d0, "a wheel scroll describes no fixed row");
    app.check().unwrap();
    // a handler's turn: one description
    click(&mut app, BUTTONS[0].0, BUTTONS[0].1);
    assert_eq!(app.view().items.len(), 100_001);
    assert_eq!(app.describes(), d0 + 1);
    app.check().unwrap();
    assert_eq!(app.lifecycle().0, 1, "one mount");
}
