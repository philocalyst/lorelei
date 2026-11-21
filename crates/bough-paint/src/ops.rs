//! The leaves. Every function an expression or a paint is made of is defined here, once, as concrete
//! integer Rust in the wgsl-rs subset: rustc compiles it into the per-archetype CPU kernels, wgsl-rs and naga
//! turn it into the WGSL and MSL the generated GPU functions call. A combinator (`expr.rs`) or a primitive
//! (`paint.rs`) is nothing but the name of one of these and its operands, so the two executors cannot
//! disagree about what a combinator means.
//!
//! The arithmetic of a pixel is not defined here: it is the round-7 kernel's (`bough_kernel::km`: `sd`, `cov`,
//! `scale`, `blend`, `shadow`, ...), imported. What is here is (a) value operations for expressions, (b) the
//! integer form of the kernel's staging (`mk_*`: operands to a `Cmd`; `stage.rs` does this in floats on the
//! CPU, which is why the generic kernel needs a CPU pass per changed record), and (c) one guarded shade per
//! primitive.
//!
//! Names called from generated code carry no trailing digit (naga's MSL writer renames those).
#[wgsl_rs::wgsl]
pub mod ops {
    use bough_kernel::km::*;
    use wgsl_rs::std::*;

    /// A table tap that needs no read: the correction there is exactly zero.
    pub const T_ZERO: u32 = 0xFFFFFFFEu32;
    /// No table value: the closed form.
    pub const T_MISS: u32 = 0xFFFFFFFFu32;

    // ---- values ----

    pub fn x_i(a: u32) -> i32 {
        a as i32
    }
    pub fn x_u(a: i32) -> u32 {
        max(a, 0i32) as u32
    }
    pub fn x_eq(a: u32, b: u32) -> u32 {
        select(0u32, 1u32, a == b)
    }
    pub fn x_sel(c: u32, a: u32, b: u32) -> u32 {
        select(b, a, c != 0u32)
    }
    pub fn x_seli(c: u32, a: i32, b: i32) -> i32 {
        select(b, a, c != 0u32)
    }
    /// ONE less a Q15 value.
    pub fn x_inv(t: u32) -> u32 {
        ONE - min(t, ONE)
    }
    pub fn x_selv(c: u32, a: Vec4u, b: Vec4u) -> Vec4u {
        if c != 0u32 {
            return a;
        }
        b
    }
    /// Packed RGBA8 to premultiplied Q15.
    pub fn x_pm(c: u32) -> Vec4u {
        premul(c)
    }
    /// Word index of slot `s` (of 256) of palette table `t`.
    pub fn x_palix(t: u32, s: u32) -> u32 {
        t * 256u32 + min(s, 255u32)
    }
    /// The same with alpha forced to 255: opaque whatever the word holds.
    pub fn x_pmo(c: u32) -> Vec4u {
        premul(c | 0xFF000000u32)
    }
    pub fn x_mixv(a: Vec4u, b: Vec4u, t: u32) -> Vec4u {
        mix4u(a, b, min(t, ONE))
    }
    pub fn x_fade(a: Vec4u, k: u32) -> Vec4u {
        scale(a, min(k, ONE))
    }
    /// A looping there-and-back progress (Q15): 0 at `now + phase == 0 mod 2 period`, ONE one period later.
    pub fn x_tri(now: u32, phase: u32, period: u32) -> u32 {
        let p = clamp(period, 1u32, 65535u32);
        let m = (now + phase) % (2u32 * p);
        let e = select(2u32 * p - m, m, m < p);
        (e * ONE) / p
    }
    /// Smoothstep of a Q15 progress.
    pub fn x_smooth(t0: u32) -> u32 {
        let t = min(t0, ONE);
        mul15(mul15(t, t), 3u32 * ONE - 2u32 * t)
    }
    /// One-shot progress (Q15) of a transition started at `t0` lasting `dur` ticks: 0 before it starts, ONE
    /// once it is over. The clock is a wrapping word: "started" means `now` is at most 2^31 ticks after `t0`
    /// modulo 2^32, so nothing happens to a finished transition when the clock wraps, and a clock pushed
    /// 2^30 ahead (reduced motion) puts every transition at its end.
    pub fn x_prog(now: u32, t0: u32, dur0: u32) -> u32 {
        let mut e = now - min(t0, now);
        if now < t0 {
            e = (0xFFFFFFFFu32 - t0) + now + 1u32;
        }
        if e >= 0x80000000u32 {
            return 0u32;
        }
        let dur = clamp(dur0, 1u32, 65535u32);
        if e >= dur {
            return ONE;
        }
        (e * ONE) / dur
    }
    /// Ease in (cubic) of a Q15 progress.
    pub fn x_ein(t0: u32) -> u32 {
        let t = min(t0, ONE);
        mul15(mul15(t, t), t)
    }
    /// Ease out (cubic).
    pub fn x_eout(t0: u32) -> u32 {
        let u = ONE - min(t0, ONE);
        ONE - mul15(mul15(u, u), u)
    }
    /// Ease in and out (cubic).
    pub fn x_einout(t0: u32) -> u32 {
        let t = min(t0, ONE);
        if t < HALF {
            return 4u32 * mul15(mul15(t, t), t);
        }
        let u = ONE - t;
        ONE - 4u32 * mul15(mul15(u, u), u)
    }
    pub fn x_rect(a: u32, b: u32, c: u32, d: u32) -> Vec4i {
        vec4i(a as i32, b as i32, c as i32, d as i32)
    }
    /// A rect moved by (dx, dy), Q8.
    pub fn x_off(b: Vec4i, dx: i32, dy: i32) -> Vec4i {
        vec4i(b.x() + dx, b.y() + dy, b.z() + dx, b.w() + dy)
    }
    /// A rect grown by `d` on every side, Q8 (a shadow's spread).
    pub fn x_grow(b: Vec4i, d: i32) -> Vec4i {
        vec4i(b.x() - d, b.y() - d, b.z() + d, b.w() + d)
    }
    pub fn x_mixr(a: Vec4i, b: Vec4i, t: u32) -> Vec4i {
        mix4i(a, b, min(t, ONE))
    }
    pub fn x_mixi(a: i32, b: i32, t: u32) -> i32 {
        mix_i(a, b, min(t, ONE))
    }
    /// The first `h` (Q8) of a rect's height: a header band.
    pub fn x_top(b: Vec4i, h: i32) -> Vec4i {
        vec4i(b.x(), b.y(), b.z(), min(b.y() + h, b.w()))
    }
    pub fn x_and(a: Vec4i, b: Vec4i) -> Vec4i {
        vec4i(max(a.x(), b.x()), max(a.y(), b.y()), min(a.z(), b.z()), min(a.w(), b.w()))
    }
    /// The bounding window of two pixel windows (an empty one, x0 >= x1, is the identity).
    pub fn x_union(a: Vec4i, b: Vec4i) -> Vec4i {
        if b.x() >= b.z() {
            return a;
        }
        if a.x() >= a.z() {
            return b;
        }
        vec4i(min(a.x(), b.x()), min(a.y(), b.y()), max(a.z(), b.z()), max(a.w(), b.w()))
    }
    pub fn x_over(acc: Vec4u, sh: Vec4u) -> Vec4u {
        blend(acc, sh)
    }
    pub fn x_in(w: Vec4i, x: i32, y: i32) -> bool {
        inside(w, x, y)
    }
    pub fn x_nev(a: Vec4u, b: Vec4u) -> bool {
        a.x() != b.x() || a.y() != b.y() || a.z() != b.z() || a.w() != b.w()
    }
    pub fn x_ner(a: Vec4i, b: Vec4i) -> bool {
        a.x() != b.x() || a.y() != b.y() || a.z() != b.z() || a.w() != b.w()
    }
    pub fn x_neu(a: u32, b: u32) -> bool {
        a != b
    }
    pub fn x_nei(a: i32, b: i32) -> bool {
        a != b
    }
    /// A packed RGBA8 word's alpha is 255.
    pub fn x_full(c: u32) -> bool {
        (c >> 24u32) == 255u32
    }
    /// `r` if `k`, else the empty rect.
    pub fn x_keep(r: Vec4i, k: bool) -> Vec4i {
        if k {
            return r;
        }
        vec4i(0i32, 0i32, 0i32, 0i32)
    }
    pub fn x_store(acc: Vec4u) -> u32 {
        store8(acc)
    }

