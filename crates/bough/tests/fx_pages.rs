//! The façade's effects and page-transition gates: non-generic styles, motion, and every transition of
//! the page router, drawn by the CPU tile kernel and by Metal and held to the specification, to a fresh
//! router's full redraw, and to the promises of the design (no page is asked anything in flight; an
//! interruption keeps position and velocity).
use bough::color::{rgb, Color};
use bough::fx_api::*;
use bough::pages::*;
use bough_kernel::cpu::Pool;
#[cfg(target_os = "macos")]
use bough_paint::flat_scene::FlatGpu;

const W: u32 = 320;
const H: u32 = 224;

fn diff(a: &[u32], b: &[u32]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}
fn ms(v: u64) -> ClockNs {
    ClockNs::from_ms(v)
}

// ---- styles ----

fn card() -> BoxStyle {
    BoxStyle::new()
        .gradient(Gradient::linear(135.0).stop(0.0, rgb(0x4f46e5)).stop(0.6, rgb(0x9333ea)).stop(1.0, rgb(0xec4899)))
        .rounded(16.0)
        .border(1.0, Color::rgba(255, 255, 255, 60))
        .shadow(ShadowSpec::drop(0.0, 12.0, 32.0, Color::rgba(0, 0, 0, 70)).spread(-6.0))
        .shadow(ShadowSpec::drop(0.0, 2.0, 4.0, Color::rgba(0, 0, 0, 50)))
        .shadow(ShadowSpec::inset(0.0, 1.0, 0.0, Color::rgba(255, 255, 255, 90)))
        .shadow(ShadowSpec::inset(0.0, -6.0, 12.0, Color::rgba(0, 0, 40, 80)))
}
struct Styled {
    flat: FlatScene,
    card: StyledBox,
    pane: StyledBox,
    dots: Vec<StyledBox>,
}
fn styled_scene() -> Styled {
    let mut flat = FlatScene::new(W, H, FlatConfig::default());
    flat.add_box([0.0, 0.0, W as f32, H as f32], &BoxStyle::new().gradient(Gradient::conic(30.0).at(0.3, 0.6).stops(&[(0.0, rgb(0x0f172a)), (0.4, rgb(0x1e3a8a)), (0.7, rgb(0x0f766e)), (1.0, rgb(0x0f172a))]))).unwrap();
    let dots = (0..6)
        .map(|i| {
            let (x, y) = (14.0 + 50.0 * i as f32, 150.0 + 7.0 * (i % 3) as f32);
            flat.add_box([x, y, x + 34.0, y + 34.0], &BoxStyle::new().gradient(Gradient::radial().focal(-0.3, -0.3).stop(0.0, rgb(0xffffff)).stop(0.3, rgb(0xfbbf24)).stop(1.0, rgb(0xb45309))).rounded(17.0).shadow(ShadowSpec::drop(0.0, 3.0, 6.0, Color::rgba(0, 0, 0, 90)))).unwrap()
        })
        .collect();
    let card = flat.add_box([30.0, 24.0, 190.0, 130.0], &card()).unwrap();
    let pane = flat.add_box([120.0, 80.0, 300.0, 200.0], &BoxStyle::new().backdrop_blur(BlurSpec::backdrop(8.0).saturate(1.5).tint(Color::rgba(255, 255, 255, 40))).rounded(20.0).border(1.0, Color::rgba(255, 255, 255, 80)).shadow(ShadowSpec::drop(0.0, 8.0, 24.0, Color::rgba(0, 0, 0, 60)))).unwrap();
    Styled { flat, card, pane, dots }
}

