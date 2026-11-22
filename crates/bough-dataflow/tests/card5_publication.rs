//! Erased leaf writes and event snapshots across view publication.
use bough_dataflow::event::{Button, Ev, Interact};
use bough_dataflow::prelude::*;
use bough_dataflow::soft::Soft;
use std::panic::{AssertUnwindSafe, catch_unwind};

struct Leaves {
    u: U,
    red: bool,
    wrong: bool,
}
impl View for Leaves {
    fn block(&self) -> &U {
        &self.u
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        let fill = Fill(rec(Color(if self.red { 0xffff0000 } else { 0xff0000ff })));
        if s.keeps() {
            if self.wrong {
                s.leaf(0, &Fill(Kc::<0xff00ff00>));
            } else {
                s.leaf(0, &fill);
            }
        } else {
            s.row(|| ([Rect::new(0, 0, 32, 32)], fill));
        }
    }
}

#[test]
fn kept_leaf_patches_only_an_admitted_record() {
    let mut v = Leaves {
        u: U::default(),
        red: false,
        wrong: false,
    };
    let mut world = World::<_, Soft>::new(32, 32, 1);
    world.mount(&v);
    v.red = true;
    world.redescribe(&v);
    let mut rendered = Soft::new(32, 32);
    world.paint(&mut rendered);
    assert_eq!(rendered.px[0], 0xffff0000);
    v.wrong = true;
    assert!(catch_unwind(AssertUnwindSafe(|| world.redescribe(&v))).is_err());
    assert!(world.at_place(0).is_none());
    assert!(world.repick(&mut v).place == u32::MAX);
    v.wrong = false;
    world.settle(&v);
    assert!(world.at_place(0).is_some());
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Padded(u8, u32);
#[derive(Clone, Copy)]
#[repr(C, align(32))]
struct Aligned(u32);
trait Item: Copy + 'static {
    fn number(self) -> u32;
}
impl Item for Padded {
    fn number(self) -> u32 {
        self.0 as u32 + self.1
    }
}
impl Item for Aligned {
    fn number(self) -> u32 {
        self.0
    }
}
struct Hover<T> {
    u: U,
    items: Rel<T>,
    events: Vec<(u32, bool)>,
}
impl<T: Item> View for Hover<T> {
    fn block(&self) -> &U {
        &self.u
    }
    fn view<S: Sink<Self>>(&self, s: &mut S) {
        s.list(
            &List {
                n: self.items.len() as u32,
                row_h: 16,
                x: 0,
                y: 0,
                shape: &[Rect::new(0, 0, 32, 16)],
                edits: &self.items.journal,
            },
            |r| {
                let item = *self.items.get(r);
                Fill(Kc::<0xff0000ff>)
                    .on_hover(move |owner: &mut Self, e: &Ev| {
                        owner.events.push((item.number(), e.on));
                    })
                    .on_click(|owner: &mut Self| owner.events.push((0, false)))
                    .focusable()
            },
        );
    }
}
fn check_hover<T: Item>(first: T, second: T) {
    let mut v = Hover {
        u: U::default(),
        items: Rel::by_item(vec![first, second]),
        events: Vec::new(),
    };
    let mut world = World::<_, Soft>::new(32, 16, 1);
    world.mount(&v);
    world.pointer(&mut v, 2, 2);
    v.items.remove(0);
    world.redescribe(&v);
    world.repick(&mut v);
    assert_eq!(
        v.events,
        [
            (first.number(), true),
            (first.number(), false),
            (second.number(), true)
        ]
    );
}
#[test]
fn dropped_hover_uses_the_saved_padded_captures() {
    check_hover(Padded(1, 10), Padded(2, 20));
}
#[test]
fn dropped_hover_uses_the_saved_overaligned_captures() {
    check_hover(Aligned(11), Aligned(22));
}

#[test]
fn a_same_version_journal_replacement_publishes_its_new_watch() {
    let mut v = Hover {
        u: U::default(),
        items: Rel::by_item(vec![Padded(1, 10), Padded(2, 20)]),
        events: Vec::new(),
    };
    let mut world = World::<_, Soft>::new(32, 16, 1);
    world.mount(&v);
    world.pointer(&mut v, 2, 2);
    v.items = Rel::by_item(vec![Padded(3, 30), Padded(4, 40)]);
    world.redescribe(&v);
    world.repick(&mut v);
    assert_eq!(v.events, [(11, true), (11, false), (33, true)]);

    // After rebinding, an edit of the new journal reaches a settle without a full description.
    v.items.remove(0);
    world.settle(&v);
    world.repick(&mut v);
    assert_eq!(&v.events[3..], &[(33, false), (44, true)]);
}

#[test]
fn a_same_version_journal_replacement_clears_focus_and_press() {
    let mut v = Hover {
        u: U::default(),
        items: Rel::by_item(vec![Padded(1, 10)]),
        events: Vec::new(),
    };
    let mut world = World::<_, Soft>::new(32, 16, 1);
    world.mount(&v);
    world.pointer_down(&mut v, 2, 2, Button::Primary);
    assert!(world.focused().is_some());
    v.items = Rel::by_item(vec![Padded(3, 30)]);
    world.redescribe(&v);
    assert!(world.focused().is_none());
    assert!(!world.pointer_up(&mut v, 2, 2, Button::Primary).consumed);
    assert!(!v.events.contains(&(0, false)));
}