    // ---- staging in integers (the twin of bough-kernel's stage.rs, which takes floats) ----

    /// The first pixel whose centre is at or after `q` (Q8).
    pub fn pxc(q: i32) -> i32 {
        (q + 127i32) >> 8u32
    }
    /// Pixels whose centres lie in [a, b) cut by `clip` (all Q8); the zero window when there are none.
    pub fn window(ax: i32, ay: i32, bx: i32, by: i32, clip: Vec4i) -> Vec4i {
        let x0 = pxc(min(max(ax, clip.x()), clip.z()));
        let y0 = pxc(min(max(ay, clip.y()), clip.w()));
        let x1 = pxc(min(max(bx, clip.x()), clip.z()));
        let y1 = pxc(min(max(by, clip.y()), clip.w()));
        if x0 < x1 && y0 < y1 {
            return vec4i(max(x0, -32768i32), max(y0, -32768i32), min(x1, 32767i32), min(y1, 32767i32));
        }
        vec4i(0i32, 0i32, 0i32, 0i32)
    }
    /// Pixels whose centres are at least `kc` inside the box (coverage provably ONE), within `win`.
    pub fn core_of(b: Vec4i, kc: i32, win: Vec4i) -> Vec4i {
        let x0 = max(pxc(b.x() + kc), win.x());
        let y0 = max(pxc(b.y() + kc), win.y());
        let x1 = min(((b.z() - kc - 128i32) >> 8u32) + 1i32, win.z());
        let y1 = min(((b.w() - kc - 128i32) >> 8u32) + 1i32, win.w());
        if x0 < x1 && y0 < y1 {
            return vec4i(x0, y0, x1, y1);
        }
        vec4i(0i32, 0i32, 0i32, 0i32)
    }
    /// The root shift for a corner radius: its legs (radius + the AA margin) must fit 14 bits.
    pub fn shift_of(r: i32) -> u32 {
        let mut s = 0u32;
        while ((r + 512i32) >> s) > 16383i32 {
            s += 1u32;
        }
        s
    }
    /// A radius clamped to half the box's shorter side.
    pub fn radius_in(b: Vec4i, r: i32) -> i32 {
        max(min(r, min(b.z() - b.x(), b.w() - b.y()) / 2i32), 0i32)
    }
    pub fn mk(kind: u32, sh: u32, win: Vec4i, core: Vec4i, b: Vec4i, rad: Vec4i, aux: u32, pm: Vec4u, pm2: Vec4u) -> Cmd {
        Cmd {
            win: win,
            core: core,
            b: b,
            par: vec4u(kind, sh, aux, 0u32),
            rad: rad,
            pm: pm,
            pm2: pm2,
            xi: vec4i(0i32, 0i32, 0i32, 0i32),
            xk: vec4i(0i32, 0i32, 0i32, 0i32),
            xs: 0i32,
        }
    }
    /// A solid box: every pixel whose centre it holds takes `pm`.
    pub fn mk_solid(b: Vec4i, pm: Vec4u, clip: Vec4i) -> Cmd {
        let win = window(b.x(), b.y(), b.z(), b.w(), clip);
        mk(K_SOLID, 0u32, win, win, b, vec4i(0i32, 0i32, 0i32, 0i32), 0u32, pm, splat(0u32))
    }
    pub fn mk_rrect(b: Vec4i, r0: i32, pm: Vec4u, clip: Vec4i) -> Cmd {
        let win = window(b.x() - 256i32, b.y() - 256i32, b.z() + 256i32, b.w() + 256i32, clip);
        let r = radius_in(b, r0);
        mk(K_RRECT, shift_of(r), win, core_of(b, max(r, 128i32) + 136i32, win), b, vec4i(r, 0i32, 0i32, 0i32), 0u32, pm, splat(0u32))
    }
    /// A rounded box filled with `fill` carrying a border of `bw` (Q8) in `pm` inside its edge.
    pub fn mk_border(b: Vec4i, r0: i32, bw0: i32, pm: Vec4u, fill: Vec4u, clip: Vec4i) -> Cmd {
        let win = window(b.x() - 256i32, b.y() - 256i32, b.z() + 256i32, b.w() + 256i32, clip);
        let r = radius_in(b, r0);
        let bw = max(bw0, 0i32);
        mk(K_BORDER, shift_of(r), win, core_of(b, max(r, 128i32 + bw) + 136i32, win), b, vec4i(r, bw, 0i32, 0i32), 0u32, pm, fill)
    }
    /// The shadow of a rounded box blurred by a spline of width `blur` (Q8, two sigma). `tab`: the handle
    /// of its corner table (`T_MISS`: none), carried in the command's spare word.
    pub fn mk_shadow(b: Vec4i, r0: i32, blur: i32, pm: Vec4u, tab: u32, clip: Vec4i) -> Cmd {
        let bw = min(max(blur, 16i32), 32768i32);
        let ext = bw + bw / 2i32 + 256i32;
        let mut win = window(b.x() - ext, b.y() - ext, b.z() + ext, b.w() + ext, clip);
        // a box a negative spread has turned inside out casts no shadow (its profile would be a difference
        // of the wrong sign)
        if b.z() <= b.x() || b.w() <= b.y() {
            win = vec4i(0i32, 0i32, 0i32, 0i32);
        }
        let r = radius_in(b, r0);
        let inv = (16777216i32 + bw - 1i32) / bw;
        let mut curve = 0i32;
        if r > 0i32 {
            curve = min((bw * 32768i32) / (8i32 * r), 32768i32);
        }
        mk(K_SHADOW, shift_of(r), win, core_of(b, bw + bw / 2i32 + r + 2i32, win), b, vec4i(r, bw, inv, curve), tab, pm, splat(0u32))
    }

