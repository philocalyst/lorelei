//! Ruling D3 as a twin test: one program, the legacy runtime and the typed path, the same steps; after every
//! step both show what the program's state says.
//!
//! The program has a handler that writes a bound signal *and* a plain field the view reads. The legacy
//! runtime describes its owner again after every handler, so both changes show in the same frame; that is the
//! façade's contract, and the typed path keeps it by describing too (as an update), never by skipping a
//! description because "only signals were written" (the hole Card 4's second report stated and D3 closed).
//! The oracle is the program itself: each block's colour is a function of the state, and both runtimes'
//! presented pixels are sampled at each block's centre. No pixel of one runtime is compared with the other's
//! (their rasters differ in text and edges; a block's centre does not).
use bough::input::Button;
use bough::prelude::*;
use bough::raw::Config;

struct Twin {
    on: Signal<bool>,
    hue: Signal<Color>,
    dark: bool,
    plain: u32,
}
const ON: Color = rgb(0xfacc15);
const OFF: Color = rgb(0x374151);
const DARK: Color = rgb(0x111827);
const LIGHT: Color = rgb(0xe5e7eb);
const STEPS: [Color; 3] = [rgb(0x2563eb), rgb(0x16a34a), rgb(0xdc2626)];
impl Twin {
    fn new() -> Twin {
        Twin {
            on: Signal::new(false),
            hue: Signal::new(rgb(0x7c3aed)),
            dark: false,
            plain: 0,
        }
    }
    /// A bound word and a plain field in one handler.
    fn both(&mut self) {
        let v = !*self.on;
        self.on.set(v);
        self.dark = !self.dark;
    }
    fn signal_only(&mut self) {
        let v = !*self.on;
        self.on.set(v);
    }
    fn plain_only(&mut self) {
        self.plain += 1;
    }
    /// A signal borrowed mutably and left as it was, and a plain field changed.
    fn same_value_and_plain(&mut self) {
        let v = *self.hue;
        *self.hue = v;
        self.plain += 1;
    }
    /// What each of the four blocks shows.
    fn model(&self) -> [Color; 4] {
        [
            if *self.on { ON } else { OFF },
            if self.dark { DARK } else { LIGHT },
            *self.hue,
            STEPS[self.plain as usize % 3],
        ]
    }
}
impl View for Twin {
    fn view(&self) -> impl Element<Self> + '_ {
        row((
            block(40.0, 40.0)
                .bg(sig!(self.on).map(|on| if *on { ON } else { OFF }))
                .on_click(Twin::both),
            block(40.0, 40.0).bg(if self.dark { DARK } else { LIGHT }),
            block(40.0, 40.0).bg(sig!(self.hue)),
            block(40.0, 40.0)
                .bg(STEPS[self.plain as usize % 3])
                .on_click(Twin::same_value_and_plain),
        ))
        .w_pct(1.0)
        .h_pct(1.0)
        .bg(rgb(0xffffff))
    }
}
const W: usize = 160;

/// The colours at the centres of the four blocks; `bgra`: the legacy surface's byte order.
fn shown(px: &[u8], bgra: bool) -> [Color; 4] {
    std::array::from_fn(|k| {
        let p = &px[(20 * W + 20 + 40 * k) * 4..][..3];
        let (r, g, b) = if bgra {
            (p[2], p[1], p[0])
        } else {
            (p[0], p[1], p[2])
        };
        rgb((r as u32) << 16 | (g as u32) << 8 | b as u32)
    })
}

#[test]
fn a_handler_that_writes_a_bound_word_and_a_plain_field_shows_both_on_both_runtimes() {
    let cfg = || Config::new(W as f32, 40.0, 1.0, bough::ui::default_font());
    let mut legacy = bough::raw::App::new(Twin::new(), cfg(), true);
    let mut typed = match bough::typed::App::new(Twin::new(), cfg()) {
        Ok(a) => a,
        Err((_, why)) => panic!("not admitted: {why:?}"),
    };
    let check =
        |legacy: &mut bough::raw::App<Twin>, typed: &mut bough::typed::App<Twin>, what: &str| {
            legacy.frame();
            legacy.wait_presented();
            typed.check().unwrap_or_else(|e| panic!("{what}: {e}"));
            let want = typed.view().model();
            assert_eq!(
                legacy.owner.model().map(|c| c.0),
                want.map(|c| c.0),
                "{what}: the two programs are in the same state"
            );
            assert_eq!(
                shown(&typed.presented_pixels(), false).map(|c| c.0),
                want.map(|c| c.0),
                "{what}: the typed path shows the state"
            );
            assert_eq!(
                shown(&legacy.presented_pixels(), true).map(|c| c.0),
                want.map(|c| c.0),
                "{what}: the legacy runtime shows the state"
            );
        };
    check(&mut legacy, &mut typed, "mount");
    let d0 = typed.describes();
    // the handler of D3, through the pointer on both
    for k in 0..3 {
        legacy.pointer_down(20.0, 20.0, Button::Primary);
        legacy.pointer_up(20.0, 20.0, Button::Primary);
        typed.pointer_down(20.0, 20.0, Button::Primary);
        typed.pointer_up(20.0, 20.0, Button::Primary);
        check(&mut legacy, &mut typed, &format!("click {k} on `both`"));
    }
    assert_eq!(
        typed.describes(),
        d0 + 3,
        "each handler's turn is described once on the typed path, as on the legacy runtime"
    );
    // the same through `turn`, and the neighbouring cases
    type Step = (&'static str, fn(&mut Twin));
    let steps: [Step; 7] = [
        ("both", Twin::both),
        ("signal only", Twin::signal_only),
        ("plain only", Twin::plain_only),
        ("same value and plain", Twin::same_value_and_plain),
        ("both", Twin::both),
        ("hue", |t| {
            t.hue.set(rgb(0x0ea5e9));
        }),
        ("plain only", Twin::plain_only),
    ];
    for (name, f) in steps {
        legacy.turn(f);
        typed.turn(f);
        check(&mut legacy, &mut typed, name);
    }
    // the fourth block's handler: a signal written to the value it has, and a plain field
    legacy.pointer_down(140.0, 20.0, Button::Primary);
    legacy.pointer_up(140.0, 20.0, Button::Primary);
    typed.pointer_down(140.0, 20.0, Button::Primary);
    typed.pointer_up(140.0, 20.0, Button::Primary);
    check(&mut legacy, &mut typed, "click on `same_value_and_plain`");
}
