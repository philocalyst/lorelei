#[path = "review_card3/common.rs"]
mod common;
use bough_dataflow::{el::prelude::*, soft::Soft};
use common::*;

struct V {
    signal: Signal<bool>,
    theme: Signal<bool>,
    y: Signal<f32>,
    hover: Signal<u32>,
    rows: Vec<u32>,
    next: u32,
}
impl View for V {
    fn view(&self) -> impl Element<Self> + '_ {
        let rows = self.rows.iter().copied().map(|key| {
            block(100., ROW)
                .bg(sig!(self.hover).map(move |id| rgb(if *id == key { HOT } else { stripe(key) })))
                .on_hover(move |v: &mut V, e: &Ev| {
                    if e.on {
                        v.hover.set(key);
                    } else if *v.hover == key {
                        v.hover.set(u32::MAX);
                    }
                })
        });
        column((
            row((
                block(32., HEADER)
                    .bg(sig!(self.signal).map(|b| rgb(if *b { LEAF } else { 0x991122 }))),
                block(16., HEADER).bg(sig!(self.y).map(|y| rgb(scroll_indicator(*y)))),
            ))
            .w_pct(1.)
            .h(HEADER),
            column(each(rows))
                .w_pct(1.)
                .h(H as f32 - HEADER)
                .scroll(sig!(self.y)),
        ))
        .w_pct(1.)
        .h_pct(1.)
        .bg(sig!(self.theme).map(|b| rgb(bg(*b))))
    }
}
struct Typed(App<V, Soft>, Option<f32>);
impl Scene for Typed {
    type State = V;
    fn state(n: usize) -> V {
        V {
            signal: Signal::new(false),
            theme: Signal::new(false),
            y: Signal::new(0.),
            hover: Signal::new(u32::MAX),
            rows: (0..n as u32).collect(),
            next: n as u32,
        }
    }
    fn mount(state: V) -> Self {
        Self(App::new(state, W, H, 1.).once(), None)
    }
    fn op(&mut self, op: Op, k: usize) {
        match op {
            Op::Signal => self.0.turn(|v| {
                v.signal.set(!*v.signal);
            }),
            Op::Hover => {
                let y = HEADER + 10. + (k % 2) as f32 * ROW;
                self.1 = Some(y);
                self.0.pointer_move(10., y);
            }
            Op::Scroll | Op::Insert => {
                if matches!(op, Op::Scroll) {
                    self.0.turn(|v| {
                        v.y.set((1 - k % 2) as f32 * ROW);
                    });
                } else {
                    self.0.turn(|v| {
                        v.rows.insert(0, v.next);
                        v.next += 1;
                    });
                    // The existing typed element API exposes indexed `each`, over this opaque Vec.
                    // Structural edits explicitly request the general re-description operation.
                    self.0.world.redescribe(&self.0.v);
                }
                // Keep the hover image current for the paired visible-scene replay.
                if let Some(y) = self.1 {
                    let row = ((*self.0.v.y + y - HEADER) / ROW) as usize;
                    self.0.turn(|v| {
                        v.hover.set(v.rows[row]);
                    });
                }
            }
            Op::Theme => self.0.turn(|v| {
                v.theme.set(!*v.theme);
            }),
            Op::Idle => (),
        }
    }
    fn paint(&mut self) -> Vec<u32> {
        let mut soft = Soft::new(W, H);
        self.0.world.paint(&mut soft);
        soft.px
    }
    fn records(&self) -> usize {
        self.0.world.root().stats().0
    }
}
fn main() {
    run::<Typed>();
}
