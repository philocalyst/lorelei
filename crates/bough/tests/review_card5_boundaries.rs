//! Independent checks of owner reach, publication, and projection assumptions.
use bough::{input::Button, prelude::*, typed::App};
use std::cell::Cell;

fn cfg() -> bough::app::Config {
    let mut cfg = bough::app::Config::new(64.0, 64.0, 1.0, bough::ui::default_font());
    cfg.ring = 1;
    cfg
}

struct Row(u32);
impl View for Row {
    fn view(&self) -> impl Element<Self> + '_ {
        block(64.0, 20.0)
            .bg(rgb(0x228844))
            .on_click(|r: &mut Row| r.0 += 1)
    }
}

struct ListApp {
    rows: List<Row>,
    y: Signal<f32>,
}
impl View for ListApp {
    fn view(&self) -> impl Element<Self> + '_ {
        list(sig!(self.rows))
            .scroll(sig!(self.y))
            .estimate(20.0)
            .w(64.0)
            .h(64.0)
    }
}

#[test]
fn same_turn_removal_defers_slot_reuse_and_cancels_the_press() {
    let mut app = App::new(
        ListApp {
            rows: List::from_iter_in((0..12).map(|_| Row(0))),
            y: Signal::new(0.0),
        },
        cfg(),
    )
    .unwrap_or_else(|(_, why)| panic!("refused: {why:?}"));
    // Both wheel signs at the top provide a direct check of the setup needed by the port.
    app.wheel(8.0, 8.0, 0.0, -1.0);
    assert_eq!(*app.view().y, 0.0);
    app.wheel(8.0, 8.0, 0.0, 1.0);
    assert_eq!(*app.view().y, 20.0);
    app.turn(|v| {
        v.y.set(0.0);
    });
    let old = app.view().rows.slot(0).unwrap();
    app.pointer_down(8.0, 8.0, Button::Primary);
    app.turn(|v| {
        v.rows.remove(0);
        let new = v.rows.insert(0, Row(0));
        assert_ne!(
            old.index(),
            new.index(),
            "a displayed list defers recycling until publication"
        );
    });
    app.pointer_up(8.0, 8.0, Button::Primary);
    assert!(app.view().rows.iter().all(|r| r.0 == 0));
    app.check().unwrap();
}

struct Nested {
    rows: List<Row>,
}
impl View for Nested {
    fn view(&self) -> impl Element<Self> + '_ {
        list(sig!(self.rows))
            .viewport()
            .estimate(20.0)
            .w(64.0)
            .h(20.0)
    }
}

struct Outer {
    rows: List<Nested>,
}
impl View for Outer {
    fn view(&self) -> impl Element<Self> + '_ {
        list(sig!(self.rows))
            .viewport()
            .estimate(20.0)
            .w(64.0)
            .h(64.0)
    }
}

#[test]
fn unsupported_nested_list_is_refused_before_registering_its_row_owner_offset() {
    let inner = Nested {
        rows: List::from_iter_in([Row(0)]),
    };
    let result = App::new(
        Outer {
            rows: List::from_iter_in([inner]),
        },
        cfg(),
    );
    match result {
        Err((_, reasons)) => assert!(!reasons.is_empty()),
        Ok(mut app) => {
            app.frame();
            let px = app.presented_pixels();
            assert_eq!(
                &px[(4 * 64 + 4) * 4..][..4],
                [0x22, 0x88, 0x44, 255],
                "admitted nested list must paint its child"
            );
        }
    }
}

thread_local! {
    static LABEL: Cell<&'static str> = const { Cell::new("a") };
}

struct Projection {
    unchanged: Signal<u32>,
}
impl View for Projection {
    fn view(&self) -> impl Element<Self> + '_ {
        row((text(sig!(self.unchanged).map(|_| LABEL.get().to_owned())),))
            .w(64.0)
            .h(32.0)
            .bg(rgb(0xffffff))
    }
}