#[test]
fn styles_are_values_and_motion_is_exact() {
    // a style is a value: these two have one Rust type, whatever they hold
    let (a, b): (BoxStyle, BoxStyle) = (card(), BoxStyle::new().bg(rgb(0x202020)));
    assert!(a != b && std::mem::size_of_val(&a) == std::mem::size_of_val(&b));
    let mut s = styled_scene();
    // paint order of a box's parts: drop shadows (the last given deepest), fill, inset shadows, border
    let parts: Vec<Part> = s.card.nodes.iter().map(|n| n.0).collect();
    assert_eq!(parts, [Part::Shadow, Part::Shadow, Part::Fill, Part::Inset, Part::Inset, Part::Border]);
    assert_eq!(s.pane.nodes.iter().map(|n| n.0).collect::<Vec<_>>(), [Part::Shadow, Part::Backdrop, Part::Border]);
    let DrawCmd::Shadow { blur, .. } = s.flat.node(s.card.nodes[0].1).cmd else { panic!() };
    assert_eq!(blur, 4.0, "the first shadow given is on top: the last one is the deepest node");

    let mut twin = styled_scene();
    // SAFETY: `twin.flat` outlives `gpu` (declared after it, dropped before it) and is written only
    // between the executor's calls.
    #[cfg(target_os = "macos")]
    let mut gpu = unsafe { FlatGpu::new(&twin.flat, 256) };
    let pool = Pool::new(2);
    let words = (W * H) as usize;
    let (mut keep, mut spec) = (vec![0u32; words], vec![0u32; words]);
    let (mut motion, mut motion2) = (Motion::new(), Motion::new());
    let (mut frames, mut gframes, mut bad) = (0, 0, 0);
    let mut draw = |s: &mut Styled, twin: &mut Styled, name: &str| -> FlatStats {
        let st = s.flat.frame(&mut keep, W as usize, &pool, 1);
        s.flat.spec(&mut spec, W as usize);
        frames += 1;
        let d = diff(&keep, &spec);
        #[allow(unused_mut)]
        let mut g = 0;
        #[cfg(target_os = "macos")]
        if let Some(gpu) = gpu.as_mut() {
            gpu.frame(&mut twin.flat, &pool, 1);
            g = diff(gpu.out(), &spec);
            gframes += 1;
        }
        let _ = &twin;
        if (d, g) != (0, 0) {
            eprintln!("{name}: {d} words differ on the CPU, {g} on Metal");
            bad += 1;
        }
        st
    };
    draw(&mut s, &mut twin, "mount");
    assert_eq!(draw(&mut s, &mut twin, "idle").frame.tiles, 0);

    // restyle in place: the same parts, other values
    let hot = card().gradient(Gradient::linear(20.0).stop(0.0, rgb(0xf97316)).stop(1.0, rgb(0xfacc15))).rounded(28.0).border(3.0, rgb(0xffffff)).opacity(0.9);
    for t in [&mut s, &mut twin] {
        let mut c = t.card.clone();
        t.flat.restyle(&mut c, [40.0, 30.0, 210.0, 120.0], &hot).unwrap();
        t.card = c;
        // other parts: refused, and nothing changed
        let mut c = t.card.clone();
        assert_eq!(t.flat.restyle(&mut c, [0.0; 4], &BoxStyle::new().bg(rgb(0))), Err(Restyle::Structure));
        assert_eq!(c, t.card);
    }
    let st = draw(&mut s, &mut twin, "restyle");
    assert_eq!(st.lowered, 6, "six parts, six records");

    // motion: springs, an eased curve, keyframes and a stagger, driven by monotone time
    let now = ms(40_000);
    for (t, m) in [(&mut s, &mut motion), (&mut twin, &mut motion2)] {
        m.animate_box(&t.flat, &t.card, Prop::Dy, -18.0, AnimSpec::spring(SpringSpec::BOUNCY), now);
        m.animate_box(&t.flat, &t.card, Prop::Scale, 1.08, AnimSpec::response(0.35, 0.7), now);
        m.animate(&t.flat, t.pane.part(Part::Backdrop).next().unwrap(), Prop::Blur, 22.0, AnimSpec::ease(260, Ease::EMPHASIZED), now);
        m.animate_box(&t.flat, &t.pane, Prop::Dx, -60.0, AnimSpec::bezier(300, 0.34, 1.56, 0.64, 1.0), now);
        let ids: Vec<NodeId> = t.dots.iter().map(|d| d.body()).collect();
        m.stagger(&t.flat, &ids, Prop::Dy, -30.0, AnimSpec::keyframes(&[Key::new(0.0, 0.0, Ease::OUT), Key::new(0.5, -30.0, Ease::IN), Key::new(1.0, 0.0, Ease::Linear)], Timing::once(280).repeat(Repeat::Count(2))), 45, now);
        m.animate(&t.flat, ids[0], Prop::Opacity, 0.2, AnimSpec::Snap, now);
    }
    let (mut k, mut moving, mut lowered) = (0u64, true, 0);
    while moving {
        let at = now.after_ms(k * 16);
        moving = motion.tick(&mut s.flat, at);
        motion2.tick(&mut twin.flat, at);
        let st = draw(&mut s, &mut twin, "motion");
        lowered += st.lowered;
        moving |= s.flat.pending();
        k += 1;
        assert!(k < 400, "the motion never came to rest");
    }
    assert!(k > 40 && lowered > 300, "{k} frames, {lowered} records rewritten");
    // at rest: exactly where it was sent, and a frame is no work
    assert_eq!((s.flat.prop(s.card.body(), Prop::Dy), s.flat.prop(s.card.body(), Prop::Scale), s.flat.prop(s.dots[5].body(), Prop::Dy)), (-18.0, 1.08, 0.0));
    let st = draw(&mut s, &mut twin, "rest");
    assert_eq!((st.lowered, st.frame.tiles, st.frame.recs_visited, motion.bank().active()), (0, 0, 0, 0));
    eprintln!("fx api: {frames} frames exact against the specification on the CPU, {gframes} on Metal; {bad} failing");
    assert_eq!(bad, 0);
}

