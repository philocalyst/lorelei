//! The author surface (`designs/reactivity/final` 02–04; plan and card in
//! `docs/lanes/integration/2026-11-22-el-surface.md`): `El` views against their typed twins, ports, identity,
//! stale handles and routes.
//!
//! Negative control: `--features mutant_el_shape` keys every item alike, so a box swapped for a text at a fixed
//! position is updated as if it were the same element; `a_kind_change_remounts_its_position` fails.
use bough::prelude::*;
use bough::raw::{App, Button, Config};

fn font() -> Vec<u8> {
    std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap()
}
fn cfg() -> Config {
    Config::new(320.0, 200.0, 1.0, font())
}

fn shot<V: bough::app::View>(app: &mut App<V>) -> Vec<u8> {
    app.frame();
    let p = app.presented_pixels();
    assert_eq!(p, app.reference_pixels(), "the repaired frame equals a full redraw");
    p
}

fn click<V: bough::app::View>(app: &mut App<V>, x: f32, y: f32) {
    app.pointer_move(x, y);
    app.pointer_down(x, y, Button::Primary);
    app.pointer_up(x, y, Button::Primary);
}

// ------------------------------------------------------------------------------------------- parity

struct Counter {
    count: u32,
    radius: f32,
}
impl Counter {
    fn inc(&mut self) {
        self.count += 1;
    }
}
impl View for Counter {
    fn view(&self) -> El<'_> {
        column((
            row((text(format!("Count: {}", self.count)), button("+").on_click(Counter::inc))).gap(8).p(8),
            row((0..self.count.min(4)).map(|i| div().size(20, 20).bg(rgb(0x204060 + i * 0x101010)).rounded(self.radius))).gap(4),
        ))
        .bg(rgb(0xffffff))
        .w_pct(1.0)
        .h_pct(1.0)
    }
}

mod twin {
    use bough::builders::*;
    pub struct Counter {
        pub count: u32,
        pub radius: f32,
    }
    impl Counter {
        fn inc(&mut self) {
            self.count += 1;
        }
    }
    impl View for Counter {
        fn view(&self) -> impl Element<Self> + '_ {
            column((
                row((text(format!("Count: {}", self.count)), button("+").on_click(Counter::inc))).gap(8.0).p(8.0),
                row(each((0..self.count.min(4)).map(|i| row(()).w(20.0).h(20.0).bg(rgb(0x204060 + i * 0x101010)).rounded(self.radius)))).gap(4.0),
            ))
            .bg(rgb(0xffffff))
            .w_pct(1.0)
            .h_pct(1.0)
        }
    }
}

#[test]
fn an_el_view_presents_its_typed_twins_pixels() {
    let mut el = App::new(Counter { count: 0, radius: 4.0 }, cfg(), true);
    let mut ty = App::new(twin::Counter { count: 0, radius: 4.0 }, cfg(), true);
    assert_eq!(shot(&mut el), shot(&mut ty), "after mount");
    // the button sits right of the label: found on a throwaway twin, then clicked on both
    let x = {
        let mut probe = App::new(twin::Counter { count: 0, radius: 4.0 }, cfg(), false);
        (40..200).step_by(4).map(|x| x as f32).find(|&x| {
            click(&mut probe, x, 20.0);
            probe.owner.count > 0
        })
    }
    .expect("the twin's button is somewhere on its row");
    for _ in 0..3 {
        click(&mut el, x, 20.0);
        click(&mut ty, x, 20.0);
        assert_eq!(el.owner.count, ty.owner.count, "the click reached the same route");
        assert_eq!(shot(&mut el), shot(&mut ty), "after a click");
    }
    assert!(el.owner.count > 0, "the button was hit");
    // negative control: one radius apart, the pixels differ
    let mut off = App::new(Counter { count: el.owner.count, radius: 5.0 }, cfg(), true);
    assert_ne!(shot(&mut off), shot(&mut el), "the compare sees one radius");
}

// ------------------------------------------------------------------------------------------- ports

struct Card {
    name: Signal<String>,
    tint: Signal<Color>,
    fade: Signal<f32>,
    far: Vec<Signal<Color>>,
}
impl View for Card {
    fn view(&self) -> El<'_> {
        column((
            div().size(60, 20).bg(&self.tint),
            div().size(60, 20).bg(rgb(0x334455)).opacity(self.fade.map(|f| 1.0 - f)),
            div().size(60, 20).bg(&self.far[0]),
            text(&self.name),
            text(self.fade.map(|f| format!("{:.0}%", f * 100.0))),
        ))
        .bg(rgb(0xffffff))
    }
}

