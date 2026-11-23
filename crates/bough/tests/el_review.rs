//! Regressions found by the review of the El surface (each failed at ba8fde8574): `El`s of a typed row mounted
//! outside a description (R1), a constant axis beside a wired one (R2), an image's layout (R3), key and click
//! dispatch on the lowered runtime (R4); and the routes that review asked to check: a bound value with padding,
//! routes on an image, a text input's layout.
use bough::input::{Button, Key, Mods};
use bough::raw::Config;

fn font() -> Vec<u8> {
    std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap()
}
fn cfg() -> Config {
    Config::new(320.0, 200.0, 1.0, font())
}
fn shot<V: bough::app::View>(app: &mut bough::raw::App<V>) -> Vec<u8> {
    app.frame();
    let p = app.presented_pixels();
    assert_eq!(p, app.reference_pixels(), "the repaired frame equals a full redraw");
    p
}
fn click<V: bough::app::View>(app: &mut bough::raw::App<V>, x: f32, y: f32) {
    app.pointer_move(x, y);
    app.pointer_down(x, y, Button::Primary);
    app.pointer_up(x, y, Button::Primary);
}

// ------------------------------------------------------------------------------------------------------------
// R1: two El values mounted by one typed element outside any outer `Walk` (a `list_with` row mounted by a
// `List::push` during `settle`): the first El's walk guard rewinds the arena, the second El is "stale".
mod r1 {
    use bough::builders::*;
    use bough::raw::{list_with, Mounter};
    pub struct Item {
        pub a: String,
    }
    pub struct Board {
        pub rows: List<Item>,
    }
    fn row(it: &Item, m: &mut Mounter<'_, Item>) {
        (bough::el::text(&it.a[..]), bough::el::text("second")).mount(m)
    }
    impl View for Board {
        fn view(&self) -> impl Element<Self> + '_ {
            div().child(list_with(bough::sig!(self.rows), row))
        }
    }
}

#[test]
fn r1_two_els_in_a_row_mounted_outside_a_walk() {
    let mut rows = bough::prelude::List::new();
    rows.push(r1::Item { a: "mounted at App::new (inside the outer walk): fine".into() });
    let mut app = bough::raw::App::new(r1::Board { rows }, cfg(), true);
    shot(&mut app);
    // a row mounted by the list's wire, during settle: no outer Walk
    app.turn(|b| {
        b.rows.push(r1::Item { a: "pushed".into() });
    });
    shot(&mut app);
}

// ------------------------------------------------------------------------------------------------------------
// R2: a wired width plus a constant height: a re-described height is ignored (LBox::update keeps cur.h).
mod r2 {
    use bough::prelude::*;
    pub struct Bar {
        pub width: Signal<f32>,
        pub tall: bool,
    }
    impl View for Bar {
        fn view(&self) -> El<'_> {
            column(div().w(&self.width).h(if self.tall { 80 } else { 20 }).bg(rgb(0x2266aa))).bg(rgb(0xffffff)).w_pct(1.0).h_pct(1.0)
        }
    }
}

#[test]
fn r2_constant_height_beside_a_wired_width_is_redescribed() {
    use bough::prelude::Signal;
    let mut app = bough::raw::App::new(r2::Bar { width: Signal::new(100.0), tall: false }, cfg(), true);
    shot(&mut app);
    app.turn(|b| b.tall = true);
    let p = shot(&mut app);
    let mut fresh = bough::raw::App::new(r2::Bar { width: Signal::new(100.0), tall: true }, cfg(), true);
    assert_eq!(p, shot(&mut fresh), "re-described height matches a fresh mount");
}

// ------------------------------------------------------------------------------------------------------------
// R3: an image's layout (`.size`) is not in its shape and its update is opaque: a re-described size is lost.
fn png_bytes() -> &'static [u8] {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, 4, 4);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut w = e.write_header().unwrap();
        w.write_image_data(&[200u8; 4 * 4 * 4]).unwrap();
    }
    Box::leak(out.into_boxed_slice())
}
mod r3 {
    use bough::prelude::*;
    pub struct Pic {
        pub png: &'static [u8],
        pub big: bool,
    }
    impl View for Pic {
        fn view(&self) -> El<'_> {
            let s = if self.big { 64 } else { 16 };
            column(image(self.png).size(s, s)).bg(rgb(0x000000)).w_pct(1.0).h_pct(1.0)
        }
    }
}

#[test]
fn r3_image_size_is_redescribed() {
    let png = png_bytes();
    let mut app = bough::raw::App::new(r3::Pic { png, big: false }, cfg(), true);
    shot(&mut app);
    app.turn(|p| p.big = true);
    let p = shot(&mut app);
    let mut fresh = bough::raw::App::new(r3::Pic { png, big: true }, cfg(), true);
    assert_eq!(p, shot(&mut fresh), "re-described image size matches a fresh mount");
}

