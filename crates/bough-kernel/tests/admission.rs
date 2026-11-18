//! Review findings K02 and K08: an empty glyph atlas is transparent on every executor, and a transition
//! whose derived fields change reaches the exact staged target state.
use bough_kernel::cpu::{self, Atlas, Binner, Damage, Opt, Pool, Prog};
use bough_kernel::stage::{self, Scene};

fn frame(s: &Scene, fw: u32, fh: u32, atlas: &Atlas, now: u32) -> (Vec<u32>, Vec<u32>) {
    let pool = Pool::new(1);
    let stride = fw as usize;
    let mut naive = vec![0u32; stride * fh as usize];
    cpu::naive(s, fw, fh, atlas, now, &mut naive, stride);
    let mut fast = vec![0u32; stride * fh as usize];
    let mut prog = Prog::default();
    Binner::default().bin(s, fw, fh, fw, atlas, now, &Damage::All, true, &mut prog);
    cpu::raster(s, &prog, atlas, &mut fast, stride, &pool, 1, Opt::default());
    (naive, fast)
}

#[test]
fn an_empty_atlas_is_transparent_on_the_fast_path_too() {
    let clip = stage::NO_CLIP;
    let mut s = Scene::default();
    s.push(stage::solid(0.0, 0.0, 64.0, 64.0, 0xff20_4060, &clip).unwrap());
    s.push(stage::glyph(8.0, 8.0, 12, 16, 0, 0xffff_ffff, &clip).unwrap());
    let empty: [u8; 0] = [];
    let (naive, fast) = frame(&s, 64, 64, &Atlas::new(&empty, 1), 0);
    assert_eq!(naive, fast);
}

#[test]
fn changing_derived_fields_reaches_the_target() {
    let clip = stage::NO_CLIP;
    // a shadow whose blur changes: the inverse blur width is a derived field `k::at` keeps from the start
    let a = stage::shadow(20.0, 20.0, 60.0, 40.0, 6.0, 4.0, 0x8000_0000, &clip).unwrap();
    let b = stage::shadow(20.0, 20.0, 60.0, 40.0, 6.0, 12.0, 0x8000_0000, &clip).unwrap();
    let atlas_px: [u8; 0] = [];
    let atlas = Atlas::new(&atlas_px, 1);
    let mut anim = Scene::default();
    anim.push_anim(a, b, 100, 50, 0);
    let mut direct = Scene::default();
    direct.push(b);
    for now in [150, 151, 100_000] {
        let (n1, f1) = frame(&anim, 128, 96, &atlas, now);
        let (n2, _) = frame(&direct, 128, 96, &atlas, now);
        assert_eq!(n1, f1, "executors disagree at tick {now}");
        assert_eq!(n1, n2, "the transition is not at its target at tick {now}");
    }
    // one the interpolation does carry (the box moves, nothing derived changes) still animates and arrives
    let b2 = stage::shadow(40.0, 30.0, 60.0, 40.0, 6.0, 4.0, 0x8000_0000, &clip).unwrap();
    let mut anim = Scene::default();
    anim.push_anim(a, b2, 100, 50, 0);
    let mut direct = Scene::default();
    direct.push(b2);
    let (mid, _) = frame(&anim, 128, 96, &atlas, 125);
    let (end, _) = frame(&anim, 128, 96, &atlas, 150);
    let (want, _) = frame(&direct, 128, 96, &atlas, 150);
    assert_ne!(mid, end);
    assert_eq!(end, want);
    // the clock wrapping does not send a finished transition back to its start
    let mut late = Scene::default();
    late.push_anim(a, b2, 0xFFFF_FFF0, 8, 0);
    let (wrapped, _) = frame(&late, 128, 96, &atlas, 5);
    assert_eq!(wrapped, want);
}

