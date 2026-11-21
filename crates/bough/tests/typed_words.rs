//! Integration Card 4: a turn that writes only bound words describes nothing and allocates nothing.
//!
//! The view's colour and its scroll offset are signals bound with `sig!`; a handler flips the colour. The turn
//! is the world's compare of the block and a delta over the columns that read the word: no description (count
//! band, exact) and no allocation (a counting global allocator), with the four oracles after every step.
use bough::input::Button;
use bough::lower::provenance;
use bough::prelude::*;
use bough::raw::Config;
use bough::typed::App;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

struct Counting;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
// SAFETY: every method forwards to the system allocator unchanged; the counter is the only addition.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller's contract, forwarded.
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: the caller's contract, forwarded.
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller's contract, forwarded.
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static A: Counting = Counting;

struct Row {
    n: u32,
}
impl View for Row {
    fn view(&self) -> impl Element<Self> + '_ {
        row((text(self.n), text("a row")))
            .gap(8.0)
            .h(24.0)
            .px(8.0)
            .bg(rgb(0xf3f4f6))
            .hover_bg(rgb(0xdbeafe))
    }
}
struct Lamp {
    on: Signal<bool>,
    tint: Signal<Color>,
    scroll: Signal<f32>,
    rows: List<Row>,
    plain: u32,
}
impl Lamp {
    fn flip(&mut self) {
        let v = !*self.on;
        self.on.set(v);
    }
    fn bump(&mut self) {
        self.plain += 1;
    }
}
impl View for Lamp {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            row((
                block(40.0, 40.0)
                    .bg(sig!(self.on).map(|on| if *on { rgb(0xfacc15) } else { rgb(0x374151) }))
                    .rounded(20.0),
                block(40.0, 40.0).bg(sig!(self.tint)),
                button("flip").on_click(Lamp::flip),
                button("bump").on_click(Lamp::bump),
                text(self.plain),
            ))
            .gap(12.0)
            .p(12.0)
            .items(Align::Center),
            list(sig!(self.rows))
                .scroll(sig!(self.scroll))
                .estimate(24.0)
                .grow(1.0)
                .w_pct(1.0)
                .bg(rgb(0xffffff)),
        ))
        .w_pct(1.0)
        .h_pct(1.0)
        .bg(rgb(0xe5e7eb))
    }
}
fn lamp() -> Lamp {
    let mut rows = List::new();
    for n in 0..500 {
        rows.push(Row { n });
    }
    Lamp {
        on: Signal::new(false),
        tint: Signal::new(rgb(0x2563eb)),
        scroll: Signal::new(0.0),
        rows,
        plain: 0,
    }
}
fn allocs(f: impl FnOnce()) -> u64 {
    let a = ALLOCS.load(Ordering::Relaxed);
    f();
    ALLOCS.load(Ordering::Relaxed) - a
}

#[test]
fn a_turn_that_writes_only_bound_words_describes_nothing_and_allocates_nothing() {
    for ring in 1..=4 {
        let mut cfg = Config::new(360.0, 300.0, 1.0, bough::ui::default_font());
        cfg.ring = ring;
        let mut app = match App::new(lamp(), cfg) {
            Ok(a) => a,
            Err((_, why)) => panic!("not admitted: {why:?}"),
        };
        app.check().unwrap();
        // warm every path once (the world's scratch vectors are reserved on first use)
        app.turn(|l| l.flip());
        app.pointer_move(100.0, 150.0);
        app.wheel(100.0, 150.0, 0.0, 1.0);
        // every row the measured turns will expose is exposed once here: a row's text is shaped (and its run
        // allocated) the first time it is exposed, which is not what the bands below are about
        for k in 0..=12 {
            app.turn(|l| {
                l.scroll.set(48.0 * k as f32);
            });
        }
        app.turn(|l| {
            l.scroll.set(0.0);
        });
        app.check().unwrap();
        let d0 = app.describes();
        // a handler writes one bound word
        let before = app.presented_pixels();
        let n = allocs(|| {
            app.pointer_down(140.0, 32.0, Button::Primary);
            app.pointer_up(140.0, 32.0, Button::Primary);
        });
        assert!(*app.view().on == false, "the click reached `flip`");
        assert_eq!(
            app.describes(),
            d0,
            "ring {ring}: a handler that wrote one bound word described nothing"
        );
        assert_eq!(n, 0, "ring {ring}: and allocated nothing");
        app.check()
            .unwrap_or_else(|e| panic!("ring {ring} after flip: {e}"));
        assert_ne!(before, app.presented_pixels(), "the lamp changed colour");
        // a turn writes two bound words (a colour and the scroll offset)
        let n = allocs(|| {
            app.turn(|l| {
                l.tint.set(rgb(0xdc2626));
                l.scroll.set(240.0);
            })
        });
        assert_eq!(
            (app.describes(), n),
            (d0, 0),
            "ring {ring}: two bound words: no description, no allocation"
        );
        app.check()
            .unwrap_or_else(|e| panic!("ring {ring} after tint and scroll: {e}"));
        // hover across rows, and the wheel: runtime events
        let n = allocs(|| {
            for k in 0..5 {
                app.pointer_move(100.0, 120.0 + 24.0 * k as f32);
            }
            app.wheel(100.0, 150.0, 0.0, 2.0);
        });
        assert_eq!(
            (app.describes(), n),
            (d0, 0),
            "ring {ring}: hover and wheel: no description, no allocation"
        );
        app.check()
            .unwrap_or_else(|e| panic!("ring {ring} after hover and wheel: {e}"));
        // a bound signal written to the value it has: the counts differ, so the view is described (and shows the same)
        app.turn(|l| {
            let v = *l.tint;
            *l.tint = v;
        });
        assert_eq!(
            app.describes(),
            d0 + 1,
            "a write that changed no word is not accounted for by the block"
        );
        // a handler that writes plain state and no signal: described, as the façade's contract says
        app.pointer_down(200.0, 32.0, Button::Primary);
        app.pointer_up(200.0, 32.0, Button::Primary);
        assert_eq!(app.view().plain, 1, "the click reached `bump`");
        assert_eq!(
            app.describes(),
            d0 + 2,
            "plain state is state: the view is described again"
        );
        app.check()
            .unwrap_or_else(|e| panic!("ring {ring} after bump: {e}"));
    }
    let p = provenance::counts();
    assert_eq!((p.legacy(), p.describe), (0, 0));
}
