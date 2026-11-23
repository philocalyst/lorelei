//! Independent tests of typed descriptions, list publications and input identity.
use bough::input::Button;
use bough::builders::*;
use bough::typed::App;
use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

const W: usize = 80;
const H: usize = 80;
const ROW: f32 = 20.0;
const HEADER: f32 = 20.0;
const BACK: Color = rgb(0x112233);
const HOT: Color = rgb(0xfeda11);

fn config(ring: usize) -> bough::app::Config {
    let mut cfg = bough::app::Config::new(W as f32, H as f32, 1.0, bough::ui::default_font());
    cfg.ring = ring;
    cfg
}

fn mount<V: View>(v: V, ring: usize) -> App<V> {
    App::new(v, config(ring)).unwrap_or_else(|(_, why)| panic!("refused: {why:?}"))
}

fn at(pixels: &[u8], x: usize, y: usize) -> [u8; 4] {
    pixels[(y * W + x) * 4..][..4].try_into().unwrap()
}

fn rgba(c: Color) -> [u8; 4] {
    c.0.to_le_bytes()
}

#[derive(Clone)]
struct Item {
    id: u32,
    ink: Color,
    fault: Option<Rc<Cell<bool>>>,
    hot: bool,
    events: [u8; 2],
    clicks: u32,
}

impl Item {
    fn new(id: u32) -> Self {
        Self {
            id,
            ink: rgb(0x220000 + id % 0xa000),
            fault: None,
            hot: false,
            events: [0; 2],
            clicks: 0,
        }
    }
}

impl View for Item {
    fn view(&self) -> impl Element<Self> + '_ {
        assert!(
            !self.fault.as_deref().is_some_and(Cell::get),
            "row view fault"
        );
        block(W as f32, ROW)
            .bg(if self.hot { HOT } else { self.ink })
            .hover_bg(HOT)
            .on_hover(|item: &mut Self, e| {
                item.hot = e.on;
                item.events[usize::from(!e.on)] += 1;
            })
            .on_click(|item: &mut Self| item.clicks += 1)
    }
}

struct Board {
    items: List<Item>,
    offset: Signal<f32>,
    width: f32,
    ink: Color,
    wave: u8,
    publications: [usize; 3],
}

impl Board {
    fn from_items(items: impl IntoIterator<Item = Item>) -> Self {
        Self {
            items: List::from_iter_in(items),
            offset: Signal::new(0.0),
            width: 20.0,
            ink: rgb(0x77aaff),
            wave: 0,
            publications: [0; 3],
        }
    }

    fn new(n: u32) -> Self {
        Self::from_items((0..n).map(Item::new))
    }
}

impl View for Board {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            row((block(self.width, HEADER).bg(self.ink),))
                .h(HEADER)
                .on_click(|board: &mut Self| {
                    board.wave = 1;
                    board.items.insert(0, Item::new(238));
                }),
            list(sig!(self.items))
                .scroll(sig!(self.offset))
                .estimate(ROW)
                .overscan(0)
                .w(W as f32)
                .h(H as f32 - HEADER),
        ))
        .w(W as f32)
        .h(H as f32)
        .bg(BACK)
    }

    fn changed(&mut self, changed: Changed<'_, Self>) {
        let Some(ops) = changed.list(sig!(self.items)) else {
            return;
        };
        if self.wave == 1 {
            self.publications[0] = ops.len();
            self.wave = 2;
            self.items.edit(0).ink = rgb(0x44aabb);
            self.items.insert(1, Item::new(239));
        } else if self.wave == 2 {
            self.publications[1] = ops.len();
            self.wave = 3;
            self.items.remove(2);
            self.width = 50.0;
        } else if self.wave == 3 {
            self.publications[2] = ops.len();
            self.wave = 4;
        }
    }
}

fn viewport(app: &mut App<Board>, order: &[Item], offset: usize) {
    app.check().unwrap();
    let pixels = app.presented_pixels();
    for (row, item) in order.iter().skip(offset).take(3).enumerate() {
        let y = (HEADER + ROW * row as f32 + 10.0) as usize;
        assert_eq!(
            at(&pixels, 12, y),
            rgba(if item.hot { HOT } else { item.ink }),
            "row {row}, item {}",
            item.id
        );
    }
}

