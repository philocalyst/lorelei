//! Independent probes of the Card 3 public façade. Ignored tests describe open semantic failures.
use bough::input::{Button, Key, Mods};
use bough::lower::App;
use bough::prelude::*;
use bough::raw::Config;

fn new<V: View>(v: V, w: f32, h: f32, s: f32, ring: usize) -> App<V> {
    let mut cfg = Config::new(w, h, s, bough::ui::default_font());
    cfg.ring = ring;
    App::new(v, cfg)
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
fn wheel_moves_by_its_full_target_on_the_same_turn() {
    let mut a = new(Rows::new(20), 80., 144., 1., 1);
    a.frame();
    let before = a.presented_pixels();
    a.wheel(10., 10., 0., -1.);
    assert_eq!(*a.view().scroll, 0.);
    assert!(!a.frame());
    a.wheel(10., 10., 0., 1.);
    assert_eq!(*a.view().scroll, 48.);
    a.frame();
    assert_ne!(a.presented_pixels(), before);
    assert!(!a.frame());
}

#[test]
#[ignore = "Card 3 currently gives Changed empty publications"]
fn changed_observes_a_single_list_insert() {
    let mut a = new(Rows::new(5), 80., 144., 1., 1);
    a.turn(|v| {
        v.rows.insert(0, Item::new(100));
    });
    assert!(a.view().observed, "Changed::list gave no insert");
}

#[test]
#[ignore = "Card 3 currently retains document and node ordinals as focus"]
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
}

#[test]
#[ignore = "Card 3 currently retargets an ordinal held over a recycled slot"]
fn removed_row_press_is_not_delivered_to_its_replacement() {
    let mut a = new(Rows::new(20), 80., 144., 1., 2);
    let stale = a.view().rows.slot(0).unwrap();
    a.pointer_down(10., 10., Button::Primary);
    a.turn(|v| {
        v.rows.remove(0);
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
}

#[test]
#[ignore = "Card 3 rehover changes paint without dispatching hover enter or leave"]
fn a_stationary_pointer_gets_leave_and_enter_when_rows_scroll_under_it() {
    let mut a = new(Rows::new(20), 80., 144., 1., 2);
    a.pointer_move(10., 20.);
    assert_eq!(a.view().rows.get(0).unwrap().hovers, [true]);
    a.wheel(10., 20., 0., 1.);
    assert_eq!(a.view().rows.get(0).unwrap().hovers, [true, false]);
    assert_eq!(a.view().rows.get(1).unwrap().hovers, [true]);
}

struct Two {
    a: List<Item>,
    b: List<Item>,
    y: Signal<f32>,
}
impl View for Two {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            list(sig!(self.a))
                .scroll(sig!(self.y))
                .estimate(48.)
                .overscan(2)
                .w(80.)
                .h(48.),
            list(sig!(self.b)).w(80.),
        ))
        .w(80.)
        .h_pct(1.)
    }
}
#[test]
fn tab_skips_clipped_rows_and_reaches_the_next_list() {
    let mut a = List::new();
    for id in 0..4 {
        a.push(Item::new(id));
    }
    let mut b = List::new();
    b.push(Item::new(100));
    let mut a = new(
        Two {
            a,
            b,
            y: Signal::new(0.),
        },
        80.,
        192.,
        1.,
        1,
    );
    key(&mut a, Key::Tab);
    key(&mut a, Key::Down);
    assert_eq!(a.view().a.get(0).unwrap().presses, 1);
    key(&mut a, Key::Tab);
    key(&mut a, Key::Down);
    assert_eq!(
        a.view().b.get(0).unwrap().presses,
        1,
        "the second visible stop belongs to list B"
    );
    a.key_down(
        Key::Tab,
        Mods {
            shift: true,
            ..Mods::default()
        },
    );
    key(&mut a, Key::Down);
    assert_eq!(a.view().a.get(0).unwrap().presses, 2);
    assert_eq!(a.view().a.get(1).unwrap().presses, 0);
}