    // ---- the corner table: a memo of `corner3`, which depends on the pixel's offset from the corner and on
    // ---- (radius, blur width) only. One table per distinct (radius, blur), entries at whole-pixel steps
    // ---- from a pixel centre, so it serves every box whose corner is on the pixel grid; anything else
    // ---- (no table, a table for another key, a box off the grid) takes the closed form. A table holds two
    // ---- header words (radius, blur width: the key it was filled for) and n x n entries.

    pub fn c_tab(c: Cmd) -> u32 {
        c.par.z()
    }
    /// The table's reach before the corner, Q8, a whole number of pixels: entries start at -t_d.
    pub fn t_d(r: i32, w: i32) -> i32 {
        ((r + w + (w >> 1u32) + 255i32) >> 8u32) << 8u32
    }
    /// Entries per side.
    pub fn t_n(r: i32, w: i32) -> u32 {
        ((t_d(r, w) >> 8u32) + ((w + (w >> 1u32) + 255i32) >> 8u32)) as u32
    }
    /// Entries are 16 bits (a correction is below 2^16: its largest possible value is about 0.75 ONE), two
    /// to a word, rows padded to whole words: a row's stride in entries.
    pub fn t_stride(n: u32) -> u32 {
        (n + 1u32) & 0xFFFFFFFEu32
    }
    /// The shadow is one whose corners go through `corner3` (rounded, and not so wide a radius that the
    /// arc's distance profile is used).
    pub fn t_regime(r: i32, w: i32) -> bool {
        r != 0i32 && r < w * SHADOW_ARC
    }
    /// The table whose header is (h0, h1) is this shadow's.
    pub fn x_tabok(c: Cmd, h0: u32, h1: u32) -> bool {
        t_regime(c.rad.x(), c.rad.y()) && h0 == (c.rad.x() as u32) && h1 == (c.rad.y() as u32)
    }
    /// One corner's table entry from its offset (dx, dy) to the pixel: `T_ZERO` out of the corner's reach
    /// (`rl` = radius + 1.5 blur widths before it, `lim` = 1.5 blur widths past it), where the correction is
    /// exactly zero.
    pub fn t_idx(dx: i32, dy: i32, rl: i32, lim: i32, d: i32, n: u32) -> u32 {
        if dx <= -rl || dy <= -rl || dx >= lim || dy >= lim {
            return T_ZERO;
        }
        (((dy + d) >> 8u32) as u32) * t_stride(n) + (((dx + d) >> 8u32) as u32)
    }
    /// The table entries holding the four corners' corrections at pixel (x, y), in the order `km::shadow`
    /// sums them (x mirrored on odd, y mirrored on the last two); `T_ZERO` where a correction is exactly
    /// zero; all `T_MISS` where the closed form must be used (no table for this key, or a box whose corners
    /// are not on the pixel grid).
    pub fn x_ctaps(c: Cmd, x: i32, y: i32, ok: bool) -> Vec4u {
        if !ok {
            return splat(T_MISS);
        }
        if !inside(c.win, x, y) || inside(c.core, x, y) {
            return splat(T_ZERO);
        }
        let hx = (c.b.z() - c.b.x()) >> 1u32;
        let hy = (c.b.w() - c.b.y()) >> 1u32;
        let px = x * 256i32 + 128i32 - (c.b.x() + hx);
        let py = y * 256i32 + 128i32 - (c.b.y() + hy);
        // on the grid: both mirrored offsets of both axes are whole pixels from a pixel centre
        if (((px - hx) | (-px - hx) | (py - hy) | (-py - hy)) & 255i32) != 128i32 || (((px - hx) & (-px - hx) & (py - hy) & (-py - hy)) & 128i32) == 0i32 {
            return splat(T_MISS);
        }
        let r = c.rad.x();
        let w = c.rad.y();
        let lim = w + (w >> 1u32);
        let rl = r + lim;
        let d = ((rl + 255i32) >> 8u32) << 8u32;
        let n = ((d >> 8u32) + ((lim + 255i32) >> 8u32)) as u32;
        vec4u(t_idx(px - hx, py - hy, rl, lim, d, n), t_idx(-px - hx, py - hy, rl, lim, d, n), t_idx(px - hx, -py - hy, rl, lim, d, n), t_idx(-px - hx, -py - hy, rl, lim, d, n))
    }
    /// Word `k` of row `j` of the table for (r, w): entries 2k and 2k + 1 of the row.
    pub fn x_cfillw(r: i32, w: i32, k: u32, j: u32) -> u32 {
        let mut hi = 0u32;
        if 2u32 * k + 1u32 < t_n(r, w) {
            hi = x_cfill(r, w, 2u32 * k + 1u32, j);
        }
        (x_cfill(r, w, 2u32 * k, j) & 0xFFFFu32) | (hi << 16u32)
    }
    /// Entry `idx` (a `t_idx`) of the table whose words follow: the two words holding it are not read here;
    /// the caller reads word `idx >> 1` and hands it over.
    pub fn t_half(word: u32, idx: u32) -> u32 {
        (word >> ((idx & 1u32) * 16u32)) & 0xFFFFu32
    }
    /// Entry (i, j) of the table for (r, w): `corner3` at that offset from the corner of any box.
    pub fn x_cfill(r: i32, w: i32, i: u32, j: u32) -> u32 {
        let d = t_d(r, w);
        let dx = (i as i32) * 256i32 - d + 128i32;
        let dy = (j as i32) * 256i32 - d + 128i32;
        let h = 1048576i32;
        let inv = (16777216i32 + w - 1i32) / w;
        let z = vec4i(0i32, 0i32, 0i32, 0i32);
        let c = mk(K_SHADOW, shift_of(r), z, z, z, vec4i(r, w, inv, 0i32), 0u32, splat(0u32), splat(0u32));
        corner3(c, h + dx, h + dy, h, h)
    }
    /// One corner's correction: the table's value, or the closed form where there was none.
    pub fn t_tap(c: Cmd, v: u32, qx: i32, qy: i32, hx: i32, hy: i32) -> u32 {
        if v == T_MISS {
            return corner3(c, qx, qy, hx, hy);
        }
        v
    }
    /// `km::shadow` with the four corner corrections given (each a table value or `T_MISS`).
    pub fn x_shadowm(c: Cmd, x: i32, y: i32, v0: u32, v1: u32, v2: u32, v3: u32) -> u32 {
        let cx = x * 256i32 + 128i32;
        let cy = y * 256i32 + 128i32;
        if v0 == T_MISS && v1 == T_MISS && v2 == T_MISS && v3 == T_MISS {
            return shadow(c, cx, cy);
        }
        let hx = (c.b.z() - c.b.x()) >> 1u32;
        let hy = (c.b.w() - c.b.y()) >> 1u32;
        let px = cx - (c.b.x() + hx);
        let py = cy - (c.b.y() + hy);
        let sep = mul15(span3(px, hx, c.rad.y(), c.rad.z()), span3(py, hy, c.rad.y(), c.rad.z()));
        let corr = t_tap(c, v0, px, py, hx, hy) + t_tap(c, v1, -px, py, hx, hy) + t_tap(c, v2, px, -py, hx, hy) + t_tap(c, v3, -px, -py, hx, hy);
        sep - min(sep, corr)
    }
    /// A two-stop linear gradient in a rounded box; (dx, dy) is the direction scaled so the parameter
    /// reaches ONE across the box (computed where the box's size is known).
    pub fn mk_grad(b: Vec4i, r0: i32, dx: i32, dy: i32, pm: Vec4u, pm2: Vec4u, clip: Vec4i) -> Cmd {
        let win = window(b.x() - 256i32, b.y() - 256i32, b.z() + 256i32, b.w() + 256i32, clip);
        let r = radius_in(b, r0);
        mk(K_GRAD, shift_of(r), win, core_of(b, max(r, 128i32) + 136i32, win), b, vec4i(r, 0i32, dx, dy), 0u32, pm, pm2)
    }
    /// A glyph: the box is its texels' box (whole pixels), `texel` its first texel in the atlas.
    pub fn mk_glyph(b: Vec4i, texel: u32, pm: Vec4u, clip: Vec4i) -> Cmd {
        let win = window(b.x(), b.y(), b.z(), b.w(), clip);
        mk(K_GLYPH, 0u32, win, vec4i(0i32, 0i32, 0i32, 0i32), b, vec4i(0i32, 0i32, 0i32, 0i32), texel, pm, splat(0u32))
    }


