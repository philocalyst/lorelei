//! Lazy port values (`crates/bough/src/el/walk.rs`, "Lazy port values"; card in
//! `docs/lanes/integration/2026-11-22-el-surface.md`, "Reader plugs and `boundary()` in El"): a port's closure
//! runs where its value is needed (mount, a cell that is not a field, a changed capture), never because the view
//! was described; a port whose wire is installed is described by its identity alone.
//!
//! Negative control: `--features mutant_el_eager_port` treats every port as a constant of its description (its
//! value evaluated into its seal and applied at every update): `a_wired_map_runs_only_when_its_signal_moves`
//! fails.
use bough::prelude::*;
use bough::raw::{App, Config};
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};

fn font() -> Vec<u8> {
    std::fs::read(std::env::var("ZGX_FONT_FILE").unwrap_or_else(|_| "/System/Library/Fonts/Supplemental/Arial.ttf".into())).unwrap()
}
fn cfg() -> Config {
    Config::new(320.0, 200.0, 1.0, font())
}

fn shot<V: bough::app::View>(app: &mut App<V>) -> Vec<u8> {
    app.frame();
    let p = app.presented_pixels();
    assert_eq!(p, app.reference_pixels(), "the repaired frame equals a full redraw");
    p
}

/// Map closures run (only `a_wired_map_runs_only_when_its_signal_moves`'s view counts).
static RUNS: AtomicU32 = AtomicU32::new(0);
fn ran() -> u32 {
    RUNS.load(Relaxed)
}
fn bump() {
    RUNS.fetch_add(1, Relaxed);
}

// ------------------------------------------------------------------------------------------- wired ports

struct Counted {
    n: Signal<u32>,
    fade: Signal<f32>,
    tint: Signal<u32>,
    /// A plain field: writing it describes the view again, and moves no signal.
    k: u32,
}
impl View for Counted {
    fn view(&self) -> El<'_> {
        column((
            text(self.n.map(|n| {
                bump();
                format!("n {n}")
            })),
            div().size(60, 16).bg(self.tint.map(|t| {
                bump();
                rgb(0x102030 + *t)
            })),
            div().size(60, 16).bg(rgb(0x204080)).opacity(self.fade.map(|f| {
                bump();
                1.0 - f
            })),
            text(format!("k {}", self.k)),
            // the same ports inside a list item (a sealed part: stepped over when described alike)
            (0..2).map(move |i| {
                div().size(30, 8).bg(rgb(0x406080)).opacity(self.fade.map(move |f| {
                    bump();
                    f * 0.5 + i as f32 * 0.25
                }))
            }),
        ))
        .bg(rgb(0xffffff))
        .w_pct(1.0)
        .h_pct(1.0)
    }
}
fn counted(n: u32, fade: f32, tint: u32, k: u32) -> Counted {
    Counted { n: Signal::new(n), fade: Signal::new(fade), tint: Signal::new(tint), k }
}

#[test]
fn a_wired_map_runs_only_when_its_signal_moves() {
    let before = ran();
    let mut app = App::new(counted(1, 0.25, 0, 0), cfg(), true);
    shot(&mut app);
    let mounted = ran() - before;
    RUNS.store(mounted, Relaxed);
    assert_eq!(mounted, 5, "each port's closure ran once, for its first value");
    for k in 1..=12 {
        app.turn(|c| c.k = k);
        assert_eq!(ran(), mounted, "turn {k}: a re-description runs no closure of a wired port");
        if k % 4 == 0 {
            // (the oracle's own mount runs its closures: not counted)
            let fresh = shot(&mut App::new(counted(1, 0.25, 0, k), cfg(), true));
            RUNS.store(mounted, Relaxed);
            assert_eq!(shot(&mut app), fresh, "turn {k}: = a fresh mount");
        }
    }
    // each signal's wires run its maps once per write (and the re-description of that turn adds nothing)
    app.turn(|c| {
        c.n.set(2);
    });
    assert_eq!(ran(), mounted + 1, "the text's wire");
    app.turn(|c| {
        c.fade.set(0.75);
        c.k = 20;
    });
    assert_eq!(ran(), mounted + 1 + 3, "the three opacity wires");
    app.turn(|c| {
        c.tint.set(0x101010);
    });
    assert_eq!(ran(), mounted + 1 + 3 + 1, "the fill's wire");
    assert_eq!(shot(&mut app), shot(&mut App::new(counted(2, 0.75, 0x101010, 20), cfg(), true)), "= a fresh mount");
}

