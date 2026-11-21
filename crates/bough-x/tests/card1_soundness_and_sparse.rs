//! Card 1's sparse incidence and packed-place gates.
use bough_x::expr::{Code, Frame};
use bough_x::prelude::*;
use bough_x::soft::Soft;
use bough_x::store::{Path, Tiles};

mod allocations {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;
    thread_local! {
        static ENABLED: Cell<bool> = const { Cell::new(false) };
        static COUNT: Cell<usize> = const { Cell::new(0) };
    }
    pub struct Counting;
    fn allocated() {
        if ENABLED.try_with(Cell::get).unwrap_or(false) {
            COUNT.with(|n| n.set(n.get() + 1));
        }
    }
    // SAFETY: all allocation operations delegate the same arguments and results
    // to System. The thread-local integer accounting never allocates.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            allocated();
            // SAFETY: forwarding the caller's valid Layout to System.
            unsafe { System.alloc(layout) }
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            allocated();
            // SAFETY: forwarding the caller's valid Layout to System.
            unsafe { System.alloc_zeroed(layout) }
        }
        unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            allocated();
            // SAFETY: caller's pointer/layout/size contract is forwarded unchanged.
            unsafe { System.realloc(p, layout, size) }
        }
        unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
            // SAFETY: caller's live allocation and matching layout are forwarded.
            unsafe { System.dealloc(p, layout) }
        }
    }
    pub fn count(f: impl FnOnce()) -> usize {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                ENABLED.with(|v| v.set(false));
            }
        }
        COUNT.with(|v| v.set(0));
        ENABLED.with(|v| assert!(!v.replace(true)));
        let reset = Reset;
        f();
        drop(reset);
        COUNT.with(Cell::get)
    }
}
#[global_allocator]
static ALLOCATOR: allocations::Counting = allocations::Counting;

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
        let mut w = World::<_, Soft>::new(96, 32, 48);
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
    let mut w = World::<_, Soft>::new(96, 32, 48);
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
    World::<Matrix, Soft>::new(32, 32, 0).place(&Path::default(), 0, 1 << 20);
}
#[test]
#[should_panic(expected = "place column exceeds 12-bit capacity")]
fn place_rejects_column_spill() {
    World::<Matrix, Soft>::new(32, 32, 0).place(&Path::default(), 4096, 0);
}

#[path = "../scenes/sheet_el.rs"]
mod sheet_el;

#[test]
fn sheet_el_retained_ledger() {
    // the element sheet is one deep view: its description frame is large. A test thread gets 2 MiB on macOS,
    // so the body runs on a thread with a stack the view fits (the app itself runs on the main thread).
    std::thread::Builder::new()
        .stack_size(32 << 20)
        .spawn(|| {
            assert!(!core::mem::needs_drop::<sheet_el::Sheet>());
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
        })
        .unwrap()
        .join()
        .unwrap();
}

mod padded_closure {
    use bough_x::el::prelude::*;
    use bough_x::reflect::Reflect;
    use bough_x::soft::{Immediate, Soft};
    use std::num::NonZeroU32;

    struct PaddedView {
        look: Signal<u32>,
        capture: (bool, u8, u32),
        nz: NonZeroU32,
    }
    impl View for PaddedView {
        fn view(&self) -> impl Element<Self> + '_ {
            let capture = self.capture;
            let nz = self.nz;
            let label: &'static str = "capture";
            div()
                .w(32.0)
                .h(32.0)
                .bg(sig!(self.look).map(move |look: &u32| {
                    let tuple = std::hint::black_box(capture);
                    rgb(*look
                        ^ tuple.2
                        ^ (tuple.0 as u32)
                        ^ tuple.1 as u32
                        ^ nz.get()
                        ^ label.len() as u32)
                }))
        }
    }
    #[test]
    fn padded_and_nonzero_captures_are_typed_cpu_records() {
        for once in [false, true] {
            let mut app = App::<_, Soft>::new(
                PaddedView {
                    look: Signal::new(7),
                    capture: (true, 11, 0xaabbcc),
                    nz: NonZeroU32::new(19).unwrap(),
                },
                32,
                32,
                1.0,
            );
            app.once = once;
            for i in 0..5 {
                app.turn(|v| {
                    v.look.set(i);
                });
                app.v.capture = (false, 27, 0x334455 + i);
                app.world.redescribe(&app.v);
                let mut full = Soft::new(32, 32);
                app.world.paint(&mut full);
                let mut now = Soft::new(32, 32);
                bough_x::sink::View::view(&app.v, &mut Immediate::new(&mut now, &app.world.shown));
                assert_eq!(full.px, now.px);
                let reflection = Reflect::learn(&app.world, &app.v);
                let items = reflection.items(app.world.root(), &app.world.shown);
                assert!(!items.is_empty());
                for item in items {
                    assert!(item.words().is_empty());
                    let _ = item.snap();
                }
            }
        }
    }
}