#[test]
fn a_signal_field_is_a_port_and_a_vec_element_is_a_constant() {
    let mut app = App::new(Card { name: Signal::new("before".into()), tint: Signal::new(rgb(0x102030)), fade: Signal::new(0.0), far: vec![Signal::new(rgb(0x708090))] }, cfg().once(), true);
    let p0 = shot(&mut app);
    app.turn(|c| {
        c.tint.set(rgb(0xff0000));
    });
    let p1 = shot(&mut app);
    assert_ne!(p0, p1, "&self.tint is a port");
    app.turn(|c| {
        c.fade.set(0.5);
    });
    let p2 = shot(&mut app);
    assert_ne!(p1, p2, "a mapped source is a port");
    app.turn(|c| {
        c.name.set("after".into());
    });
    let p3 = shot(&mut app);
    assert_ne!(p2, p3, "a borrowed string signal is a text port");
    app.turn(|c| {
        c.far[0].set(rgb(0xabcdef));
    });
    assert_eq!(p3, shot(&mut app), "a signal behind a Vec is not wired");
}

#[test]
fn ports_and_redescription_agree() {
    let card = || Card { name: Signal::new("a".into()), tint: Signal::new(rgb(0x102030)), fade: Signal::new(0.25), far: vec![Signal::new(rgb(0x708090))] };
    let mut wired = App::new(card(), cfg().once(), true);
    let mut redescribed = App::new(card(), cfg(), true);
    for k in 0..4u32 {
        let f = |c: &mut Card| {
            c.tint.set(rgb(0x100000 * (k + 1)));
            c.fade.set(k as f32 / 4.0);
            c.name.set(format!("n{k}"));
        };
        wired.turn(f);
        redescribed.turn(f);
        assert_eq!(shot(&mut wired), shot(&mut redescribed));
    }
}

struct Tally {
    count: Signal<u32>,
    delta: Signal<i64>,
}
impl View for Tally {
    fn view(&self) -> El<'_> {
        column((text(&self.count), text(&self.delta))).bg(rgb(0xffffff))
    }
}

/// `text(&self.count)`: an integer signal field is a text port (the zero-rerun counter label of the plan's
/// deferred list): under `once` publishing it changes the pixels, and they equal a fresh mount of the same
/// state, negative values included.
#[test]
fn an_integer_signal_is_a_text_port() {
    let tally = |c: u32, d: i64| Tally { count: Signal::new(c), delta: Signal::new(d) };
    let mut wired = App::new(tally(7, 0), cfg().once(), true);
    let mut redescribed = App::new(tally(7, 0), cfg(), true);
    let p0 = shot(&mut wired);
    for (c, d) in [(8u32, -3i64), (1234567, i64::MIN), (0, 42), (u32::MAX, -1)] {
        let f = |t: &mut Tally| {
            t.count.set(c);
            t.delta.set(d);
        };
        wired.turn(f);
        redescribed.turn(f);
        let p = shot(&mut wired);
        assert_ne!(p, p0, "the label follows the signal without a re-description");
        assert_eq!(p, shot(&mut redescribed), "port and re-description agree");
        let mut fresh = App::new(tally(c, d), cfg(), true);
        assert_eq!(p, shot(&mut fresh), "the patched label equals a fresh mount");
    }
}

// ------------------------------------------------------------------------------------------- identity

struct Swap {
    text_first: bool,
    n: usize,
    hits: u32,
}
impl Swap {
    fn hit(&mut self) {
        self.hits += 1;
    }
}
impl View for Swap {
    fn view(&self) -> El<'_> {
        let first = if self.text_first { text("a text where a box was") } else { div().size(80, 16).bg(rgb(0x885500)) };
        column((
            first,
            div().size(80, 20).bg(rgb(0x2266aa)).on_click(Swap::hit),
            (self.n > 0).then(|| text("shown")),
            column((0..self.n).map(|i| div().size(40 + 10 * i as u32, 6).bg(rgb(0x00aa00)))),
        ))
        .bg(rgb(0xffffff))
        .w_pct(1.0)
        .h_pct(1.0)
    }
}

