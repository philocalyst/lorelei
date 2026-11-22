//! The façade on the flat core (`bough::rethink`), held to its gates: the same prelude views as the lowered
//! runtime, pixel for pixel, through scripts of every kind of turn; and the flat core's own oracles on every
//! frame (retained == full redraw == fresh mount == the lowered canvas's immediate paint).
#[path = "../examples/deep_view.rs"]
mod deep_view;
#[path = "../examples/slice.rs"]
mod slice;

use bough::input::{Button, Key, Mods};
use bough::raw::Config;
use bough::rethink::FlatApp;
use slice::{ROW_H, Slice};

fn cfg(w: f32, h: f32, scale: f32) -> Config {
    let mut c = Config::new(w, h, scale, bough::ui::default_font());
    c.ring = 1;
    c
}
fn rng(seed: u64) -> impl FnMut(u64) -> u64 {
    let mut x = seed;
    move |m| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x % m
    }
}
fn centre(r: [f32; 4]) -> (f32, f32) {
    (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0)
}

/// Both runtimes, driven by the same calls.
struct Twin {
    low: bough::lower::App<Slice>,
    flat: FlatApp<Slice>,
}
impl Twin {
    fn new(n: usize, scale: f32) -> Twin {
        Twin { low: bough::lower::App::new(Slice::new(n), cfg(640.0, 480.0, scale)), flat: FlatApp::new(Slice::new(n), cfg(640.0, 480.0, scale)) }
    }
    fn both(&mut self, f: impl Fn(&mut dyn Rt)) {
        f(&mut self.low);
        f(&mut self.flat);
    }
    fn click(&mut self, (x, y): (f32, f32)) {
        self.both(|a| {
            a.down(x, y);
            a.up(x, y);
        });
    }
    fn check(&mut self, what: &str) {
        self.low.check().unwrap_or_else(|e| panic!("{what}: the lowered runtime: {e}"));
        self.flat.check().unwrap_or_else(|e| panic!("{what}: the flat runtime: {e}"));
        let (a, b) = (self.low.presented_pixels(), self.flat.presented_pixels());
        if a != b {
            let n = a.chunks_exact(4).zip(b.chunks_exact(4)).filter(|(x, y)| x != y).count();
            panic!("{what}: the two runtimes differ in {n} pixels");
        }
    }
}
trait Rt {
    fn mv(&mut self, x: f32, y: f32);
    fn down(&mut self, x: f32, y: f32);
    fn up(&mut self, x: f32, y: f32);
    fn wheel(&mut self, x: f32, y: f32, dy: f32);
    fn key(&mut self, k: Key);
    fn title(&mut self, t: &str);
    fn size(&mut self, w: f32, h: f32);
    fn leave(&mut self);
}
macro_rules! rt {
    ($t:ty) => {
        impl Rt for $t {
            fn mv(&mut self, x: f32, y: f32) {
                self.pointer_move(x, y)
            }
            fn down(&mut self, x: f32, y: f32) {
                self.pointer_down(x, y, Button::Primary)
            }
            fn up(&mut self, x: f32, y: f32) {
                self.pointer_up(x, y, Button::Primary)
            }
            fn wheel(&mut self, x: f32, y: f32, dy: f32) {
                <$t>::wheel(self, x, y, 0.0, dy)
            }
            fn key(&mut self, k: Key) {
                self.key_down(k, Mods::default())
            }
            fn title(&mut self, t: &str) {
                self.turn(|s| {
                    s.title.set(t.to_string());
                })
            }
            fn size(&mut self, w: f32, h: f32) {
                self.resize(w, h)
            }
            fn leave(&mut self) {
                self.pointer_leave()
            }
        }
    };
}
rt!(bough::lower::App<Slice>);
rt!(FlatApp<Slice>);

#[test]
fn the_slice_on_the_flat_core_equals_the_lowered_runtime_through_a_random_script() {
    for (seed, scale) in [(0x2545_f491_4f6c_dd1du64, 1.0f32), (77, 2.0)] {
        let mut t = Twin::new(300, scale);
        t.check("mount");
        let mut r = rng(seed);
        let mut kinds = [0u32; 13];
        for step in 0..90 {
            let what = r(13);
            kinds[what as usize] += 1;
            match what {
                0..=4 => {
                    let b = t.low.interactive();
                    assert!(b.len() >= 6, "five buttons and the list");
                    t.click(centre(b[what as usize]));
                }
                5 | 6 => {
                    let dy = r(7) as f32 - 3.0;
                    t.both(|a| a.wheel(320.0, 300.0, dy));
                }
                7 | 8 => {
                    let (x, y) = (r(640) as f32, r(480) as f32);
                    t.both(|a| a.mv(x, y));
                }
                9 => {
                    t.click((320.0, 300.0));
                    let k = [Key::Down, Key::Up, Key::PageDown, Key::PageUp][r(4) as usize];
                    t.both(|a| a.key(k));
                }
                10 => {
                    let title = format!("Slice {step}");
                    t.both(|a| a.title(&title));
                }
                11 => t.both(|a| a.leave()),
                _ => {
                    let (w, h) = (400.0 + r(400) as f32, 300.0 + r(300) as f32);
                    t.both(|a| a.size(w, h));
                }
            }
            t.check(&format!("seed {seed} step {step} (op {what})"));
        }
        assert!(kinds.iter().all(|k| *k > 0), "{kinds:?}");
    }
}

