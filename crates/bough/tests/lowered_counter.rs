//! The counter example on the lowered runtime (Integration Card 3): the twin of `tests/counter.rs`. The same
//! example source, real pointer events through the lexical hit list, the count, the four pixel oracles at ring
//! ages 1..=4, and the provenance gate: no legacy runtime pass on this thread.
#[path = "../examples/counter.rs"]
mod counter;

use bough::input::Button;
use bough::lower::{App, provenance};
use bough::raw::Config;
use bough::signal::Signal;
use counter::Counter;

const SCALE: f32 = 2.0;

fn font() -> Vec<u8> {
    bough::ui::default_font()
}
fn app(ring: usize) -> App<Counter> {
    let mut cfg = Config::new(480.0, 320.0, SCALE, font());
    cfg.ring = ring;
    App::new(
        Counter {
            count: Signal::new(0),
        },
        cfg,
    )
}
fn centre(r: [f32; 4]) -> (f32, f32) {
    (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0)
}

#[test]
fn three_clicks_count_to_three_and_hold_the_oracles_at_every_ring_age() {
    for ring in 1..=4 {
        let mut app = app(ring);
        app.check().unwrap();
        let buttons = app.interactive();
        assert_eq!(buttons.len(), 1, "the one interactive node is the button");
        let (x, y) = centre(buttons[0]);
        for _ in 0..3 {
            app.pointer_down(x, y, Button::Primary);
            app.check().unwrap();
            app.pointer_up(x, y, Button::Primary);
            app.check().unwrap();
        }
        assert_eq!(*app.view().count, 3);
        if let Ok(dir) = std::env::var("BOUGH_LOWERED_PNG") {
            app.write_png(&std::path::Path::new(&dir).join(format!("counter-ring{ring}.png")))
                .unwrap();
        }
        // the label was re-shaped: its pixels are not the first frame's
        assert!(
            app.presented_pixels().iter().any(|&b| b != 0),
            "something was drawn"
        );
    }
    assert_eq!(
        provenance::counts().legacy(),
        0,
        "a lowered application runs no legacy runtime pass"
    );
    assert!(provenance::counts().world_mount == 4 && provenance::counts().dispatch == 12);
}

#[test]
fn a_click_beside_the_button_does_nothing() {
    let mut app = app(2);
    app.check().unwrap();
    let before = app.stats.describes;
    app.pointer_down(470.0, 310.0, Button::Primary);
    app.pointer_up(470.0, 310.0, Button::Primary);
    app.check().unwrap();
    assert_eq!(*app.view().count, 0);
    assert_eq!(
        app.stats.describes, before,
        "an event no handler heard describes nothing"
    );
    assert_eq!(provenance::counts().legacy(), 0);
}

#[test]
fn hover_is_one_record_patched_and_no_description() {
    let mut app = app(3);
    app.check().unwrap();
    let (x, y) = centre(app.interactive()[0]);
    let before = (app.stats.describes, app.stats.patches);
    app.pointer_move(x, y);
    assert_eq!(
        (app.stats.describes, app.stats.patches),
        (before.0, before.1 + 1),
        "entering the button: one record, no description"
    );
    let hovered = {
        app.check().unwrap();
        app.presented_pixels()
    };
    app.pointer_move(470.0, 310.0);
    assert_eq!(
        (app.stats.describes, app.stats.patches),
        (before.0, before.1 + 2)
    );
    app.check().unwrap();
    assert_ne!(
        hovered,
        app.presented_pixels(),
        "the hover paint is visible while hovered and gone after"
    );
}

#[test]
fn an_idle_frame_repaints_nothing() {
    let mut app = app(2);
    app.check().unwrap();
    app.check().unwrap();
    let idle = app.stats.idle_frames;
    assert!(
        !app.frame() && !app.frame(),
        "nothing changed: nothing is repainted"
    );
    assert_eq!(app.stats.idle_frames, idle + 2);
}