    // ---- layers: a backdrop record reads the flattened prefix beneath it through a pyramid --------------
    // Level 0 is the 8-bit prefix (what `x_store` would have written with nothing above); level l + 1 is
    // level l under a separable [1 3 3 1] / 8 tent, decimated by two, anchored to the frame's pixel grid
    // (level-l pixel i covers frame pixels [i 2^l, (i + 1) 2^l)). A read is bilinear at one or two levels.

    /// Paint kinds the generic kernel does not know.
    pub const K_BACKDROP: u32 = 16u32;
    pub const K_VIDEO: u32 = 17u32;
    /// The coarsest pyramid level.
    pub const BD_LMAX: u32 = 5u32;
    /// Standard deviation (Q8 px) of a read at level 1; level l is 2^(l-1) times it.
    pub const BD_SIGMA: i32 = 330i32;

    /// Three words as a vector (a backdrop's filter factors).
    pub fn x_vecf(a: u32, b: u32, c: u32) -> Vec4u {
        vec4u(a, b, c, 0u32)
    }
    pub fn x_bytes(w: u32) -> Vec4u {
        vec4u(w & 255u32, (w >> 8u32) & 255u32, (w >> 16u32) & 255u32, w >> 24u32)
    }
    pub fn x_word(v: Vec4u) -> u32 {
        v.x() | (v.y() << 8u32) | (v.z() << 16u32) | (v.w() << 24u32)
    }
    /// A stored pixel (BGRA8, `x_store`'s word) as a premultiplied Q15 accumulator.
    pub fn x_unstore(w: u32) -> Vec4u {
        vec4u(q15((w >> 16u32) & 255u32), q15((w >> 8u32) & 255u32), q15(w & 255u32), q15(w >> 24u32))
    }
    /// The accumulator as it is after being stored and read back: what a backdrop record sees beneath it.
    pub fn x_flat(acc: Vec4u) -> Vec4u {
        x_unstore(store8(acc))
    }
    /// A backdrop: what is beneath the rounded box `b`, blurred by about `blur` (Q8 px, a standard
    /// deviation) and passed through brightness, saturation and contrast (`f`: Q8 each, 256 = unchanged).
    /// `rad.y` is the finer level read, `rad.z` the weight (Q15) of the next coarser one.
    pub fn mk_backdrop(b: Vec4i, r0: i32, blur: i32, f: Vec4u, clip: Vec4i) -> Cmd {
        let win = window(b.x() - 256i32, b.y() - 256i32, b.z() + 256i32, b.w() + 256i32, clip);
        let r = radius_in(b, r0);
        let s = max(blur, 0i32);
        let mut lvl = 0i32;
        let mut base = BD_SIGMA;
        let mut fr = 0i32;
        if s < BD_SIGMA {
            fr = (s * 32768i32) / BD_SIGMA;
        } else {
            lvl = 1i32;
            while lvl < (BD_LMAX as i32) && s >= base * 2i32 {
                lvl += 1i32;
                base = base * 2i32;
            }
            if lvl < (BD_LMAX as i32) {
                fr = min(((s - base) * 512i32 / base) * 64i32, 32767i32);
            }
        }
        mk(K_BACKDROP, shift_of(r), win, core_of(b, max(r, 128i32) + 136i32, win), b, vec4i(r, lvl, fr, 0i32), 0u32, vec4u(min(f.x(), 4096u32), min(f.y(), 4096u32), min(f.z(), 4096u32), 0u32), splat(0u32))
    }
    /// The coarsest level a backdrop reads.
    pub fn bd_top(c: Cmd) -> u32 {
        (c.rad.y() as u32) + select(0u32, 1u32, c.rad.z() != 0i32)
    }
    /// The level-`l` pixels a bilinear read at level `l` takes for the frame pixels of `win`.
    pub fn bd_recon(win: Vec4i, l: u32) -> Vec4i {
        let h = (1i32 << l) - 1i32;
        vec4i((2i32 * win.x() - h) >> (l + 1u32), (2i32 * win.y() - h) >> (l + 1u32), ((2i32 * win.z() - 2i32 - h) >> (l + 1u32)) + 2i32, ((2i32 * win.w() - 2i32 - h) >> (l + 1u32)) + 2i32)
    }
    /// The level-`l` pixels (x0, y0, x1, y1, not cut to the frame) a backdrop needs: its reads, and what the
    /// levels above are made from. Level 0 is its halo: the frame pixels whose prefix it keeps. Empty for a
    /// level it does not have.
    pub fn bd_rect(c: Cmd, l: u32) -> Vec4i {
        let top = bd_top(c);
        if l > top || c.win.x() >= c.win.z() {
            return vec4i(0i32, 0i32, 0i32, 0i32);
        }
        let mut r = bd_recon(c.win, top);
        let mut k = top;
        while k > l {
            k -= 1u32;
            r = vec4i(2i32 * r.x() - 1i32, 2i32 * r.y() - 1i32, 2i32 * r.z() + 1i32, 2i32 * r.w() + 1i32);
            if k == (c.rad.y() as u32) {
                let q = bd_recon(c.win, k);
                r = vec4i(min(r.x(), q.x()), min(r.y(), q.y()), max(r.z(), q.z()), max(r.w(), q.w()));
            }
        }
        r
    }
    /// The lower of the two level-`l` pixels a read at frame pixel `x` takes, and the weight of the upper (Q8).
    pub fn bd_i(x: i32, l: u32) -> i32 {
        (2i32 * x + 1i32 - (1i32 << l)) >> (l + 1u32)
    }
    pub fn bd_f(x: i32, l: u32) -> u32 {
        (((2i32 * x + 1i32 - (1i32 << l)) & ((1i32 << (l + 1u32)) - 1i32)) as u32) << (7u32 - l)
    }
    /// A level's size along an axis of `n` frame pixels.
    pub fn bd_dim(n: u32, l: u32) -> i32 {
        ((n + (1u32 << l) - 1u32) >> l) as i32
    }
    /// Pixel (x, y) (already inside the level) in a toroidal window of `cw` x `ch` words at `base`.
    pub fn bd_addr(x: i32, y: i32, cw: u32, ch: u32, base: u32) -> u32 {
        base + ((y as u32) % ch) * cw + ((x as u32) % cw)
    }
    /// Four stored pixels under bilinear weights (Q8 fractions), as Q15.
    pub fn bd_bil(t0: u32, t1: u32, t2: u32, t3: u32, fx: u32, fy: u32) -> Vec4u {
        let a = x_unstore(t0);
        let b = x_unstore(t1);
        let d = x_unstore(t2);
        let e = x_unstore(t3);
        let w0 = (256u32 - fx) * (256u32 - fy);
        let w1 = fx * (256u32 - fy);
        let w2 = (256u32 - fx) * fy;
        let w3 = fx * fy;
        vec4u((a.x() * w0 + b.x() * w1 + d.x() * w2 + e.x() * w3) >> 16u32, (a.y() * w0 + b.y() * w1 + d.y() * w2 + e.y() * w3) >> 16u32, (a.z() * w0 + b.z() * w1 + d.z() * w2 + e.z() * w3) >> 16u32, (a.w() * w0 + b.w() * w1 + d.w() * w2 + e.w() * w3) >> 16u32)
    }
    pub fn bd_ch(v: u32, a: u32, luma: i32, f: Vec4u) -> u32 {
        let b = min((v * f.x()) >> 8u32, a) as i32;
        let s = clamp(luma + (((b - luma) * (f.y() as i32)) >> 8u32), 0i32, a as i32);
        let h = (a >> 1u32) as i32;
        clamp(h + (((s - h) * (f.z() as i32)) >> 8u32), 0i32, a as i32) as u32
    }
    /// Brightness, then saturation about the luma, then contrast about mid grey, on a premultiplied colour.
    pub fn bd_filter(c: Vec4u, f: Vec4u) -> Vec4u {
        let a = c.w();
        let l0 = (min((c.x() * f.x()) >> 8u32, a) * 54u32 + min((c.y() * f.x()) >> 8u32, a) * 183u32 + min((c.z() * f.x()) >> 8u32, a) * 19u32) >> 8u32;
        let luma = l0 as i32;
        vec4u(bd_ch(c.x(), a, luma, f), bd_ch(c.y(), a, luma, f), bd_ch(c.z(), a, luma, f), a)
    }
    /// A backdrop at one pixel: `acc` is the (flattened) accumulator, `bl` the blurred prefix there.
    pub fn p_bdrop(acc: Vec4u, c: Cmd, x: i32, y: i32, bl: Vec4u) -> Vec4u {
        if !inside(c.win, x, y) {
            return acc;
        }
        let mut cv = ONE;
        if !inside(c.core, x, y) {
            cv = cov(sd(x * 256i32 + 128i32, y * 256i32 + 128i32, c.b, c.rad.x(), c.par.y()));
        }
        mix4u(acc, bd_filter(bl, c.pm), cv)
    }
    /// One row of a downsample: four stored pixels under (1, 3, 3, 1), per byte.
    pub fn dn_row(a: u32, b: u32, c: u32, d: u32) -> Vec4u {
        let p = x_bytes(a);
        let q = x_bytes(b);
        let r = x_bytes(c);
        let s = x_bytes(d);
        vec4u(p.x() + 3u32 * (q.x() + r.x()) + s.x(), p.y() + 3u32 * (q.y() + r.y()) + s.y(), p.z() + 3u32 * (q.z() + r.z()) + s.z(), p.w() + 3u32 * (q.w() + r.w()) + s.w())
    }
    /// Four such rows under (1, 3, 3, 1) / 64, rounded half up, as a stored pixel.
    pub fn dn_fin(p: Vec4u, q: Vec4u, r: Vec4u, s: Vec4u) -> u32 {
        x_word(vec4u((p.x() + 3u32 * (q.x() + r.x()) + s.x() + 32u32) >> 6u32, (p.y() + 3u32 * (q.y() + r.y()) + s.y() + 32u32) >> 6u32, (p.z() + 3u32 * (q.z() + r.z()) + s.z() + 32u32) >> 6u32, (p.w() + 3u32 * (q.w() + r.w()) + s.w() + 32u32) >> 6u32))
    }