// ------------------------------------------------------------------------------------------------------------
// R4: key routes: the kernel lets a key no route of the focused element names pass to its ancestors; the
// lowered runtime's per-key handler "runs" (and does nothing), so the key stops there.
mod r4 {
    use bough::prelude::*;
    #[derive(Default)]
    pub struct Form {
        pub submitted: u32,
        pub closed: u32,
        pub clicks: u32,
    }
    impl Form {
        fn submit(&mut self) {
            self.submitted += 1;
        }
        fn close(&mut self) {
            self.closed += 1;
        }
        fn a(&mut self) {
            self.clicks += 1;
        }
        fn b(&mut self) {
            self.clicks += 10;
        }
    }
    impl View for Form {
        fn view(&self) -> El<'_> {
            div()
                .size(300, 190)
                .bg(rgb(0xeeeeee))
                .on_key(Key::Enter, Form::submit)
                .child(div().size(100, 100).bg(rgb(0x3355aa)).on_key(Key::Escape, Form::close).on_click(Form::a).on_click(Form::b))
        }
    }
}

#[test]
fn r4_kernel_key_passes_to_ancestor() {
    let mut app = bough::raw::App::new(r4::Form::default(), cfg(), false);
    click(&mut app, 50.0, 50.0);
    app.key_down(Key::Enter, Mods::default());
    assert_eq!(app.owner.submitted, 1, "kernel: Enter reaches the outer element");
    assert_eq!(app.owner.clicks, 10, "kernel: only the last on_click runs");
}

#[test]
fn r4_lowered_key_passes_to_ancestor() {
    let mut app = bough::lower::App::new(r4::Form::default(), Config::new(320.0, 200.0, 1.0, font()));
    app.pointer_down(50.0, 50.0, Button::Primary);
    app.pointer_up(50.0, 50.0, Button::Primary);
    let clicks = app.view().clicks;
    app.key_down(Key::Enter, Mods::default());
    assert_eq!((clicks, app.view().submitted), (10, 1), "lowered: same as the kernel (clicks, submitted)");
}

// ------------------------------------------------------------------------------------------------------------
// R6: a bound value with padding (`(Axis, f32)`): the route works, and its rows are updated rather than compared
// through uninitialised bytes; a route on an image; a text input keeps its layout.
mod r6 {
    use bough::prelude::*;
    #[derive(Clone, Copy, PartialEq, Debug)]
    pub enum Axis {
        H,
        V,
    }
    pub struct Panes {
        pub splits: Vec<(Axis, f32)>,
        pub n: usize,
        pub opened: u32,
        pub png: &'static [u8],
        pub note: Signal<String>,
    }
    impl Panes {
        fn split(&mut self, a: (Axis, f32)) {
            self.splits.push(a);
        }
        fn open(&mut self) {
            self.opened += 1;
        }
    }
    impl View for Panes {
        fn view(&self) -> El<'_> {
            column((
                column((0..self.n).map(|i| div().size(80, 20).bg(rgb(0x2266aa)).on_click(Panes::split.with((if i % 2 == 0 { Axis::H } else { Axis::V }, i as f32 * 0.5))))),
                image(self.png).size(40, 40).on_click(Panes::open),
                text_input(&self.note).margin(10, 6, 0, 0),
            ))
            .bg(rgb(0xffffff))
            .w_pct(1.0)
            .h_pct(1.0)
        }
    }
}

#[test]
fn r6_padded_bound_values_images_and_inputs() {
    assert!(!bough::reflect::padding_free::<(r6::Axis, f32)>());
    let panes = |n| r6::Panes { splits: vec![], n, opened: 0, png: png_bytes(), note: bough::prelude::Signal::new("note".into()) };
    let mut app = bough::raw::App::new(panes(2), cfg(), true);
    shot(&mut app);
    click(&mut app, 40.0, 30.0);
    assert_eq!(app.owner.splits, [(r6::Axis::V, 0.5)], "the second row's bound value");
    app.turn(|p| p.n = 3);
    shot(&mut app);
    click(&mut app, 40.0, 50.0);
    assert_eq!(app.owner.splits[1], (r6::Axis::H, 1.0), "a row added later carries its own value");
    click(&mut app, 20.0, 80.0);
    assert_eq!(app.owner.opened, 1, "an image takes a route");
    app.pointer_leave();
    let p = shot(&mut app);
    let mut fresh = bough::raw::App::new(panes(3), cfg(), true);
    assert_eq!(p, shot(&mut fresh), "the re-described panes match a fresh mount (the input's margin included)");
    let mut no_margin = bough::raw::App::new(r7::Plain { note: bough::prelude::Signal::new("note".into()) }, cfg(), true);
    let _ = shot(&mut no_margin);
}

mod r7 {
    use bough::prelude::*;
    pub struct Plain {
        pub note: Signal<String>,
    }
    impl View for Plain {
        fn view(&self) -> El<'_> {
            column(text_input(&self.note)).bg(rgb(0xffffff)).w_pct(1.0).h_pct(1.0)
        }
    }
}
