//! Free rows (`Sink::free`, Integration Card 3 decision D2) on the core's own reference raster: rows that come,
//! go, move and change, held to retained == full redraw; the one-record patch (`World::free_set`); growth past
//! the store's capacity; and the checked refusals of `free_set`.
use bough_dataflow::expr::U;
use bough_dataflow::prim::{P_BOX, Prim};
use bough_dataflow::sink::{Sink, View};
use bough_dataflow::soft::Soft;
use bough_dataflow::store::{Rect, Space};
use bough_dataflow::world::World;

const W: i32 = 320;
const H: i32 = 240;

struct Boxes {
    u: U,
    /// (x, y, w, h, colour)
    rows: Vec<(i32, i32, i32, i32, u32)>,
    /// A second scope, cut to the right half and shifted.
    inner: Vec<(i32, i32, i32, i32, u32)>,
    shift: i32,
}
fn prim(c: u32) -> Prim {
    Prim {
        kind: P_BOX,
        color: c,
        alpha: 255,
        ..Prim::default()
    }
}
impl View for Boxes {
    fn block(&self) -> &U {
        &self.u
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        let rows = &self.rows;
        s.scope(Space::root(W, H), |s| {
            s.free(rows.len() as u32, |i| {
                (
                    Rect::new(rows[i].0, rows[i].1, rows[i].2, rows[i].3),
                    prim(rows[i].4),
                )
            })
        });
        let inner = &self.inner;
        let space = Space {
            clip: Rect::new(W / 2, 0, W / 2, H),
            base: (W / 2, -self.shift),
            scroll: bough_dataflow::store::NO_WORD,
        };
        s.scope(space, |s| {
            s.free(inner.len() as u32, |i| {
                (
                    Rect::new(inner[i].0, inner[i].1, inner[i].2, inner[i].3),
                    prim(inner[i].4),
                )
            })
        });
    }
}

fn check(tag: &str, world: &mut World<Boxes, Soft>, retained: &mut Soft) {
    retained.repair(&world.tiles, |s| world.paint(s));
    world.tiles.clear();
    let mut full = Soft::new(W, H);
    world.paint(&mut full);
    let diff = retained
        .px
        .iter()
        .zip(&full.px)
        .filter(|(a, b)| a != b)
        .count();
    assert!(diff == 0, "{tag}: retained != full redraw ({diff} pixels)");
}

#[test]
fn free_rows_come_go_move_and_change_under_the_oracle() {
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    let mut rnd = |m: u64| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x % m
    };
    let mut row = |rnd: &mut dyn FnMut(u64) -> u64| {
        (
            rnd(280) as i32,
            rnd(200) as i32,
            8 + rnd(60) as i32,
            6 + rnd(40) as i32,
            0x10_0000 + rnd(0xef_ffff) as u32,
        )
    };
    let mut v = Boxes {
        u: U::default(),
        rows: (0..5).map(|_| row(&mut rnd)).collect(),
        inner: (0..3).map(|_| row(&mut rnd)).collect(),
        shift: 0,
    };
    let mut world = World::<Boxes, Soft>::new(W, H, 0);
    world.mount(&v);
    let mut retained = Soft::new(W, H);
    check("mount", &mut world, &mut retained);
    for step in 0..400 {
        let list = if rnd(3) == 0 {
            &mut v.inner
        } else {
            &mut v.rows
        };
        match rnd(7) {
            0 | 1 => list.push(row(&mut rnd)),
            2 if !list.is_empty() => {
                let i = rnd(list.len() as u64) as usize;
                list.remove(i);
            }
            3 if !list.is_empty() => {
                let i = rnd(list.len() as u64) as usize;
                list[i].4 ^= 0x00ff_00;
            }
            4 if !list.is_empty() => {
                let i = rnd(list.len() as u64) as usize;
                list[i].0 = rnd(280) as i32;
            }
            5 => {
                let i = rnd(list.len() as u64 + 1) as usize;
                list.insert(i, row(&mut rnd));
            }
            _ => v.shift = rnd(60) as i32,
        }
        // past the first capacity (64 slots) now and then, and back to nothing
        if step == 150 {
            v.rows.extend((0..200).map(|_| row(&mut rnd)));
        }
        if step == 300 {
            v.rows.clear();
        }
        world.redescribe(&v);
        check(&format!("step {step}"), &mut world, &mut retained);
        let live = v.rows.len() + v.inner.len();
        let mut count = Soft::new(W, H);
        world.paint(&mut count);
        assert_eq!(
            count.records as usize, live,
            "step {step}: every live row is painted, and no released one"
        );
    }
}

#[test]
fn one_record_is_patched_without_a_description() {
    let v = Boxes {
        u: U::default(),
        rows: vec![(10, 10, 40, 40, 0x11_2233), (100, 10, 40, 40, 0x44_5566)],
        inner: vec![(4, 4, 20, 20, 0x77_8899)],
        shift: 0,
    };
    let mut world = World::<Boxes, Soft>::new(W, H, 0);
    world.mount(&v);
    let mut retained = Soft::new(W, H);
    check("mount", &mut world, &mut retained);
    assert!(
        !world.free_set(0, 1, prim(0x44_5566)),
        "the same record: nothing stored, nothing damaged"
    );
    assert!(!world.tiles.any());
    assert!(world.free_set(0, 1, prim(0xaa_bbcc)));
    assert!(
        world.tiles.any() && world.tiles.count() <= 4,
        "one record's tiles"
    );
    check("patched", &mut world, &mut retained);
    assert_eq!(retained.px[(20 * W + 110) as usize], 0xaa_bbcc);
    assert!(world.free_set(1, 0, prim(0x01_0203)));
    check("patched in the cut scope", &mut world, &mut retained);
}

#[test]
#[should_panic(expected = "no such free row")]
fn a_row_past_the_count_is_refused() {
    let v = Boxes {
        u: U::default(),
        rows: vec![(10, 10, 40, 40, 1)],
        inner: vec![],
        shift: 0,
    };
    let mut world = World::<Boxes, Soft>::new(W, H, 0);
    world.mount(&v);
    world.free_set(0, 1, prim(2));
}

#[test]
#[should_panic(expected = "free rows of another record type")]
fn a_record_of_another_type_is_refused() {
    let v = Boxes {
        u: U::default(),
        rows: vec![(10, 10, 40, 40, 1)],
        inner: vec![],
        shift: 0,
    };
    let mut world = World::<Boxes, Soft>::new(W, H, 0);
    world.mount(&v);
    world.free_set(0, 0, bough_dataflow::kinds::Fill(bough_dataflow::expr::Color(3)));
}

/// The typed element layer has an unpainted scroller with one layout leaf and no record.
#[test]
#[should_panic(expected = "a free row must have one concrete record site")]
fn a_hole_cannot_be_used_as_a_free_row() {
    struct UnpaintedScroll {
        u: U,
        y: bough_dataflow::el::Signal<f32>,
    }
    impl View for UnpaintedScroll {
        fn block(&self) -> &U {
            &self.u
        }
        fn view<S: Sink<Self>>(&self, s: &mut S) {
            use bough_dataflow::el::{Scrolls, div};
            s.free(1, |_| {
                (Rect::new(0, 0, 20, 20), div().scroll(bough_dataflow::sig!(self.y)))
            });
        }
    }
    let view = UnpaintedScroll {
        u: U::default(),
        y: bough_dataflow::el::Signal::new(0.),
    };
    World::<UnpaintedScroll, Soft>::new(W, H, 0).mount(&view);
}
