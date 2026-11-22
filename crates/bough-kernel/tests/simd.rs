//! The vector loops against the scalar kernel on random and extreme lanes (the scene gate, `exact.rs`,
//! holds them equal on whole frames; this holds them equal lane by lane, at 0 and ONE in every position).
use bough_kernel::{cpu, k, scenes::Rng};
use wgsl_rs::std::Vec4;

fn px(r: &mut Rng) -> [u16; 4] {
    // a premultiplied colour: channels <= alpha <= ONE, with the extremes over-represented
    let a = match r.u(5) {
        0 => 0,
        1 => k::ONE,
        _ => r.u(k::ONE + 1),
    };
    let ch = |r: &mut Rng| match r.u(4) {
        0 => 0,
        1 => a,
        _ => r.u(a + 1),
    } as u16;
    [ch(r), ch(r), ch(r), a as u16]
}
fn v(p: [u16; 4]) -> wgsl_rs::std::Vec4u {
    Vec4 { x: p[0] as u32, y: p[1] as u32, z: p[2] as u32, w: p[3] as u32 }
}
fn a(v: wgsl_rs::std::Vec4u) -> [u16; 4] {
    [v.x as u16, v.y as u16, v.z as u16, v.w as u16]
}

#[test]
fn vector_loops_equal_the_scalar_kernel() {
    let mut r = Rng(0xfeed_f00d_1234_5678);
    for round in 0..20_000 {
        let n = 1 + r.u(32) as usize;
        let row: Vec<[u16; 4]> = (0..n).map(|_| px(&mut r)).collect();
        let cov: Vec<u16> = (0..n)
            .map(|_| match r.u(4) {
                0 => 0,
                1 => k::ONE as u16,
                _ => r.u(k::ONE + 1) as u16,
            })
            .collect();
        let (pm, pm2) = (px(&mut r), px(&mut r));
        // constant blend
        let mut got = row.clone();
        cpu::vec::blend_const(&mut got, pm);
        let want: Vec<[u16; 4]> = row.iter().map(|p| a(k::blend(v(*p), v(pm)))).collect();
        assert_eq!(got, want, "blend_const, round {round}");
        // one colour under a coverage row
        let mut got = row.clone();
        cpu::vec::blend_cov(&mut got, pm, &cov);
        let want: Vec<[u16; 4]> = row.iter().zip(&cov).map(|(p, &c)| a(k::blend(v(*p), k::scale(v(pm), c as u32)))).collect();
        assert_eq!(got, want, "blend_cov, round {round}");
        // a border: ring and fill coverages that sum to at most ONE
        let h2: Vec<u16> = cov.iter().map(|&c| r.u(c as u32 + 1) as u16).collect();
        let h1: Vec<u16> = cov.iter().zip(&h2).map(|(&c, &i)| c - i).collect();
        let mut got = row.clone();
        cpu::vec::blend_cov2(&mut got, pm, &h1, pm2, &h2);
        let want: Vec<[u16; 4]> = (0..n).map(|i| a(k::blend(v(row[i]), k::scale(v(pm), h1[i] as u32) + k::scale(v(pm2), h2[i] as u32)))).collect();
        assert_eq!(got, want, "blend_cov2, round {round}");
        // a profile times a value
        let b = cov[0];
        let mut got = vec![0u16; n];
        cpu::vec::scale_row(&cov, b, &mut got);
        let want: Vec<u16> = cov.iter().map(|&c| k::mul15(c as u32, b as u32) as u16).collect();
        assert_eq!(got, want, "scale_row, round {round}");
        // coverage texels to Q15
        let t: Vec<u8> = (0..n).map(|_| [0, 255, r.u(256) as u8][r.u(3) as usize]).collect();
        let mut got = vec![0u16; n];
        let any = cpu::vec::q15_row(&t, &mut got);
        assert_eq!(got, t.iter().map(|&t| k::q15(t as u32) as u16).collect::<Vec<_>>(), "q15_row, round {round}");
        assert_eq!(any, t.iter().any(|&t| t != 0));
        // the store
        let mut got = vec![0u32; n];
        cpu::vec::store_row(&row, &mut got);
        let want: Vec<u32> = row.iter().map(|p| k::store8(v(*p))).collect();
        assert_eq!(got, want, "store_row, round {round}");
    }
}