    // ---- video: an NV12 frame (BT.709, limited range) scaled into a rounded box ----------------------

    /// `slot`: the frame's header in the media pool (the glue reads it); `dims` = width | height << 16 of
    /// its luma plane; `op`: opacity, Q15. `rad.z`, `rad.w`: luma texels per pixel, Q12.
    pub fn mk_video(b: Vec4i, r0: i32, slot: u32, dims: u32, op: u32, clip: Vec4i) -> Cmd {
        let win = window(b.x() - 256i32, b.y() - 256i32, b.z() + 256i32, b.w() + 256i32, clip);
        let r = radius_in(b, r0);
        let vw = clamp(dims & 65535u32, 2u32, 2047u32) as i32;
        let vh = clamp(dims >> 16u32, 2u32, 2047u32) as i32;
        let kx = min((vw << 20u32) / max(b.z() - b.x(), 1i32), 8388607i32);
        let ky = min((vh << 20u32) / max(b.w() - b.y(), 1i32), 8388607i32);
        mk(K_VIDEO, shift_of(r), win, core_of(b, max(r, 128i32) + 136i32, win), b, vec4i(r, 0i32, kx, ky), slot, vec4u(min(op, ONE), vw as u32, vh as u32, 0u32), splat(0u32))
    }
    /// The pixel's position in luma texels (Q8; texel 0's centre is 0).
    pub fn vd_pos(c: Cmd, x: i32, y: i32) -> Vec2i {
        let dx = clamp(x * 256i32 + 128i32 - c.b.x(), 0i32, 4194303i32);
        let dy = clamp(y * 256i32 + 128i32 - c.b.y(), 0i32, 4194303i32);
        vec2i(mulq16(dx, c.rad.z()) * 16i32 - 128i32, mulq16(dy, c.rad.w()) * 16i32 - 128i32)
    }
    /// Texel index `j` (0 or 1) of a bilinear pair at Q8 position `t`, in a plane of `n` texels.
    pub fn vd_ix(t: i32, j: i32, n: i32) -> u32 {
        clamp((t >> 8u32) + j, 0i32, n - 1i32) as u32
    }
    /// Four bytes under bilinear weights of the Q8 position (tx, ty): Q8.
    pub fn vd_bil(a: u32, b: u32, d: u32, e: u32, tx: i32, ty: i32) -> u32 {
        let fx = (tx & 255i32) as u32;
        let fy = (ty & 255i32) as u32;
        (a * (256u32 - fx) * (256u32 - fy) + b * fx * (256u32 - fy) + d * (256u32 - fx) * fy + e * fx * fy) >> 8u32
    }
    /// Y, Cb, Cr (Q8 of 0..255) as an opaque Q15 colour.
    pub fn vd_rgb(y8: u32, cb8: u32, cr8: u32) -> Vec4u {
        let y = ((((y8 as i32) - 4096i32) >> 1u32) * 38304i32) >> 15u32;
        let cb = ((cb8 as i32) - 32768i32) >> 1u32;
        let cr = ((cr8 as i32) - 32768i32) >> 1u32;
        let r = y + ((cr * 58975i32) >> 15u32);
        let g = y - ((cb * 7015i32) >> 15u32) - ((cr * 17531i32) >> 15u32);
        let bl = y + ((cb * 69491i32) >> 15u32);
        vec4u(clamp(r, 0i32, 32768i32) as u32, clamp(g, 0i32, 32768i32) as u32, clamp(bl, 0i32, 32768i32) as u32, ONE)
    }
    pub fn p_video(c: Cmd, x: i32, y: i32, rgb: Vec4u) -> Vec4u {
        if !inside(c.win, x, y) {
            return splat(0u32);
        }
        let v = scale(rgb, c.pm.x());
        if inside(c.core, x, y) {
            return v;
        }
        scale(v, cov(sd(x * 256i32 + 128i32, y * 256i32 + 128i32, c.b, c.rad.x(), c.par.y())))
    }