struct Nested {
    outer: Signal<f32>,
    inner: Signal<f32>,
}
impl View for Nested {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            column(each((0..8).map(|_| block(60., 48.).bg(rgb(0x555555)))))
                .w(60.)
                .h(96.)
                .scroll(sig!(self.inner)),
            block(80., 500.).bg(rgb(0x777777)),
        ))
        .w(80.)
        .h(144.)
        .scroll(sig!(self.outer))
    }
}
#[test]
fn nested_wheel_selects_the_innermost_port() {
    let mut a = new(
        Nested {
            outer: Signal::new(0.),
            inner: Signal::new(0.),
        },
        80.,
        144.,
        1.,
        3,
    );
    a.wheel(10., 20., 0., 1.);
    assert!(*a.view().inner > 0.);
    assert_eq!(*a.view().outer, 0.);
    a.wheel(70., 110., 0., 1.);
    assert!(*a.view().outer > 0.);
    a.check().unwrap();
}

#[test]
#[ignore = "Card 3 retains a raw offset beyond the enlarged viewport's maximum"]
fn resizing_at_the_end_of_100000_rows_keeps_the_port_filled() {
    let mut a = new(Rows::new(100_000), 80., 144., 1., 3);
    a.turn(|v| {
        v.scroll.set(99_997. * 48.);
    });
    let n = a.record_count();
    a.resize(100., 384.);
    a.check().unwrap();
    assert!(a.record_count() <= 400);
    let px = a.presented_pixels();
    let pixel = |y: usize| &px[(y * 100 + 20) * 4..][..4];
    assert!(
        pixel(192)[3] == 255,
        "blank row at y=192, old/new record counts {n}/{}",
        a.record_count()
    );
}

struct Baseline {
    align: Align,
}
impl View for Baseline {
    fn view(&self) -> impl Element<Self> + '_ {
        row((
            text("Big").font(32., 44.).focusable(),
            text("small").font(10., 14.).focusable(),
        ))
        .items(self.align)
        .w_pct(1.)
        .h(70.)
    }
}
#[test]
fn baseline_is_downgraded_to_start() {
    let mut base = new(
        Baseline {
            align: Align::Baseline,
        },
        160.,
        70.,
        1.,
        1,
    );
    let mut start = new(
        Baseline {
            align: Align::Start,
        },
        160.,
        70.,
        1.,
        1,
    );
    let mut end = new(Baseline { align: Align::End }, 160., 70., 1., 1);
    base.frame();
    start.frame();
    end.frame();
    assert_eq!(base.presented_pixels(), start.presented_pixels());
    assert_ne!(base.presented_pixels(), end.presented_pixels());
}

struct Abs;
impl View for Abs {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            block(60., 30.),
            div()
                .w(60.)
                .h(60.)
                .child(block(10., 10.).abs(7., 9.).bg(rgb(0xff0000)).focusable()),
        ))
        .relative()
        .w(100.)
        .h(100.)
    }
}
#[test]
fn absolute_child_uses_the_immediate_parent_as_containing_block() {
    let mut a = new(Abs, 100., 100., 1., 1);
    a.frame();
    let px = a.presented_pixels();
    let at = |x: usize, y: usize| &px[(y * 100 + x) * 4..][..4];
    assert_eq!(at(8, 40), [255, 0, 0, 255]);
    assert_ne!(at(8, 10), [255, 0, 0, 255]);
}

struct Mixed;
impl View for Mixed {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            text("A العربية B 中文 😀 Z")
                .font(24., 34.)
                .color(rgb(0x111111)),
            text("D Ελληνικά E 😀 Ж")
                .font(24., 34.)
                .color(rgb(0x111111)),
        ))
        .w_pct(1.)
        .h_pct(1.)
        .bg(rgb(0xffffff))
    }
}
#[test]
fn mixed_script_and_emoji_paint_at_fractional_scale() {
    let mut a = new(Mixed, 320., 80., 1.25, 4);
    for _ in 0..4 {
        a.check().unwrap();
    }
    let px = a.presented_pixels();
    assert_eq!(px.len(), 400 * 100 * 4);
    assert!(a.text_stats().fallback_segments > 0);
    for row in 0..2 {
        let p = &px[row * 43 * 400 * 4..(row + 1) * 43 * 400 * 4];
        assert!(
            p.chunks_exact(4)
                .filter(|p| p[0] < 96 && p[1] < 96 && p[2] < 96)
                .count()
                > 50
        );
        if std::path::Path::new("/System/Library/Fonts/Apple Color Emoji.ttc").exists() {
            assert!(
                p.chunks_exact(4)
                    .filter(|p| p[0] > p[2].saturating_add(50))
                    .count()
                    > 20
            );
        }
    }
}

