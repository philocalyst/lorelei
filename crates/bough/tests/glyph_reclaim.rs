use bough::app::{App, Config};
use bough::backend::MemSurfaces;
use bough::builders::*;

const LABELS: usize = 120;
const SIZE: (u32, u32) = (240, 96);

struct Rows {
    shown: bool,
}

impl View for Rows {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            text("Pressure test"),
            each((0..if self.shown { LABELS } else { 0 }).map(|i| text(format!("Label {i}")))),
        ))
        .bg(rgb(0xf8f8f8))
        .size(SIZE.0 as f32, SIZE.1 as f32)
        .on_key(|v: &mut Self, _| v.shown = !v.shown)
    }
}

fn mount(font: &[u8], ring: usize) -> App<Rows> {
    let mut cfg = Config::new(SIZE.0 as f32, SIZE.1 as f32, 1.0, font.to_vec());
    cfg.caps.glyphs = 2048;
    cfg.ring = ring;
    let mut app = App::new(Rows { shown: true }, cfg, false);
    app.target = Some(Box::new(MemSurfaces::new(SIZE.0, SIZE.1, ring)));
    app.key_down(Key::Tab, Mods::default());
    assert!(app.frame());
    app
}

#[test]
fn input_can_reuse_detached_glyphs_before_an_idle_frame() {
    let font = bough::ui::default_font();
    for ring in 1..=4 {
        let mut app = mount(&font, ring);
        let fresh = app.presented_pixels();
        for step in 0..8 {
            app.key_down(Key::Char('c'), Mods::default());
            assert!(app.frame());
            let retained = app.presented_pixels();
            assert!(
                retained == app.reference_pixels(),
                "ring {ring}, step {step}"
            );
            if step % 2 == 0 {
                assert!(!app.owner.shown);
                assert!(retained != fresh);
            } else {
                assert!(app.owner.shown);
                assert!(retained == fresh);
            }
        }
    }
}