    // ---- author shaders: the kernel's user node as a paint -------------------------------------------

    /// `id` selects the shader (`bough_kernel::fx::user_shade`), `now` is its clock, `pm` and `pm2` its two
    /// colours. The clock rides in the command's spare word.
    pub fn mk_user(b: Vec4i, r0: i32, id: u32, now: u32, pm: Vec4u, pm2: Vec4u, clip: Vec4i) -> Cmd {
        let win = window(b.x() - 256i32, b.y() - 256i32, b.z() + 256i32, b.w() + 256i32, clip);
        let r = radius_in(b, r0);
        Cmd {
            win: win,
            core: core_of(b, max(r, 128i32) + 136i32, win),
            b: b,
            par: vec4u(K_USER, shift_of(r), id, now),
            rad: vec4i(r, 0i32, 0i32, 0i32),
            pm: pm,
            pm2: pm2,
            xi: vec4i(0i32, 0i32, 0i32, 0i32),
            xk: vec4i(0i32, 0i32, 0i32, 0i32),
            xs: 0i32,
        }
    }
    pub fn p_user(c: Cmd, x: i32, y: i32) -> Vec4u {
        if !inside(c.win, x, y) {
            return splat(0u32);
        }
        if inside(c.core, x, y) {
            return user_color(c, x * 256i32 + 128i32, y * 256i32 + 128i32, c.par.w());
        }
        sh_user(c, x * 256i32 + 128i32, y * 256i32 + 128i32, c.par.w())
    }