struct Overlap {
    order: bool,
}
impl View for Overlap {
    fn view(&self) -> impl Element<Self> + '_ {
        let colors = if self.order {
            [Color::rgba(10, 10, 240, 170), Color::rgba(240, 10, 10, 85)]
        } else {
            [Color::rgba(240, 10, 10, 85), Color::rgba(10, 10, 240, 170)]
        };
        div()
            .relative()
            .w(50.)
            .h(40.)
            .bg(rgb(0x226644))
            .child(block(24., 20.).abs(3., 5.).bg(colors[0]))
            .child(block(24., 20.).abs(13., 12.).bg(colors[1]))
    }
}
#[test]
fn translucent_overlap_matches_an_independent_source_over_image() {
    let mut a = new(Overlap { order: false }, 50., 40., 1., 4);
    for order in [false, true, false] {
        a.turn(|v| v.order = order);
        let colors = if order {
            [[10, 10, 240, 170], [240, 10, 10, 85]]
        } else {
            [[240, 10, 10, 85], [10, 10, 240, 170]]
        };
        for _ in 0..4 {
            a.frame();
            for (i, pixel) in a.presented_pixels().chunks_exact(4).enumerate() {
                let (x, y) = (i % 50, i / 50);
                let mut rgb = [34., 102., 68.];
                for ((left, top), source) in [(3, 5), (13, 12)].into_iter().zip(colors) {
                    if (left..left + 24).contains(&x) && (top..top + 20).contains(&y) {
                        let alpha = f64::from(source[3]) / 255.;
                        for c in 0..3 {
                            rgb[c] = rgb[c] * (1. - alpha) + f64::from(source[c]) * alpha;
                        }
                    }
                }
                for c in 0..3 {
                    assert!(
                        (f64::from(pixel[c]) - rgb[c]).abs() <= 1.5,
                        "pixel {x},{y}, order {order}: {pixel:?} vs {rgb:?}"
                    );
                }
                assert_eq!(pixel[3], 255);
            }
        }
    }
}

struct Refused {
    which: u8,
    field: Signal<String>,
    scroll: Signal<f32>,
    rows: List<Item>,
    entities: Entities<Item>,
    entity: Entity<Item>,
    reader: Reader<u32>,
}
impl View for Refused {
    fn view(&self) -> impl Element<Self> + '_ {
        div().child((
            (self.which == 0).then(|| text_field(sig!(self.field))),
            (self.which == 1).then(|| block(20., 20.).scrolls(sig!(self.scroll), 48., 100.)),
            (self.which == 2).then(|| {
                block(20., 20.)
                    .bg(rgb(0))
                    .paint_with(sig!(self.scroll), |_| (rgb(0), rgb(0xffffff)))
            }),
            (self.which == 3).then(|| bough::raw::list_with(sig!(self.rows), |_, _| {})),
            (self.which == 4).then(|| bough::raw::index_list(2, |_, _, _| {})),
            (self.which == 5).then(|| bough::raw::entity_view(sig!(self.entities), self.entity)),
            (self.which == 6).then(|| boundary(sig!(self.reader)).child(text("ready"))),
            (self.which == 7).then(|| bough::raw::quad(0., 0., 20., 20.)),
        ))
    }
}
#[test]
fn remaining_reported_refusals_name_the_construct() {
    for (which, expected) in [
        "TextField",
        "Scrolls",
        "PaintOp",
        "ListRows",
        "IndexRows",
        "EntityView",
        "Boundary",
        "Quad",
    ]
    .into_iter()
    .enumerate()
    {
        let mut entities = Entities::with_capacity(1);
        let entity = entities.insert(Item::new(0));
        let view = Refused {
            which: which as u8,
            field: Signal::new(String::new()),
            scroll: Signal::new(0.),
            rows: List::new(),
            entities,
            entity,
            reader: Reader::default(),
        };
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| new(view, 80., 80., 1., 1)));
        let error = result.err().expect(expected);
        let message = error.downcast_ref::<String>().expect("named refusal");
        assert!(
            message.contains("does not lower") && message.contains(expected),
            "wanted {expected}, got {message}"
        );
    }
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
#[test]
#[ignore = "Card 3 discards clip_overflow from the layout spec"]
fn clip_overflow_cuts_paint_outside_the_parent() {
    let mut a = new(Clip, 50., 50., 1., 1);
    a.check().unwrap();
    let px = a.presented_pixels();
    let at = |x: usize, y: usize| &px[(y * 50 + x) * 4..][..4];
    assert_eq!(at(25, 25), [255, 0, 0, 255]);
    assert_ne!(at(35, 25), [255, 0, 0, 255]);
}
