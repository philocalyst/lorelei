//! Integration Card 3: the vertical-slice application on the lowered runtime, held to the card's gates.
#[path = "../examples/slice.rs"]
mod slice;

use bough::input::{Button, Key, Mods};
use bough::lower::{App, provenance};
use bough::raw::Config;
use slice::{ROW_H, Slice};

fn app(n: usize, ring: usize, scale: f32) -> App<Slice> {
    let mut cfg = Config::new(640.0, 480.0, scale, bough::ui::default_font());
    cfg.ring = ring;
    App::new(Slice::new(n), cfg)
}
fn centre(r: [f32; 4]) -> (f32, f32) {
    (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0)
}
fn rng(seed: u64) -> impl FnMut(u64) -> u64 {
    let mut x = seed;
    move |m| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x % m
    }
}
fn press(app: &mut App<Slice>, button: usize) {
    let at = buttons(app)[button];
    click(app, at);
}
fn click(app: &mut App<Slice>, (x, y): (f32, f32)) {
    app.pointer_down(x, y, Button::Primary);
    app.pointer_up(x, y, Button::Primary);
}
/// The header's buttons, left to right: Add, Add first, Remove, Reverse, Theme.
fn buttons(app: &App<Slice>) -> Vec<(f32, f32)> {
    let b = app.interactive();
    assert!(b.len() >= 6, "five buttons and the list: {b:?}");
    b[..5].iter().map(|&r| centre(r)).collect()
}

/// The card's first gate: retained == full redraw == immediate == fresh mount, at ring ages 1..=4, across a
/// script of every kind of turn the slice has.
#[test]
fn a_random_script_holds_the_four_oracles_at_ring_ages_1_to_4() {
    for ring in 1..=4 {
        let mut app = app(300, ring, if ring % 2 == 0 { 2.0 } else { 1.0 });
        app.check().unwrap_or_else(|e| panic!("ring {ring} mount: {e}"));
        let mut r = rng(0x2545_f491_4f6c_dd1d ^ ring as u64);
        for step in 0..80 {
            let what = r(12);
            match what {
                0..=4 => press(&mut app, what as usize),
                5 | 6 => app.wheel(320.0, 300.0, 0.0, r(7) as f32 - 3.0),
                7 | 8 => app.pointer_move(r(640) as f32, r(480) as f32),
                9 => {
                    // focus the list, then keys
                    click(&mut app, (320.0, 300.0));
                    app.key_down([Key::Down, Key::Up, Key::PageDown, Key::PageUp][r(4) as usize], Mods::default());
                }
                10 => app.turn(|s| {
                    let t = format!("Slice {}", s.items.len());
                    s.title.set(t);
                }),
                _ => {
                    let (w, h) = (400.0 + r(400) as f32, 300.0 + r(300) as f32);
                    app.resize(w, h);
                }
            }
            app.check().unwrap_or_else(|e| panic!("ring {ring} step {step} (op {what}): {e}"));
        }
        assert_eq!(app.lifecycle().0, 1, "one mount, whatever was resized");
    }
    assert_eq!(provenance::counts().legacy(), 0, "no legacy runtime pass");
}

/// O(exposed): a list of 100,000 rows materialises what its port shows.
#[test]
fn a_hundred_thousand_rows_cost_what_is_exposed() {
    let mut app = app(100_000, 2, 1.0);
    app.check().unwrap();
    let records = app.record_count();
    assert!(records <= 400, "{records} records for 100,000 rows");
    assert!(app.world_stats().0 <= 400 + 64, "the world holds {} record slots", app.world_stats().0);
    // scrolled to the middle and to the end: the same count band
    for to in [40_000.0 * ROW_H, 99_990.0 * ROW_H] {
        app.turn(|s| {
            s.scroll.set(to);
        });
        app.check().unwrap();
        assert!(app.record_count() <= 400, "{} records at offset {to}", app.record_count());
    }
    assert_eq!(provenance::counts().legacy(), 0);
}