    // ---- springs: a damped oscillator in closed form, integers only ---------------------------------

    /// e^(-u) for u in Q16, as Q16.
    pub fn x_exp(u: u32) -> u32 {
        let n = u / 45426u32;
        if n >= 17u32 {
            return 0u32;
        }
        let x = (u - n * 45426u32) >> 1u32;
        let mut p = ONE - mul15(x, ONE) / 5u32;
        p = ONE - mul15(x, p) / 4u32;
        p = ONE - mul15(x, p) / 3u32;
        p = ONE - mul15(x, p) / 2u32;
        p = ONE - mul15(x, p);
        (p << 1u32) >> n
    }
    /// sin of a phase in turns (Q24, wrapping), as Q16 in [-65536, 65536].
    pub fn x_sin(ph: u32) -> i32 {
        let p = ph & 0xFFFFFFu32;
        let q = p >> 22u32;
        let mut f = p & 0x3FFFFFu32;
        if (q & 1u32) == 1u32 {
            f = 0x400000u32 - f;
        }
        let z = f >> 7u32;
        let z2 = mul15(z, z);
        // sin(pi x / 2) = x (a - x^2 (b - x^2 (c - x^2 (d - x^2 e)))), coefficients in Q15 (they sum to ONE)
        let t = 21167u32 - mul15(z2, 2611u32 - mul15(z2, 153u32 - mul15(z2, 5u32)));
        let s = min((z * (51472u32 - mul15(z2, t))) >> 14u32, 65536u32) as i32;
        if q >= 2u32 {
            return -s;
        }
        s
    }
    pub fn x_cos(ph: u32) -> i32 {
        x_sin(ph + 0x400000u32)
    }
    /// A spring's value (Q8) at `now`: it left for `to` at `t0` and is exactly `to` from `t0 + dur` on.
    /// `decay`: the envelope's rate per tick (Q16 nepers). Three forms, by `omega`:
    /// * a turn rate (Q24 turns per tick, below 2^31): `to + env (a0 cos + b0 sin)`;
    /// * 0, critically damped: `to + env (a0 + b0 t / 1024)`;
    /// * bit 31 set, damped beyond critical: `to + a0 env + b0 env2`, the second decay in its low 16 bits.
    /// Before `t0` it rests where it started (`to + a0`, or `to + a0 + b0` in the last form).
    pub fn x_spring(now: u32, t0: u32, dur: u32, to: i32, a0: i32, b0: i32, decay: u32, omega: u32) -> i32 {
        let mut e = now - min(t0, now);
        if now < t0 {
            e = (0xFFFFFFFFu32 - t0) + now + 1u32;
        }
        if e >= 0x80000000u32 {
            if omega >= 0x80000000u32 {
                return to + a0 + b0;
            }
            return to + a0;
        }
        if e >= min(dur, 65535u32) {
            return to;
        }
        let a = clamp(a0, -2097151i32, 2097151i32);
        let b = clamp(b0, -2097151i32, 2097151i32);
        let env = x_exp(min(decay, 65535u32) * e) as i32;
        if omega == 0u32 {
            return to + mulq16(a + mulq16(b, (e * 64u32) as i32), env);
        }
        if omega >= 0x80000000u32 {
            // damped beyond critical: two decays (`decay` the slow one, `omega`'s low bits the fast one)
            return to + mulq16(a, env) + mulq16(b, x_exp(min(omega & 65535u32, 65535u32) * e) as i32);
        }
        let ph = omega * e;
        to + mulq16(mulq16(a, x_cos(ph)) + mulq16(b, x_sin(ph)), env)
    }

