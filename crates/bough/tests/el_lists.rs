//! Keyed collections as El children and routes on the owner chain (`designs/reactivity/final` 02 §4, 01 §4,
//! 04 §2–3; card "Keyed collections as El children, and routes on the owner chain (B3)" in
//! `docs/lanes/integration/2026-11-22-el-surface.md`).
//!
//! Oracle: after every step of seeded scripts of list operations, the presented pixels equal a fresh mount of
//! the same items, and the repaired frame equals a full redraw.
//!
//! Negative controls (each must fail a test here):
//! * `--features mutant_el_list_slot`: the El row mounter takes the slot id for a position
//!   (`the_retained_list_equals_a_fresh_mount`).
//! * `--features mutant_el_list_edit`: an edited row is not re-described (`an_edit_redescribes_its_row_alone`,
//!   the oracle).
//! * `--features mutant_el_list_count`: rows mounted outside a walk are not counted in the dynamic items
//!   enclosing the list (`a_list_child_leaves_and_is_replaced_whole`).
//! * `--features mutant_el_route_slot`: a row route's dispatch takes the slot id for a position
//!   (`a_row_route_reaches_its_row_after_reorders`).
//! * `--features mutant_el_route_far`: the farthest matching owner (`nested_owners_of_one_type_resolve_to_the_nearest`).
use bough::prelude::*;
use bough::raw::{App, Button, Config};

fn font() -> Vec<u8> {
    std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap()
}
fn cfg() -> Config {
    Config::new(320.0, 240.0, 1.0, font())
}

fn shot<V: bough::app::View>(app: &mut App<V>) -> Vec<u8> {
    app.frame();
    let p = app.presented_pixels();
    assert_eq!(p, app.reference_pixels(), "the repaired frame equals a full redraw");
    p
}

fn click<V: bough::app::View>(app: &mut App<V>, x: f32, y: f32) {
    app.pointer_move(x, y);
    app.pointer_down(x, y, Button::Primary);
    app.pointer_up(x, y, Button::Primary);
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

// ------------------------------------------------------------------------------------------- the items

/// A row: a signal field (a port of the row's own), plain fields (read by its description only), its own
/// routes.
struct Item {
    id: u32,
    label: Signal<String>,
    note: u32,
    hits: u32,
    keys: u32,
    at: f32,
}
impl Item {
    fn new(id: u32) -> Item {
        Item { id, label: Signal::new(format!("item {id}")), note: id % 7, hits: 0, keys: 0, at: 0.0 }
    }
    fn copy(&self) -> Item {
        Item { id: self.id, label: Signal::new((*self.label).clone()), note: self.note, hits: self.hits, keys: self.keys, at: self.at }
    }
    fn hit(&mut self) {
        self.hits += 1;
    }
    fn key(&mut self) {
        self.keys += 1;
    }
    fn add(&mut self, k: u32) {
        self.note += k;
    }
    fn pressed(&mut self, e: &Ev) {
        self.at = e.x;
    }
}
impl View for Item {
    fn view(&self) -> El<'_> {
        row((
            text(self.id).w(30),
            text(&self.label).w(90),
            text(self.note).w(30),
            text(self.hits).w(30),
            div().size(16, 16).bg(rgb(0x2266aa)).on_click(Item::add.with(10)),
            div().size(16, 16).bg(rgb(0xaa6622)).on_click(Board::picked.with(self.id)),
        ))
        .h(20)
        .bg(rgb(0xe0e0e0 + (self.id % 4) * 0x060606))
        .hover(|s| s.bg(rgb(0xffeeaa)))
        .focusable()
        .on_click(Item::hit)
        .on_press(Item::pressed)
        .on_key(Key::Char('x'), Item::key)
    }
}