mod safe_clients {
    use bough_x::el::{self, prelude::*};
    use bough_x::expr::{APP_WORDS, Expr, Lens, Slot, U, Un};
    use bough_x::soft::Soft;
    use bough_x::store::Path;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[repr(C)]
    struct Padded {
        first: u8,
        next: Signal<u32>,
    }
    impl View for Padded {
        fn view(&self) -> impl Element<Self> + '_ {
            div()
                .w(32.0)
                .h(32.0)
                .bg(sig!(self.next).map(|v: &u32| rgb(*v)))
        }
    }
    #[test]
    fn arbitrary_nominal_offset_reads_only_the_safe_projection() {
        const OFF: usize = usize::MAX;
        let p = Padded {
            first: 17,
            next: Signal::new(0xaabbcc),
        };
        let _ = el::prove::<Padded, _, OFF>(&p.next, |v| &v.next).get();
        let mut slots = Vec::new();
        <Un<el::At<Padded, u32, OFF>> as Expr>::slots(&mut |s| slots.push(s));
        let mut out = U::default();
        let out = bough_x::sink::View::gathered(&p, &slots, &mut out);
        assert_eq!(out.w[<el::At<Padded, u32, OFF> as Lens>::SLOT], 0xaabbcc);
    }
    #[test]
    fn foreign_gathered_owner_is_refused_before_reader_execution() {
        let p = Padded {
            first: 1,
            next: Signal::new(2),
        };
        let slot = Slot::typed::<u32, ()>(0, 0, |_| panic!("foreign reader executed"));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut out = U::default();
            bough_x::sink::View::gathered(&p, &[slot], &mut out);
        }));
        assert!(result.is_err());
    }
    #[test]
    #[should_panic(expected = "field slot outside application block")]
    fn out_of_block_slot_is_refused() {
        let _ = Slot::typed::<u32, ()>(APP_WORDS, 0, |v| *v);
    }
    struct Collision {
        a: Signal<u32>,
        b: Signal<u32>,
    }
    impl View for Collision {
        fn view(&self) -> impl Element<Self> + '_ {
            column((
                div()
                    .w(32.0)
                    .h(32.0)
                    .bg(el::prove::<Self, _, 0>(&self.a, |v| &v.a).map(|v: &u32| rgb(*v))),
                div()
                    .w(32.0)
                    .h(32.0)
                    .bg(el::prove::<Self, _, 192>(&self.b, |v| &v.b).map(|v: &u32| rgb(*v))),
            ))
        }
    }
    #[test]
    #[should_panic(expected = "two fields of the view share word")]
    fn distinct_projections_cannot_alias_a_slot() {
        let _ = App::<_, Soft>::new(
            Collision {
                a: Signal::new(1),
                b: Signal::new(2),
            },
            32,
            64,
            1.0,
        );
    }
    struct Counted {
        look: Signal<u32>,
        calls: Arc<AtomicUsize>,
    }
    impl View for Counted {
        fn view(&self) -> impl Element<Self> + '_ {
            self.calls.fetch_add(1, Ordering::Relaxed);
            div()
                .w(32.0)
                .h(32.0)
                .bg(sig!(self.look).map(|v: &u32| rgb(*v)))
        }
    }
    #[test]
    fn single_owner_steady_state_allocates_nothing_and_signal_has_no_drop() {
        assert!(!core::mem::needs_drop::<Signal<u32>>());
        assert!(!core::mem::needs_drop::<Signal<f32>>());
        let calls = Arc::new(AtomicUsize::new(0));
        let mut app = App::<_, Soft>::new(
            Counted {
                look: Signal::new(1),
                calls: calls.clone(),
            },
            32,
            32,
            1.0,
        )
        .once();
        let baseline = calls.load(Ordering::Relaxed);
        let retables = app.world.retables;
        let allocations = super::allocations::count(|| {
            for value in 2..1002 {
                assert!(app.v.look.set(value));
                assert_eq!(*app.v.look, value);
                app.settle();
            }
        });
        assert_eq!(
            allocations, 0,
            "steady signal writes, reads and settles allocated"
        );
        assert_eq!(calls.load(Ordering::Relaxed), baseline);
        assert_eq!(app.world.retables, retables);
    }

    #[test]
    fn shared_signal_notifies_its_worlds_but_not_an_unrelated_app() {
        use bough_x::world::World;
        let calls = Arc::new(AtomicUsize::new(0));
        let mut unrelated = App::<_, Soft>::new(
            Counted {
                look: Signal::new(1),
                calls: calls.clone(),
            },
            32,
            32,
            1.0,
        )
        .once();
        let mut shared = Counted {
            look: Signal::new(1),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let mut a = World::<Counted, Soft>::new(32, 32, APP_WORDS);
        let mut b = World::<Counted, Soft>::new(32, 32, APP_WORDS);
        a.mount(&shared);
        b.mount(&shared);
        // The third world observes an already-shared sentinel, not a single-owner token.
        let mut c = World::<Counted, Soft>::new(32, 32, APP_WORDS);
        c.mount(&shared);
        let before = [a.input_revision(), b.input_revision(), c.input_revision()];
        let own = unrelated.world.input_revision();
        let descriptions = calls.load(Ordering::Relaxed);
        let retables = unrelated.world.retables;
        shared.look.set(2);
        for (world, before) in [a, b, c].iter().zip(before) {
            assert!(world.input_revision() > before);
        }
        unrelated.settle();
        assert_eq!(unrelated.world.input_revision(), own);
        assert_eq!(calls.load(Ordering::Relaxed), descriptions);
        assert_eq!(unrelated.world.retables, retables);
    }

    #[test]
    fn stale_signal_token_is_inert_until_observed_by_a_new_world() {
        use bough_x::world::World;
        let mut model = Counted {
            look: Signal::new(1),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let mut old = World::<Counted, Soft>::new(32, 32, APP_WORDS);
        old.mount(&model);
        drop(old);
        let mut new = World::<Counted, Soft>::new(32, 32, APP_WORDS);
        let before = new.input_revision();
        model.look.set(2);
        assert_eq!(new.input_revision(), before);
        new.mount(&model);
        let before = new.input_revision();
        model.look.set(3);
        assert_eq!(new.input_revision(), before + 1);
    }

    #[test]
    fn worlds_are_isolated_and_foreign_tokens_are_refused() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut a = App::<_, Soft>::new(
            Counted {
                look: Signal::new(1),
                calls: calls.clone(),
            },
            32,
            32,
            1.0,
        )
        .once();
        let baseline = calls.load(Ordering::Relaxed);
        let retables = a.world.retables;
        let mut b = App::<_, Soft>::new(
            Counted {
                look: Signal::new(1),
                calls: Arc::new(AtomicUsize::new(0)),
            },
            32,
            32,
            1.0,
        )
        .once();
        b.turn(|v| {
            v.look.set(2);
        });
        a.settle();
        assert_eq!(calls.load(Ordering::Relaxed), baseline);
        assert_eq!(a.world.retables, retables);
        let token = a.world.place_token(&Path::default(), 0, 0).unwrap();
        assert!(b.world.at_token(token).is_none());
        assert!(!b.world.accept_pick(&mut b.v, token).consumed);
        let reflection = bough_x::reflect::Reflect::learn(&a.world, &a.v);
        assert!(!reflection.current(b.world.root()));
        let after_reflection = calls.load(Ordering::Relaxed);
        // External writes are still tracked after App and Signal have moved.
        a.v.look.set(3);
        a.settle();
        assert_eq!(calls.load(Ordering::Relaxed), after_reflection);
        assert_ne!(a.world.shown, b.world.shown);
    }
}