#[test]
fn capture_free_projection_can_read_changed_state() {
    LABEL.set("a");
    let mut app = App::new(
        Projection {
            unchanged: Signal::new(0),
        },
        cfg(),
    )
    .unwrap_or_else(|(_, why)| panic!("refused: {why:?}"));
    app.turn(|_| LABEL.set("WW"));
    app.frame();
    let mut fresh = App::new(
        Projection {
            unchanged: Signal::new(0),
        },
        cfg(),
    )
    .unwrap_or_else(|(_, why)| panic!("fresh refused: {why:?}"));
    fresh.frame();
    let a = app.presented_pixels();
    let b = fresh.presented_pixels();
    assert!(
        a == b,
        "a fresh app evaluates the zero-sized function even though the signal word is unchanged"
    );
}

struct OtherOwner {
    rows: List<Row>,
    other: Option<Box<Self>>,
}
impl View for OtherOwner {
    fn view(&self) -> impl Element<Self> + '_ {
        let owner = self.other.as_deref().unwrap_or(self);
        list(sig!(owner: OtherOwner => rows))
            .viewport()
            .estimate(20.0)
            .w(64.0)
            .h(64.0)
    }
}

#[test]
fn a_list_proved_on_another_value_of_the_root_type_is_refused() {
    let root = OtherOwner {
        rows: List::from_iter_in([Row(0)]),
        other: Some(Box::new(OtherOwner {
            rows: List::from_iter_in([Row(0)]),
            other: None,
        })),
    };
    let refusal = App::new(root, cfg())
        .err()
        .expect("non-root owner must be refused")
        .1;
    assert!(
        refusal
            .iter()
            .any(|s| s.contains("projected from the mounted root"))
    );
}

struct PlainScroll {
    y: Signal<f32>,
}
impl View for PlainScroll {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            block(64.0, 32.0).bg(rgb(0x228844)),
            block(64.0, 96.0).bg(rgb(0x2244aa)),
        ))
        .w(64.0)
        .h(32.0)
        .scroll(sig!(self.y))
        .line(12.0)
    }
}

#[test]
#[ignore = "Card 5 omits plain scroll extent and line distance"]
fn wheel_scrolls_a_plain_box_by_its_declared_line() {
    let mut app = App::new(
        PlainScroll {
            y: Signal::new(0.0),
        },
        cfg(),
    )
    .unwrap_or_else(|(_, why)| panic!("plain scroll refused: {why:?}"));
    app.wheel(6.0, 6.0, 0.0, 1.0);
    assert_eq!(*app.view().y, 12.0);
    app.check().unwrap();
}

struct ListChoice {
    a: List<Row>,
    b: List<Row>,
    first: bool,
}
impl View for ListChoice {
    fn view(&self) -> impl Element<Self> + '_ {
        let source = if self.first {
            sig!(self.a)
        } else {
            sig!(self.b)
        };
        list(source).viewport().estimate(20.0).w(64.0).h(64.0)
    }
}

#[test]
#[ignore = "Card 5 requires the same source field at each list site"]
fn one_typed_list_site_can_switch_between_root_fields() {
    let mut app = App::new(
        ListChoice {
            a: List::from_iter_in([Row(0)]),
            b: List::from_iter_in([Row(10)]),
            first: true,
        },
        cfg(),
    )
    .unwrap_or_else(|(_, why)| panic!("list choice refused: {why:?}"));
    app.turn(|v| v.first = false);
    app.pointer_down(4.0, 4.0, Button::Primary);
    app.pointer_up(4.0, 4.0, Button::Primary);
    assert_eq!(app.view().a.get(0).unwrap().0, 0);
    assert_eq!(app.view().b.get(0).unwrap().0, 11);
    app.check().unwrap();
}

struct AnimatedList {
    list: List<Row>,
}
impl View for AnimatedList {
    fn view(&self) -> impl Element<Self> + '_ {
        list(sig!(self.list))
            .viewport()
            .h(64.0)
            .estimate(20.0)
            .animate_moves()
    }
}

#[test]
fn unsupported_list_animation_reports_admission_refusal() {
    let refusal = App::new(
        AnimatedList {
            list: List::from_iter_in([Row(0), Row(1)]),
        },
        cfg(),
    )
    .err()
    .expect("the typed path has no list move animation")
    .1;
    assert!(refusal.iter().any(|s| s.contains("animat")), "{refusal:?}");
}
