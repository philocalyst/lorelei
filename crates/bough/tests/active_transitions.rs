use bough::app::{App, Config};
use bough::backend::MemSurfaces;
use bough::prelude::*;

struct Scene {
    x: Signal<f32>,
    other: Signal<f32>,
}

impl View for Scene {
    fn view(&self) -> impl Element<Self> + '_ {
        div().size(96.0, 96.0).bg(rgb(0x172736)).child((
            each((0..100).map(|_| {
                block(4.0, 4.0).abs(0.0, 0.0).bg(rgb(0xfe3070)).translate(
                    sig!(self.other)
                        .map(|v| (*v, 0.0))
                        .transition(Curve::out(0.1)),
                )
            })),
            block(20.0, 20.0)
                .abs(30.0, 30.0)
                .bg(rgb(0x6688de))
                .translate(
                    sig!(self.x)
                        .map(|v| (*v, 0.0))
                        .transition(Curve::smooth(0.12)),
                ),
        ))
    }
}

fn mount(x: f32, ring: usize) -> App<Scene> {
    let mut cfg = Config::new(96.0, 96.0, 1.0, bough::ui::default_font()).once();
    cfg.ring = ring;
    let mut app = App::new(
        Scene {
            x: Signal::new(x),
            other: Signal::new(0.0),
        },
        cfg,
        false,
    );
    app.target = Some(Box::new(MemSurfaces::new(96, 96, ring)));
    app.frame();
    app
}

#[test]
fn foreground_retargets_and_leaves_the_active_set_at_every_age() {
    for ring in 1..=4 {
        let mut app = mount(0.0, ring);
        let initial = app.presented_pixels();
        for target in [15.0, -10.0, 20.0] {
            app.turn(|s| {
                s.x.set(target);
            });
            for _ in 0..5 {
                let before = app.rt.transitions.steps;
                assert!(app.tick(1.0 / 120.0));
                assert_eq!(app.rt.transitions.steps, before + 1);
                app.frame();
                assert_eq!(app.presented_pixels(), app.reference_pixels());
            }
        }
        assert_ne!(initial, app.presented_pixels());
        app.tick(1.0);
        app.frame();
        assert!(!app.animating());
        assert_eq!(app.presented_pixels(), mount(20.0, ring).presented_pixels());
    }
}