struct Board {
    items: List<Item>,
    title: u32,
    picked: Vec<u32>,
}
impl Board {
    fn new(items: impl IntoIterator<Item = Item>) -> Board {
        Board { items: List::from_iter_in(items), title: 0, picked: Vec::new() }
    }
    fn picked(&mut self, id: u32) {
        self.picked.push(id);
    }
    fn copy(&self) -> Board {
        Board { items: List::from_iter_in(self.items.iter().map(Item::copy)), title: self.title, picked: Vec::new() }
    }
}
impl View for Board {
    fn view(&self) -> El<'_> {
        column((text(self.title), column(&self.items).gap(2))).w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff))
    }
}

/// The y of position `k`'s row (the title line is 20 points tall; rows 20 apart by 2).
fn row_y(k: usize) -> f32 {
    20.0 + k as f32 * 22.0 + 10.0
}

fn described<V: bough::app::View>(app: &App<V>) -> u64 {
    app.rt.lists.regs().iter().map(|r| r.stats.rows_described).sum()
}
fn mounted<V: bough::app::View>(app: &App<V>) -> u64 {
    app.rt.lists.regs().iter().map(|r| r.stats.rows_mounted).sum()
}

fn ids(b: &Board) -> Vec<u32> {
    b.items.iter().map(|i| i.id).collect()
}

// ------------------------------------------------------------------------------------------- the oracle

#[test]
fn the_retained_list_equals_a_fresh_mount() {
    for seed in 1..=6u64 {
        let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut next = 100;
        let mut app = App::new(Board::new((0..6).map(Item::new)), cfg(), true);
        shot(&mut app);
        for step in 0..36 {
            let n = app.owner.items.len();
            let k = r.below(10);
            let what = match k {
                0 => {
                    app.turn(|b| {
                        b.items.push(Item::new(next));
                    });
                    next += 1;
                    "push"
                }
                1 => {
                    let at = r.below(n + 1);
                    app.turn(|b| {
                        b.items.insert(at, Item::new(next));
                    });
                    next += 1;
                    "insert"
                }
                2 if n > 0 => {
                    let at = r.below(n);
                    app.turn(|b| {
                        b.items.remove(at);
                    });
                    "remove"
                }
                3 if n > 1 => {
                    let (a, b2) = (r.below(n), r.below(n));
                    app.turn(|b| b.items.move_to(a, b2));
                    "move"
                }
                4 if n > 1 => {
                    let (a, b2) = (r.below(n), r.below(n));
                    app.turn(|b| b.items.swap(a, b2));
                    "swap"
                }
                5 => {
                    let key = r.next() % 5;
                    app.turn(|b| b.items.sort_by_key(|i| (i.id as u64 * 7919 + key) % 11));
                    "sort"
                }
                6 if n > 0 => {
                    let at = r.below(n);
                    app.turn(|b| b.items.edit(at).note += 3);
                    "edit a plain field"
                }
                7 if n > 0 => {
                    let at = r.below(n);
                    let s = format!("edited {step}");
                    app.turn(|b| {
                        b.items.edit(at).label.set(s);
                    });
                    "edit a signal field"
                }
                8 if n > 2 => {
                    let m = 2 + r.below(3) as u32;
                    app.turn(|b| b.items.retain(|i| i.id % m != 0));
                    "retain"
                }
                _ => {
                    app.turn(|b| b.title += 1);
                    "re-describe the parent"
                }
            };
            let p = shot(&mut app);
            let mut fresh = App::new(app.owner.copy(), cfg(), true);
            assert_eq!(p, shot(&mut fresh), "seed {seed} step {step} ({what}): retained = a fresh mount of {:?}", ids(&app.owner));
        }
    }
}

#[test]
fn the_list_follows_a_signal_port_with_no_redescription() {
    let mut app = App::new(Board::new((0..4).map(Item::new)), cfg().once(), true);
    let p0 = shot(&mut app);
    app.turn(|b| {
        b.items.edit(2).label.set("changed".into());
    });
    let p1 = shot(&mut app);
    assert_ne!(p0, p1, "a row's signal field is a port of the row");
    let mut fresh = App::new(app.owner.copy(), cfg().once(), true);
    assert_eq!(p1, shot(&mut fresh));
    // splices and moves need no description either
    app.turn(|b| {
        b.items.move_to(0, 3);
        b.items.insert(1, Item::new(9));
    });
    let p2 = shot(&mut app);
    let mut fresh = App::new(app.owner.copy(), cfg().once(), true);
    assert_eq!(p2, shot(&mut fresh));
}

