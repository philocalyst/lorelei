use bough_x::prelude::*;
use bough_x::soft::Soft;
use bough_x::store::Path;
use std::cell::Cell;

struct Rows {
    u: U,
    items: Rel<u32>,
    height: i32,
}

impl View for Rows {
    fn block(&self) -> &U {
        &self.u
    }

    fn view<S: Sink<Self>>(&self, sink: &mut S) {
        sink.scope(
            Space {
                scroll: 0,
                ..Space::root(16, 8)
            },
            |sink| {
                sink.list(
                    &List {
                        n: self.items.len() as u32,
                        row_h: self.height,
                        x: 0,
                        y: 0,
                        shape: &[Rect::new(0, 0, 16, 8)],
                        edits: &self.items.journal,
                    },
                    |row| Fill(rec(Color(*self.items.get(row)))),
                )
            },
        );
    }
}

fn image(world: &World<Rows, Soft>) -> Vec<u32> {
    let mut soft = Soft::new(16, 8);
    world.paint(&mut soft);
    soft.px
}

#[test]
fn empty_zero_height_list_can_later_gain_rows() {
    let mut view = Rows {
        u: U::default(),
        items: Rel::new(vec![]),
        height: 0,
    };
    let mut world = World::<_, Soft>::new(16, 8, 1);
    world.mount(&view);
    assert_eq!(world.root().stats().0, 0);
    view.height = 8;
    view.items.insert(0, 0xffff_0000);
    world.settle(&view);
    assert_ne!(world.root().stats().0, 0);
    let mut fresh = World::<_, Soft>::new(16, 8, 1);
    fresh.mount(&view);
    assert_eq!(image(&world), image(&fresh));
}

#[test]
#[ignore = "TR1-02 open"]
fn stale_place_after_ring_reuse_is_refused() {
    let mut view = Rows {
        u: U::default(),
        items: Rel::new(vec![0xff00_0000; 130]),
        height: 8,
    };
    let mut world = World::<_, Soft>::new(16, 8, 1);
    world.mount(&view);
    let path = Path { at: [0; 6], n: 2 };
    let first = world.place_token(&path, 0, 0).unwrap();
    view.u.w[0] = 64 * 8;
    world.settle(&view);
    assert!(
        world.at_token(first).is_none(),
        "old place now resolves to row 64"
    );
}

#[test]
fn redescribe_subscribes_to_replacement_relation() {
    let mut view = Rows {
        u: U::default(),
        items: Rel::new(vec![0xff00_0000]),
        height: 8,
    };
    let mut world = World::<_, Soft>::new(16, 8, 1);
    world.mount(&view);
    view.items = Rel::new(vec![0xffff_0000]);
    world.redescribe(&view);
    view.items.insert(0, 0xff00_ff00);
    world.settle(&view);
    let mut fresh = World::<_, Soft>::new(16, 8, 1);
    fresh.mount(&view);
    assert_eq!(image(&world)[0], image(&fresh)[0]);
}

#[test]
#[ignore = "TR1-03 open"]
fn settle_subscribes_to_replacement_relation() {
    let mut view = Rows {
        u: U::default(),
        items: Rel::new(vec![0xff00_0000]),
        height: 8,
    };
    let mut world = World::<_, Soft>::new(16, 8, 1);
    world.mount(&view);
    view.items = Rel::new(vec![0xffff_0000]);
    view.items.insert(0, 0xff00_ff00);
    world.settle(&view);
    let mut fresh = World::<_, Soft>::new(16, 8, 1);
    fresh.mount(&view);
    assert_eq!(image(&world)[0], image(&fresh)[0]);
}

#[test]
fn wide_clip_intersecting_row_zero_does_not_wrap() {
    let list = List {
        n: 1,
        row_h: 8,
        x: 0,
        y: 0,
        shape: &[],
        edits: &FIXED,
    };
    let clip = Rect {
        x0: 0,
        y0: i32::MIN,
        x1: 16,
        y1: i32::MAX,
    };
    assert_eq!(bough_x::sink::exposed(&list, (0, 0), clip), (0, 1));
}

struct CountingRows {
    u: U,
    items: Rel<u32>,
    calls: Cell<usize>,
}

impl View for CountingRows {
    fn block(&self) -> &U {
        &self.u
    }

    fn view<S: Sink<Self>>(&self, sink: &mut S) {
        sink.list(
            &List {
                n: self.items.len() as u32,
                row_h: 1,
                x: 0,
                y: 0,
                shape: &[Rect::new(0, 0, 16, 1)],
                edits: &self.items.journal,
            },
            |row| {
                self.calls.set(self.calls.get() + 1);
                Fill(rec(Color(*self.items.get(row))))
            },
        );
    }
}

#[test]
#[ignore = "TR1-10 open"]
fn front_insert_does_not_redescribe_every_visible_row() {
    let mut counts = Vec::new();
    for height in [32, 256] {
        let mut view = CountingRows {
            u: U::default(),
            items: Rel::new(vec![0xff00_0000; 512]),
            calls: Cell::new(0),
        };
        let mut world = World::<_, Soft>::new(16, height, 1);
        world.mount(&view);
        view.calls.set(0);
        view.items.insert(0, 0xffff_0000);
        world.settle(&view);
        counts.push(view.calls.get());
    }
    assert!(
        counts[1] <= counts[0] + 2,
        "row descriptions at 32 and 256 visible rows: {counts:?}"
    );
}

thread_local! {
    static POINTER_EVALS: Cell<usize> = const { Cell::new(0) };
}

struct PointerRows(U, u32);

impl View for PointerRows {
    fn block(&self) -> &U {
        &self.0
    }

    fn view<S: Sink<Self>>(&self, sink: &mut S) {
        sink.list(
            &List {
                n: self.1,
                row_h: 1,
                x: 0,
                y: 0,
                shape: &[Rect::new(0, 0, 16, 1)],
                edits: &FIXED,
            },
            |_| {
                Fill(un::<PtrX>().map(|x| {
                    POINTER_EVALS.with(|n| n.set(n.get() + 1));
                    Color(0xff00_0000 | u32::from(x == 1))
                }))
            },
        );
    }
}

#[test]
#[ignore = "TR1-09 open"]
fn pointer_change_without_damage_has_bounded_record_work() {
    let mut counts = Vec::new();
    for height in [32, 256] {
        let view = PointerRows(U::default(), height as u32);
        let mut world = World::<_, Soft>::new(16, height, 0);
        world.mount(&view);
        world.pointer_words(2, 0);
        world.tiles.clear();
        POINTER_EVALS.with(|n| n.set(0));
        world.pointer_words(3, 0);
        assert!(!world.tiles.any());
        counts.push(POINTER_EVALS.with(Cell::get));
    }
    assert!(
        counts[1] <= counts[0] + 2,
        "evaluations at 32 and 256 exposed rows: {counts:?}"
    );
}
