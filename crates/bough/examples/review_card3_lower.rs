#[path = "review_card3/common.rs"]
mod common;
use bough::lower::App;
use bough::builders::*;
use common::*;

struct Item(u32);
impl View for Item {
    fn view(&self) -> impl Element<Self> + '_ {
        block(100., ROW).bg(rgb(stripe(self.0))).hover_bg(rgb(HOT))
    }
}
struct V {
    signal: Signal<bool>,
    theme: Signal<bool>,
    y: Signal<f32>,
    rows: List<Item>,
    next: u32,
}
impl View for V {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            row((
                block(32., HEADER)
                    .bg(sig!(self.signal).map(|b| rgb(if *b { LEAF } else { 0x991122 }))),
                block(16., HEADER).bg(sig!(self.y).map(|y| rgb(scroll_indicator(*y)))),
            ))
            .w_pct(1.)
            .h(HEADER),
            list(sig!(self.rows))
                .scroll(sig!(self.y))
                .estimate(ROW)
                .overscan(0)
                .w_pct(1.)
                .h(H as f32 - HEADER),
        ))
        .w_pct(1.)
        .h_pct(1.)
        .bg(sig!(self.theme).map(|b| rgb(bg(*b))))
    }
}
struct Lower(App<V>);
impl Scene for Lower {
    type State = V;
    fn state(n: usize) -> V {
        let mut rows = List::new();
        for i in 0..n {
            rows.push(Item(i as u32));
        }
        V {
            signal: Signal::new(false),
            theme: Signal::new(false),
            y: Signal::new(0.),
            rows,
            next: n as u32,
        }
    }
    fn mount(state: V) -> Self {
        let mut cfg = bough::raw::Config::new(W as f32, H as f32, 1., bough::ui::default_font());
        cfg.ring = 1;
        Self(App::new(state, cfg))
    }
    fn op(&mut self, op: Op, k: usize) {
        match op {
            Op::Signal => self.0.turn(|v| {
                v.signal.set(!*v.signal);
            }),
            Op::Hover => self
                .0
                .pointer_move(10., HEADER + 10. + (k % 2) as f32 * ROW),
            Op::Scroll => self.0.turn(|v| {
                v.y.set((1 - k % 2) as f32 * ROW);
            }),
            Op::Insert => self.0.turn(|v| {
                v.rows.insert(0, Item(v.next));
                v.next += 1;
            }),
            Op::Theme => self.0.turn(|v| {
                v.theme.set(!*v.theme);
            }),
            Op::Idle => (),
        }
    }
    fn paint(&mut self) -> Vec<u32> {
        self.0.frame();
        self.0
            .presented_pixels()
            .chunks_exact(4)
            .map(|p| u32::from(p[0]) << 16 | u32::from(p[1]) << 8 | u32::from(p[2]))
            .collect()
    }
    fn records(&self) -> usize {
        self.0.world_stats().0
    }
}
fn main() {
    run::<Lower>();
}