// ------------------------------------------------------------------------------------------- deltas

#[test]
fn an_edit_redescribes_its_row_alone() {
    let mut app = App::new(Board::new((0..5).map(Item::new)), cfg(), true);
    let p0 = shot(&mut app);
    let (d0, m0) = (described(&app), mounted(&app));
    assert_eq!(d0, 5, "each row described once to mount it");
    app.turn(|b| b.items.edit(3).note = 99);
    let p1 = shot(&mut app);
    assert_ne!(p0, p1, "a plain field's edit shows (its row re-describes)");
    assert_eq!((described(&app) - d0, mounted(&app) - m0), (1, 0), "an edit describes one row and mounts none");
    let (d1, m1) = (described(&app), mounted(&app));
    app.turn(|b| b.items.move_to(0, 4));
    shot(&mut app);
    assert_eq!((described(&app) - d1, mounted(&app) - m1), (0, 0), "a move describes and mounts nothing");
    app.turn(|b| b.items.reverse());
    shot(&mut app);
    assert_eq!((described(&app) - d1, mounted(&app) - m1), (0, 0), "a permutation describes and mounts nothing");
    app.turn(|b| b.title += 1);
    shot(&mut app);
    assert_eq!((described(&app) - d1, mounted(&app) - m1), (0, 0), "the parent re-described: the rows are not");
    app.turn(|b| {
        b.items.insert(2, Item::new(50));
    });
    shot(&mut app);
    assert_eq!((described(&app) - d1, mounted(&app) - m1), (1, 1), "a splice mounts the new row alone");
}

// ------------------------------------------------------------------------------------------- identity

#[test]
fn focus_stays_with_the_item_across_permutations() {
    let mut app = App::new(Board::new((0..5).map(Item::new)), cfg(), true);
    shot(&mut app);
    click(&mut app, 12.0, row_y(1)); // focus (and a hit) on item 1
    app.pointer_leave();
    let hits = |b: &Board| b.items.iter().map(|i| (i.id, i.hits, i.keys)).collect::<Vec<_>>();
    assert_eq!(app.owner.items.get(1).unwrap().hits, 1);
    let none = Mods::default();
    for op in 0..4 {
        app.turn(|b| match op {
            0 => b.items.reverse(),
            1 => b.items.move_to(0, 4),
            2 => b.items.sort_by_key(|i| (i.id * 3) % 5),
            _ => {
                b.items.insert(0, Item::new(70));
            }
        });
        shot(&mut app);
        app.key_down(Key::Char('x'), none);
        let item1 = app.owner.items.iter().find(|i| i.id == 1).unwrap();
        assert_eq!(item1.keys, op + 1, "after op {op} the focused row is still item 1's: {:?}", hits(&app.owner));
        assert_eq!(app.owner.items.iter().map(|i| i.keys).sum::<u32>(), op + 1, "no other row heard the key");
    }
}

/// `&Vec<T: View>` is ordinal: what the kernel keeps per row does not follow the item.
struct VItem {
    id: u32,
}
impl View for VItem {
    fn view(&self) -> El<'_> {
        row(text(self.id).w(30)).h(20).bg(rgb(0xdddddd)).focusable().on_key(Key::Char('x'), VBoard::heard.with(self.id))
    }
}
struct VBoard {
    items: Vec<VItem>,
    heard: Vec<u32>,
}
impl VBoard {
    fn heard(&mut self, id: u32) {
        self.heard.push(id);
    }
}
impl View for VBoard {
    fn view(&self) -> El<'_> {
        column((text("v"), column(&self.items).gap(2))).w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff))
    }
}

