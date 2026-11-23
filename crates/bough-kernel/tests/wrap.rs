//! The CPU expansion of a `#[wgsl]` module has WGSL's integer semantics in every profile: `+ - *`, unary `-`
//! and the compound forms wrap, a shift takes its amount modulo 32 (the macro rewrites them into
//! `wgsl_rs::std::wrap` calls), while the WGSL it emits still spells the plain operators. Run in a debug build,
//! each of these overflowed (a panic) before the rewrite.
#[wgsl_rs::wgsl(skip_validation)]
pub mod w {
    use wgsl_rs::std::*;
    pub const BIG: u32 = 0x80000000u32 + 1u32;
    pub fn mul_u(a: u32, b: u32) -> u32 {
        a * b
    }
    pub fn add_i(a: i32, b: i32) -> i32 {
        a + b + 1
    }
    pub fn sub_u(a: u32) -> u32 {
        0u32 - a
    }
    pub fn neg_i(a: i32) -> i32 {
        -a
    }
    pub fn shifts(a: u32, k: u32) -> u32 {
        (a << k) + (a >> (k + 1u32))
    }
    pub fn sar(a: i32, k: u32) -> i32 {
        a >> k
    }
    pub fn compound(a: u32, b: u32) -> u32 {
        let mut x = a;
        x += b;
        x *= 3u32;
        x -= 0x7fffffffu32;
        x <<= 33u32;
        x >>= 1u32;
        x
    }
    pub fn vec_scale(v: Vec4u, k: u32) -> Vec4u {
        v * k + 2u32 * v
    }
    pub fn lit(a: u32) -> u32 {
        a * 0x400000u32 + (1 << 31u32)
    }
    pub fn floats(a: f32, v: Vec2f) -> Vec2f {
        -(v * a) + 2.0 * v - v * 0.5
    }
}

#[test]
fn wgsl_integer_arithmetic_wraps_on_the_cpu() {
    use wgsl_rs::std::*;
    assert_eq!(w::BIG, 0x80000001);
    assert_eq!(w::mul_u(0x10000, 0x10000), 0);
    assert_eq!(w::add_i(i32::MAX, 0), i32::MIN);
    assert_eq!(w::sub_u(1), u32::MAX);
    assert_eq!(w::neg_i(i32::MIN), i32::MIN);
    assert_eq!(w::shifts(3, 33), (3 << 1) + (3 >> 2));
    assert_eq!(w::sar(-8, 33), -4);
    let mut x = u32::MAX.wrapping_add(2);
    x = x.wrapping_mul(3).wrapping_sub(0x7fffffff).wrapping_shl(33) >> 1;
    assert_eq!(w::compound(u32::MAX, 2), x);
    assert_eq!(
        w::vec_scale(vec4u(u32::MAX, 1, 2, 3), 2),
        vec4u(u32::MAX - 3, 4, 8, 12)
    );
    assert_eq!(w::lit(0x400), 0x80000000);
    assert_eq!(w::floats(2.0, vec2f(1.0, -4.0)), vec2f(-0.5, 2.0));
}

#[test]
fn the_wgsl_keeps_the_plain_operators() {
    let s = w::WGSL_SOURCE.wgsl_source().unwrap();
    for needle in [
        "a * b",
        "a + b + 1;",
        "0u - a",
        "-a",
        "(a << k) + (a >> (k + 1u))",
        "x += b",
        "x <<= 33u",
        "v * k + 2u * v",
    ] {
        assert!(s.contains(needle), "{needle:?} not in\n{s}");
    }
    assert!(!s.contains("wrap"), "{s}");
}