/// Text the UI face does not cover is shaped by a fallback face and drawn: other scripts, and emoji in colour.
#[test]
fn non_latin_text_and_emoji_are_drawn_through_the_fallback() {
    let mut app = app(8, 1, 2.0);
    app.check().unwrap();
    let st = app.text_stats();
    assert!(st.fallback_segments > 0, "some run needed a face other than the UI face: {st:?}");
    // row 3 is "emoji 😀 🎉 row": some pixel of it is saturated colour, which no grey text ink on a grey row is
    let (w, px) = (1280usize, app.presented_pixels());
    let row_y = |i: usize| ((48.0 + i as f32 * ROW_H) * 2.0) as usize;
    let coloured = |i: usize| {
        (row_y(i)..row_y(i + 1)).flat_map(|y| (0..w).map(move |x| (x, y))).filter(|&(x, y)| {
            let p = &px[(y * w + x) * 4..][..4];
            let (lo, hi) = (p[0].min(p[1]).min(p[2]) as i32, p[0].max(p[1]).max(p[2]) as i32);
            hi - lo > 90 && p[0] as i32 > p[2] as i32 + 60
        }).count()
    };
    if std::path::Path::new("/System/Library/Fonts/Apple Color Emoji.ttc").exists() {
        assert!(coloured(3) > 50, "the emoji row has {} warm saturated pixels", coloured(3));
    }
    assert_eq!(coloured(0), 0, "the ASCII row has none");
    // every row drew ink: no run was dropped (rows 2, 4, 7 are CJK, Greek/Cyrillic, Arabic)
    for i in 0..8 {
        let ink = (row_y(i)..row_y(i + 1)).flat_map(|y| (240..900).map(move |x| (x, y))).filter(|&(x, y)| {
            let p = &px[(y * w + x) * 4..][..4];
            (p[0] as u32 + p[1] as u32 + p[2] as u32) < 3 * 150
        }).count();
        assert!(ink > 40, "row {i} drew {ink} dark pixels: its text is missing");
    }
    if let Ok(dir) = std::env::var("BOUGH_LOWERED_PNG") {
        app.write_png(&std::path::Path::new(&dir).join("slice-unicode.png")).unwrap();
    }
}

/// A translucent paint is composited, not forced opaque: the band over the list shows the rows through it.
#[test]
fn a_translucent_band_is_composited_over_what_is_under_it() {
    let mut app = app(40, 1, 1.0);
    app.check().unwrap();
    let px = app.presented_pixels();
    let at = |x: usize, y: usize| -> [u8; 4] { px[(y * 640 + x) * 4..][..4].try_into().unwrap() };
    // the band is 160 x 40 at (24, 48 + 12); row backgrounds under it are 0xf9fafb / 0xffffff
    let under = at(600, 48 + 30);
    let through = at(24 + 150, 48 + 12 + 20);
    assert!(under[0] > 240 && under[2] > 240, "beside the band the row is its own colour: {under:?}");
    // src-over of rgba(47,107,255,96) on a near-white row: red about 255 - (255-47)*96/255 = 177
    assert!((through[0] as i32 - 177).abs() <= 6 && through[2] > 245, "under the band the row shows through: {through:?}");
}

/// Resize keeps the world: no remount, focus, a held press and the scroll anchor survive.
#[test]
fn resize_keeps_the_world_focus_a_held_press_and_the_scroll_anchor() {
    let mut app = app(500, 3, 1.0);
    app.check().unwrap();
    app.turn(|s| {
        s.scroll.set(100.0 * ROW_H);
    });
    // press the Add button, resize while it is held, release: the click lands
    let add = buttons(&app)[0];
    let n = app.view().items.len();
    app.pointer_down(add.0, add.1, Button::Primary);
    app.resize(800.0, 600.0);
    app.check().unwrap();
    app.resize(700.0, 420.0);
    app.check().unwrap();
    let add = buttons(&app)[0];
    app.pointer_up(add.0, add.1, Button::Primary);
    app.check().unwrap();
    assert_eq!(app.view().items.len(), n + 1, "the press held across two resizes became a click");
    // focus the list, resize twice more: the same node is focused and still hears keys
    click(&mut app, (320.0, 300.0));
    let focus = app.focused().expect("the list took focus");
    app.resize(640.0, 480.0);
    app.check().unwrap();
    app.resize(700.0, 420.0);
    app.check().unwrap();
    assert_eq!(app.lifecycle(), (1, 4), "one mount, four in-place resizes");
    assert_eq!(*app.view().scroll, 100.0 * ROW_H, "the scroll anchor is the view's and is kept");
    let after = app.focused().expect("focus survives a resize");
    assert_eq!((focus[0], focus[1]), (after[0], after[1]), "the same node is focused");
    app.key_down(Key::Down, Mods::default());
    assert_eq!(*app.view().scroll, 101.0 * ROW_H, "and it still hears keys");
    assert_eq!(provenance::counts().legacy(), 0);
}

