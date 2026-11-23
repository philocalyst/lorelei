use bough::builders::*;

pub const WIDTH: f32 = 640.0;
pub const HEIGHT: f32 = 400.0;
pub const BACKGROUND: u32 = 0x142638;
const TILE: &[u8] = include_bytes!("tile.png");

#[derive(Clone, Copy, Default)]
pub struct State {
    pub theme: bool,
    pub motion: i8,
    pub foreground: f32,
    pub scroll: f32,
}

pub struct Scene {
    cards: usize,
    strip: bool,
    pub theme: Signal<bool>,
    pub motion: Signal<i8>,
    pub foreground: Signal<f32>,
    pub scroll: Signal<f32>,
}

impl Scene {
    pub fn new(cards: usize, strip: bool, s: State) -> Self {
        Self {
            cards,
            strip,
            theme: Signal::new(s.theme),
            motion: Signal::new(s.motion),
            foreground: Signal::new(s.foreground),
            scroll: Signal::new(s.scroll),
        }
    }

    pub fn state(&self) -> State {
        State {
            theme: *self.theme,
            motion: *self.motion,
            foreground: *self.foreground,
            scroll: *self.scroll,
        }
    }

    fn card(&self, i: usize) -> impl Element<Self> + '_ {
        let slot = if self.strip { i } else { i % 100 };
        let stagger = if self.strip {
            0.0
        } else {
            ((i / 100) % 8) as f32 * 0.25
        };
        let x = (slot % 10) as f32 * 64.0 + stagger;
        let y = (slot / 10) as f32 * 40.0 + stagger;
        let base = (i as u32).wrapping_mul(0x0077_9b83) & 0x00ff_ffff;
        let last = i + 1 == self.cards;
        let curve = Curve::smooth(0.26 + ((i / 100 + i) % 11) as f32 / 40.0);
        let distance = 5.0 + (i % 9) as f32;
        let lift = -((i % 3) as f32);
        let paint = move |dark: bool, motion: i8| {
            rgb(base
                ^ if dark { 0x00bfdf } else { 0x22b080 }
                ^ if motion > 0 { 0x403068 } else { 0 })
        };
        let card = || {
            block(60.0, 36.0)
                .abs(x, y)
                .rounded(6.0)
                .border(1.0, rgb(0x69849d))
        };
        // A foreground-only signal lets this trace distinguish a single changing card from fan-out.
        let foreground = last.then(|| {
            card()
                .bg(sig!(self.theme).map(move |dark| paint(*dark, 0)))
                .translate(
                    sig!(self.foreground)
                        .map(|x| (*x, 0.0))
                        .transition(Curve::smooth(0.3)),
                )
                .opacity(0.83)
                .hover_bg(rgb(0xf0b65d))
        });
        let normal = (!last).then(|| {
            card()
                .bg(sig!(self.theme).map(move |dark| paint(*dark, 0)))
                .translate(
                    sig!(self.motion)
                        .map(move |m| (*m as f32 * distance, lift * *m as f32))
                        .transition(curve),
                )
                .opacity(
                    sig!(self.motion)
                        .map(|m| 0.83 - 0.14 * (*m as f32).abs())
                        .transition(curve),
                )
                .w_op(
                    sig!(self.motion)
                        .map(|m| 60.0 - 5.0 * (*m as f32).abs())
                        .transition(curve),
                )
                .child(
                    block(12.0, 12.0).abs(8.0, 10.0).bg(sig!(self.motion)
                        .map(move |m| paint(false, *m))
                        .transition(curve)),
                )
                .hover_bg(rgb(0xf0b65d))
        });
        let media = i
            .is_multiple_of(16)
            .then(|| image(TILE).size(12.0, 12.0).abs(x + 34.0, y + 12.0));
        (normal, foreground, media)
    }
}

impl View for Scene {
    fn view(&self) -> impl Element<Self> + '_ {
        let cards = || each((0..self.cards).map(|i| self.card(i)));
        let root = || div().w_pct(1.0).h_pct(1.0).bg(rgb(BACKGROUND));
        (
            (!self.strip).then(|| root().child(cards())),
            self.strip.then(|| {
                root().scroll(sig!(self.scroll)).child(
                    div()
                        .size(WIDTH, self.cards.div_ceil(10) as f32 * 40.0)
                        .child(cards()),
                )
            }),
        )
    }
}