#[test]
fn a_vec_child_is_ordinal() {
    let mut app = App::new(VBoard { items: (0..4).map(|id| VItem { id }).collect(), heard: vec![] }, cfg(), true);
    shot(&mut app);
    click(&mut app, 5.0, row_y(1));
    let none = Mods::default();
    app.key_down(Key::Char('x'), none);
    app.turn(|b| b.items.reverse());
    let p = shot(&mut app);
    app.key_down(Key::Char('x'), none);
    // the position was realigned (here: mount-cycled, the reorder read as a removal and an insertion), so the
    // focus did not follow item 1, as it does in a `List`
    assert_eq!(app.owner.heard.iter().filter(|&&i| i == 1).count(), 1, "the focus did not follow item 1: {:?}", app.owner.heard);
    let mut fresh = App::new(VBoard { items: (0..4).rev().map(|id| VItem { id }).collect(), heard: vec![] }, cfg(), true);
    app.pointer_leave();
    assert_eq!(shot(&mut app), shot(&mut fresh), "the realigned rows match a fresh mount");
    let _ = p;
}

// ------------------------------------------------------------------------------------------- routes

#[test]
fn a_row_route_reaches_its_row_after_reorders() {
    let mut r = Rng(7);
    let mut app = App::new(Board::new((0..6).map(Item::new)), cfg(), true);
    shot(&mut app);
    let mut expect: std::collections::HashMap<u32, u32> = Default::default();
    for step in 0..24 {
        let n = app.owner.items.len();
        app.turn(|b| match step % 4 {
            0 => b.items.reverse(),
            1 => b.items.sort_by_key(|i| (i.id * 5 + step) % 7),
            2 => b.items.move_to(0, n - 1),
            _ => {
                b.items.insert(1, Item::new(200 + step));
            }
        });
        shot(&mut app);
        let k = r.below(app.owner.items.len().min(8));
        let id = app.owner.items.get(k).unwrap().id;
        click(&mut app, 12.0, row_y(k));
        *expect.entry(id).or_default() += 1;
        for i in app.owner.items.iter() {
            assert_eq!(i.hits, expect.get(&i.id).copied().unwrap_or(0), "step {step}: clicked position {k} (item {id}); item {}", i.id);
        }
    }
    app.pointer_leave();
    let p = shot(&mut app);
    let mut fresh = App::new(app.owner.copy(), cfg(), true);
    assert_eq!(p, shot(&mut fresh), "the clicked rows re-described as a fresh mount shows them");
}

#[test]
fn bound_routes_event_routes_and_root_routes_in_rows() {
    let mut app = App::new(Board::new((0..4).map(Item::new)), cfg(), true);
    shot(&mut app);
    app.turn(|b| b.items.reverse());
    shot(&mut app);
    // the blue box: `Item::add.with(10)` on the row's own item
    let x_add = 30.0 + 90.0 + 30.0 + 30.0 + 8.0;
    let x_pick = x_add + 16.0;
    click(&mut app, x_add, row_y(0));
    assert_eq!(app.owner.items.get(0).unwrap().id, 3);
    // item 3's note starts at 3 (`id % 7`)
    assert_eq!(app.owner.items.get(0).unwrap().note, 13, "a bound route on the row's item");
    // the orange box: `Board::picked.with(id)` on the root, from inside the row
    click(&mut app, x_pick, row_y(2));
    assert_eq!(app.owner.picked, [1], "a route to the root from a row");
    // `fn(&mut Item, &Ev)`: the press position
    click(&mut app, 12.0, row_y(1));
    assert_eq!(app.owner.items.get(1).unwrap().at, 12.0, "an event route on the row");
    app.pointer_leave();
    let p = shot(&mut app);
    let mut fresh = App::new(app.owner.copy(), cfg(), true);
    assert_eq!(p, shot(&mut fresh));
}

/// A row that captures the pointer: its drag route runs for as long as the button is held.
struct Grip {
    id: u32,
    drags: u32,
}
impl Grip {
    fn dragged(&mut self, _: &Ev) {
        self.drags += 1;
    }
}
impl View for Grip {
    fn view(&self) -> El<'_> {
        row(text(self.id)).h(20).bg(rgb(0xdddddd)).on_drag(Grip::dragged)
    }
}
struct Grips {
    items: List<Grip>,
}
impl View for Grips {
    fn view(&self) -> El<'_> {
        column((text("g"), column(&self.items).gap(2))).w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff))
    }
}

