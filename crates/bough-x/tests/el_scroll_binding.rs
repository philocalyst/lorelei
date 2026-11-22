//! Independent Card 3 probe. A scroll-only signal is a dependency of its space.
use bough_x::{el::prelude::*, soft::Soft};

struct ScrollOnly {
    y: Signal<f32>,
}

impl View for ScrollOnly {
    fn view(&self) -> impl Element<Self> + '_ {
        let rows =
            each((0..8).map(|row| {
                block(24., 20.).bg(rgb(if row % 2 == 0 { 0xee3311 } else { 0x1144cc }))
            }));
        column(rows).w(24.).h(40.).scroll(sig!(self.y))
    }
}

#[test]
fn a_signal_used_only_as_a_scroll_offset_moves_the_space() {
    let mut app = App::<_, Soft>::new(ScrollOnly { y: Signal::new(0.) }, 24, 40, 1.).once();
    let draw_first = |app: &App<ScrollOnly, Soft>| {
        let mut pixels = Soft::new(24, 40);
        app.world.paint(&mut pixels);
        pixels.px[0]
    };
    assert_eq!(draw_first(&app), 0xee3311);
    app.turn(|v| {
        v.y.set(20.);
    });
    assert_eq!(draw_first(&app), 0x1144cc);
}
