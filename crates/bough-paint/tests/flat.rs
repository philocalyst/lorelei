//! The flat-scene gates: every effect of the non-generic bank on the CPU tile kernel, against the
//! per-pixel specification, against a fresh scene's full redraw, and against the Metal executor, in words.
use bough_kernel::cpu::Pool;
use bough_kernel::scenes::Rng;
use bough_paint::flat_scene::*;
use bough_paint::scene::bake_ramp;

fn diff(a: &[u32], b: &[u32]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}
fn rgba(r: u32, g: u32, b: u32, a: u32) -> u32 {
    r | g << 8 | b << 16 | a << 24
}
/// A stored pixel (BGRA8 word) as [r, g, b, a].
fn px(w: u32) -> [i32; 4] {
    [((w >> 16) & 255) as i32, ((w >> 8) & 255) as i32, (w & 255) as i32, (w >> 24) as i32]
}

/// One scene drawn three ways each step: the retained CPU frame, the specification, Metal.
struct Rig {
    flat: FlatScene,
    #[cfg(target_os = "macos")]
    twin: Option<(Box<FlatScene>, FlatGpu)>,
    keep: Vec<u32>,
    spec: Vec<u32>,
    pool: Pool,
    pub frames: usize,
    pub gpu_frames: usize,
}
impl Rig {
    /// `make` builds the scene; it is called twice (the Metal twin is its own scene, as an application's
    /// would be).
    fn new(make: &dyn Fn() -> FlatScene) -> Rig {
        Rig::with(make, None)
    }
    /// `mutate`: a change to the Metal source that must be caught (a negative control).
    fn with(make: &dyn Fn() -> FlatScene, mutate: Option<&dyn Fn(&str) -> String>) -> Rig {
        let flat = make();
        let (w, h) = flat.size();
        #[cfg(target_os = "macos")]
        let twin = {
            let t = Box::new(make());
            let good = FlatGpu::source(&t);
            let src = match mutate {
                Some(f) => {
                    let bad = f(&good);
                    assert!(bad != good, "the mutation did not apply");
                    bad
                }
                None => good,
            };
            // SAFETY: the twin scene is boxed and kept beside its executor, which is dropped first (the
            // tuple is dropped as a whole, and the executor only reads the arena during its own calls);
            // it is written only between the executor's calls.
            unsafe { FlatGpu::with_source(&t, 256, &src) }.map(|g| (t, g))
        };
        #[cfg(not(target_os = "macos"))]
        let _ = mutate;
        Rig {
            flat,
            #[cfg(target_os = "macos")]
            twin,
            keep: vec![0; (w * h) as usize],
            spec: vec![0; (w * h) as usize],
            pool: Pool::new(2),
            frames: 0,
            gpu_frames: 0,
        }
    }
    /// Apply `f` to the scene (and its twin), draw, and count the differing words: (retained vs
    /// specification, Metal vs specification).
    fn step(&mut self, f: &dyn Fn(&mut FlatScene)) -> (usize, usize, FlatStats) {
        let (w, _) = self.flat.size();
        f(&mut self.flat);
        let st = self.flat.frame(&mut self.keep, w as usize, &self.pool, 1);
        self.flat.spec(&mut self.spec, w as usize);
        self.frames += 1;
        let d = diff(&self.keep, &self.spec);
        #[allow(unused_mut)]
        let mut g = 0;
        #[cfg(target_os = "macos")]
        if let Some((t, gpu)) = self.twin.as_mut() {
            f(t);
            gpu.frame(t, &self.pool, 1);
            g = diff(gpu.out(), &self.spec);
            self.gpu_frames += 1;
        }
        (d, g, st)
    }
    fn at(&self, x: u32, y: u32) -> [i32; 4] {
        px(self.keep[(y * self.flat.size().0 + x) as usize])
    }
}

const STOPS: [(f32, u32); 5] = [(0.0, 0xff20_30ff), (0.25, 0xffff_e020), (0.5, 0xff30_ff60), (0.8, 0x80ff_4040), (1.0, 0xffff_ffff)];