#[test]
fn a_late_event_for_a_removed_row_does_nothing() {
    let mut app = App::new(Grips { items: List::from_iter_in((0..4).map(|id| Grip { id, drags: 0 })) }, cfg(), true);
    shot(&mut app);
    // press on item 2: its drag route captures the pointer
    app.pointer_move(4.0, row_y(2));
    app.pointer_down(4.0, row_y(2), Button::Primary);
    app.pointer_move(5.0, row_y(2));
    assert_eq!(app.owner.items.get(2).unwrap().drags, 1, "the drag route runs on its row");
    app.turn(|b| {
        b.items.remove(2);
    });
    shot(&mut app);
    app.turn(|b| {
        b.items.push(Grip { id: 2, drags: 0 }); // a new item (perhaps in the freed slot, a later generation)
    });
    shot(&mut app);
    app.pointer_move(6.0, row_y(1));
    app.pointer_up(6.0, row_y(1), Button::Primary);
    assert!(app.owner.items.iter().all(|i| i.drags == 0), "no row heard the removed row's drag: {:?}", app.owner.items.iter().map(|i| (i.id, i.drags)).collect::<Vec<_>>());
}

/// Nested lists of one owner type: a route resolves to the nearest owner of its receiver type.
struct Node {
    id: u32,
    hits: u32,
    kids: List<Node>,
}
impl Node {
    fn hit(&mut self) {
        self.hits += 1;
    }
}
impl View for Node {
    fn view(&self) -> El<'_> {
        column((row(text(self.id).w(60)).h(18).bg(rgb(0xcceecc)).on_click(Node::hit), column(&self.kids).gap(1))).p(2)
    }
}
struct Tree {
    nodes: List<Node>,
}
impl View for Tree {
    fn view(&self) -> El<'_> {
        column(&self.nodes).w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff))
    }
}

#[test]
fn nested_owners_of_one_type_resolve_to_the_nearest() {
    let leaf = |id| Node { id, hits: 0, kids: List::new() };
    let tree = Tree { nodes: List::from_iter_in([Node { id: 1, hits: 0, kids: List::from_iter_in([leaf(10), leaf(11)]) }, Node { id: 2, hits: 0, kids: List::new() }]) };
    let mut app = App::new(tree, cfg(), true);
    shot(&mut app);
    // node 1's line (2 + 9), then its kids: 2 + 18 + 2 (padding) + kid k at (18 + 4 + 1) apart
    let kid_y = |k: usize| 2.0 + 18.0 + 2.0 + k as f32 * (18.0 + 4.0 + 1.0) + 9.0;
    click(&mut app, 20.0, kid_y(1));
    let n1 = app.owner.nodes.get(0).unwrap();
    assert_eq!((n1.hits, n1.kids.get(0).unwrap().hits, n1.kids.get(1).unwrap().hits), (0, 0, 1), "the kid's own line: the nearest `Node`");
    app.turn(|t| t.nodes.edit(0).kids.reverse());
    shot(&mut app);
    click(&mut app, 20.0, kid_y(1));
    let n1 = app.owner.nodes.get(0).unwrap();
    assert_eq!((n1.kids.get(0).unwrap().id, n1.kids.get(1).unwrap().hits), (11, 1), "after the kids' reverse, position 1 is node 10");
    assert_eq!(n1.kids.iter().map(|k| k.hits).sum::<u32>(), 2);
    click(&mut app, 20.0, 11.0);
    assert_eq!(app.owner.nodes.get(0).unwrap().hits, 1, "the outer line: the outer node");
    app.pointer_leave();
    let p = shot(&mut app);
    let copy = |n: &Node| -> Node {
        fn c(n: &Node) -> Node {
            Node { id: n.id, hits: n.hits, kids: List::from_iter_in(n.kids.iter().map(c)) }
        }
        c(n)
    };
    let mut fresh = App::new(Tree { nodes: List::from_iter_in(app.owner.nodes.iter().map(copy)) }, cfg(), true);
    assert_eq!(p, shot(&mut fresh), "nested regions match a fresh mount");
}