#[test]
fn unrelated_relation_edits_do_not_describe_or_retable() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct Related {
        u: U,
        rows: Rel<u32>,
        calls: Arc<AtomicUsize>,
    }
    impl View for Related {
        fn block(&self) -> &U {
            &self.u
        }
        fn view<S: Sink<Self>>(&self, s: &mut S) {
            self.calls.fetch_add(1, Ordering::Relaxed);
            s.list(
                &List {
                    n: self.rows.len() as u32,
                    row_h: 10,
                    x: 0,
                    y: 0,
                    shape: &[Rect::new(0, 0, 32, 10)],
                    edits: &self.rows.journal,
                },
                |i| Fill(rec(Color(0xff000000 | *self.rows.get(i)))),
            );
        }
    }
    let make = || Related {
        u: U::default(),
        rows: Rel::new(vec![1, 2, 3]),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let a = make();
    let mut b = make();
    let mut wa = World::<Related, Soft>::new(32, 32, 0);
    let mut wb = World::<Related, Soft>::new(32, 32, 0);
    wa.mount(&a);
    wb.mount(&b);
    let before = a.calls.load(Ordering::Relaxed);
    let retables = wa.retables;
    *b.rows.edit(0) = 9;
    wb.settle(&b);
    wa.settle(&a);
    assert_eq!(a.calls.load(Ordering::Relaxed), before);
    assert_eq!(wa.retables, retables);
    assert_eq!(*b.rows.get(0), 9);
}