#[test]
fn a_wheel_scroll_of_the_slice_is_a_blit_and_a_hover_is_two_patches() {
    let mut app = FlatApp::new(Slice::new(1000), cfg(640.0, 480.0, 2.0));
    app.check().unwrap();
    // settle the list's measured heights
    app.wheel(320.0, 300.0, 0.0, 1.0);
    app.check().unwrap();
    let before = app.host.world.stats;
    for k in 0..6 {
        app.wheel(320.0, 300.0, 0.0, 1.0);
        app.check().unwrap_or_else(|e| panic!("wheel {k}: {e}"));
        assert_eq!(app.last.blits, 1, "wheel {k}: {:?}", app.last);
        assert!(app.last.repair_percent < 30.0, "wheel {k} repaired {}%", app.last.repair_percent);
    }
    assert_eq!(app.host.world.stats.blits - before.blits, 6);
    // hover: no description, one patch per row
    app.pointer_move(320.0, 200.0);
    app.check().unwrap();
    let (d, p) = (app.stats.describes, app.stats.patches);
    app.pointer_move(320.0, 200.0 + ROW_H);
    app.check().unwrap();
    assert_eq!((app.stats.describes - d, app.stats.patches - p), (0, 2));
    assert!(app.last.dirty_tiles <= 2 * 40 * 3, "{:?}", app.last);
    assert_eq!(app.last.choice, bough::rethink::flat::ExecutorChoice::Cpu);
    // a turn that changes nothing compares every record and patches none
    let s = app.host.sync;
    app.turn(|_| {});
    assert_eq!(app.host.sync.patches, s.patches);
    assert!(app.host.sync.compared > s.compared);
    assert!(!app.frame(), "an unchanged description is an idle frame");
}

#[test]
fn the_deep_view_on_the_flat_core_equals_the_lowered_runtime() {
    let c = || cfg(900.0, 1400.0, 1.0);
    let mut low = bough::lower::App::new(deep_view::Deep::new(), c());
    let mut flat = FlatApp::new(deep_view::Deep::new(), c());
    let same = |low: &mut bough::lower::App<deep_view::Deep>, flat: &mut FlatApp<deep_view::Deep>, what: &str| {
        low.check().unwrap_or_else(|e| panic!("{what}: lowered: {e}"));
        flat.check().unwrap_or_else(|e| panic!("{what}: flat: {e}"));
        assert!(low.presented_pixels() == flat.presented_pixels(), "{what}: the two runtimes differ");
    };
    same(&mut low, &mut flat, "mount");
    assert_eq!(low.record_count(), flat.record_count());
    assert!(flat.record_count() > 100);
    // the buttons, the hover handlers, the press handlers, the signals
    let buttons = low.interactive();
    let mut r = rng(3);
    for step in 0..60 {
        match r(5) {
            0 => {
                let (x, y) = centre(buttons[r(buttons.len() as u64) as usize]);
                low.pointer_down(x, y, Button::Primary);
                low.pointer_up(x, y, Button::Primary);
                flat.pointer_down(x, y, Button::Primary);
                flat.pointer_up(x, y, Button::Primary);
            }
            1 | 2 => {
                let (x, y) = (r(900) as f32, r(700) as f32);
                low.pointer_move(x, y);
                flat.pointer_move(x, y);
            }
            3 => {
                let f = r(100) as f32 / 100.0;
                low.turn(|d| { d.fade.set(f); });
                flat.turn(|d| { d.fade.set(f); });
            }
            _ => {
                let s = r(40) as f32 / 4.0;
                low.turn(|d| { d.shift.set(s); });
                flat.turn(|d| { d.shift.set(s); });
            }
        }
        same(&mut low, &mut flat, &format!("step {step}"));
        assert_eq!((low.view().hovers, low.view().presses, *low.view().count), (flat.view().hovers, flat.view().presses, *flat.view().count));
    }
    assert!(*flat.view().count > 0 && flat.view().hovers > 0 && flat.view().presses > 0);
}
