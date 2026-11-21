//! The Card 3 review's open probes (`tests/lowered_review.rs`, ignored there: they fail on the lowered
//! fallback), run unchanged on the typed path: the same views, the same input, the same assertions; only the
//! runtime under them is `bough::typed::App`. What the review asked for is one publication carrying the item,
//! its generation and its call site; on the typed path a handler reaches its item by slot and generation, a
//! list keeps a row's records across edits, and the hovered set and a held press are stamped records.
use bough::input::{Button, Key, Mods};
use bough::prelude::*;
use bough::raw::Config;
use bough::typed::App;

fn new<V: View>(v: V, w: f32, h: f32, s: f32, ring: usize) -> App<V> {
    let mut cfg = Config::new(w, h, s, bough::ui::default_font());
    cfg.ring = ring;
    match App::new(v, cfg) {
        Ok(a) => a,
        Err((_, why)) => panic!("not admitted to the typed path: {why:?}"),
    }
}

fn key<V: View>(a: &mut App<V>, key: Key) {
    a.key_down(key, Mods::default());
}

struct Item {
    id: u32,
    presses: u32,
    hovers: Vec<bool>,
}
impl Item {
    fn new(id: u32) -> Self {
        Self {
            id,
            presses: 0,
            hovers: Vec::new(),
        }
    }
}
impl View for Item {
    fn view(&self) -> impl Element<Self> + '_ {
        block(80., 48.)
            .bg(rgb(0x123456 + self.id % 256))
            .hover_bg(rgb(0x224488))
            .focusable()
            .on_key(|r: &mut Self, _| r.presses += 1)
            .on_click(|r: &mut Self| r.presses += 1)
            .on_hover(|r: &mut Self, e| r.hovers.push(e.on))
    }
}
struct Rows {
    rows: List<Item>,
    scroll: Signal<f32>,
    observed: bool,
}
impl Rows {
    fn new(n: u32) -> Self {
        let mut rows = List::new();
        for i in 0..n {
            rows.push(Item::new(i));
        }
        Self {
            rows,
            scroll: Signal::new(0.),
            observed: false,
        }
    }
}
impl View for Rows {
    fn view(&self) -> impl Element<Self> + '_ {
        list(sig!(self.rows))
            .scroll(sig!(self.scroll))
            .estimate(48.)
            .overscan(1)
            .w(80.)
            .h_pct(1.)
    }
    fn changed(&mut self, c: Changed<'_, Self>) {
        self.observed |= c.list(sig!(self.rows)).is_some_and(|ops| !ops.is_empty());
    }
}

#[test]
fn changed_observes_a_single_list_insert() {
    let mut a = new(Rows::new(5), 80., 144., 1., 1);
    a.turn(|v| {
        v.rows.insert(0, Item::new(100));
    });
    assert!(a.view().observed, "Changed::list gave no insert");
    a.check().unwrap();
}

#[test]
fn insert_at_zero_keeps_the_same_row_focused() {
    let mut a = new(Rows::new(10), 80., 192., 1., 1);
    let key_before = a.view().rows.slot(1).unwrap();
    a.pointer_down(10., 55., Button::Primary);
    a.pointer_up(10., 55., Button::Primary);
    let count_before = a.view().rows.get_slot(key_before).unwrap().presses;
    a.turn(|v| {
        v.rows.insert(0, Item::new(100));
    });
    key(&mut a, Key::Down);
    let row = a.view().rows.get_slot(key_before).unwrap();
    assert_eq!(
        row.presses,
        count_before + 1,
        "focused item {} lost the key; presses by order {:?}",
        row.id,
        (0..4)
            .map(|i| a.view().rows.get(i).unwrap().presses)
            .collect::<Vec<_>>()
    );
    a.check().unwrap();
}