#[test]
fn signal_into_inner_moves_and_drops_exactly_once() {
    let value = std::sync::Arc::new(());
    let signal = bough_x::el::Signal::new(value.clone());
    let extracted = signal.into_inner();
    assert_eq!(std::sync::Arc::strong_count(&value), 2);
    drop(extracted);
    assert_eq!(std::sync::Arc::strong_count(&value), 1);
}

#[test]
#[should_panic(expected = "world column capacity exceeds 4096")]
fn excess_columns_are_refused_at_admission() {
    let v = Matrix {
        u: U::default(),
        columns: 4097,
        many: false,
    };
    World::<Matrix, Soft>::new(96, 32, 48).mount(&v);
}

#[test]
fn typed_slot_construction_and_projected_reads_allocate_nothing() {
    use bough_x::expr::Slot;
    #[repr(C, align(64))]
    struct Padded {
        first: u8,
        next: u32,
    }
    let v = Padded { first: 3, next: 42 };
    let count = allocations::count(|| {
        for _ in 0..1000 {
            let direct = Slot::typed::<Padded, ()>(0, usize::MAX, |v| v.next);
            let projected =
                Slot::projected::<Padded, u32, u32>(1, usize::MAX, |v| &v.next, |word| *word);
            assert_eq!(
                std::hint::black_box(direct).read(std::hint::black_box(&v)),
                42
            );
            assert_eq!(
                std::hint::black_box(projected).read(std::hint::black_box(&v)),
                42
            );
        }
    });
    assert_eq!(count, 0);
    let projected = Slot::projected::<Padded, u32, ()>(1, 0, |v| &v.next, |word| *word);
    assert!(std::panic::catch_unwind(|| projected.read(&42u32)).is_err());
}

#[test]
fn allocation_counter_detects_a_deliberate_allocation() {
    let count = allocations::count(|| {
        drop(std::hint::black_box(Box::new(std::hint::black_box(7u32))));
    });
    assert!(
        count > 0,
        "positive control: allocator probe must detect an allocation"
    );
}

struct Overlapping(U);

impl View for Overlapping {
    fn block(&self) -> &U {
        &self.0
    }

    fn view<S: Sink<Self>>(&self, sink: &mut S) {
        for _ in 0..65 {
            sink.row(|| {
                (
                    [Rect::new(0, 0, 32, 32)],
                    Fill(Kc::<0xff11_2233>).on_hover(|_: &mut Self, _: &Ev| {}),
                )
            });
        }
    }
}

#[test]
#[ignore = "TR1-09 open"]
fn pointer_dispatch_does_not_allocate_past_64_overlaps() {
    let mut view = Overlapping(U::default());
    let mut world = World::<_, Soft>::new(32, 32, 0);
    world.mount(&view);
    let growth = allocations::count(|| {
        std::hint::black_box(world.pointer(&mut view, 1, 1));
    });
    assert_eq!(growth, 0, "allocations during the first overlapping pick");
}
