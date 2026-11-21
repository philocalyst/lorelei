use bough_kernel::cpu::{self, Atlas, Binner, Damage, Opt, Pool, Prog};
use bough_kernel::stage;

#[test]
fn fractional_shadow_reuse_preserves_integer_coverage() {
    const W: u32 = 173;
    const H: u32 = 149;
    let atlas = Atlas::new(&[], 1);
    let pool = Pool::new(2);
    let mut bin = Binner::default();
    let mut prog = Prog::default();
    for resize in [0.0, 1.25, 0.0] {
        for phase in [0.375, 0.125, 0.375] {
            let mut scene = stage::Scene::default();
            let clip = [5.25, 2.0, 164.5, 140.25];
            for (j, (radius, sigma)) in [(18.0, 2.0), (13.0, 10.0), (0.0, 4.0)]
                .into_iter()
                .enumerate()
            {
                for dx in [-12.0, 6.0, 56.0] {
                    for alpha in [0x803759a4, 0xaa773054] {
                        let c = stage::shadow(
                            dx + phase,
                            18.625 + (j * 9) as f32,
                            65.75 + resize,
                            51.0 + resize,
                            radius,
                            sigma,
                            alpha,
                            &clip,
                        )
                        .unwrap();
                        scene.push(c);
                    }
                }
            }
            let mut expected = vec![0u32; (W * H) as usize];
            cpu::naive(&scene, W, H, &atlas, 0, &mut expected, W as usize);
            for cores in [0, 2] {
                bin.bin(&scene, W, H, W, &atlas, 0, &Damage::All, true, &mut prog);
                let mut pixels = vec![0; expected.len()];
                cpu::raster(
                    &scene,
                    &prog,
                    &atlas,
                    &mut pixels,
                    W as usize,
                    &pool,
                    cores,
                    Opt::default(),
                );
                assert!(
                    pixels == expected,
                    "fractional shadow coverage differs at {phase}, {resize}, {cores}"
                );
            }
        }
    }
}