struct Other;
impl Other {
    fn poke(&mut self) {}
}
struct Stray {
    id: u32,
}
impl View for Stray {
    fn view(&self) -> El<'_> {
        text(self.id).on_click(Other::poke)
    }
}
struct StrayBoard {
    items: List<Stray>,
}
impl View for StrayBoard {
    fn view(&self) -> El<'_> {
        column(&self.items)
    }
}

#[test]
#[should_panic(expected = "a route taking `&mut el_lists::Other` is mounted in a view of `el_lists::Stray`, whose owner chain is `el_lists::Stray` < `el_lists::StrayBoard`")]
fn a_row_route_off_the_chain_panics_naming_both() {
    let mut app = App::new(StrayBoard { items: List::from_iter_in([Stray { id: 1 }]) }, cfg(), false);
    app.frame();
}

// ------------------------------------------------------------------------------------------- each

impl Item {
    fn compact(&self) -> El<'_> {
        row((text(self.id).w(30), text(&self.label))).h(16).bg(rgb(0xeeeeff)).on_click(Item::hit)
    }
    fn slotted(&self, s: Slot<Item>) -> El<'_> {
        row((text(self.id).w(30), text(s.index()).w(30))).h(16).bg(rgb(0xeeffee)).on_click(Each2::take.with(s))
    }
}
struct Each2 {
    items: List<Item>,
    slotted: bool,
}
impl Each2 {
    fn take(&mut self, s: Slot<Item>) {
        self.items.take(s);
    }
}
impl View for Each2 {
    fn view(&self) -> El<'_> {
        if self.slotted { column(self.items.each(Item::slotted)).gap(1) } else { column(self.items.each(Item::compact)).gap(1) }
    }
}

#[test]
fn each_takes_both_function_shapes() {
    for slotted in [false, true] {
        let mut app = App::new(Each2 { items: List::from_iter_in((0..5).map(Item::new)), slotted }, cfg(), true);
        shot(&mut app);
        app.turn(|b| b.items.reverse());
        shot(&mut app);
        click(&mut app, 10.0, 17.0 + 8.0); // position 1: item 3
        if slotted {
            assert_eq!(app.owner.items.iter().map(|i| i.id).collect::<Vec<_>>(), [4, 2, 1, 0], "the slot-carrying route took its row");
        } else {
            assert_eq!(app.owner.items.get(1).unwrap().hits, 1, "the projection's route reached its row");
        }
        app.pointer_leave();
        let p = shot(&mut app);
        let mut fresh = App::new(Each2 { items: List::from_iter_in(app.owner.items.iter().map(Item::copy)), slotted }, cfg(), true);
        if !slotted {
            // (the slotted rows show their slot ids, which a fresh list numbers anew)
            assert_eq!(p, shot(&mut fresh), "each({slotted}) matches a fresh mount");
        }
    }
}

// ------------------------------------------------------------------------------------------- misuse

struct Far {
    lists: Vec<List<Item>>,
}
impl View for Far {
    fn view(&self) -> El<'_> {
        column(&self.lists[0])
    }
}

#[test]
#[should_panic(expected = "`bough::list::List<el_lists::Item>` is a child of a view of `el_lists::Far` but is not a field of it")]
fn a_list_that_is_not_a_field_panics_naming_it() {
    let _ = App::new(Far { lists: vec![List::from_iter_in([Item::new(1)])] }, cfg(), false);
}

struct Sideways {
    items: List<Item>,
}
impl View for Sideways {
    fn view(&self) -> El<'_> {
        row(&self.items)
    }
}

#[test]
#[should_panic(expected = "in a row: a list region stacks its rows top to bottom")]
fn a_list_in_a_row_panics() {
    let _ = App::new(Sideways { items: List::from_iter_in([Item::new(1)]) }, cfg(), false);
}


// ------------------------------------------------------------------------------------------- the region as an item