#[test]
fn multiple_edits_overflow_and_inverse_match_an_independent_vec() {
    for ring in 1..=4 {
        let mut app = mount(Board::new(30), ring);
        let mut order: Vec<_> = (0..30).map(Item::new).collect();
        let cases = [(2, 3), (3, 11), (0, 17)];
        for (offset, edits) in cases {
            app.turn(|v| {
                v.offset.set(offset as f32 * ROW);
            });
            app.turn(|v| {
                for step in 0..edits {
                    let at = (offset + step * 3) % order.len();
                    if step % 3 == 0 {
                        v.items.edit(at).ink = rgb(0xbb8866);
                        order[at].ink = rgb(0xbb8866);
                    } else if step % 3 == 1 {
                        let item = Item::new(100 + step as u32);
                        v.items.insert(at, item.clone());
                        order.insert(at, item);
                    } else {
                        v.items.remove(at);
                        order.remove(at);
                    }
                }
            });
            assert_eq!(
                app.view().items.iter().map(|i| i.id).collect::<Vec<_>>(),
                order.iter().map(|i| i.id).collect::<Vec<_>>()
            );
            viewport(&mut app, &order, offset);
            // Inverse operations restore the same order within one description interval.
            app.turn(|v| {
                v.items.insert(offset, Item::new(500));
                v.items.remove(offset);
                v.items.move_to(offset + 1, offset + 3);
                v.items.move_to(offset + 3, offset + 1);
            });
            viewport(&mut app, &order, offset);
        }
    }
}

#[test]
fn changed_handler_writes_publish_once_per_wave() {
    let mut app = mount(Board::new(20), 2);
    let before = app.describes();
    app.pointer_down(10.0, 10.0, Button::Primary);
    app.pointer_up(10.0, 10.0, Button::Primary);
    assert_eq!(app.view().publications, [1, 2, 1]);
    assert_eq!(app.view().wave, 4);
    assert_eq!(app.describes(), before + 1);
    let mut order: Vec<_> = (1..20).map(Item::new).collect();
    order.insert(0, Item::new(239));
    order.insert(
        0,
        Item {
            ink: rgb(0x44aabb),
            ..Item::new(238)
        },
    );
    viewport(&mut app, &order, 0);
    assert_eq!(at(&app.presented_pixels(), 40, 10), rgba(app.view().ink));
}

#[test]
fn held_press_survives_redescription_and_hover_follows_entering_rows() {
    let mut app = mount(Board::new(20), 3);
    let held = app.view().items.slot(1).unwrap();
    app.pointer_move(10.0, 50.0);
    app.pointer_down(10.0, 50.0, Button::Primary);
    app.turn(|v| {
        v.width = 50.0;
        v.items.insert(0, Item::new(100));
    });
    assert_eq!(app.view().items.get_slot(held).unwrap().events, [1, 1]);
    assert_eq!(app.view().items.get(1).unwrap().events, [1, 0]);
    app.pointer_up(10.0, 70.0, Button::Primary);
    assert_eq!(app.view().items.get_slot(held).unwrap().clicks, 1);
    assert_eq!(app.view().items.get(1).unwrap().clicks, 0);
    app.check().unwrap();
}

#[test]
fn hover_moves_to_a_row_entering_from_outside_the_old_window() {
    let mut app = mount(Board::new(20), 3);
    app.pointer_move(10.0, 30.0);
    assert_eq!(app.view().items.get(0).unwrap().events, [1, 0]);
    assert_eq!(app.view().items.get(10).unwrap().events, [0, 0]);

    app.turn(|board| {
        board.offset.set(10.0 * ROW);
    });
    assert_eq!(app.view().items.get(0).unwrap().events, [1, 1]);
    assert_eq!(app.view().items.get(10).unwrap().events, [1, 0]);
    let mut order: Vec<_> = (0..20).map(Item::new).collect();
    order[10].hot = true;
    viewport(&mut app, &order, 10);
}

#[test]
fn journal_overflow_cancels_a_press_on_removed_item() {
    let mut app = mount(Board::new(30), 2);
    app.pointer_down(10.0, 30.0, Button::Primary);
    let old = app.view().items.slot(0).unwrap();
    app.turn(|v| {
        v.items.remove(0);
        for id in 100..109 {
            v.items.insert(0, Item::new(id));
        }
    });
    assert!(app.view().items.get_slot(old).is_none());
    app.pointer_up(10.0, 30.0, Button::Primary);
    assert_eq!(app.view().items.get(0).unwrap().clicks, 0);
    app.check().unwrap();
}

#[test]
fn interrupted_description_withdraws_tables_and_retries_pending_row() {
    let mut app = mount(Board::new(20), 2);
    let fault = Rc::new(Cell::new(true));
    let message = catch_unwind(AssertUnwindSafe(|| {
        app.turn(|v| {
            v.ink = rgb(0x4488aa);
            let row = v.items.edit(0);
            row.ink = rgb(0x886644);
            row.fault = Some(fault.clone());
        });
    }))
    .expect_err("the exposed row must be visited");
    assert_eq!(message.downcast_ref::<&str>(), Some(&"row view fault"));
    fault.set(false);
    // Inspect and dispatch while the failed description's table is withdrawn. Retry with no further list edit.
    std::hint::black_box(app.reference_pixels());
    app.pointer_move(79.0, 5.0);
    app.turn(|_| {});
    let mut order: Vec<_> = (0..20).map(Item::new).collect();
    order[0].ink = rgb(0x886644);
    viewport(&mut app, &order, 0);
    assert_eq!(at(&app.presented_pixels(), 4, 10), rgba(rgb(0x4488aa)));
}

