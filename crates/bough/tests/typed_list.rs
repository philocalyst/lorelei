//! Integration Card 5: a `list(..)` on the typed path is its `List`'s own log.
//!
//! A row is described when it enters exposure or when the log names its item; a row that an insert or a remove
//! elsewhere moves is the same records at another place (its rects move, nothing of it is described). What an
//! item's handlers and the hovered set hold is the item, not the row index: a press, a hover and a click follow
//! the item through edits. Bands are exact counts of rows described; every step holds the four oracles (the
//! fresh mount among them describes the same state from nothing, with none of the retained path's memory).
use bough::input::{Button, Ev};
use bough::lower::provenance;
use bough::builders::*;
use bough::raw::Config;
use bough::typed::App;

const ROW: f32 = 24.0;
const HEAD: f32 = 32.0;
const ROWS: usize = 8;

struct Item {
    id: u32,
    label: String,
    done: bool,
    hot: bool,
    clicks: u32,
}
impl Item {
    fn toggle(&mut self) {
        self.done = !self.done;
        self.clicks += 1;
    }
    fn hover(&mut self, e: &Ev) {
        self.hot = e.on;
    }
}
impl View for Item {
    fn view(&self) -> impl Element<Self> + '_ {
        row((
            text(self.id).w(48.0),
            text(self.label.as_str()).grow(1.0),
            text(if self.done { "done" } else { "open" }),
        ))
        .gap(8.0)
        .px(8.0)
        .h(ROW)
        .bg(if self.hot {
            rgb(0xfde68a)
        } else if self.id % 2 == 0 {
            rgb(0xf3f4f6)
        } else {
            rgb(0xffffff)
        })
        .on_click(Item::toggle)
        .on_hover(Item::hover)
    }
}
struct Board {
    items: List<Item>,
    scroll: Signal<f32>,
    next: u32,
}
impl Board {
    fn new(n: usize) -> Board {
        let mut b = Board {
            items: List::new(),
            scroll: Signal::new(0.0),
            next: 0,
        };
        for _ in 0..n {
            let it = b.item();
            b.items.push(it);
        }
        b
    }
    fn item(&mut self) -> Item {
        self.next += 1;
        Item {
            id: self.next,
            label: format!("item {}", self.next % 37),
            done: false,
            hot: false,
            clicks: 0,
        }
    }
    fn insert(&mut self, at: usize) {
        let it = self.item();
        self.items.insert(at, it);
    }
}
impl View for Board {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            row((text("board"),)).h(HEAD).px(8.0).bg(rgb(0xe5e7eb)),
            list(sig!(self.items))
                .scroll(sig!(self.scroll))
                .estimate(ROW)
                .grow(1.0)
                .w_pct(1.0)
                .bg(rgb(0xffffff)),
        ))
        .w_pct(1.0)
        .h_pct(1.0)
        .bg(rgb(0xd1d5db))
    }
}
fn board(n: usize, ring: usize) -> App<Board> {
    let mut cfg = Config::new(
        320.0,
        HEAD + ROW * ROWS as f32,
        1.0,
        bough::ui::default_font(),
    );
    cfg.ring = ring;
    match App::new(Board::new(n), cfg) {
        Ok(a) => a,
        Err((_, why)) => panic!("not admitted: {why:?}"),
    }
}
/// The middle of row `r` of the window.
fn at(r: usize) -> (f32, f32) {
    (160.0, HEAD + ROW * r as f32 + ROW / 2.0)
}
fn rows_of(app: &mut App<Board>, what: &str, f: impl FnOnce(&mut Board)) -> u32 {
    app.turn(f);
    let n = app.pass_stats().rows;
    app.check().unwrap_or_else(|e| panic!("{what}: {e}"));
    n
}