#[test]
fn a_kind_change_remounts_its_position() {
    let mut app = App::new(Swap { text_first: false, n: 0, hits: 0 }, cfg(), true);
    shot(&mut app);
    for (k, n) in [(true, 2), (false, 3), (true, 0), (false, 1), (true, 4)] {
        app.turn(|s| {
            s.text_first = k;
            s.n = n;
        });
        let p = shot(&mut app);
        let mut fresh = App::new(Swap { text_first: k, n, hits: 0 }, cfg(), true);
        assert_eq!(p, shot(&mut fresh), "a re-described swap matches a fresh mount (text_first {k}, n {n})");
    }
}

#[test]
fn a_sibling_keeps_its_routes_across_a_kind_change() {
    let mut app = App::new(Swap { text_first: false, n: 0, hits: 0 }, cfg(), true);
    shot(&mut app);
    let y = |s: &Swap| if s.text_first { 20.0 + 10.0 } else { 16.0 + 10.0 };
    let at = y(&app.owner);
    click(&mut app, 40.0, at);
    app.turn(|s| s.text_first = true);
    shot(&mut app);
    let at = y(&app.owner);
    click(&mut app, 40.0, at);
    assert_eq!(app.owner.hits, 2, "the blue box answers before and after its sibling changed kind");
}

#[test]
#[should_panic(expected = "used after the description")]
fn a_kept_element_panics_by_name() {
    let kept: El<'static> = div();
    let mut app = App::new(Counter { count: 0, radius: 0.0 }, cfg(), false);
    app.turn(|c| c.count += 1);
    let _ = kept.p(1);
}

// ------------------------------------------------------------------------------------------- routes

struct Other;
impl Other {
    fn poke(&mut self) {}
}
struct Wrong;
impl View for Wrong {
    fn view(&self) -> El<'_> {
        div().size(10, 10).bg(rgb(0)).on_click(Other::poke)
    }
}

#[test]
#[should_panic(expected = "a route taking `&mut el_surface::Other` is mounted in a view of `el_surface::Wrong`")]
fn a_route_of_another_type_panics_at_mount_naming_both() {
    let _ = App::new(Wrong, cfg(), false);
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Axis {
    H,
    V,
}
struct Pane {
    splits: Vec<Axis>,
    escaped: u32,
}
impl Pane {
    fn split(&mut self, a: Axis) {
        self.splits.push(a);
    }
    fn escape(&mut self) {
        self.escaped += 1;
    }
}
impl View for Pane {
    fn view(&self) -> El<'_> {
        div()
            .size(100, 100)
            .bg(rgb(0xeeeeee))
            .on_key(Key::Backslash, Pane::split.with(Axis::V))
            .on_key(Key::Minus, Pane::split.with(Axis::H))
            .on_key(Key::Escape, Pane::escape)
    }
}

#[test]
fn keys_route_to_methods_and_bound_methods() {
    let mut app = App::new(Pane { splits: vec![], escaped: 0 }, cfg(), false);
    click(&mut app, 50.0, 50.0);
    let none = Mods::default();
    app.key_down(Key::Backslash, none);
    app.key_down(Key::Minus, none);
    app.key_down(Key::Escape, none);
    app.key_down(Key::Char('q'), none);
    assert_eq!(app.owner.splits, [Axis::V, Axis::H]);
    assert_eq!(app.owner.escaped, 1);
}

// ------------------------------------------------------------------------------------------- the ladder

#[path = "../examples/progressive.rs"]
mod progressive;

/// Where the twin's button is: the first x along its row a click lands on.
fn find_button<V: bough::app::View>(app: &mut App<V>, y: f32, hit: impl Fn(&V) -> bool) -> f32 {
    (20..300).step_by(4).map(|x| x as f32).find(|&x| {
        click(app, x, y);
        hit(&app.owner)
    })
    .expect("a button on the row")
}

