//! The clipped column walk (`ColRef::each_in`): a painter cut to a rectangle visits the records whose rects
//! meet it, and what it paints is what the unclipped walk paints there.
//!
//! * every repair of a retained frame equals a full redraw, and the full redraw equals painting while
//!   describing (`Immediate`, which walks no column), over a script of scrolls, pointer moves, edits and
//!   theme changes on the sheet fixture;
//! * the records a one-tile repair visits do not grow with the rows exposed.
//!
//! `--features mutant_each_in` must fail this file.
#[path = "../scenes/sheet.rs"]
mod sheet;
use bough_dataflow::prelude::*;
use bough_dataflow::soft::{Immediate, Soft};

struct Rng(u64);
impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n
    }
}

/// Rows of one fill each, so a tile's worth of damage meets a known number of records.
struct Bars(U, u32);
impl View for Bars {
    fn block(&self) -> &U {
        &self.0
    }
    fn view<S: Sink<Self>>(&self, sink: &mut S) {
        sink.list(
            &List {
                n: self.1,
                row_h: 4,
                x: 0,
                y: 0,
                shape: &[Rect::new(0, 0, 64, 3)],
                edits: &FIXED,
            },
            |row| Fill(rec(Color(0xff00_0000 | (row as u32 * 0x01_0305)))),
        );
    }
}

#[test]
fn a_one_tile_repair_visits_the_records_under_it() {
    let mut visited = Vec::new();
    for rows in [64, 512, 4096] {
        let h = rows * 4;
        let view = Bars(U::default(), rows as u32);
        let mut world = World::<_, Soft>::new(64, h, 0);
        world.mount(&view);
        let mut frame = Soft::new(64, h);
        world.paint(&mut frame);
        let mut full = Soft::new(64, h);
        view.view(&mut Immediate::new(&mut full, &world.shown));
        assert!(frame.px == full.px, "rows {rows}: stores != immediate");
        world.tiles.clear();
        world.tiles.mark(Rect::new(0, 96, 32, 32));
        frame.records = 0;
        frame.repair(&world.tiles, |s| world.paint(s));
        assert!(
            frame.px == full.px,
            "rows {rows}: the repair changed pixels"
        );
        visited.push(frame.records);
    }
    assert!(
        visited.iter().all(|&v| v == visited[0]) && visited[0] <= 9,
        "records visited by a 32 px repair at 64, 512, 4096 rows: {visited:?}"
    );
}

#[test]
fn every_repair_equals_a_full_redraw() {
    let (w, h) = (sheet::W, sheet::H);
    let mut v = sheet::Sheet::new(2000);
    let mut world = World::<_, Soft>::new(w, h, sheet::NWORDS);
    world.mount(&v);
    let mut retained = Soft::new(w, h);
    retained.repair(&world.tiles, |s| world.paint(s));
    world.tiles.clear();
    let mut rng = Rng(0xeac4);
    for step in 0..120 {
        match rng.below(5) {
            0 => {
                v.scroll_by([44, -44, 7, 900, -900][rng.below(5)]);
                world.settle(&v);
            }
            1 => {
                let (r, c) = (rng.below(2000), rng.below(sheet::COLS));
                v.edit(r, c, rng.below(100_000) as f32);
                world.settle(&v);
            }
            2 if step % 9 == 0 => {
                v.toggle_theme();
                world.settle(&v);
            }
            _ => world.pointer_words(rng.below(w as usize) as i32, rng.below(h as usize) as i32),
        }
        retained.repair(&world.tiles, |s| world.paint(s));
        world.tiles.clear();
        let mut full = Soft::new(w, h);
        world.paint(&mut full);
        assert!(
            retained.px == full.px,
            "step {step}: retained != full redraw"
        );
        let mut now = Soft::new(w, h);
        v.view(&mut Immediate::new(&mut now, &world.shown));
        assert!(full.px == now.px, "step {step}: stores != immediate");
    }
}
