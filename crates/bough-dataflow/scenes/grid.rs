//! A fixture for the bounded deltas (not an API): a scrolling grid of `rows` rows of 8 + 8 cells.
//!
//! The left eight cells of a row are *selectable*: one cell of the whole grid is the selected one (`Sel` holds
//! its id) and is drawn in another colour. The right eight *glow* with the pointer: a cell within 255 pixels
//! of it is tinted by the distance. Each half is written twice, chosen by a const parameter:
//!
//! * `DECLARED = true`: `sel(..)` and `near(..)`, the forms that declare what bounds their change;
//! * `DECLARED = false`: the same values as opaque Rust functions of the block words (what there was before).
//!
//! The two must paint the same pixels at every step (`tests/select.rs`), and the declared form must leave the
//! same damage as its own page scan.
#![allow(dead_code)]
use bough_dataflow::expr::{near, sel};
use bough_dataflow::prelude::*;

pub const CELLS: usize = 8;
pub const CELL_W: i32 = 64;
pub const ROW_H: i32 = 8;
pub const W: i32 = 2 * CELLS as i32 * CELL_W;
pub const NONE: u32 = u32::MAX;
pub const REACH: i32 = 255;

lens!(pub Scroll: i32 = 0);
lens!(pub Sel: u32 = 1);
lens!(pub Theme: u32 = 2);
pub const NWORDS: usize = 3;

pub struct Grid<const DECLARED: bool> {
    pub u: U,
    pub rows: u32,
    pub h: i32,
}

const HI: u32 = 0xff2f_6bff;
const FAR: u32 = 0xff00_0020;
fn lo(theme: u32) -> Color {
    Color(if theme != 0 { 0xff20_2428 } else { 0xfff4_f6f8 })
}
fn pick((sel, theme, id): (u32, u32, u32)) -> Color {
    if sel == id { Color(HI) } else { lo(theme) }
}
fn glow((x, y, at): (i32, i32, [i32; 2])) -> Color {
    let d = ((x as i64 - at[0] as i64).abs() + (y as i64 - at[1] as i64).abs()).min(255) as u32;
    Color(0xff00_0000 | (255 - d) << 8 | 0x20)
}

const SHAPE: [Rect; CELLS] = {
    let mut r = [Rect::new(0, 0, CELL_W, ROW_H); CELLS];
    let mut c = 0;
    while c < CELLS {
        r[c] = Rect::new(c as i32 * CELL_W, 1, CELL_W - 2, ROW_H - 2);
        c += 1;
    }
    r
};
const SHAPE2: [Rect; 2 * CELLS] = {
    let mut r = [Rect::new(0, 0, CELL_W, ROW_H); 2 * CELLS];
    let mut c = 0;
    while c < 2 * CELLS {
        r[c] = Rect::new(c as i32 * CELL_W, 1, CELL_W - 2, ROW_H - 2);
        c += 1;
    }
    r
};

impl<const DECLARED: bool> Grid<DECLARED> {
    pub fn new(rows: u32, h: i32) -> Self {
        let mut u = U::default();
        u.set::<Sel>(NONE);
        Grid { u, rows, h }
    }
    pub fn body(&self) -> Rect {
        Rect { x0: 0, y0: 0, x1: W, y1: self.h }
    }
    pub fn scroll_by(&mut self, dy: i32) {
        let max = (self.rows as i32 * ROW_H - self.h).max(0);
        let y = (self.u.get::<Scroll>() + dy).clamp(0, max);
        self.u.set::<Scroll>(y);
    }
    /// The id of cell `c` of row `r`.
    pub fn cell(r: u32, c: u32) -> u32 {
        r * CELLS as u32 + c
    }
    /// The centre of glowing cell `c` of row `r`, on screen at scroll 0 (the glow does not follow the scroll:
    /// a fixture).
    pub fn centre(r: usize, c: usize) -> [i32; 2] {
        [(CELLS + c) as i32 * CELL_W + CELL_W / 2, r as i32 * ROW_H + ROW_H / 2]
    }
}

impl<const DECLARED: bool> View for Grid<DECLARED> {
    fn block(&self) -> &U {
        &self.u
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        s.scope(Space { clip: self.body(), base: (0, 0), scroll: Scroll::SLOT as u8 }, |s| {
            let l = List { n: self.rows, row_h: ROW_H, x: 0, y: 0, shape: &SHAPE2, edits: &FIXED };
            if DECLARED {
                s.list(&l, |r| {
                    (
                        std::array::from_fn::<_, CELLS, _>(|c| Fill(sel::<Sel, _, _, _>(rec(Self::cell(r as u32, c as u32)), Kc::<HI>, un::<Theme>().map(lo)))),
                        std::array::from_fn::<_, CELLS, _>(|c| {
                            let at = Self::centre(r, c);
                            Fill(near::<REACH, _, _, _>(rec(at), (un::<PtrX>(), un::<PtrY>(), rec(at)).map(glow), Kc::<FAR>))
                        }),
                    )
                });
            } else {
                s.list(&l, |r| {
                    (
                        std::array::from_fn::<_, CELLS, _>(|c| Fill((un::<Sel>(), un::<Theme>(), rec(Self::cell(r as u32, c as u32))).map(pick))),
                        std::array::from_fn::<_, CELLS, _>(|c| Fill((un::<PtrX>(), un::<PtrY>(), rec(Self::centre(r, c))).map(glow))),
                    )
                });
            }
        });
    }
}
