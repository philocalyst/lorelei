//! Integration Card 3: the documented boundaries of the lowered runtime, as probes.
//!
//! Each probe pins what the lowered runtime does today with a `bough::prelude` construct it does not lower yet.
//! A refusal is by name and whole (the application does not start half-drawn); nothing is silently dropped.
//! When a construct is lowered, its probe fails and is replaced by a gate. The list is in the lane report.
use bough::lower::App;
use bough::prelude::*;
use bough::raw::Config;

fn cfg() -> Config {
    Config::new(320.0, 200.0, 1.0, bough::ui::default_font())
}
fn refusal<V: View>(v: V) -> String {
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| App::new(v, cfg())));
    match r {
        Ok(_) => String::new(),
        Err(e) => e.downcast_ref::<String>().cloned().unwrap_or_default(),
    }
}

struct Form {
    s: Signal<String>,
}
impl View for Form {
    fn view(&self) -> impl Element<Self> + '_ {
        div().p(8.0).child(text_input(sig!(self.s)))
    }
}
#[test]
fn boundary_text_input_is_refused_by_name() {
    let msg = refusal(Form {
        s: Signal::new("abc".into()),
    });
    assert!(
        msg.contains("does not lower these elements yet") && msg.contains("TextInput"),
        "{msg}"
    );
}

struct Tip;
impl View for Tip {
    fn view(&self) -> impl Element<Self> + '_ {
        div().p(8.0).child(tooltip(
            button("hover me"),
            "a tip",
            [0.0, 40.0, 80.0, 24.0],
        ))
    }
}
#[test]
fn boundary_tooltip_is_refused_by_name() {
    let msg = refusal(Tip);
    assert!(msg.contains("Tooltip"), "{msg}");
}

struct Fade {
    on: Signal<bool>,
}
impl View for Fade {
    fn view(&self) -> impl Element<Self> + '_ {
        div().p(8.0).child(
            block(40.0, 40.0).bg(sig!(self.on)
                .map(|on| if *on { rgb(0xff0000) } else { rgb(0x0000ff) })
                .transition(Curve::new(0.2, Ease::Linear))),
        )
    }
}
/// A transition's operand lowers to its target value: the paint is at the target in the turn that moves it,
/// with no frames in between. (The motion itself, as an expression of `Time` in the record, is the next slice.)
#[test]
fn boundary_a_transition_jumps_to_its_target() {
    let mut app = App::new(
        Fade {
            on: Signal::new(false),
        },
        cfg(),
    );
    app.check().unwrap();
    let before = app.presented_pixels();
    app.turn(|f| {
        f.on.set(true);
    });
    app.check().unwrap();
    let px = app.presented_pixels();
    let at = |p: &[u8], x: usize, y: usize| -> [u8; 4] {
        p[(y * 320 + x) * 4..][..4].try_into().unwrap()
    };
    assert_eq!(at(&before, 20, 20), [0, 0, 255, 255]);
    assert_eq!(at(&px, 20, 20), [255, 0, 0, 255], "the target, at once");
}
