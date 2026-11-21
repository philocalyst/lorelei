//! Hand-placed typed sink for the round 2 opaque rectangle comparison.
#[path = "review_card3/common.rs"]
mod common;
use bough_x::{
    expr::{Color, Lens, U, U_SCALE, rec, sel, un},
    kinds::Fill,
    rel::Seq,
    sink::{Journal, List, Sink, View},
    soft::Soft,
    store::{POINTS, Rect, Space},
    world::World,
};
use common::*;

macro_rules! lens {
    ($name:ident, $slot:expr, $value:ty) => {
        #[derive(Clone, Copy)]
        struct $name;
        impl Lens for $name {
            type T = $value;
            const SLOT: usize = $slot;
        }
    };
}
lens!(LeafColor, 0, Color);
lens!(Theme, 1, Color);
lens!(Hover, 2, u32);
lens!(Scroll, 3, u32);

const PORT: Rect = Rect::new(0, HEADER as i32, W, H - HEADER as i32);
const ROW_RECT: [Rect; 1] = [Rect::new(0, 0, 100, ROW as i32)];
struct V {
    block: U,
    rows: Seq<u32>,
    next: u32,
}
impl View for V {
    fn block(&self) -> &U {
        &self.block
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        s.row(|| {
            (
                [
                    Rect::new(0, 0, W, H),
                    Rect::new(0, 0, 32, HEADER as i32),
                    Rect::new(32, 0, 16, HEADER as i32),
                ],
                (
                    Fill(un::<Theme>()),
                    Fill(un::<LeafColor>()),
                    Fill(sel::<Scroll, _, _, _>(
                        rec(0u32),
                        Color(scroll_indicator(0.0)),
                        Color(scroll_indicator(ROW)),
                    )),
                ),
            )
        });
        s.scope(
            Space {
                clip: PORT,
                base: (0, HEADER as i32),
                scroll: POINTS | Scroll::SLOT as u8,
            },
            |s| {
                let edits: &Journal = &self.rows.journal;
                s.list(
                    &List {
                        n: self.rows.len() as u32,
                        row_h: ROW as i32,
                        x: 0,
                        y: 0,
                        shape: &ROW_RECT,
                        edits,
                    },
                    |index| {
                        let key = *self.rows.get(index);
                        Fill(sel::<Hover, _, _, _>(
                            rec(key),
                            Color(HOT),
                            Color(stripe(key)),
                        ))
                    },
                );
            },
        );
    }
}
struct Hand {
    view: V,
    world: World<V, Soft>,
    pointer: Option<f32>,
}
impl Hand {
    fn pick(&mut self) {
        if let Some(y) = self.pointer {
            let offset = f32::from_bits(self.view.block.w[Scroll::SLOT]);
            let position = ((offset + y - HEADER) / ROW) as usize;
            self.view.block.w[Hover::SLOT] =
                if y >= HEADER && y < H as f32 && position < self.view.rows.len() {
                    *self.view.rows.get(position)
                } else {
                    u32::MAX
                };
        }
    }
}
impl Scene for Hand {
    type State = V;
    fn state(n: usize) -> V {
        let mut block = U::default();
        block.w[LeafColor::SLOT] = 0x991122;
        block.w[Theme::SLOT] = bg(false);
        block.w[Hover::SLOT] = u32::MAX;
        block.w[U_SCALE] = 1.0f32.to_bits();
        V {
            block,
            rows: Seq::new((0..n as u32).collect()),
            next: n as u32,
        }
    }
    fn mount(view: V) -> Self {
        let mut world = World::new(W, H, 4);
        world.mount(&view);
        world.broad = 1 << Theme::SLOT;
        Self {
            view,
            world,
            pointer: None,
        }
    }
    fn op(&mut self, op: Op, k: usize) {
        match op {
            Op::Signal => self.view.block.w[LeafColor::SLOT] ^= LEAF ^ 0x991122,
            Op::Hover => {
                self.pointer = Some(HEADER + 10. + (k % 2) as f32 * ROW);
                self.pick();
            }
            Op::Scroll => {
                self.view.block.w[Scroll::SLOT] = ((1 - k % 2) as f32 * ROW).to_bits();
                self.pick();
            }
            Op::Insert => {
                self.view.rows.insert(0, self.view.next);
                self.view.next += 1;
                self.pick();
            }
            Op::Theme => self.view.block.w[Theme::SLOT] ^= bg(false) ^ bg(true),
            Op::Idle => return,
        }
        self.world.settle(&self.view);
    }
    fn paint(&mut self) -> Vec<u32> {
        let mut soft = Soft::new(W, H);
        self.world.paint(&mut soft);
        soft.px
    }
    fn records(&self) -> usize {
        self.world.root().stats().0
    }
}
fn main() {
    run::<Hand>();
}