// ---- pages ----

const HERO: u64 = 7;
/// Page `i` of the test application: a backdrop, cards, and one element every page has (the hero), in
/// a different place, size, shape and colour on each.
fn page(i: usize) -> (FlatScene, Hero, NodeId) {
    let mut p = FlatScene::new(W, H, FlatConfig::default());
    let sky = [[0x0f172a, 0x334155], [0xfef3c7, 0xfb923c], [0x022c22, 0x10b981]][i % 3];
    p.add_box([0.0, 0.0, W as f32, H as f32], &BoxStyle::new().gradient(Gradient::linear(160.0 + 40.0 * i as f32).stop(0.0, rgb(sky[0])).stop(1.0, rgb(sky[1])))).unwrap();
    let mut first = None;
    for k in 0..4 {
        let (x, y) = (16.0 + 74.0 * k as f32, 20.0 + 26.0 * ((k + i) % 3) as f32);
        let b = p.add_box([x, y, x + 62.0, y + 70.0], &BoxStyle::new().bg(Color::rgba(255, 255, 255, 220)).rounded(10.0).shadow(ShadowSpec::drop(0.0, 4.0, 12.0, Color::rgba(0, 0, 0, 60))).border(1.0, Color::rgba(0, 0, 0, 40))).unwrap();
        first.get_or_insert(b.body());
    }
    let (rect, radius, color) = [([24.0, 130.0, 104.0, 200.0], 12.0, rgb(0xe11d48)), ([60.0, 40.0, 260.0, 170.0], 28.0, rgb(0x2563eb)), ([200.0, 120.0, 300.0, 210.0], 45.0, rgb(0xf59e0b))][i % 3];
    let hero = p.add_box(rect, &BoxStyle::new().bg(color).rounded(radius).shadow(ShadowSpec::drop(0.0, 6.0 + 4.0 * i as f32, 14.0 + 6.0 * i as f32, Color::rgba(0, 0, 0, 90)))).unwrap();
    let h = Hero::of(&p, &hero);
    (p, h, first.unwrap())
}
struct App {
    r: PageRouter,
    pages: Vec<PageId>,
    card: Vec<NodeId>,
}
fn app() -> App {
    let mut r = PageRouter::new(W, H, RouterConfig { background: rgb(0x101010), ..Default::default() }).unwrap();
    let (mut pages, mut card) = (Vec::new(), Vec::new());
    for i in 0..3 {
        let (scene, hero, first) = page(i);
        let id = r.add_page(scene).unwrap();
        r.declare_hero(id, HERO, hero);
        pages.push(id);
        card.push(first);
    }
    r.show(pages[0]);
    App { r, pages, card }
}
#[derive(Clone, Debug)]
enum Cmd {
    Push(usize, PageTransition),
    Pop,
    /// Recolour a card of a page (its content changes in flight).
    Edit(usize, u32),
}
fn apply(a: &mut App, c: &Cmd, now: ClockNs) {
    match c {
        Cmd::Push(p, t) => a.r.push(a.pages[*p], t.clone(), now),
        Cmd::Pop => {
            a.r.pop(now);
        }
        Cmd::Edit(p, c) => {
            let (id, n) = (a.pages[*p], a.card[*p]);
            a.r.page_mut(id).set_color(n, *c);
        }
    }
}
const STEP: u64 = 16;
/// A fresh application brought to frame `upto` of a script without drawing a frame on the way.
fn replay(script: &[(u64, Cmd)], upto: u64) -> App {
    let mut a = app();
    for f in 0..=upto {
        let now = ms(1_000 + f * STEP);
        for (_, c) in script.iter().filter(|(at, _)| *at == f) {
            apply(&mut a, c, now);
        }
        a.r.tick(now);
    }
    a
}