struct ByValue {
    width: Signal<f32>,
    step: u32,
    total: u32,
}

impl View for ByValue {
    fn view(&self) -> impl Element<Self> + '_ {
        let step = self.step;
        row((
            block(*self.width, 20.0)
                .bg(rgb(0x227744))
                .on_click(move |v: &mut Self| v.total += step),
            block(10.0, 20.0).bg(rgb(0x3366ff)),
            text(self.total).font(10.0, 16.0),
        ))
        .w(W as f32)
        .h(20.0)
    }
}

#[test]
fn a_layout_word_read_by_value_and_a_capture_both_change() {
    let mut app = mount(
        ByValue {
            width: Signal::new(20.0),
            step: 3,
            total: 0,
        },
        3,
    );
    app.check().unwrap();
    assert_eq!(at(&app.presented_pixels(), 25, 10), rgba(rgb(0x3366ff)));
    app.turn(|v| {
        v.width.set(40.0);
        v.step = 7;
    });
    app.check().unwrap();
    assert_eq!(at(&app.presented_pixels(), 25, 10), rgba(rgb(0x227744)));
    assert_eq!(at(&app.presented_pixels(), 45, 10), rgba(rgb(0x3366ff)));
    app.pointer_down(15.0, 8.0, Button::Primary);
    app.pointer_up(15.0, 8.0, Button::Primary);
    assert_eq!(app.view().total, 7);
    app.check().unwrap();
}

struct Choice {
    wired: bool,
    ink: Signal<Color>,
}

impl View for Choice {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            self.wired.then(|| block(20.0, 20.0).bg(sig!(self.ink))),
            (!self.wired).then(|| block(20.0, 20.0).bg(rgb(0x228844))),
        ))
    }
}

#[test]
fn switching_between_operand_types_has_an_explicit_admission_boundary() {
    let refused = App::new(
        Choice {
            wired: false,
            ink: Signal::new(rgb(0x22aa44)),
        },
        config(1),
    );
    match refused {
        Err((_, why)) => assert!(!why.is_empty()),
        Ok(mut app) => {
            app.turn(|v| v.wired = true);
            app.check().unwrap();
            assert_eq!(at(&app.presented_pixels(), 5, 5), rgba(rgb(0x22aa44)));
        }
    }
}

#[derive(Clone, Copy)]
struct PlainRow;
impl View for PlainRow {
    fn view(&self) -> impl Element<Self> + '_ {
        block(W as f32, ROW).bg(rgb(0x228844)).hover_bg(HOT)
    }
}

struct TwoLists {
    first: List<PlainRow>,
    second: List<PlainRow>,
    y1: Signal<f32>,
    y2: Signal<f32>,
}
impl View for TwoLists {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            list(sig!(self.first))
                .scroll(sig!(self.y1))
                .estimate(ROW)
                .h(40.0)
                .w(W as f32),
            list(sig!(self.second))
                .scroll(sig!(self.y2))
                .estimate(ROW)
                .h(40.0)
                .w(W as f32),
        ))
        .w(W as f32)
        .h(H as f32)
    }
}

#[test]
fn hovering_one_list_does_not_light_the_other_lists_equal_slot() {
    let mut app = mount(
        TwoLists {
            first: List::from_iter_in([PlainRow; 2]),
            second: List::from_iter_in([PlainRow; 2]),
            y1: Signal::new(0.0),
            y2: Signal::new(0.0),
        },
        2,
    );
    app.pointer_move(10.0, 10.0);
    app.frame();
    let pixels = app.presented_pixels();
    assert_eq!(at(&pixels, 10, 10), rgba(HOT));
    assert_eq!(
        at(&pixels, 10, 50),
        rgba(rgb(0x228844)),
        "other list's matching slot painted hot"
    );
}

struct TextRow(&'static str);
impl View for TextRow {
    fn view(&self) -> impl Element<Self> + '_ {
        row((text(self.0).font(13.0, ROW),))
            .h(ROW)
            .w(W as f32)
            .bg(BACK)
    }
}

