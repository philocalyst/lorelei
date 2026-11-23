//! Independent step response and layer allocation ledger for the staged paint seam.
use bough_kernel::cpu::Pool;
use bough_paint::{expr::*, paint::*, scene::Scene};

const W: u32 = 321;
const H: u32 = 161;
type Gray = Solid<Lane, Pm<Rec>>;
type Glass = Backdrop<Lane, Ki<0>, I<Rec>, Filt<K<256>, K<256>, K<256>>>;

fn rect(x0: u32, y0: u32, x1: u32, y1: u32) -> [i32; 4] {
    [x0, y0, x1, y1].map(|n| (n * 256) as i32)
}

fn pane(sigma: f64) -> Glass {
    Backdrop {
        b: Lane,
        r: Ki,
        blur: I {
            a: Rec((sigma * 256.0).round() as u32),
        },
        f: V3 { a: K, b: K, c: K },
    }
}

fn gray(color: u32) -> Gray {
    Solid {
        b: Lane,
        c: Pm { c: Rec(color) },
    }
}

/// Convolution of an opaque byte step with a discrete normalized Gaussian.
fn gaussian_step(x: i32, edge: i32, sigma: f64) -> f64 {
    let support = (8.0 * sigma).ceil() as i32;
    let (mut mass, mut lit) = (0.0, 0.0);
    for dx in -support..=support {
        let weight = (-0.5 * (f64::from(dx) / sigma).powi(2)).exp();
        mass += weight;
        if (x + dx).clamp(0, W as i32 - 1) >= edge {
            lit += weight;
        }
    }
    255.0 * lit / mass
}

#[test]
fn staged_blur_step_error_against_discrete_gaussian() {
    let pool = Pool::new(1);
    for sigma in [0.5f64, 1.2890625, 3.0, 6.0, 12.0, 24.0, 40.0] {
        let mut sum_squared = 0.0;
        let mut max: f64 = 0.0;
        let mut samples = 0;
        let mut pixels = vec![0; (W * H) as usize];
        // Move the vertical edge across the horizontal reconstruction phases.
        for offset in [0, 1, 3, 7, 15, 23, 31] {
            let edge = 144 + offset;
            let mut scene = Scene::new(W, H, 4096);
            let base = scene.column::<Gray>(2);
            let glass = scene.column::<Glass>(1);
            scene.push(base, rect(0, 0, W, H), gray(0xff00_0000));
            scene.push(base, rect(edge as u32, 0, W, H), gray(0xffff_ffff));
            scene.push(glass, rect(0, 0, W, H), pane(sigma));
            scene.frame(&U::default(), &mut pixels, W as usize, &pool, 1);
            let row = &pixels[(H / 2 * W) as usize..((H / 2 + 1) * W) as usize];
            assert_eq!(row[0], 0xff00_0000);
            assert_eq!(row[W as usize - 1], 0xffff_ffff);
            let radius = (3.0 * sigma).ceil() as i32;
            for x in edge - radius..edge + radius {
                let sample = row[x as usize];
                let blue = sample & 255;
                assert_eq!(sample, 0xff00_0000 | blue * 0x0001_0101);
                let error = (f64::from(blue) - gaussian_step(x, edge, sigma)).abs();
                sum_squared += error * error;
                max = max.max(error);
                samples += 1;
            }
        }
        let rms = (sum_squared / f64::from(samples)).sqrt();
        println!(
            "gaussian_step,{sigma:.6},{samples},{max:.4},{rms:.4},{}",
            max <= 16.0 && rms <= 5.0
        );
    }
}

#[test]
fn layer_storage_for_four_spec_sized_panels() {
    let mut scene = Scene::new(640, 400, 4096);
    let glass = scene.column::<Glass>(4);
    for (x, y) in [(2, 1), (186, 114), (246, 160), (528, 324)] {
        scene.push(glass, rect(x, y, x + 112, y + 76), pane(6.0));
    }
    scene.update(&U::default());
    let mut total = 0;
    for layer in &scene.layers.list {
        let reserved: usize = layer.lv.iter().map(|v| (v.cw * v.ch) as usize * 4).sum();
        let logical: usize = layer
            .lv
            .iter()
            .map(|v| ((v.r[2] - v.r[0]) * (v.r[3] - v.r[1])) as usize * 4)
            .sum();
        total += reserved;
        println!("layer_bytes,{},{logical},{reserved}", layer.z);
    }
    assert_eq!(total, scene.layers.live * 4);
    println!(
        "layer_pool_bytes,{total},{},{}",
        scene.layers.pool.len() * 4,
        scene.layers.pool.capacity() * 4
    );
}