#[test]
fn held_press_across_resize_releases_on_the_same_row() {
    let mut a = new(Rows::new(20), 80., 144., 1., 2);
    let slot = a.view().rows.slot(1).unwrap();
    a.pointer_down(10., 55., Button::Primary);
    a.resize(98., 192.);
    a.pointer_up(10., 55., Button::Primary);
    assert_eq!(a.view().rows.get_slot(slot).unwrap().presses, 1);
    assert_eq!(a.lifecycle(), (1, 1));
    a.check().unwrap();
}

// One difference from the probe as written for the lowered runtime: there the removal and the insert are one
// turn and the slot is recycled inside it. A displayed list (here, as on the legacy runtime) frees a slot at
// the end of the turn that removed its item, so that no log entry names two items; the replacement therefore
// arrives in the next turn. The property is the probe's: a press held on an item that was removed is not
// released on the item that got its slot.
#[test]
fn removed_row_press_is_not_delivered_to_its_replacement() {
    let mut a = new(Rows::new(20), 80., 144., 1., 2);
    let stale = a.view().rows.slot(0).unwrap();
    a.pointer_down(10., 10., Button::Primary);
    a.turn(|v| {
        v.rows.remove(0);
    });
    a.turn(|v| {
        let replacement = v.rows.insert(0, Item::new(100));
        assert_eq!(stale.index(), replacement.index());
        assert_ne!(stale, replacement);
        assert!(v.rows.get_slot(stale).is_none());
        assert!(v.rows.edit_slot(stale).is_none());
    });
    a.pointer_up(10., 10., Button::Primary);
    assert_eq!(
        a.view().rows.get(0).unwrap().presses,
        0,
        "the release clicked a replacement generation"
    );
    a.check().unwrap();
}

#[test]
fn a_stationary_pointer_gets_leave_and_enter_when_rows_scroll_under_it() {
    let mut a = new(Rows::new(20), 80., 144., 1., 2);
    a.pointer_move(10., 20.);
    assert_eq!(a.view().rows.get(0).unwrap().hovers, [true]);
    // (the wheel's sign is the legacy runtime's, `raw::App::wheel`: a positive turn scrolls down; the lowered
    // runtime the probe was written against takes the opposite sign, which is its defect, recorded in the report)
    a.wheel(10., 20., 0., 1.);
    assert_eq!(*a.view().scroll, 48.);
    assert_eq!(a.view().rows.get(0).unwrap().hovers, [true, false]);
    assert_eq!(a.view().rows.get(1).unwrap().hovers, [true]);
    a.check().unwrap();
}

#[test]
fn resizing_at_the_end_of_100000_rows_keeps_the_port_filled() {
    let mut a = new(Rows::new(100_000), 80., 144., 1., 3);
    a.turn(|v| {
        v.scroll.set(99_997. * 48.);
    });
    let n = a.world_stats().0;
    a.resize(100., 384.);
    a.check().unwrap();
    assert!(a.world_stats().0 <= 400);
    let px = a.presented_pixels();
    let pixel = |y: usize| &px[(y * 100 + 20) * 4..][..4];
    assert!(
        pixel(192)[3] == 255,
        "blank row at y=192, old/new record counts {n}/{}",
        a.world_stats().0
    );
}

struct Clip;
impl View for Clip {
    fn view(&self) -> impl Element<Self> + '_ {
        div()
            .w(30.)
            .h(30.)
            .clip_overflow()
            .child(block(20., 20.).abs(20., 20.).bg(rgb(0xff0000)))
    }
}
// Open on the typed path as on the lowered one: `clip_overflow` is not carried into the layout words, and a
// clip is a space (a child store), which a box whose clip is a run-time flag cannot become without changing
// its record type. Kept as the review wrote it.
#[test]
#[ignore = "clip_overflow is not a space on the typed path yet"]
fn clip_overflow_cuts_paint_outside_the_parent() {
    let mut a = new(Clip, 50., 50., 1., 1);
    a.check().unwrap();
    let px = a.presented_pixels();
    let at = |x: usize, y: usize| &px[(y * 50 + x) * 4..][..4];
    assert_eq!(at(25, 25), [255, 0, 0, 255]);
    assert_ne!(at(35, 25), [255, 0, 0, 255]);
}
