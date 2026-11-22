//! A borrowed cell is a port (`designs/reactivity/final/03-ports.md` §2): `.bg(&self.tint)` and
//! `text(&self.name)` install the wire `sig!` would, proven from the owner type's cell table
//! (`reflect::cells`), so publishing the signal changes the pixels with no re-description. The apps here are
//! mounted with `Config::once()`: nothing but a wire can change what they show.
//!
//! Negative control: `--features mutant_cell_offset` lists every cell one head further on, so no borrowed
//! field is proven and both tests here fail.
use bough::prelude::*;
use bough::raw::{App, Config};
use bough::reflect::{cell_id, cells};

fn cfg() -> Config {
    let font = std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap();
    Config::new(240.0, 120.0, 1.0, font).once()
}

struct Inner {
    pad: u8,
    hue: Signal<Color>,
}
struct Card {
    name: Signal<String>,
    tint: Signal<Color>,
    inner: Inner,
    pair: (u16, Signal<f32>),
    lamps: [Signal<Color>; 3],
    grid: [[Signal<u8>; 2]; 2],
    far: Vec<Signal<Color>>,
    boxed: Box<Signal<Color>>,
    maybe: Option<Signal<Color>>,
    shared: std::cell::RefCell<Signal<Color>>,
}

fn card() -> Card {
    let c = |v| Signal::new(rgb(v));
    Card {
        name: Signal::new("before".into()),
        tint: c(0x102030),
        inner: Inner { pad: 0, hue: c(0x405060) },
        pair: (0, Signal::new(1.0)),
        lamps: [c(1), c(2), c(3)],
        grid: [[Signal::new(0), Signal::new(1)], [Signal::new(2), Signal::new(3)]],
        far: vec![c(0x708090)],
        boxed: Box::new(c(0xa0b0c0)),
        maybe: Some(c(0xd0e0f0)),
        shared: std::cell::RefCell::new(c(0x112233)),
    }
}

impl View for Card {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            row(()).w(60.0).h(20.0).bg(&self.tint),
            row(()).w(60.0).h(20.0).bg(&self.inner.hue),
            row(()).w(60.0).h(20.0).bg(&self.lamps[2]),
            row(()).w(60.0).h(20.0).bg(&self.far[0]),
            text(&self.name),
        ))
        .bg(rgb(0xffffff))
    }
}

#[test]
fn the_cell_table_lists_fields_and_nothing_behind_a_pointer() {
    let t = cells::<Card>();
    let at = |f: usize, id| t.has(f, id);
    let (color, string) = (cell_id::<Signal<Color>>(), cell_id::<Signal<String>>());
    assert!(at(std::mem::offset_of!(Card, name), string));
    assert!(at(std::mem::offset_of!(Card, tint), color));
    assert!(at(std::mem::offset_of!(Card, inner) + std::mem::offset_of!(Inner, hue), color));
    assert!(at(std::mem::offset_of!(Card, pair) + std::mem::offset_of!((u16, Signal<f32>), 1), cell_id::<Signal<f32>>()));
    for i in 0..3 {
        assert!(at(std::mem::offset_of!(Card, lamps) + i * size_of::<Signal<Color>>(), color));
    }
    for i in 0..4 {
        assert!(at(std::mem::offset_of!(Card, grid) + i * size_of::<Signal<u8>>(), cell_id::<Signal<u8>>()));
    }
    // 2 + 1 + 1 + 3 + 4 cells are fields; the Vec's, the Box's, the Option's and the RefCell's are not
    assert_eq!(t.len(), 11);
    assert!(!t.truncated);
    // the negative controls: the right offset with the wrong type, one element past an array, a neighbour
    assert!(!at(std::mem::offset_of!(Card, tint), string));
    assert!(!at(std::mem::offset_of!(Card, lamps) + 3 * size_of::<Signal<Color>>(), color) || std::mem::offset_of!(Card, lamps) + 3 * size_of::<Signal<Color>>() == std::mem::offset_of!(Card, tint));
    assert!(!at(std::mem::offset_of!(Card, tint) + 4, color) || t.offsets().contains(&(std::mem::offset_of!(Card, tint) + 4)));
    assert!(!at(std::mem::offset_of!(Card, shared), color));
    assert!(!at(std::mem::offset_of!(Card, maybe), color));
    let _ = (card().inner.pad, card().pair.0, &card().boxed, &card().maybe, &card().shared, &card().grid);
}

fn shot(app: &mut App<Card>) -> Vec<u8> {
    app.frame();
    let p = app.presented_pixels();
    assert_eq!(p, app.reference_pixels(), "the repaired frame equals a full redraw");
    p
}

#[test]
fn publishing_a_borrowed_field_patches_without_a_redescription() {
    let mut app = App::new(card(), cfg(), true);
    let p0 = shot(&mut app);
    app.turn(|c| { c.tint.set(rgb(0xff0000)); });
    let p1 = shot(&mut app);
    assert_ne!(p0, p1, "a field of the owner is a port");
    app.turn(|c| { c.inner.hue.set(rgb(0x00ff00)); });
    let p2 = shot(&mut app);
    assert_ne!(p1, p2, "a field of a field is a port");
    app.turn(|c| { c.lamps[2].set(rgb(0x0000ff)); });
    let p3 = shot(&mut app);
    assert_ne!(p2, p3, "an array element is a port");
    app.turn(|c| { c.name.set("after".into()); });
    let p4 = shot(&mut app);
    assert_ne!(p3, p4, "a borrowed string is a text port");
    // the negative controls: an element of the array that is not bound, and a cell behind a Vec (no fixed
    // place in the owner: a constant of the one description this app ever makes)
    app.turn(|c| { c.lamps[0].set(rgb(0x123456)); });
    assert_eq!(p4, shot(&mut app), "an unbound cell paints nothing");
    app.turn(|c| { c.far[0].set(rgb(0xabcdef)); });
    assert_eq!(p4, shot(&mut app), "a cell behind a Vec is not wired");
}