fn gradient_scene() -> FlatScene {
    let mut s = FlatScene::new(320, 240, FlatConfig::default());
    s.push(Node::new([0.0, 0.0, 320.0, 240.0], DrawCmd::Solid { color: 0xff18_1c20 })).unwrap();
    let opaque = s.lut(&[(0.0, 0xff00_00ff), (0.3, 0xff00_ff00), (0.7, 0xffff_0000), (1.0, 0xff00_ffff)]).unwrap();
    let mixed = s.lut(&STOPS).unwrap();
    assert!(opaque.opaque && !mixed.opaque);
    // the same stops again are the same table
    assert_eq!(s.lut(&STOPS).unwrap(), mixed);
    assert_eq!(s.scene().luts(), 2);
    for (i, angle) in [0.0f32, 90.0, 37.0, 180.0, 233.5, 300.0].into_iter().enumerate() {
        let x = 8.0 + 52.0 * i as f32;
        s.push(Node::new([x, 8.0, x + 46.5, 70.25], DrawCmd::Ramp { radius: [0.0, 6.0, 20.0][i % 3], geo: RampGeo::Linear { angle_deg: angle }, lut: if i % 2 == 0 { opaque } else { mixed } })).unwrap();
    }
    s.push(Node::new([10.0, 80.0, 110.0, 160.0], DrawCmd::Ramp { radius: 12.0, geo: RampGeo::Radial { center: [0.5, 0.5], radius: [50.0, 40.0], focal: [0.0, 0.0] }, lut: opaque })).unwrap();
    s.push(Node::new([120.0, 80.0, 210.5, 160.0], DrawCmd::Ramp { radius: 0.0, geo: RampGeo::Radial { center: [0.3, 0.4], radius: [70.0, 70.0], focal: [0.45, -0.3] }, lut: mixed })).unwrap();
    s.push(Node::new([220.0, 80.0, 310.0, 170.0], DrawCmd::Ramp { radius: 45.0, geo: RampGeo::Conic { center: [0.5, 0.5], start_deg: 0.0 }, lut: opaque })).unwrap();
    s.push(Node::new([10.0, 170.0, 150.0, 232.0], DrawCmd::Ramp { radius: 9.0, geo: RampGeo::Conic { center: [0.25, 0.7], start_deg: 77.0 }, lut: mixed }).opacity(0.6).clip([20.0, 175.0, 140.0, 228.5])).unwrap();
    s.push(Node::new([160.0, 180.0, 300.0, 230.0], DrawCmd::Ramp { radius: 25.0, geo: RampGeo::Linear { angle_deg: 90.0 }, lut: mixed }).opacity(0.85)).unwrap();
    s
}