#[test]
fn promotion_moves_no_pixels() {
    // rung 0 (a plain field, re-described) and rung 1 (a signal port) show the same thing after the same clicks;
    // rung 1 also works with no re-description at all
    let mut r0 = App::new(progressive::rung0::Counter { count: 0 }, cfg(), true);
    let mut r1 = App::new(progressive::rung1::Counter { count: Signal::new(0) }, cfg(), true);
    let mut once = App::new(progressive::rung1::Counter { count: Signal::new(0) }, cfg().once(), true);
    let x = find_button(&mut r0, 34.0, |c| c.count > 0);
    for a in [&mut r1] {
        click(a, x, 34.0);
    }
    click(&mut once, x, 34.0);
    assert_eq!((*r1.owner.count, *once.owner.count), (1, 1));
    let p = shot(&mut r0);
    assert_eq!(p, shot(&mut r1), "rung 0 and rung 1");
    assert_eq!(p, shot(&mut once), "rung 1 mounted once: the port alone");
    for _ in 0..2 {
        click(&mut r0, x, 34.0);
        click(&mut once, x, 34.0);
    }
    assert_eq!(shot(&mut r0), shot(&mut once));
}

#[test]
fn rows_take_bound_routes() {
    use progressive::rung2::{Board, Row};
    let mut app = App::new(Board { rows: vec![Row { label: "a".into(), count: 0 }] }, cfg(), true);
    shot(&mut app);
    app.turn(|b| {
        b.add();
        b.add();
    });
    shot(&mut app);
    // the third row's "+" bumps the third row (its route carries `.with(2)`)
    let y = 16.0 + 2.0 * (32.0 + 4.0) + 16.0;
    let hit = (100..300).step_by(4).map(|x| x as f32).find(|&x| {
        click(&mut app, x, y);
        app.owner.rows[2].count > 0
    });
    assert!(hit.is_some(), "a row's button");
    assert_eq!((app.owner.rows[0].count, app.owner.rows[1].count, app.owner.rows[2].count), (0, 0, 1));
    app.pointer_leave(); // the clicked button shows its hover style while the pointer is on it
    let p = shot(&mut app);
    let mut fresh = App::new(Board { rows: (0..3).map(|i| Row { label: if i == 0 { "a".into() } else { format!("row {i}") }, count: (i == 2) as u32 }).collect() }, cfg(), true);
    assert_eq!(p, shot(&mut fresh), "the re-described board matches a fresh mount");
}

// ------------------------------------------------------------------------------------------- drag vs click

struct Draggable {
    clicks: u32,
    inner: u32,
    dragged: f32,
    drags: u32,
}
impl Draggable {
    fn click(&mut self) {
        self.clicks += 1;
    }
    fn inner(&mut self) {
        self.inner += 1;
    }
    fn drag(&mut self, e: &Ev) {
        self.drags += 1;
        self.dragged = e.dx;
    }
}
impl View for Draggable {
    fn view(&self) -> El<'_> {
        column(row((text("row").w(80), div().size(20, 20).bg(rgb(0x2266aa)).on_click(Draggable::inner))).h(30).bg(rgb(0xddeeff)).on_click(Draggable::click).on_drag(Draggable::drag))
            .bg(rgb(0xffffff))
            .w_pct(1.0)
            .h_pct(1.0)
    }
}

/// A drag route arms at the press and starts after `DRAG_SLOP` points of travel: a press released in place on a
/// draggable element (or on a button inside it) is a click, and a real drag is a drag and no click.
/// Negative control: `--features mutant_drag_eager` (the drag captured the press at once) fails it.
#[test]
fn a_press_in_place_on_a_draggable_element_is_a_click() {
    let mut app = App::new(Draggable { clicks: 0, inner: 0, dragged: 0.0, drags: 0 }, cfg(), true);
    shot(&mut app);
    click(&mut app, 20.0, 15.0);
    assert_eq!((app.owner.clicks, app.owner.drags), (1, 0), "a press released in place is a click");
    click(&mut app, 90.0, 10.0);
    assert_eq!((app.owner.inner, app.owner.clicks), (1, 1), "a button inside a draggable row takes its click");
    app.pointer_move(20.0, 15.0);
    app.pointer_down(20.0, 15.0, Button::Primary);
    app.pointer_move(21.0, 15.0);
    assert_eq!(app.owner.drags, 0, "under the slop the press is not a drag yet");
    app.pointer_move(30.0, 15.0);
    app.pointer_up(30.0, 15.0, Button::Primary);
    assert!(app.owner.drags >= 2 && app.owner.dragged == 10.0, "a real drag runs the drag route with the travel since the press");
    assert_eq!(app.owner.clicks, 1, "a drag is no click");
    shot(&mut app);
}