struct TextList {
    rows: List<TextRow>,
    scroll: Signal<f32>,
}
impl TextList {
    fn new() -> Self {
        Self {
            rows: List::from_iter_in([
                TextRow("north"),
                TextRow("south"),
                TextRow("east"),
                TextRow("west"),
            ]),
            scroll: Signal::new(0.0),
        }
    }
}
impl View for TextList {
    fn view(&self) -> impl Element<Self> + '_ {
        list(sig!(self.rows))
            .scroll(sig!(self.scroll))
            .estimate(ROW)
            .w(W as f32)
            .h(H as f32)
    }
}

#[test]
fn repeated_resize_descriptions_keep_runs_referenced_by_unchanged_rows() {
    let mut app = mount(TextList::new(), 2);
    app.frame();
    let expected = app.presented_pixels();
    assert!(expected.chunks_exact(4).any(|p| p != rgba(BACK)));
    for n in 1..=550 {
        app.resize(W as f32, H as f32);
        assert_eq!(
            app.pass_stats().rows,
            0,
            "unchanged rows visited after {n} descriptions"
        );
        if n >= 500 || n % 128 == 0 {
            app.frame();
            let pixels = app.presented_pixels();
            assert!(
                pixels == expected,
                "run disappeared after {n} resize descriptions, stats {:?}",
                app.pass_stats()
            );
        }
    }
    app.check().unwrap();
}

#[test]
fn a_changed_plain_paint_patches_one_leaf_without_solving_layout() {
    let mut app = mount(Board::new(4), 1);
    app.frame();
    let before = app.describes();
    app.turn(|v| v.ink = rgb(0x887722));
    assert_eq!(app.describes(), before + 1);
    let stats = app.pass_stats();
    assert_eq!((stats.rows, stats.layouts, stats.lookups), (0, 0, 0));
    app.check().unwrap();
    assert_eq!(at(&app.presented_pixels(), 4, 10), rgba(rgb(0x887722)));
}

struct Title {
    label: Signal<String>,
}
impl View for Title {
    fn view(&self) -> impl Element<Self> + '_ {
        row((
            text(sig!(self.label)).font(12.0, 20.0),
            block(8.0, 20.0).bg(rgb(0x887722)),
        ))
        .w(W as f32)
        .h(20.0)
        .bg(BACK)
    }
}

#[test]
fn a_fresh_title_with_different_advance_solves_the_layout_once() {
    let mut app = mount(
        Title {
            label: Signal::new("i".into()),
        },
        1,
    );
    app.frame();
    let initial = app.presented_pixels();
    app.turn(|v| {
        v.label.set("Wide WW".into());
    });
    let stats = app.pass_stats();
    assert_eq!((stats.lookups, stats.layouts), (1, 1));
    app.check().unwrap();
    assert_ne!(app.presented_pixels(), initial);
}

struct RootFault {
    fail: bool,
    ink: Color,
}
impl View for RootFault {
    fn view(&self) -> impl Element<Self> + '_ {
        assert!(!self.fail, "root view fault");
        block(20.0, 20.0).bg(self.ink)
    }
}

#[test]
fn a_panic_before_layout_can_retry_an_identical_shape() {
    let mut app = mount(
        RootFault {
            fail: false,
            ink: BACK,
        },
        1,
    );
    app.check().unwrap();
    let result = catch_unwind(AssertUnwindSafe(|| {
        app.turn(|v| {
            v.fail = true;
            v.ink = rgb(0x4488aa);
        });
    }));
    assert!(result.is_err());
    app.turn(|v| v.fail = false);
    app.check().unwrap();
    assert_eq!(at(&app.presented_pixels(), 4, 4), rgba(rgb(0x4488aa)));
}

struct ProjectionFault {
    width: f32,
    word: Signal<u32>,
}
impl View for ProjectionFault {
    fn view(&self) -> impl Element<Self> + '_ {
        row((
            block(self.width, 20.0).bg(rgb(0x227744)),
            block(10.0, 20.0).bg(rgb(0x3366ff)),
            text(sig!(self.word).map(|n| {
                assert_ne!(*n, 1, "projection fault");
                "ok".to_owned()
            })),
        ))
        .w(W as f32)
        .h(20.0)
    }
}

#[test]
fn a_panic_after_updating_a_spec_cannot_certify_the_old_rects() {
    let mut app = mount(
        ProjectionFault {
            width: 20.0,
            word: Signal::new(0),
        },
        1,
    );
    app.check().unwrap();
    let result = catch_unwind(AssertUnwindSafe(|| {
        app.turn(|v| {
            v.width = 40.0;
            v.word.set(1);
        });
    }));
    assert!(result.is_err());
    app.turn(|v| {
        v.word.set(2);
    });
    assert_eq!(app.pass_stats().layouts, 1);
    app.check().unwrap();
    assert_eq!(at(&app.presented_pixels(), 25, 10), rgba(rgb(0x227744)));
    assert_eq!(at(&app.presented_pixels(), 45, 10), rgba(rgb(0x3366ff)));
}