/// An oracle that shares nothing with the runtime: the application's own state says which item is at which
/// row and what colour its row is (hot, even, odd), and the presented pixels are sampled in each exposed row's
/// left padding, where nothing but the row's background is drawn. The header's colour is checked the same way.
fn model(app: &App<Board>) -> Result<(), String> {
    let px = app.presented_pixels();
    let (w, h) = (320usize, (HEAD + ROW * ROWS as f32) as usize);
    let at = |x: usize, y: usize| {
        let p = &px[(y * w + x) * 4..][..3];
        (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32
    };
    if at(3, 4) != 0xe5e7eb {
        return Err(format!("the header is {:06x}", at(3, 4)));
    }
    let b = app.view();
    let scroll = scroll_px(b);
    for r in 0..b.items.len() {
        let top = HEAD as i64 + r as i64 * ROW as i64 - scroll;
        let y = top + ROW as i64 / 2;
        if y < HEAD as i64 || y >= h as i64 {
            if top > h as i64 {
                break;
            }
            continue;
        }
        let it = b.items.get(r).unwrap();
        let want = if it.hot {
            0xfde68a
        } else if it.id % 2 == 0 {
            0xf3f4f6
        } else {
            0xffffff
        };
        let got = at(3, y as usize);
        if got != want {
            return Err(format!(
                "model: row {r} (item {}) shows {got:06x}, its state says {want:06x}",
                it.id
            ));
        }
    }
    Ok(())
}
/// The list's scroll offset in device pixels, as shown: the signal, kept within the content.
fn scroll_px(b: &Board) -> i64 {
    (*b.scroll).round() as i64
}

#[test]
fn a_row_is_described_when_it_enters_or_its_item_is_edited() {
    for ring in [1, 3] {
        let mut app = board(400, ring);
        app.check().unwrap();
        assert_eq!(
            rows_of(&mut app, "insert at 0", |b| b.insert(0)),
            1,
            "a front insert describes the row that entered"
        );
        assert_eq!(
            rows_of(&mut app, "remove at 0", |b| drop(b.items.remove(0))),
            1,
            "a front remove describes the row that entered at the bottom"
        );
        // inside the window: the rows of the smaller side take other slots and are described with the new row
        assert_eq!(
            rows_of(&mut app, "insert at 3", |b| b.insert(3)),
            4,
            "three rows above the insert, and the row inserted"
        );
        assert_eq!(
            rows_of(&mut app, "insert at 6", |b| b.insert(6)),
            2,
            "the row inserted and the one row after it that stays exposed"
        );
        assert_eq!(
            rows_of(&mut app, "remove at 1", |b| drop(b.items.remove(1))),
            2,
            "the one row above the removal, and the row that entered at the bottom"
        );
        assert_eq!(
            rows_of(&mut app, "edit 2", |b| b.items.edit(2).done = true),
            1,
            "an edited item's row"
        );
        assert_eq!(
            rows_of(&mut app, "edit 200", |b| b.items.edit(200).done = true),
            0,
            "an item that is not exposed"
        );
        assert_eq!(
            rows_of(&mut app, "insert at 300", |b| b.insert(300)),
            0,
            "an insert past the window"
        );
        assert_eq!(
            rows_of(&mut app, "reverse", |b| b.items.reverse()),
            ROWS as u32,
            "a permutation: every exposed row is another row"
        );
        // scrolled: an insert before the window moves every exposed row and describes the one that entered
        app.turn(|b| {
            b.scroll.set(ROW * 100.0);
        });
        app.check().unwrap();
        assert_eq!(
            rows_of(&mut app, "insert at 0, scrolled", |b| b.insert(0)),
            1
        );
        assert_eq!(
            rows_of(&mut app, "remove at 50, scrolled", |b| drop(
                b.items.remove(50)
            )),
            1
        );
        // two edits in one turn, one of them inside the window: every exposed row, which is always right
        let n = rows_of(&mut app, "two edits", |b| {
            b.insert(103);
            b.insert(0);
        });
        assert!(n <= ROWS as u32 + 1, "{n} rows described");
        // the list put in the field's place is another list: every row of it is another row
        assert_eq!(
            rows_of(&mut app, "replaced", |b| {
                let mut other = Board::new(300);
                b.items = std::mem::take(&mut other.items);
                b.scroll.set(0.0);
            }),
            ROWS as u32
        );
        assert_eq!(
            rows_of(&mut app, "insert after replace", |b| b.insert(0)),
            1
        );
    }
    let p = provenance::counts();
    assert_eq!((p.legacy(), p.describe), (0, 0));
}

#[test]
fn hover_and_press_follow_the_item_not_the_row() {
    let mut app = board(100, 1);
    app.check().unwrap();
    let (x, y) = at(3);
    app.pointer_move(x, y);
    app.check().unwrap();
    let hot = |app: &App<Board>| -> Vec<u32> {
        app.view()
            .items
            .iter()
            .filter(|i| i.hot)
            .map(|i| i.id)
            .collect()
    };
    let id_at = |app: &App<Board>, r: usize| app.view().items.get(r).unwrap().id;
    let a = id_at(&app, 3);
    assert_eq!(hot(&app), [a], "the row under the pointer is entered");
    // content moves under a still pointer: the item that was entered hears its leave, the one now there its
    // enter (the review's stationary-pointer probe: [true, false] on the old row)
    app.turn(|b| b.insert(0));
    app.check().unwrap();
    let b = id_at(&app, 3);
    assert_ne!(a, b);
    assert_eq!(id_at(&app, 4), a, "the item moved down one row");
    assert_eq!(
        hot(&app),
        [b],
        "the item now under the pointer is the hovered one"
    );
    // a press on an item, the item moved away by an edit, the release where it was: no click, on anyone
    app.pointer_down(x, y, Button::Primary);
    app.turn(|b| b.insert(0));
    app.pointer_up(x, y, Button::Primary);
    app.check().unwrap();
    assert_eq!(
        app.view().items.iter().map(|i| i.clicks).sum::<u32>(),
        0,
        "a press on one item released on another is not a click"
    );
    // and a plain click reaches the item under the pointer
    let c = id_at(&app, 3);
    app.pointer_down(x, y, Button::Primary);
    app.pointer_up(x, y, Button::Primary);
    app.check().unwrap();
    let clicked: Vec<u32> = app
        .view()
        .items
        .iter()
        .filter(|i| i.clicks > 0)
        .map(|i| i.id)
        .collect();
    assert_eq!(clicked, [c]);
    // the item under the pointer is removed: it is no longer hovered (it is gone), the next one is
    app.turn(|b| drop(b.items.remove(3)));
    app.check().unwrap();
    assert_eq!(hot(&app), [id_at(&app, 3)]);
}

#[test]
fn random_edits_hold_the_oracles_at_every_ring_age_and_at_100_000_rows() {
    for (n, ring) in [
        (40usize, 1usize),
        (40, 2),
        (1000, 3),
        (1000, 4),
        (100_000, 2),
    ] {
        let mut app = board(n, ring);
        app.check().unwrap();
        let mut seed = 0x9e37_79b9_7f4a_7c15u64 ^ (n as u64) << 8 ^ ring as u64;
        let mut r = move |m: u64| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) % m
        };
        for step in 0..160 {
            let len = app.view().items.len();
            let top = (*app.view().scroll / ROW) as usize;
            // edits near the window as often as far from it
            let near = |r: &mut dyn FnMut(u64) -> u64, len: usize| {
                if len == 0 {
                    0
                } else if r(2) == 0 {
                    (top + r(ROWS as u64 + 4) as usize)
                        .saturating_sub(2)
                        .min(len - 1)
                } else {
                    r(len as u64) as usize
                }
            };
            let what = r(12);
            match what {
                0..=2 => {
                    let at = near(&mut r, len + 1).min(len);
                    app.turn(|b| b.insert(at));
                }
                3..=4 if len > 0 => {
                    let at = near(&mut r, len);
                    app.turn(|b| drop(b.items.remove(at)));
                }
                5 if len > 0 => {
                    let at = near(&mut r, len);
                    app.turn(|b| b.items.edit(at).done ^= true);
                }
                6 => {
                    let max = (len as f32 * ROW - ROW * ROWS as f32).max(0.0);
                    let to = match r(4) {
                        0 => 0.0,
                        1 => max,
                        _ => (r(len as u64 + 1) as f32 * ROW).min(max),
                    };
                    app.turn(|b| {
                        b.scroll.set(to);
                    });
                }
                7 => app.wheel(160.0, 100.0, 0.0, r(7) as f32 - 3.0),
                8 => {
                    let (x, y) = at(r(ROWS as u64) as usize);
                    app.pointer_move(x, y);
                }
                9 => {
                    let (x, y) = at(r(ROWS as u64) as usize);
                    app.pointer_down(x, y, Button::Primary);
                    app.pointer_up(x, y, Button::Primary);
                }
                10 if len > 1 && n <= 1000 => {
                    let (a, b) = (near(&mut r, len), near(&mut r, len));
                    app.turn(|bd| bd.items.swap(a, b));
                }
                _ => {
                    // several edits in one turn
                    let k = 1 + r(3) as usize;
                    app.turn(|b| {
                        for i in 0..k {
                            let len = b.items.len();
                            if i % 2 == 0 || len == 0 {
                                b.insert((top + i).min(len));
                            } else {
                                b.items.remove(top.min(len - 1));
                            }
                        }
                    });
                }
            }
            app.check()
                .unwrap_or_else(|e| panic!("{n} rows, ring {ring}, step {step} (op {what}): {e}"));
            model(&app)
                .unwrap_or_else(|e| panic!("{n} rows, ring {ring}, step {step} (op {what}): {e}"));
        }
        // what exists is what is exposed, whatever the list holds
        let (records, pages, bytes) = app.world_stats();
        assert!(
            records <= 64 && pages <= 12,
            "{n} rows: {records} live records in {pages} pages ({bytes} bytes)"
        );
    }
    let p = provenance::counts();
    assert_eq!((p.legacy(), p.describe), (0, 0));
}
