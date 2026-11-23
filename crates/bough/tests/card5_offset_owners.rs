//! Independent probes for typed list owner offsets.
use bough::{builders::*, typed::App};

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