/// A stored premultiplied pixel (BGRA8), the extremes over-represented.
fn stored(r: &mut Rng) -> u32 {
    let a = match r.u(5) {
        0 => 0,
        1 => 255,
        _ => r.u(256),
    };
    let ch = |r: &mut Rng| match r.u(4) {
        0 => 0,
        1 => a,
        _ => r.u(a + 1),
    };
    ch(r) | ch(r) << 8 | ch(r) << 16 | a << 24
}
fn unstore(w: u32) -> wgsl_rs::std::Vec4u {
    Vec4 { x: k::q15((w >> 16) & 255), y: k::q15((w >> 8) & 255), z: k::q15(w & 255), w: k::q15(w >> 24) }
}

/// The rows of stored pixels and of colours under an opacity, the bilinear read and the mix: each against
/// the scalar arithmetic it stands for.
#[test]
fn surface_loops_equal_the_scalar_kernel() {
    use bough_kernel::simd;
    let mut r = Rng(0x0dd_ba11_5eed);
    for round in 0..20_000 {
        let n = 1 + r.u(40) as usize;
        let row: Vec<[u16; 4]> = (0..n).map(|_| px(&mut r)).collect();
        let src: Vec<u32> = (0..n).map(|_| if round % 7 == 0 { 0 } else { stored(&mut r) }).collect();
        let cols: Vec<[u16; 4]> = (0..n).map(|_| px(&mut r)).collect();
        let op = match r.u(4) {
            0 => 0,
            1 => k::ONE,
            _ => r.u(k::ONE + 1),
        };
        let mut got = row.clone();
        simd::blend_bgra(&mut got, &src, op as u16);
        let want: Vec<[u16; 4]> = row.iter().zip(&src).map(|(p, &w)| a(k::blend(v(*p), k::scale(unstore(w), op)))).collect();
        assert_eq!(got, want, "blend_bgra, round {round}");
        let mut got = row.clone();
        simd::blend_rows(&mut got, &cols, op as u16);
        let want: Vec<[u16; 4]> = row.iter().zip(&cols).map(|(p, c)| a(k::blend(v(*p), k::scale(v(*c), op)))).collect();
        assert_eq!(got, want, "blend_rows, round {round}");
        // the bilinear read: every weight, the extremes over-represented
        let t = [stored(&mut r), stored(&mut r), stored(&mut r), stored(&mut r)];
        let f = |r: &mut Rng| match r.u(4) {
            0 => 0,
            1 => 255,
            _ => r.u(256),
        };
        let (fx, fy) = (f(&mut r), f(&mut r));
        let (p, q, d, e) = (unstore(t[0]), unstore(t[1]), unstore(t[2]), unstore(t[3]));
        let (w0, w1, w2, w3) = ((256 - fx) * (256 - fy), fx * (256 - fy), (256 - fx) * fy, fx * fy);
        let bil = |a: u32, b: u32, c: u32, d: u32| (a * w0 + b * w1 + c * w2 + d * w3) >> 16;
        let want = [bil(p.x, q.x, d.x, e.x), bil(p.y, q.y, d.y, e.y), bil(p.z, q.z, d.z, e.z), bil(p.w, q.w, d.w, e.w)];
        assert_eq!(simd::bil_bgra(t, fx, fy), want, "bil_bgra, round {round}");
        let (m0, m1) = (v(px(&mut r)), v(px(&mut r)));
        let m = k::mix4u(m0, m1, op);
        assert_eq!(simd::mix15([m0.x, m0.y, m0.z, m0.w], [m1.x, m1.y, m1.z, m1.w], op), [m.x, m.y, m.z, m.w], "mix15, round {round}");
    }
}