// ------------------------------------------------------------------------------------------- constants

struct Far {
    names: Vec<Signal<String>>,
    fades: Vec<Signal<f32>>,
    tints: Vec<Signal<Color>>,
    k: u32,
}
impl View for Far {
    fn view(&self) -> El<'_> {
        column((
            // children of a box (an unsealed part: updated each turn)
            text(self.names[0].map(|s| format!("{s}!"))),
            div().size(60, 16).bg(&self.tints[0]).opacity(self.fades[0].map(|f| 1.0 - f)),
            text(&self.names[1]),
            // items of a list (a sealed part: a constant port's value is in its seal)
            (0..2).map(move |i| row((text(self.names[i].map(|s| s.to_uppercase())), div().size(20, 8).bg(&self.tints[i]).opacity(&self.fades[i])))),
            text(format!("k {}", self.k)),
        ))
        .bg(rgb(0xffffff))
        .w_pct(1.0)
        .h_pct(1.0)
    }
}
fn far(a: &str, b: &str, f: [f32; 2], t: [u32; 2], k: u32) -> Far {
    Far { names: vec![Signal::new(a.into()), Signal::new(b.into())], fades: f.map(Signal::new).into(), tints: t.map(|c| Signal::new(rgb(c))).into(), k }
}

/// A port over a cell that is not a field of the view (behind a `Vec`) is not wired: its value is evaluated
/// into the description's seal, so a re-description shows a new value, in a box's children and in a list.
#[test]
fn a_map_over_a_signal_behind_a_vec_shows_on_redescription() {
    let mut app = App::new(far("a", "b", [0.0, 0.5], [0x102030, 0x304050], 0), cfg(), true);
    shot(&mut app);
    let steps: [(&str, &str, [f32; 2], [u32; 2]); 5] = [
        ("c", "b", [0.0, 0.5], [0x102030, 0x304050]),
        ("c", "d", [0.25, 0.5], [0x102030, 0x304050]),
        ("c", "d", [0.25, 1.0], [0x902030, 0x304050]),
        ("e", "d", [0.25, 1.0], [0x902030, 0x30f050]),
        ("e", "dd", [0.5, 0.0], [0x102030, 0x304050]),
    ];
    for (k, (a, b, f, t)) in steps.into_iter().enumerate() {
        let k = k as u32 + 1;
        app.turn(|v| {
            v.names[0].set(a.into());
            v.names[1].set(b.into());
            v.fades[0].set(f[0]);
            v.fades[1].set(f[1]);
            v.tints[0].set(rgb(t[0]));
            v.tints[1].set(rgb(t[1]));
            v.k = k;
        });
        assert_eq!(shot(&mut app), shot(&mut App::new(far(a, b, f, t, k), cfg(), true)), "step {k}: = a fresh mount");
    }
    // nothing described differently: the same pixels, a seal of constants stepped over
    let before = shot(&mut app);
    app.turn(|_| {});
    assert_eq!(shot(&mut app), before);
}

// ------------------------------------------------------------------------------------------- captures

