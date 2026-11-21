//! The soundness review of 2026-11-21 (`independent-review` lane, agent `sound`): each probe below crashed,
//! hung or read freed memory at 71ee3be (SIGSEGV for S03, S04a, S04b and S05 in release; a heap corruption
//! that hung the process for S02; Miri for S01 and S03). S01 and S05 are now compile errors (doctests on
//! `store::Store` and `sink::Row`); S07 is a compile error for a `!Clone` iterator (doctest on `el::Each`)
//! and a runtime check here for a `Clone` one.
#![feature(trusted_random_access, min_specialization)]
use bough_x::draw::{Align, Draws, Prims};
use bough_x::expr::StageId;
use bough_x::prelude::*;
use bough_x::soft::Soft;
use bough_x::store::{Arch, Backend, ColRef, Path, Tiles};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// S02: `Tiles` has public fields; a mark that does not fit the bitmap is refused, not written.
#[test]
#[should_panic(expected = "tile damage outside its bitmap")]
fn tiles_refuse_damage_outside_their_bitmap() {
    let mut t = Tiles::new(32, 32);
    t.w = 1 << 16;
    t.h = 1 << 16;
    t.mark(Rect::new(0, 0, 1 << 16, 1 << 16));
}

/// S02 through the world's public field.
#[test]
#[should_panic(expected = "tile damage outside its bitmap")]
fn world_tiles_refuse_a_bitmap_made_smaller() {
    let v = Grows {
        u: U::default(),
        extra: false,
    };
    let mut w = World::<_, Soft>::new(64, 64, 1);
    w.tiles.bits.clear();
    w.mount(&v);
}

/// S02, negative control: a consistent bitmap still takes every mark.
#[test]
fn tiles_mark_whole_window() {
    let mut t = Tiles::new(100, 70);
    t.mark_all();
    assert_eq!(t.count(), 4 * 3);
}

// ---- S03: a panic inside `view` must not leave the column table naming freed stores ----------------------

struct Lists {
    u: U,
    n: usize,
    boom: bool,
    shape: Vec<Rect>,
}
impl View for Lists {
    fn block(&self) -> &U {
        &self.u
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        for k in 0..self.n {
            s.list(
                &List {
                    n: 4,
                    row_h: 4,
                    x: 0,
                    y: 8 * k as i32,
                    shape: &self.shape,
                    edits: &FIXED,
                },
                |_| Fill(Kc::<0xff00ff00>).on_click(|v: &mut Lists| v.u.w[0] += 1),
            );
        }
        if self.boom {
            panic!("view panics after growing the store tree");
        }
    }
}
#[test]
fn a_panicking_view_leaves_no_dangling_table() {
    let mut v = Lists {
        u: U::default(),
        n: 1,
        boom: false,
        shape: vec![Rect::new(0, 0, 4, 4)],
    };
    let mut w = World::<_, Soft>::new(64, 128, 1);
    w.mount(&v);
    let p = w.place(&Path { at: [0; 6], n: 1 }, 0, 0);
    assert!(w.at_place(p).is_some());
    // nine lists: the root's `subs` buffer is reallocated under the table, then the view panics
    v.n = 9;
    v.boom = true;
    assert!(catch_unwind(AssertUnwindSafe(|| w.redescribe(&v))).is_err());
    assert!(w.at_place(p).is_none(), "a stale table names nothing");
    // the next pass that completes brings the world back
    v.boom = false;
    w.redescribe(&v);
    let p = w.place(&Path { at: [0; 6], n: 1 }, 0, 0);
    let (st, _, _) = w.at_place(p).expect("the table is current again");
    assert_eq!(st.cols.len(), 1);
    w.click(&mut v, 1, 1);
    assert_eq!(v.u.w[0], 1);
}

// ---- S04: records that are live but were never written ---------------------------------------------------

#[derive(Clone, Copy)]
struct Fp(fn() -> u32);
impl Arch for Fp {
    const READS: u64 = 0;
    const NAME: &'static str = "fp";
    const STAGE: StageId = StageId::Mount;
    fn same_at(&self, o: &Self, _: &U) -> bool {
        (self.0)() == (o.0)()
    }
    fn changed(&self, _: &U, _: &U) -> bool {
        false
    }
    fn paint<B: Backend>(b: &mut B, col: ColRef<'_, Self>, u: &U) {
        b.draws(col, u)
    }
}
impl Draws for Fp {
    fn draw<P: Prims>(&self, r: Rect, cut: Rect, _: &U, p: &mut P) {
        let _ = Align::Left;
        p.solid(r.and(cut), 0xff00_0000 | (self.0)());
    }
}
fn blue() -> u32 {
    0xff
}
struct Grows {
    u: U,
    extra: bool,
}
impl View for Grows {
    fn block(&self) -> &U {
        &self.u
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        s.row(|| ([Rect::new(0, 0, 8, 8)], Fill(Kc::<0xff00ff00>)));
        if self.extra {
            s.row(|| ([Rect::new(8, 0, 16, 8)], Fp(blue)));
        }
    }
}
/// S04a: a fixed row a re-description appends is written, not compared with a slot that holds nothing.
#[test]
fn a_row_appended_by_a_redescription_is_written() {
    let mut v = Grows {
        u: U::default(),
        extra: false,
    };
    let mut w = World::<_, Soft>::new(32, 32, 1);
    w.mount(&v);
    v.extra = true;
    w.redescribe(&v);
    let mut soft = Soft::new(32, 32);
    w.paint(&mut soft);
    assert_eq!(
        soft.px[2 * 32 + 10],
        0xff00_00ff,
        "the appended record is painted"
    );
    assert_eq!(soft.px[2 * 32 + 2], 0xff00_ff00);
}

