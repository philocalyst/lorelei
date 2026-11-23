//! Independent replay of a mixed write and a scroll-only operand on the ordinary typed façade.
use bough::{lower, builders::*, typed};

fn config() -> bough::app::Config {
    let mut cfg = bough::app::Config::new(80., 40., 1., bough::ui::default_font());
    cfg.ring = 2;
    cfg
}
fn new<V: View>(v: V) -> typed::App<V> {
    typed::App::new(v, config()).unwrap_or_else(|(_, reason)| panic!("refused: {reason:?}"))
}

struct Mixed {
    color: Signal<Color>,
    width: f32,
}
impl Mixed {
    fn new() -> Self {
        Self {
            color: Signal::new(rgb(0x7788bb)),
            width: 20.,
        }
    }
    fn edit(&mut self) {
        self.color.set(rgb(0xaa3322));
        self.width = 60.;
    }
}
impl View for Mixed {
    fn view(&self) -> impl Element<Self> + '_ {
        column((block(self.width, 20.).bg(sig!(self.color)),))
            .w(80.)
            .h(40.)
    }
}
#[test]
fn ordinary_mixed_turn_publishes_the_signal_and_plain_layout() {
    let mut actual = new(Mixed::new());
    let mut expected = lower::App::new(Mixed::new(), config());
    actual.turn(Mixed::edit);
    expected.turn(Mixed::edit);
    assert_eq!(actual.view().width, 60.);
    actual.frame();
    let p = (10 * 80 + 30) * 4;
    assert_eq!(
        &actual.presented_pixels()[p..p + 4],
        &expected.reference_pixels()[p..p + 4]
    );
}

struct Scroll {
    offset: Signal<f32>,
}
impl View for Scroll {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            block(80., 48.).bg(rgb(0xbb3344)),
            block(80., 48.).bg(rgb(0x0033dd)),
            block(80., 48.).bg(rgb(0x448825)),
        ))
        .w(80.)
        .h(40.)
        .scroll(sig!(self.offset))
    }
}
#[test]
fn offset_is_observed_when_no_paint_reads_its_word() {
    let mut actual = new(Scroll {
        offset: Signal::new(0.),
    });
    let mut expected = lower::App::new(
        Scroll {
            offset: Signal::new(0.),
        },
        config(),
    );
    actual.turn(|v| {
        v.offset.set(48.);
    });
    expected.turn(|v| {
        v.offset.set(48.);
    });
    actual.frame();
    assert_eq!(
        &actual.presented_pixels()[..4],
        &expected.reference_pixels()[..4]
    );
}