struct Run {
    frames: usize,
    gpu_frames: usize,
    bad: usize,
    gbad: usize,
    /// Frames in flight on which nothing changed any page's content, and the work pages did on them.
    flight_frames: usize,
    page_work: u32,
    /// Stage tiles drawn in flight, of all the tiles those frames have.
    tiles: u64,
    all_tiles: u64,
    last: Vec<u32>,
}
/// Run a script: every frame is drawn by the CPU tile kernel and held to the stage's specification and
/// to a fresh application's full redraw; a twin is drawn by Metal and held to the same.
fn run(script: &[(u64, Cmd)], frames: u64, mutate: Option<&dyn Fn(&str) -> String>) -> Run {
    let pool = Pool::new(2);
    let words = (W * H) as usize;
    let mut a = app();
    let mut twin = app();
    #[cfg(target_os = "macos")]
    let mut gpu = {
        let good = FlatGpu::source(twin.r.stage());
        let src = mutate.map_or(good.clone(), |f| f(&good));
        assert!(mutate.is_none() || src != good, "the mutation did not apply");
        // SAFETY: `twin` outlives `gpu` (declared before it) and is written only between its calls.
        unsafe { FlatGpu::with_source(twin.r.stage(), 256, &src) }
    };
    #[cfg(not(target_os = "macos"))]
    let _ = mutate;
    let (mut keep, mut spec, mut fresh) = (vec![0u32; words], vec![0u32; words], vec![0u32; words]);
    let mut out = Run { frames: 0, gpu_frames: 0, bad: 0, gbad: 0, flight_frames: 0, page_work: 0, tiles: 0, all_tiles: 0, last: Vec::new() };
    let nt = (W.div_ceil(32) * H.div_ceil(32)) as u64;
    for f in 0..frames {
        let now = ms(1_000 + f * STEP);
        let cmds: Vec<&Cmd> = script.iter().filter(|(at, _)| *at == f).map(|(_, c)| c).collect();
        for c in &cmds {
            apply(&mut a, c, now);
            apply(&mut twin, c, now);
        }
        let flying = a.r.in_flight();
        let st = a.r.frame(now, &mut keep, W as usize, &pool, 1);
        a.r.stage_mut().spec(&mut spec, W as usize);
        let mut fr = replay(script, f);
        fr.r.stage_mut().frame(&mut fresh, W as usize, &pool, 1);
        out.frames += 1;
        let d = (diff(&keep, &spec), diff(&keep, &fresh));
        if d != (0, 0) {
            eprintln!("frame {f} {cmds:?}: {} words differ from the specification, {} from a fresh router", d.0, d.1);
            out.bad += 1;
        }
        twin.r.tick(now);
        #[cfg(target_os = "macos")]
        if let Some(gpu) = gpu.as_mut() {
            gpu.frame(twin.r.stage_mut(), &pool, 1);
            let g = diff(gpu.out(), &fresh);
            if g != 0 && mutate.is_none() {
                eprintln!("frame {f} {cmds:?}: {g} words differ on Metal");
            }
            out.gbad += (g != 0) as usize;
            out.gpu_frames += 1;
        }
        // in flight before and after, on a frame no command touched: no page is asked anything. (The frame a
        // shared element takes off and the frame it lands are commands and arrivals: each page repaints
        // the element's own tiles once there.)
        let _ = st.animating;
        if flying && cmds.is_empty() && f > 0 && a.r.in_flight() {
            out.flight_frames += 1;
            out.page_work += st.flat.group_tiles + st.flat.group_blocks + st.flat.group_recs;
            out.tiles += st.flat.frame.tiles as u64;
            out.all_tiles += nt;
        }
    }
    out.last = keep;
    out
}