struct Panicky {
    u: U,
    bad: usize,
    hit: u32,
}
fn bump(v: &mut Panicky) {
    v.hit += 1;
}
impl View for Panicky {
    fn block(&self) -> &U {
        &self.u
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        let f: fn(&mut Panicky) = bump;
        s.list(
            &List {
                n: 8,
                row_h: 4,
                x: 0,
                y: 0,
                shape: &[Rect::new(0, 0, 4, 4)],
                edits: &FIXED,
            },
            |r| {
                assert!(r != self.bad, "row function fails at row {r}");
                Fill(Kc::<0xff00ff00>).on_click(move |v: &mut Panicky| f(v))
            },
        );
    }
}
/// S04b: a click on a row a panicking row function never wrote runs no handler (its captures were zero).
#[test]
fn a_click_on_a_row_never_written_runs_nothing() {
    let mut v = Panicky {
        u: U::default(),
        bad: 2,
        hit: 0,
    };
    let mut w = World::<_, Soft>::new(32, 32, 1);
    assert!(catch_unwind(AssertUnwindSafe(|| w.mount(&v))).is_err());
    v.bad = usize::MAX;
    w.click(&mut v, 1, 3 * 4 + 1);
    assert_eq!(v.hit, 0, "row 3 was never written");
    w.click(&mut v, 1, 1);
    assert_eq!(v.hit, 1, "row 0 was written before the panic");
}

// ---- S13: rows of a list that hold a scrolled box ---------------------------------------------------------

mod holes {
    use bough_x::el::App;
    use bough_x::el::prelude::*;
    use bough_x::soft::Soft;
    struct Nested {
        y: Signal<f32>,
        inner: Signal<f32>,
    }
    impl View for Nested {
        fn view(&self) -> impl Element<Self> + '_ {
            column(each((0..5usize).map(|_| {
                div()
                    .h(20.0)
                    .w(50.0)
                    .bg(rgb(0x00ff00))
                    .scroll(sig!(self.inner))
            })))
            .h(100.0)
            .scroll(sig!(self.y))
        }
    }
    /// A row's hole site (the scrolled box's content space) has no page: laying the row out and entering
    /// it into the window wrote its rect through a null pointer at 71ee3be.
    #[test]
    fn list_rows_holding_a_scrolled_box_lay_out() {
        let mut app = App::<Nested, Soft>::new(
            Nested {
                y: Signal::new(0.0),
                inner: Signal::new(0.0),
            },
            64,
            128,
            1.0,
        );
        app.turn(|v| {
            v.y.set(10.0);
        });
        let mut s = Soft::new(64, 128);
        app.world.paint(&mut s);
        let (records, _, _) = app.world.root().stats();
        assert!(records >= 5, "{records} records for five exposed rows");
    }
}

// ---- S07: `each` over a trusted-random-access iterator ----------------------------------------------------

mod each {
    use bough_x::el::App;
    use bough_x::el::prelude::*;
    use bough_x::soft::Soft;
    use std::cell::RefCell;
    use std::rc::Rc;
    /// A `Clone` iterator may be asked an index more than once (std's condition 2 is for `!Clone` ones);
    /// it counts how often.
    #[derive(Clone)]
    struct Counted {
        n: usize,
        asked: Rc<RefCell<Vec<usize>>>,
    }
    impl Iterator for Counted {
        type Item = bough_x::el::Text<bough_x::el::Str, Color>;
        fn next(&mut self) -> Option<Self::Item> {
            None
        }
        fn size_hint(&self) -> (usize, Option<usize>) {
            (self.n, Some(self.n))
        }
        unsafe fn __iterator_get_unchecked(&mut self, i: usize) -> Self::Item {
            assert!(i < self.n, "index {i} outside size {}", self.n);
            self.asked.borrow_mut().push(i);
            text("row")
        }
    }
    // SAFETY: `size` is exact and `__iterator_get_unchecked` is pure for every index under it.
    unsafe impl core::iter::TrustedRandomAccessNoCoerce for Counted {
        const MAY_HAVE_SIDE_EFFECT: bool = false;
        fn size(&self) -> usize {
            self.n
        }
    }
    thread_local! { static ASKED: Rc<RefCell<Vec<usize>>> = Default::default(); }
    struct Iter {
        y: Signal<f32>,
    }
    impl View for Iter {
        fn view(&self) -> impl Element<Self> + '_ {
            let asked = ASKED.with(Rc::clone);
            column(each(Counted { n: 5, asked }))
                .h(100.0)
                .scroll(sig!(self.y))
        }
    }
    #[test]
    fn each_asks_only_indices_under_size() {
        let _app = App::<Iter, Soft>::new(
            Iter {
                y: Signal::new(0.0),
            },
            64,
            64,
            1.0,
        );
        let asked = ASKED.with(|a| a.borrow().clone());
        assert!(asked.iter().all(|&i| i < 5));
        // the pass asks row 0 to lay the list out, then again to write it: why `I: Clone` is required
        assert!(asked.iter().filter(|&&i| i == 0).count() >= 2, "{asked:?}");
    }
}