    /// A backdrop's finer level and the weight of the next; a video's slot and plane size (for the glue).
    pub fn c_lvl(c: Cmd) -> u32 {
        c.rad.y() as u32
    }
    pub fn c_frac(c: Cmd) -> u32 {
        c.rad.z() as u32
    }
    pub fn c_slot(c: Cmd) -> u32 {
        c.par.z()
    }
    pub fn c_vw(c: Cmd) -> i32 {
        c.pm.y() as i32
    }
    pub fn c_vh(c: Cmd) -> i32 {
        c.pm.z() as i32
    }
    pub fn c_win(c: Cmd) -> Vec4i {
        c.win
    }
    pub fn c_core(c: Cmd) -> Vec4i {
        c.core
    }

    // ---- one guarded shade per primitive: the kernel's definition restricted to a kind the type fixed ----

    pub fn p_solid(c: Cmd, x: i32, y: i32) -> Vec4u {
        if !inside(c.win, x, y) {
            return splat(0u32);
        }
        c.pm
    }
    pub fn p_rrect(c: Cmd, x: i32, y: i32) -> Vec4u {
        if !inside(c.win, x, y) {
            return splat(0u32);
        }
        if inside(c.core, x, y) {
            return c.pm;
        }
        sh_rrect(c, x * 256i32 + 128i32, y * 256i32 + 128i32)
    }
    pub fn p_border(c: Cmd, x: i32, y: i32) -> Vec4u {
        if !inside(c.win, x, y) {
            return splat(0u32);
        }
        if inside(c.core, x, y) {
            return c.pm2;
        }
        sh_border(c, x * 256i32 + 128i32, y * 256i32 + 128i32)
    }
    pub fn p_shadow(c: Cmd, x: i32, y: i32) -> Vec4u {
        if !inside(c.win, x, y) {
            return splat(0u32);
        }
        if inside(c.core, x, y) {
            return c.pm;
        }
        sh_shadow(c, x * 256i32 + 128i32, y * 256i32 + 128i32)
    }
    /// `p_shadow` with the corner corrections given.
    pub fn p_shadowm(c: Cmd, x: i32, y: i32, v0: u32, v1: u32, v2: u32, v3: u32) -> Vec4u {
        if !inside(c.win, x, y) {
            return splat(0u32);
        }
        if inside(c.core, x, y) {
            return c.pm;
        }
        scale(c.pm, x_shadowm(c, x, y, v0, v1, v2, v3))
    }
    pub fn p_grad(c: Cmd, x: i32, y: i32) -> Vec4u {
        if !inside(c.win, x, y) {
            return splat(0u32);
        }
        if inside(c.core, x, y) {
            return grad_color(c, x * 256i32 + 128i32, y * 256i32 + 128i32);
        }
        sh_grad(c, x * 256i32 + 128i32, y * 256i32 + 128i32)
    }
    /// The atlas index of the texel a glyph reads at (x, y); NO_TEXEL outside it.
    pub fn x_texel(c: Cmd, x: i32, y: i32, aw: u32) -> u32 {
        if !inside(c.win, x, y) {
            return NO_TEXEL;
        }
        texel(c, x, y, aw)
    }
    /// A glyph from its coverage byte (0 where there is no texel).
    pub fn p_glyph(c: Cmd, x: i32, y: i32, cov8: u32) -> Vec4u {
        if !inside(c.win, x, y) {
            return splat(0u32);
        }
        scale(c.pm, q15(cov8))
    }
}
