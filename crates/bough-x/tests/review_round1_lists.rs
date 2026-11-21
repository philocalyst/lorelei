use bough_x::prelude::*;
use bough_x::soft::Soft;
use bough_x::store::Path;

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
#[ignore = "known issue: packed place needs a publication generation"]
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
#[ignore = "known issue: replacing a relation disconnects the observed journal"]
fn replace_relation_at_same_version_refreshes_list() {
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