/// The colour a ramp shows at parameter `t`, from its stops, in floats (premultiplied interpolation).
fn ramp_ref(stops: &[(f32, u32)], t: f32) -> [f32; 4] {
    let pmul = |c: u32| {
        let a = (c >> 24) as f32 / 255.0;
        [(c & 255) as f32 * a, ((c >> 8) & 255) as f32 * a, ((c >> 16) & 255) as f32 * a, (c >> 24) as f32]
    };
    let mut k = 0;
    while k + 2 < stops.len() && stops[k + 1].0 <= t {
        k += 1;
    }
    let (a, b) = (stops[k], stops[k + 1]);
    let f = ((t - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
    let (ca, cb) = (pmul(a.1), pmul(b.1));
    [0, 1, 2, 3].map(|i| ca[i] + (cb[i] - ca[i]) * f)
}

#[test]
fn gradients_are_exact_on_every_executor_and_right() {
    let mut rig = Rig::new(&gradient_scene);
    let (d, g, st) = rig.step(&|_| {});
    assert_eq!((d, g), (0, 0), "mount: retained vs specification, Metal vs specification");
    assert!(st.frame.tiles > 0);
    // an idle frame does nothing
    let (d, g, st) = rig.step(&|_| {});
    assert_eq!((d, g, st.lowered, st.frame.tiles, st.frame.recs_visited), (0, 0, 0, 0, 0));
    // an opaque ramp covers the background beneath it: its tiles' lists are cut at it
    let opaque_stops = [(0.0, 0xff00_00ffu32), (0.3, 0xff00_ff00), (0.7, 0xffff_0000), (1.0, 0xff00_ffff)];
    // right, not only consistent: against the stops in floats.
    // a horizontal ramp (90 degrees: left to right) in an unrounded stretch of the box
    for xx in [5u32, 12, 20, 30, 40] {
        // node 1: x 60 .. 106.5, mixed stops, over the dark background
        let t = (xx as f32 + 0.5 + 60.0 - 60.0) / 46.5;
        let c = ramp_ref(&STOPS, t);
        let bg = [0x20 as f32, 0x1c as f32, 0x18 as f32];
        let a = c[3] / 255.0;
        let want = [c[0] + bg[0] * (1.0 - a), c[1] + bg[1] * (1.0 - a), c[2] + bg[2] * (1.0 - a)];
        let got = rig.at(60 + xx, 40);
        for i in 0..3 {
            assert!((got[i] as f32 - want[i]).abs() <= 2.5, "linear ramp at x {xx}: {got:?} vs {want:?}");
        }
    }
    // a radial ramp with the focal point at the centre: the parameter is the distance over the radius
    for (dx, dy) in [(0.0f32, 0.0f32), (20.0, 0.0), (0.0, 30.0), (-30.0, 10.0), (15.0, -20.0)] {
        let (cx, cy) = (60.0 + dx, 120.0 + dy);
        let t = ((dx / 50.0).powi(2) + (dy / 40.0).powi(2)).sqrt().min(1.0);
        let c = ramp_ref(&opaque_stops, t);
        let got = rig.at(cx as u32, cy as u32);
        // the pixel's centre is half a pixel from (cx, cy): allow the gradient's slope over that
        for i in 0..3 {
            assert!((got[i] as f32 - c[i]).abs() <= 14.0, "radial ramp at ({dx}, {dy}): {got:?} vs {c:?}");
        }
    }
    // a conic ramp: clockwise from 12 o'clock
    for (deg, r) in [(10.0f32, 30.0f32), (95.0, 25.0), (181.0, 35.0), (265.0, 20.0), (350.0, 30.0)] {
        let th = deg.to_radians();
        let (cx, cy) = (265.0 + r * th.sin(), 125.0 - r * th.cos());
        let (pxl, pyl) = (cx.floor(), cy.floor());
        let t = ((pxl + 0.5 - 265.0).atan2(-(pyl + 0.5 - 125.0)) / std::f32::consts::TAU).rem_euclid(1.0);
        let c = ramp_ref(&opaque_stops, t);
        let got = rig.at(pxl as u32, pyl as u32);
        for i in 0..3 {
            assert!((got[i] as f32 - c[i]).abs() <= 4.0, "conic ramp at {deg} degrees: {got:?} vs {c:?}");
        }
    }
    // every property a gradient has, changed one record at a time
    let mut g = Rng(77);
    let mut bad = 0;
    for k in 0..40u32 {
        let id = NodeId(1 + g.u(11));
        let pick = g.u(6);
        let (a, b, c) = (g.f(0.0, 1.0), g.f(0.0, 1.0), g.f(-1.0, 1.0));
        let (d, gd, st) = rig.step(&|s: &mut FlatScene| {
            let n = s.node(id);
            let DrawCmd::Ramp { radius, geo, lut } = n.cmd else { panic!() };
            match pick {
                0 => s.set_xf(id, c * 30.0, a * 20.0 - 10.0, 0.6 + b),
                1 => s.set_opacity(id, a),
                2 => s.set_prop(id, Prop::Radius, a * 40.0),
                3 => s.set(id, DrawCmd::Ramp { radius, lut, geo: match geo {
                    RampGeo::Linear { .. } => RampGeo::Linear { angle_deg: a * 720.0 - 360.0 },
                    RampGeo::Radial { .. } => RampGeo::Radial { center: [a, b], radius: [5.0 + 90.0 * b, 5.0 + 90.0 * a], focal: [c * 0.9, (a - 0.5) * 1.2] },
                    RampGeo::Conic { .. } => RampGeo::Conic { center: [b, a], start_deg: c * 400.0 },
                } }),
                4 => {
                    let l = s.lut(&[(0.0, rgba(255, (a * 255.0) as u32, 0, 255)), (b, rgba(0, 40, 255, (a * 255.0) as u32)), (1.0, rgba(250, 250, 250, 255))]).unwrap();
                    s.set(id, DrawCmd::Ramp { radius, geo, lut: l });
                }
                _ => s.set_rect(id, [n.rect[0] + c * 9.0, n.rect[1] + a * 5.0, n.rect[2] + b * 7.0, n.rect[3] - c * 4.0]),
            }
        });
        assert_eq!(st.lowered, 1, "step {k}: one property is one record");
        if (d, gd) != (0, 0) {
            eprintln!("gradient step {k} (node {}, change {pick}): {d} words differ on the CPU, {gd} on Metal", id.0);
            bad += 1;
        }
    }
    eprintln!("flat gradients: {} frames exact against the specification on the CPU tile kernel, {} on Metal; {bad} failing", rig.frames, rig.gpu_frames);
    assert_eq!(bad, 0);
}

#[test]
fn a_ramp_table_is_its_stops() {
    let (t, opaque) = bake_ramp(&STOPS);
    assert!(!opaque);
    // the ends are the end stops, premultiplied; every entry is a premultiplied colour
    assert_eq!(t[0], 0xff20_30ff);
    assert_eq!(t[255], 0xffff_ffff);
    for (i, w) in t.iter().enumerate() {
        let a = w >> 24;
        assert!(w & 255 <= a && (w >> 8) & 255 <= a && (w >> 16) & 255 <= a, "entry {i} is not premultiplied");
        let c = ramp_ref(&STOPS, i as f32 / 255.0);
        for k in 0..4 {
            assert!((((w >> (8 * k)) & 255) as f32 - c[k]).abs() <= 1.0, "entry {i} channel {k}");
        }
    }
    // unsorted stops are sorted; one stop is its colour throughout; none is transparent
    assert_eq!(bake_ramp(&[(1.0, 0xffff_ffff), (0.0, 0xff00_0000)]).0, bake_ramp(&[(0.0, 0xff00_0000), (1.0, 0xffff_ffff)]).0);
    assert!(bake_ramp(&[(0.4, 0xff12_3456)]).0.iter().all(|&w| w == 0xff12_3456));
    assert!(bake_ramp(&[]).0.iter().all(|&w| w == 0));
}

// ---- shadows ------------------------------------------------------------------------------------------

const PAPER: u32 = 0xffe8_e4e0;
fn shadow_scene(insets: bool, outer: usize) -> FlatScene {
    let mut s = FlatScene::new(330, 240, FlatConfig::default());
    s.push(Node::new([0.0, 0.0, 330.0, 240.0], DrawCmd::Solid { color: PAPER })).unwrap();
    // a card: three stacked drop shadows, an opaque fill, two inset shadows, a border
    let a = [30.0, 30.0, 150.0, 120.0];
    let stack = [(30.0, 12.0, -4.0, 0x5000_0000u32), (10.0, 4.0, 0.0, 0x4000_0000), (2.0, 1.0, 0.0, 0x3000_0000)];
    for &(blur, dy, spread, color) in stack.iter().take(outer) {
        s.push(Node::new(a, DrawCmd::Shadow { radius: 14.0, spread, dx: 0.0, dy, blur, color })).unwrap();
    }
    s.push(Node::new(a, DrawCmd::RRect { radius: 14.0, color: 0xffff_ffff })).unwrap();
    if insets {
        s.push(Node::new(a, DrawCmd::Inset { radius: 14.0, spread: 0.0, dx: 0.0, dy: 2.0, blur: 8.0, color: 0x9000_0000 })).unwrap();
        s.push(Node::new(a, DrawCmd::Inset { radius: 14.0, spread: 2.0, dx: -1.0, dy: 0.0, blur: 1.0, color: 0x60ff_c080 })).unwrap();
    }
    s.push(Node::new(a, DrawCmd::Border { radius: 14.0, width: 1.0, color: 0x4000_0000, fill: 0 })).unwrap();
    // a square card off the pixel grid: a spread shadow, a hard fill, a wide inset
    let b = [180.25, 20.5, 300.75, 90.0];
    s.push(Node::new(b, DrawCmd::Shadow { radius: 0.0, spread: 6.0, dx: 5.0, dy: 5.0, blur: 12.0, color: 0x6020_1000 })).unwrap();
    s.push(Node::new(b, DrawCmd::RRect { radius: 0.0, color: 0xfff0_f8ff })).unwrap();
    if insets {
        s.push(Node::new(b, DrawCmd::Inset { radius: 0.0, spread: 4.0, dx: 0.0, dy: 0.0, blur: 16.0, color: 0x8000_4080 })).unwrap();
    }
    // a pill: its radius is wide against the blur (the arc regime), under a translucent fill
    let c = [170.0, 110.0, 310.0, 220.0];
    s.push(Node::new(c, DrawCmd::Shadow { radius: 40.0, spread: 0.0, dx: -3.0, dy: 6.0, blur: 6.0, color: 0x7000_0000 })).unwrap();
    s.push(Node::new(c, DrawCmd::RRect { radius: 40.0, color: 0xc0ff_e0c0 })).unwrap();
    if insets {
        s.push(Node::new(c, DrawCmd::Inset { radius: 40.0, spread: 1.0, dx: 2.0, dy: 3.0, blur: 4.0, color: 0xa000_0000 })).unwrap();
        // a spread wider than the box: the blurred shape is gone and the inset shadow fills the box
        s.push(Node::new(c, DrawCmd::Inset { radius: 40.0, spread: 100.0, dx: 0.0, dy: 0.0, blur: 5.0, color: 0x2000_ff00 }).clip([200.0, 150.0, 330.0, 240.0])).unwrap();
    }
    s
}

#[test]
fn stacked_and_inset_shadows_are_exact_on_every_executor() {
    let mut rig = Rig::new(&|| shadow_scene(true, 3));
    let (d, g, st) = rig.step(&|_| {});
    assert_eq!((d, g), (0, 0), "mount");
    // the drop shadows read retained coverage
    assert!(rig.flat.scene().shadow_coverage_bytes() > 0 && st.frame.raster.px_table > 0, "no shadow pixel came from a coverage patch");
    // what an inset shadow is: nothing outside its box, nothing far from its edges, darker near them
    let plain = {
        let mut r = Rig::new(&|| shadow_scene(false, 3));
        assert_eq!(r.step(&|_| {}).0, 0);
        r
    };
    let (w, h) = rig.flat.size();
    let boxes = [[30.0f32, 30.0, 150.0, 120.0], [180.25, 20.5, 300.75, 90.0], [170.0, 110.0, 310.0, 220.0]];
    let (mut outside, mut inside) = (0, 0);
    for y in 0..h {
        for x in 0..w {
            let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
            let within = boxes.iter().any(|b| cx > b[0] - 1.0 && cx < b[2] + 1.0 && cy > b[1] - 1.0 && cy < b[3] + 1.0);
            let same = rig.at(x, y) == plain.at(x, y);
            if !within {
                outside += (!same) as usize;
            } else {
                inside += (!same) as usize;
            }
        }
    }
    assert_eq!(outside, 0, "an inset shadow drew outside its box");
    assert!(inside > 2000, "the inset shadows changed {inside} pixels");
    assert_eq!(rig.at(90, 75), [255, 255, 255, 255], "the card's centre is beyond its inset shadows' reach");
    assert!(rig.at(90, 33)[0] < 235 && rig.at(90, 33)[0] > 60, "three pixels inside the top edge the inset shadow shows: {:?}", rig.at(90, 33));
    // stacked: each further shadow darkens the paper beneath the card
    let one = {
        let mut r = Rig::new(&|| shadow_scene(true, 1));
        assert_eq!(r.step(&|_| {}).0, 0);
        r
    };
    assert!(rig.at(90, 126)[0] < one.at(90, 126)[0] && one.at(90, 126)[0] < 0xe0, "{:?} {:?}", rig.at(90, 126), one.at(90, 126));
    // every property, one record at a time; a shadow whose shape changed takes its tables back one frame
    // later (one more rewrite, the same pixels), and then the scene is idle
    let n = rig.flat.nodes() as u32;
    let mut g = Rng(5);
    let (mut bad, mut settles) = (0, 0);
    for k in 0..60u32 {
        let id = NodeId(1 + g.u(n - 1));
        let (pick, a, b) = (g.u(9), g.f(0.0, 1.0), g.f(-1.0, 1.0));
        let (d, gd, st) = rig.step(&|s: &mut FlatScene| match pick {
            0 => s.set_prop(id, Prop::Blur, a * 40.0),
            1 => s.set_prop(id, Prop::Spread, b * 12.0),
            2 => s.set_prop(id, Prop::OffX, b * 15.0),
            3 => s.set_prop(id, Prop::OffY, b * 15.0),
            4 => s.set_prop(id, Prop::Radius, a * 60.0),
            5 => s.set_opacity(id, a),
            6 => s.set_xf(id, b * 20.0, a * 9.0, 0.7 + a * 0.6),
            7 => s.set_prop(id, Prop::Alpha, a * 255.0),
            _ => {
                let r = s.node(id).rect;
                s.set_rect(id, [r[0] + b * 3.0, r[1] + a, r[2] + a * 4.0, r[3] + b * 2.0]);
            }
        });
        assert!(st.lowered <= 1);
        let (d2, g2, st2) = rig.step(&|_| {});
        settles += st2.lowered;
        let (d3, g3, st3) = rig.step(&|_| {});
        assert_eq!((st3.lowered, st3.frame.tiles, rig.flat.unsettled()), (0, 0, 0), "step {k}: the scene did not come to rest");
        if (d, gd, d2, g2, d3, g3) != (0, 0, 0, 0, 0, 0) {
            eprintln!("shadow step {k} (node {}, change {pick}): CPU {d} {d2} {d3}, Metal {gd} {g2} {g3} words differ", id.0);
            bad += 1;
        }
    }
    eprintln!("flat shadows: {} frames exact on the CPU, {} on Metal; {settles} shadows took their tables back; {} bytes of retained coverage; {bad} failing", rig.frames, rig.gpu_frames, rig.flat.scene().shadow_coverage_bytes());
    assert!(settles > 0);
    assert_eq!(bad, 0);
}

/// The Metal executor's generated source, broken on purpose: each must be seen.
#[cfg(target_os = "macos")]
#[test]
fn the_oracle_catches_broken_executors() {
    let cases: [(&str, &dyn Fn() -> FlatScene, &dyn Fn(&str) -> String); 4] = [
        ("a ramp that never reaches its next stop", &gradient_scene, &|s| s.replace("T[base], T[base + 1u], at & 0xFFFFu", "T[base], T[base], at & 0xFFFFu")),
        ("a ramp one table entry off", &gradient_scene, &|s| s.replace("metal::min(x_34 >> 15u, 254u)", "metal::min((x_34 >> 15u) + 1u, 254u)")),
        ("an inset shadow that is its own complement", &|| shadow_scene(true, 3), &|s| s.replace("mul15_(co, ONE - metal::min(s_8, ONE))", "mul15_(co, metal::min(s_8, ONE))")),
        ("an inset shadow not cut to its box", &|| shadow_scene(true, 3), &|s| s.replace("uint _e37 = in_cov(_e23, _e36);", "uint _e37 = in_cov(ONE, _e36);")),
    ];
    for (name, make, mutate) in cases {
        let mut rig = Rig::with(make, Some(mutate));
        if rig.twin.is_none() {
            eprintln!("no Metal device: skipped");
            return;
        }
        let (d, g, _) = rig.step(&|_| {});
        eprintln!("flat mutant ({name}): {g} words differ on Metal");
        assert_eq!(d, 0);
        assert!(g > 0, "{name} was not caught");
    }
}