/// A list child inside a branch, and a list child whose gap changes: the region leaves (or is replaced) with all
/// the rows it mounted since its owner last described it.
struct Branch {
    items: List<Grip>,
    show: bool,
    gap: u32,
}
impl View for Branch {
    fn view(&self) -> El<'_> {
        column((text("b"), self.show.then(|| column(&self.items).gap(self.gap)), text("end"))).w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff))
    }
}

#[test]
fn a_list_child_leaves_and_is_replaced_whole() {
    let fresh = |b: &Branch| App::new(Branch { items: List::from_iter_in(b.items.iter().map(|g| Grip { id: g.id, drags: g.drags })), show: b.show, gap: b.gap }, cfg(), true);
    let mut app = App::new(Branch { items: List::from_iter_in((0..3).map(|id| Grip { id, drags: 0 })), show: true, gap: 2 }, cfg(), true);
    shot(&mut app);
    // rows mounted after the owner's last description (a push is placed, not described)
    app.turn(|b| {
        b.items.push(Grip { id: 7, drags: 0 });
        b.items.push(Grip { id: 8, drags: 0 });
    });
    shot(&mut app);
    for (k, (show, gap)) in [(false, 2), (true, 2), (true, 5), (true, 1), (false, 1), (true, 3)].into_iter().enumerate() {
        app.turn(|b| {
            b.items.insert(1, Grip { id: 20 + k as u32, drags: 0 });
        });
        shot(&mut app);
        app.turn(|b| {
            b.show = show;
            b.gap = gap;
        });
        let p = shot(&mut app);
        assert_eq!(p, shot(&mut fresh(&app.owner)), "step {k}: show {show}, gap {gap}");
    }
}

/// A list inside a list's row, behind a branch of that row: rows pushed into the inner list (mounted outside a
/// walk) are counted by the row's own dynamic items, so closing the branch takes out the run it holds now.
struct Folder {
    id: u32,
    open: bool,
    files: List<Grip>,
}
impl View for Folder {
    fn view(&self) -> El<'_> {
        column((row(text(self.id).w(40)).h(18).bg(rgb(0xddeedd)), self.open.then(|| column(&self.files).gap(1)), text("·"))).p(1)
    }
}
struct Folders {
    folders: List<Folder>,
}
impl View for Folders {
    fn view(&self) -> El<'_> {
        column((text("f"), column(&self.folders).gap(2))).w_pct(1.0).h_pct(1.0).bg(rgb(0xffffff))
    }
}

#[test]
fn a_list_in_a_row_leaves_with_the_rows_it_mounted() {
    let copy = |f: &Folders| Folders {
        folders: List::from_iter_in(f.folders.iter().map(|d| Folder { id: d.id, open: d.open, files: List::from_iter_in(d.files.iter().map(|g| Grip { id: g.id, drags: 0 })) })),
    };
    let folder = |id: u32| Folder { id, open: true, files: List::from_iter_in([Grip { id: id * 10, drags: 0 }]) };
    let mut app = App::new(Folders { folders: List::from_iter_in([folder(1), folder(2)]) }, cfg(), true);
    shot(&mut app);
    // a push into the second folder changes the outer region's total too
    app.turn(|f| {
        f.folders.edit(1).files.push(Grip { id: 250, drags: 0 });
    });
    assert_eq!(shot(&mut app), shot(&mut App::new(copy(&app.owner), cfg(), true)), "an inner list changes its enclosing row's run");
    for (k, step) in ["push", "close", "open", "push", "push", "close", "open", "close"].into_iter().enumerate() {
        let id = 100 + k as u32;
        app.turn(|f| {
            let d = f.folders.edit(0);
            match step {
                "push" => {
                    d.files.push(Grip { id, drags: 0 });
                }
                "close" => d.open = false,
                _ => d.open = true,
            }
        });
        let p = shot(&mut app);
        let fresh = shot(&mut App::new(copy(&app.owner), cfg(), true));
        assert_eq!(p, fresh, "step {k} ({step})");
    }
}