/// el-motion's `a_map_whose_captures_changed_*` (lane/reactivity/el-motion, tests/el_motion.rs) without motion:
/// a map whose closure captured something new is the same port (its shape does not see captures); its installed
/// wire takes the new captures at the update, and the value is applied on that turn's frame.
struct ScaledNow {
    n: Signal<f32>,
    k: f32,
}
impl View for ScaledNow {
    fn view(&self) -> El<'_> {
        let k = self.k;
        column((
            // items of a sealed part: a description sealed alike is stepped over
            (0..2).map(move |i| div().size(60, 16).bg(rgb(0x203060 + i * 0x200000)).opacity(self.n.map(move |n| n * k))),
            Some(text(self.n.map(move |n| format!("{:.0}", n * k * 100.0)))),
            // a box's child (an unsealed part)
            div().size(60, 16).bg(rgb(0x206030)).opacity(self.n.map(move |n| 1.0 - n * k)),
        ))
        .bg(rgb(0xffffff))
        .w_pct(1.0)
        .h_pct(1.0)
    }
}

/// No ticks between the turn and its frame: the frame of the very turn that changes a capture equals a fresh
/// mount, and later writes of the signal compute with the new capture.
#[test]
fn a_map_whose_captures_changed_is_right_on_the_same_frame() {
    let mut app = App::new(ScaledNow { n: Signal::new(0.5), k: 1.0 }, cfg(), true);
    shot(&mut app);
    for (n, k) in [(0.5, 0.0), (0.8, 0.0), (0.8, 0.5), (0.3, 0.5), (0.3, 1.0), (0.3, 0.25), (0.9, 0.25)] {
        app.turn(|s| {
            s.n.set(n);
            s.k = k;
        });
        assert_eq!(shot(&mut app), shot(&mut App::new(ScaledNow { n: Signal::new(n), k }, cfg(), true)), "n {n}, k {k}: the turn's own frame = a fresh mount");
    }
}

struct ManyScaled {
    n: Signal<f32>,
    k: f32,
}
impl View for ManyScaled {
    fn view(&self) -> El<'_> {
        let k = self.k;
        let row = move |i: u32| div().size(4, 4).bg(rgb(0x203060 + i * 0x010000)).opacity(self.n.map(move |n| n * k * (1.0 - i as f32 / 100.0)));
        // each row captures its own factor too: the update must give each port's installed wire its own row's
        // capture (same signal, same patch: only the order tells them apart), 80 wired ports of one item
        column((0..1).map(move |_| {
            column((
                (row(0), row(1), row(2), row(3), row(4), row(5), row(6), row(7), row(8), row(9)),
                (row(10), row(11), row(12), row(13), row(14), row(15), row(16), row(17), row(18), row(19)),
                (row(20), row(21), row(22), row(23), row(24), row(25), row(26), row(27), row(28), row(29)),
                (row(30), row(31), row(32), row(33), row(34), row(35), row(36), row(37), row(38), row(39)),
                (row(40), row(41), row(42), row(43), row(44), row(45), row(46), row(47), row(48), row(49)),
                (row(50), row(51), row(52), row(53), row(54), row(55), row(56), row(57), row(58), row(59)),
                (row(60), row(61), row(62), row(63), row(64), row(65), row(66), row(67), row(68), row(69)),
                (row(70), row(71), row(72), row(73), row(74), row(75), row(76), row(77), row(78), row(79)),
            ))
        }))
        .bg(rgb(0xffffff))
        .w_pct(1.0)
        .h_pct(1.0)
    }
}

#[test]
fn a_map_whose_captures_changed_is_right_past_many_ports() {
    let mut app = App::new(ManyScaled { n: Signal::new(0.5), k: 1.0 }, cfg(), true);
    shot(&mut app);
    for (n, k) in [(0.5, 0.0), (0.8, 0.0), (0.8, 0.5), (0.3, 0.5)] {
        app.turn(|s| {
            s.n.set(n);
            s.k = k;
        });
        assert_eq!(shot(&mut app), shot(&mut App::new(ManyScaled { n: Signal::new(n), k }, cfg(), true)), "n {n}, k {k}: = a fresh mount");
    }
}