#[test]
fn animation_endpoint_matches_every_staged_field() {
    use bough_kernel::k;
    let c = stage::NO_CLIP;
    let pairs = [
        (
            stage::shadow(10., 10., 30., 30., 4., 2., 0xff000000, &c).unwrap(),
            stage::shadow(15., 15., 40., 40., 8., 8., 0xff000000, &c).unwrap(),
        ),
        (
            stage::grad(10., 10., 30., 30., 4., 0., 0xffffffff, 0xff000000, &c).unwrap(),
            stage::grad(15., 15., 40., 40., 8., 1., 0xffffffff, 0xff000000, &c).unwrap(),
        ),
        (
            stage::xf(
                stage::rrect(0., 0., 10., 10., 2., 0xffffffff, &c),
                &stage::Xf {
                    m: [1., 0., 0., 1.],
                    k: [0.; 2],
                    t: [0.; 2],
                },
                &c,
            )
            .unwrap(),
            stage::xf(
                stage::rrect(0., 0., 10., 10., 2., 0xffffffff, &c),
                &stage::Xf {
                    m: [0.8, 0.2, -0.2, 0.8],
                    k: [0.; 2],
                    t: [10.; 2],
                },
                &c,
            )
            .unwrap(),
        ),
    ];
    for (a, b) in pairs {
        let mut anim = Scene::default();
        anim.push_anim(a, b, u32::MAX - 9, 20, 0);
        let mut want = Scene::default();
        want.push(b);
        let mut got = Scene::default();
        got.push(anim.load_at(0, 10));
        assert_eq!(got.words, want.words);
        assert_eq!(k::progress(0, u32::MAX - 9, 20, 0), k::ONE / 2);
    }
}

#[test]
fn malformed_programs_are_refused_before_execution() {
    let mut scene = Scene::default();
    scene.add(stage::solid(0., 0., 32., 32., 0xffffffff, &stage::NO_CLIP));
    let atlas = Atlas::new(&[], 1);
    let mut p = Prog::default();
    Binner::default().bin(&scene, 32, 32, 32, &atlas, 0, &Damage::All, false, &mut p);
    for (at, value) in [
        (14, u32::MAX),
        (15, u32::MAX),
        (16, u32::MAX),
        (2, 1),
        (9, u32::MAX),
    ] {
        let mut bad = p.clone();
        bad.words[at] = value;
        assert!(
            std::panic::catch_unwind(|| bad.validate(&scene)).is_err(),
            "accepted field {at}"
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn gpu_rejects_foreign_targets_and_short_outputs() {
    let Some(a) = bough_kernel::gpu_executor() else {
        return;
    };
    let Some(mut b) = bough_kernel::gpu_executor() else {
        return;
    };
    #[repr(align(16384))]
    struct Pages([u8; 16384]);
    let mut pages = Box::new(Pages([0; 16384]));
    // SAFETY: pages remains mapped, aligned and exclusively owned until after target drops.
    let Some(target) = (unsafe { a.wrap(pages.0.as_mut_ptr(), pages.0.len()) }) else {
        return;
    };
    let mut scene = Scene::default();
    scene.add(stage::solid(0., 0., 32., 32., 0xffffffff, &stage::NO_CLIP));
    let atlas = Atlas::new(&[], 1);
    let mut prog = Prog::default();
    Binner::default().bin(
        &scene,
        32,
        32,
        32,
        &atlas,
        0,
        &Damage::All,
        false,
        &mut prog,
    );
    b.upload(&scene, &atlas, &prog);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| b.render_to(1, &target))).is_err()
    );
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| b.render(1, 1))).is_err());
    eprintln!("foreign target and short output checked before dispatch");
    drop(target);
    drop(pages);
}

#[test]
fn safe_gpu_constructors_refuse_custom_programs() {
    #[cfg(target_os = "macos")]
    assert!(bough_kernel::metal::Gpu::new("invalid custom shader").is_none());
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    assert!(bough_kernel::vulkan::Gpu::new(&[0; 4]).is_none());
}