/// Two worlds on one thread do not see each other.
#[test]
fn a_second_world_is_isolated() {
    let mut a = app(50, 2, 1.0);
    let mut b = app(50, 2, 1.0);
    a.check().unwrap();
    b.check().unwrap();
    let before = b.presented_pixels();
    for _ in 0..5 {
        press(&mut a, 0);
        a.wheel(320.0, 300.0, 0.0, 2.0);
        a.pointer_move(300.0, 200.0);
        a.check().unwrap();
    }
    assert_eq!(a.view().items.len(), 55);
    assert_eq!(b.view().items.len(), 50);
    assert!(!b.frame() || b.presented_pixels() == before, "nothing was damaged in the other world");
    b.check().unwrap();
    assert_eq!(b.presented_pixels(), before);
    drop(a);
    press(&mut b, 2);
    b.check().unwrap();
    assert_eq!(b.view().items.len(), 49);
}

/// Count bands (the gates' exact numbers): a hover over a row is one or two records patched and no description;
/// an edit is one description per turn; an idle frame repaints nothing.
#[test]
fn count_bands() {
    let mut app = app(1000, 1, 1.0);
    app.check().unwrap();
    let d0 = app.stats.describes;
    app.pointer_move(320.0, 200.0);
    app.pointer_move(320.0, 200.0 + ROW_H);
    app.pointer_move(320.0, 200.0 + 2.0 * ROW_H);
    assert_eq!(app.stats.describes, d0, "hover describes nothing");
    assert_eq!(app.stats.patches, 5, "enter one row, then leave and enter twice: five records");
    app.check().unwrap();
    press(&mut app, 0);
    assert_eq!(app.stats.describes, d0 + 1, "a click that changes state is one description");
    app.check().unwrap();
    let idle = app.stats.idle_frames;
    app.frame();
    assert_eq!(app.stats.idle_frames, idle + 1);
}

/// Records that come and go in place: a column whose number of painted children follows the state. A child
/// added at the end enters where nothing else changed, and one removed leaves the same way, so the only damage
/// is the free row's own (the negative controls `mutant_free_enter` and `mutant_free_leave` fail here).
mod grow {
    use bough::prelude::*;
    pub struct Grow {
        pub n: usize,
    }
    impl View for Grow {
        fn view(&self) -> impl Element<Self> + '_ {
            column(each((0..self.n).map(|i| block(40.0 + 8.0 * i as f32, 12.0).bg(rgb(0x2f6bff + 0x1100 * i as u32)).rounded(3.0)))).gap(4.0).p(8.0).w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff))
        }
    }
}
#[test]
fn records_that_enter_and_leave_in_place_are_damaged() {
    for ring in 1..=4 {
        let mut cfg = Config::new(320.0, 240.0, 1.0, bough::ui::default_font());
        cfg.ring = ring;
        let mut app = App::new(grow::Grow { n: 3 }, cfg);
        app.check().unwrap();
        for n in [4, 5, 9, 8, 3, 0, 1, 12, 2] {
            app.turn(|g| g.n = n);
            app.check().unwrap_or_else(|e| panic!("ring {ring}, {n} blocks: {e}"));
            assert_eq!(app.record_count(), n + 1);
        }
    }
}