fn transitions() -> Vec<(&'static str, PageTransition)> {
    vec![
        ("cross-fade", PageTransition::cross_fade()),
        ("cross-fade on a curve", PageTransition::CrossFade { drive: Drive::Curve { ease: Ease::IN_OUT, dur_ms: 220 } }),
        ("slide left", PageTransition::slide(Dir::Left)),
        ("slide up, pushed whole", PageTransition::Slide { dir: Dir::Up, parallax: 1.0, drive: Drive::BOUNCY }),
        ("scale-fade", PageTransition::scale_fade()),
        ("blur-reveal", PageTransition::blur_reveal()),
        ("shared element", PageTransition::shared(&[HeroMatch::key(HERO)])),
    ]
}

/// Every transition, start to rest: exact on both executors on every frame, no page asked anything in
/// flight, and at rest the stage is the destination page shown plainly.
#[test]
fn every_transition_is_exact_and_asks_no_page_anything() {
    let pool = Pool::new(1);
    let words = (W * H) as usize;
    let plain = |p: usize| {
        let mut a = app();
        a.r.show(a.pages[p]);
        let mut out = vec![0u32; words];
        a.r.frame(ms(0), &mut out, W as usize, &pool, 0);
        out
    };
    for (name, t) in transitions() {
        let script = [(2, Cmd::Push(1, t.clone())), (70, Cmd::Pop)];
        let r = run(&script, 140, None);
        eprintln!("page transition ({name}): {} frames exact on the CPU, {} on Metal; {} frames in flight drew {} of {} stage tiles and asked the pages for {} units of work; {} + {} failing", r.frames, r.gpu_frames, r.flight_frames, r.tiles, r.all_tiles, r.page_work, r.bad, r.gbad);
        assert_eq!((r.bad, r.gbad), (0, 0), "{name}");
        assert!(r.flight_frames >= 12, "{name}: {} frames in flight", r.flight_frames);
        assert_eq!(r.page_work, 0, "{name}: a page was repainted, re-described or re-blurred during the transition");
        assert_eq!(diff(&r.last, &plain(0)), 0, "{name}: pushed and popped, the stage is not the first page again");
        // pushed and left to rest: the destination, plainly
        let r = run(&[(1, Cmd::Push(2, t.clone()))], 120, None);
        assert_eq!((r.bad, r.gbad), (0, 0), "{name}");
        assert_eq!(diff(&r.last, &plain(2)), 0, "{name}: at rest the stage is not the destination page");
    }
}

