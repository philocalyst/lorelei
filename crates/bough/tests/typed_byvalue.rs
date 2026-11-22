//! Integration Card 5: elements the update walk does not read where they are.
use bough::input::Button;
use bough::prelude::*;
use bough::raw::Config;
use bough::typed::App;

/// Elements the update walk does not read where they are: a plain scrolled box (its content is a row of a
/// child space) and a handler that captures a value (stored with its record). A view that holds one is
/// described by the conversion every time; what it shows is the same, under the same oracles.
struct ByValue {
    y: Signal<f32>,
    step: u32,
    n: u32,
}
impl View for ByValue {
    fn view(&self) -> impl Element<Self> + '_ {
        let step = self.step;
        column((
            row((
                button("add").on_click(move |v: &mut Self| v.n += step),
                text(self.n),
            ))
            .gap(8.0)
            .p(8.0),
            column((
                block(200.0, 120.0).bg(rgb(0x93c5fd)),
                block(200.0, 120.0).bg(rgb(0xfca5a5)),
                block(200.0, 120.0).bg(rgb(0x86efac)),
            ))
            .w(200.0)
            .h(160.0)
            .scroll(sig!(self.y)),
        ))
        .w_pct(1.0)
        .h_pct(1.0)
        .bg(rgb(0xf3f4f6))
    }
}
#[test]
fn a_view_read_by_value_is_described_by_the_conversion_and_shows_the_same() {
    for ring in [1, 3] {
        let mut cfg = Config::new(240.0, 220.0, 1.0, bough::ui::default_font());
        cfg.ring = ring;
        let mut app = match App::new(
            ByValue {
                y: Signal::new(0.0),
                step: 3,
                n: 0,
            },
            cfg,
        ) {
            Ok(a) => a,
            Err((_, why)) => panic!("not admitted: {why:?}"),
        };
        app.check().unwrap();
        for k in 0..6 {
            app.pointer_down(30.0, 20.0, Button::Primary);
            app.pointer_up(30.0, 20.0, Button::Primary);
            app.check()
                .unwrap_or_else(|e| panic!("ring {ring} click {k}: {e}"));
            // the captured step changes: the handler stored with the record is the new one
            app.turn(|v| v.step += 1);
            app.wheel(100.0, 120.0, 0.0, if k % 2 == 0 { 1.0 } else { -1.0 });
            app.check()
                .unwrap_or_else(|e| panic!("ring {ring} wheel {k}: {e}"));
        }
        assert_eq!(
            app.view().n,
            3 + 4 + 5 + 6 + 7 + 8,
            "each click added the step of its turn"
        );
        assert!(
            app.pass_stats().layouts <= 1,
            "the conversion solves the fixed part at most once a description"
        );
    }
}
