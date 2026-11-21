use bough::app::{App, Config};
use bough::backend::MemSurfaces;
use bough::prelude::*;

struct Card;

impl View for Card {
    fn view(&self) -> impl Element<Self> + '_ {
        div().size(64.0, 64.0).bg(rgb(0)).child(
            block(38.0, 36.0)
                .abs(2.0, 2.0)
                .rounded(6.0)
                .border(1.0, rgb(0xf0f0f0))
                .bg(rgb(0xff0000))
                .opacity(0.5)
                .hover_bg(rgb(0x0000ff)),
        )
    }
}

#[test]
fn hover_fill_keeps_its_opacity_on_enter_and_leave() {
    let font = bough::ui::default_font();
    for ring in 1..=4 {
        let mut cfg = Config::new(64.0, 64.0, 1.0, font.clone()).once();
        cfg.ring = ring;
        let mut app = App::new(Card, cfg, false);
        app.target = Some(Box::new(MemSurfaces::new(64, 64, ring)));
        assert!(app.frame());
        let initial = app.presented_pixels();
        for _ in 0..4 {
            app.pointer_move(17.0, 18.0);
            assert!(app.frame());
            let hover = app.presented_pixels();
            assert_eq!(hover, app.reference_pixels());
            let at = 4 * (18 * 64 + 17);
            assert_eq!(hover[at + 1], 0);
            assert_eq!(hover[at + 2], 0);
            assert!((126..=129).contains(&hover[at]), "50% blue over black");
            app.pointer_move(60.0, 60.0);
            assert!(app.frame());
            assert_eq!(app.presented_pixels(), initial);
        }
    }
}