/// The largest jump between consecutive milliseconds of a page's layer, in each of its five quantities.
fn roughness(a: &App, p: PageId, from: ClockNs, ms_n: u64) -> [f32; 5] {
    let mut worst = [0f32; 5];
    let mut prev = a.r.layer_at(p, from).0;
    for k in 1..=ms_n {
        let s = a.r.layer_at(p, from.after_ms(k)).0;
        for (i, (x, y)) in [(s.dx, prev.dx), (s.dy, prev.dy), (s.scale, prev.scale), (s.opacity, prev.opacity), (s.blur, prev.blur)].into_iter().enumerate() {
            worst[i] = worst[i].max((x - y).abs());
        }
        prev = s;
    }
    worst
}

/// Interrupted in flight (popped before it arrived, pushed again before it left, a third page pushed on
/// top): every page on screen goes on from the position **and the velocity** it has. And every frame of
/// it is exact.
#[test]
fn an_interrupted_transition_keeps_position_and_velocity() {
    for (name, t) in transitions() {
        let spring = !name.contains("curve");
        // the measure, at the instants of interruption
        let mut a = app();
        let t0 = ms(5_000);
        a.r.tick(t0);
        a.r.push(a.pages[1], t.clone(), t0);
        let mut worst = (0f32, 0f64, 0f64);
        let mut at = t0;
        for (k, gap) in [70u64, 45, 110, 30, 60].into_iter().enumerate() {
            at = at.after_ms(gap);
            a.r.tick(at);
            let on_screen = a.r.visible();
            let before: Vec<_> = on_screen.iter().map(|&p| a.r.layer_at(p, at)).collect();
            let hero_before = a.r.hero_at(0, at);
            match k % 3 {
                0 => {
                    a.r.pop(at);
                }
                1 => a.r.push(a.pages[1], t.clone(), at),
                _ => a.r.push(a.pages[2], t.clone(), at),
            }
            for (&p, (s0, v0)) in on_screen.iter().zip(&before) {
                let (s1, v1) = a.r.layer_at(p, at);
                for (x0, x1) in [(s0.dx, s1.dx), (s0.dy, s1.dy), (s0.scale, s1.scale), (s0.opacity, s1.opacity), (s0.blur, s1.blur)] {
                    worst.0 = worst.0.max((x0 - x1).abs());
                }
                for i in 0..5 {
                    worst.1 = worst.1.max((v0[i] - v1[i]).abs() / (1.0 + v0[i].abs()));
                    worst.2 = worst.2.max(v0[i].abs());
                }
            }
            if let (Some((h0, hv0)), Some((h1, hv1))) = (hero_before, a.r.hero_at(0, at)) {
                for i in 0..12 {
                    worst.0 = worst.0.max((h0[i] - h1[i]).abs());
                    worst.1 = worst.1.max((hv0[i] - hv1[i]).abs() / (1.0 + hv0[i].abs()));
                }
            }
        }
        eprintln!("interrupted ({name}): five interruptions in flight; worst jump in position {:.1e}, in velocity {:.1e} (relative); fastest {:.0} units/s", worst.0, worst.1, worst.2);
        assert!(worst.0 < 1e-3, "{name}: a page snapped by {} at an interruption", worst.0);
        assert!(worst.2 > 0.5, "{name}: nothing was moving when it was interrupted");
        if spring {
            assert!(worst.1 < 1e-6, "{name}: a page's velocity jumped by {} (relative) at an interruption", worst.1);
        } else {
            // the negative control: an eased curve restarts from rest, and the same measure sees it
            assert!(worst.1 > 0.5, "{name}: a curve's restart went unseen ({})", worst.1);
        }
        // no snap anywhere in the whole of it
        let rough = roughness(&a, a.pages[1], t0, 900);
        assert!(rough[0] < 12.0 && rough[1] < 12.0 && rough[2] < 0.02 && rough[3] < 0.04 && rough[4] < 1.5, "{name}: {rough:?}");

        // and the frames of such a flight are exact, with a page's content edited on the way
        let script = [(2, Cmd::Push(1, t.clone())), (6, Cmd::Pop), (9, Cmd::Push(1, t.clone())), (11, Cmd::Edit(1, 0xff20_c0f0)), (14, Cmd::Push(2, t.clone())), (17, Cmd::Pop), (19, Cmd::Edit(0, 0xff40_40ff)), (40, Cmd::Pop)];
        let r = run(&script, 110, None);
        eprintln!("interrupted ({name}): {} frames exact on the CPU, {} on Metal; {} + {} failing", r.frames, r.gpu_frames, r.bad, r.gbad);
        assert_eq!((r.bad, r.gbad), (0, 0), "{name}");
        assert_eq!(r.page_work, 0, "{name}");
    }
}

