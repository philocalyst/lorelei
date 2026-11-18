//! Card 1's sparse incidence and packed-place gates.
use bough_x::expr::{Code, Frame};
use bough_x::prelude::*;
use bough_x::soft::Soft;
use bough_x::store::{Path, Tiles};

#[derive(Clone, Copy)]
struct Many;
// SAFETY: zero-sized, no padding or invalid stored bits.
unsafe impl Words for Many {}
impl Expr for Many {
    type Out = Color;
    type Stage = Frame;
    const READS: u64 = (1 << 48) - 1;
    const CODE: &'static Code = &Code::Cpu(&[]);
    fn eval(&self, cx: &Cx) -> Color {
        Color(0xff000000 | cx.u.w[..48].iter().fold(0, |a, &b| a ^ b))
    }
}
struct Matrix {
    u: U,
    columns: usize,
    many: bool,
}
impl View for Matrix {
    fn block(&self) -> &U {
        &self.u
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        for i in 0..self.columns {
            // Separate scopes deliberately prevent same-archetype column grouping.
            s.scope(Space::root(96, 32), |s| {
                let rect = Rect::new((i % 3) as i32 * 32, 0, 32, 32);
                if self.many {
                    s.row(|| ([rect], Fill(Many)));
                } else if i == 0 {
                    s.row(|| ([rect], Fill(un::<First>().map(|x| Color(0xff000000 | x)))));
                } else if i == 1 {
                    s.row(|| ([rect], Fill(un::<Second>().map(|x| Color(0xff000000 | x)))));
                } else {
                    s.row(|| ([rect], Fill(Kc::<0xff000000>)));
                }
            });
        }
    }
}
lens!(First: u32 = 0);
lens!(Second: u32 = 1);

#[test]
fn sparse_multiword_slope_and_damage_oracle() {
    for columns in [64, 256, 1024, 2048] {
        let mut v = Matrix {
            u: U::default(),
            columns,
            many: false,
        };
        let mut w = World::<Soft>::new(96, 32, 48);
        w.mount(&v);
        w.tiles.clear();
        v.u.w[0] = 1;
        v.u.w[1] = 2;
        let mut want = Tiles::new(96, 32);
        w.tree_delta(&w.shown.clone(), &v.u, 3, Some(&mut want));
        w.settle(&v);
        assert_eq!(w.tiles.bits, want.bits, "damage at {columns} columns");
        assert_eq!(w.delta_stats.evaluated_columns, 2);
        assert_eq!(w.delta_stats.visited_incidences, 2);
        assert_eq!(w.delta_stats.dense_fallbacks, 0);
    }
}
#[test]
fn seventy_two_thousand_incidences() {
    let mut v = Matrix {
        u: U::default(),
        columns: 1500,
        many: true,
    };
    let mut w = World::<Soft>::new(96, 32, 48);
    w.mount(&v);
    w.tiles.clear();
    v.u.w[47] = 1;
    w.settle(&v);
    assert_eq!(w.delta_stats.evaluated_columns, 1500);
    assert_eq!(w.delta_stats.visited_incidences, 1500);
    v.u.w[0] = 2;
    v.u.w[1] = 4;
    w.settle(&v);
    assert_eq!(w.delta_stats.evaluated_columns, 1500);
    assert_eq!(w.delta_stats.dense_fallbacks, 1);
}
#[test]
#[should_panic(expected = "place record exceeds 20-bit capacity")]
fn place_rejects_record_spill() {
    World::<Soft>::new(32, 32, 0).place(&Path::default(), 0, 1 << 20);
}
#[test]
#[should_panic(expected = "place column exceeds 12-bit capacity")]
fn place_rejects_column_spill() {
    World::<Soft>::new(32, 32, 0).place(&Path::default(), 4096, 0);
}

#[path = "../scenes/sheet_el.rs"]
mod sheet_el;

#[test]
fn sheet_el_retained_ledger() {
    let app = bough_x::el::App::<sheet_el::Sheet, Soft>::new(
        *sheet_el::Sheet::with_rows(10_000),
        2560,
        1600,
        2.0,
    );
    let (records, pages, bytes) = app.world.root().stats();
    fn columns(s: &bough_x::store::Store) -> usize {
        s.cols.len() + s.subs.iter().map(columns).sum::<usize>()
    }
    eprintln!(
        "sheet_el rows=10000 records={records} columns={} pages={pages} column_bytes={bytes}",
        columns(app.world.root())
    );
    assert!(records < 860);
    assert!(bytes < 320 * 1024);
}