/// A shared element in the middle of its flight is between its two pages in every quantity, and is drawn
/// by the stage, not by either page.
#[test]
fn a_shared_element_morphs_between_its_pages() {
    let mut a = app();
    let t0 = ms(100);
    a.r.tick(t0);
    let (from, to) = (page(0).1, page(1).1);
    a.r.push(a.pages[1], PageTransition::SharedElement { keys: vec![HeroMatch::key(HERO)], drive: Drive::Curve { ease: Ease::Linear, dur_ms: 200 } }, t0);
    let (v, _) = a.r.hero_at(0, t0.after_ms(100)).expect("the element is in flight");
    let mid = |x: f32, y: f32| (x + y) * 0.5;
    for i in 0..4 {
        assert!((v[i] - mid(from.rect[i], to.rect[i])).abs() < 1e-3, "rect {i}: {} between {} and {}", v[i], from.rect[i], to.rect[i]);
    }
    assert!((v[4] - mid(from.radius, to.radius)).abs() < 1e-3);
    for (i, sh) in [0u32, 8, 16].into_iter().enumerate() {
        let ch = |c: Color| ((c.0 >> sh) & 255) as f32;
        assert!((v[5 + i] - mid(ch(from.color), ch(to.color))).abs() < 1e-3);
    }
    assert!((v[9] - mid(from.shadow.unwrap().blur, to.shadow.unwrap().blur)).abs() < 1e-3 && (v[10] - mid(from.shadow.unwrap().dy, to.shadow.unwrap().dy)).abs() < 1e-3);
    // in flight neither page draws it; landed, the destination does and the overlay is gone
    let hidden = |a: &App, p: usize| page(p).1.nodes.iter().all(|&n| !a.r.page_scene(a.pages[p]).node(n).visible);
    a.r.tick(t0.after_ms(100));
    assert!(hidden(&a, 0) && hidden(&a, 1));
    assert!(a.r.tick(t0.after_ms(150)));
    a.r.tick(t0.after_ms(260));
    assert!(!a.r.in_flight() && !hidden(&a, 1) && !hidden(&a, 0) && a.r.hero_at(0, t0.after_ms(260)).is_none());
    assert_eq!(a.r.visible(), [a.pages[1]]);
}

/// The Metal executor's source broken on purpose under a page transition: seen.
#[cfg(target_os = "macos")]
#[test]
fn the_oracle_catches_a_broken_transition() {
    let cases: [(&str, PageTransition, &dyn Fn(&str) -> String); 2] = [
        ("a page's blur never reads its coarser level", PageTransition::blur_reveal(), &|s| s.replace("if (lv.y != 0) { v = x_mixv(v, mip_read", "if (false) { v = x_mixv(v, mip_read")),
        ("a page composited one texel off", PageTransition::slide(Dir::Left), &|s| s.replace("int tx = mp_pos(t.x, k), ty = mp_pos(t.y, k);", "int tx = mp_pos(t.x, k) + 256, ty = mp_pos(t.y, k);")),
    ];
    for (name, t, f) in cases {
        let r = run(&[(2, Cmd::Push(1, t))], 30, Some(f));
        if r.gpu_frames == 0 {
            eprintln!("no Metal device: skipped");
            return;
        }
        eprintln!("page mutant ({name}): {} of {} frames differ on Metal", r.gbad, r.gpu_frames);
        assert_eq!(r.bad, 0);
        assert!(r.gbad > 0, "{name} was not caught");
    }
}
