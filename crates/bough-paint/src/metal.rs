//! The GPU executor (Metal). The scene lives here: the arena of columns is wrapped without a copy, and
//! every per-record step of a frame runs on the GPU from it.
//!
//! One command buffer per frame, CPU side: write the block (and the pointer), encode a fixed list of
//! dispatches. GPU side:
//!
//! 1. `k_pick` (one thread): the record under the pointer, from the bins of the frame the surface shows; its
//!    id goes into the block's `Pick` word *on the GPU*, so the passes below already see it. The CPU reads
//!    one word back afterwards.
//! 2. `k_win{c}` (one thread per record, per column whose archetype reads a changed word): the archetype's
//!    window and cover into the window lane, and damage (atomic bits) for a record whose window moved or
//!    whose operands differ between the old and the new block. Frame-stage geometry is evaluated here.
//! 3. `k_bin` (one thread per damaged tile, only when geometry can have moved): the tile's list from the
//!    window lane, in paint order, restarted at the last opaque cover.
//! 4. `k_compact` (one thread): the damage bits as a tile list and an indirect dispatch count.
//! 5. `k_shade` (16 x 16 threads x 2 x 2 pixels per damaged tile, dispatched indirectly): each entry
//!    through its archetype's generated function, stored into the surface's own words.
//!
//! The same shade kernel also runs from bins and a tile list the CPU provides (`shade_with`), which is how
//! the generated functions are gated against the CPU kernel before anything else is trusted.
use crate::expr::{U, U_PICK};
use crate::emit;
use crate::scene::{Desc, IDX_BITS, IDX_MASK, LANES, Scene};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSString;
use objc2_metal::*;
use std::fmt::Write;

type Buf = Retained<ProtocolObject<dyn MTLBuffer>>;
type Pso = Retained<ProtocolObject<dyn MTLComputePipelineState>>;

/// META words: header, then the new block, the old block, and one length per column.
const M_UNEW: usize = 16;
const M_UOLD: usize = 80;
const M_LEN: usize = 144;
/// Tile-list buffer: three words of indirect dispatch arguments, one spare, then tile indices.
const TL_HDR: usize = 4;

/// A column's place in the arena, baked into the source (a column never moves).
struct Lay {
    base: usize,
    words: usize,
}

fn loads(d: &Desc) -> String {
    (0..d.words).map(|i| format!(", rw[{i}]")).collect()
}
fn unis(d: &Desc, pre: char) -> String {
    emit::uniform_names(d.reads, pre).iter().map(|n| format!(", {n}")).collect()
}
/// `rect` and `rw` of record `idx` of column `c`.
fn record(c: usize, l: &Lay) -> String {
    format!(
        "const device uint* pg = COLS + {}u + (idx >> 6u) * {}u; uint r = idx & 63u; metal::int4 rect = metal::int4(int(pg[r]), int(pg[64u + r]), int(pg[128u + r]), int(pg[192u + r])); const device uint* rw = pg + {}u + r * {}u; (void)rw; (void)rect; // column {c}\n",
        l.base,
        64 * (LANES + l.words),
        64 * LANES,
        l.words
    )
}

/// The layers' and the media pool's reads (`layer::sample`, `media::sample`: the same leaves, the same
/// order of operations), and the kernels of the staged frame: `k_seg` (a tile program over a range of its
/// list, started from a stored prefix or from nothing, stored into the surface or into a layer's level 0)
/// and `k_down` (one pyramid block from the level beneath).
const FX_GLUE: &str = r#"
// ---- layers and media (bough-paint layer.rs / media.rs) ----
static inline uint fx_at(const device uint* LT, uint lid, uint k, int x, int y) {
    const device uint* r = LT + lid * 32u + 4u + k * 4u;
    return bd_addr(x, y, r[2] & 0xFFFFu, r[2] >> 16u, r[3]);
}
static inline metal::int4 fx_rect(const device uint* LT, uint lid, uint k) {
    const device uint* r = LT + lid * 32u + 4u + k * 4u;
    return metal::int4(int(r[0] << 16u) >> 16, int(r[0]) >> 16, int(r[1] << 16u) >> 16, int(r[1]) >> 16);
}
static metal::uint4 bd_read(const device uint* POOL, const device uint* LT, uint lid, uint k, int x, int y) {
    int dw = bd_dim(FRAMEW, k), dh = bd_dim(FRAMEH, k);
    int i = bd_i(x, k), j = bd_i(y, k);
    int x0 = metal::clamp(i, 0, dw - 1), x1 = metal::clamp(i + 1, 0, dw - 1), y0 = metal::clamp(j, 0, dh - 1), y1 = metal::clamp(j + 1, 0, dh - 1);
    return bd_bil(POOL[fx_at(LT, lid, k, x0, y0)], POOL[fx_at(LT, lid, k, x1, y0)], POOL[fx_at(LT, lid, k, x0, y1)], POOL[fx_at(LT, lid, k, x1, y1)], bd_f(x, k), bd_f(y, k));
}
static metal::uint4 bd_px(const device uint* POOL, const device uint* LT, const device uint* MEDIA, uint lid, metal::uint4 a, Cmd c, int x, int y) {
    if (!x_in(c_win(c), x, y) || lid == 0xFFFFFFFFu) { return a; }
    metal::uint4 bl = bd_read(POOL, LT, lid, c_lvl(c), x, y);
    if (c_frac(c) != 0u) { bl = x_mixv(bl, bd_read(POOL, LT, lid, c_lvl(c) + 1u, x, y), c_frac(c)); }
    return p_bdrop(x_flat(a), c, x, y, bl);
}
static inline uint fx_byte(const device uint* M, uint i) { return (M[metal::min(i >> 2u, MEDIALIM)] >> ((i & 3u) * 8u)) & 255u; }
static metal::uint4 vd_px(const device uint* POOL, const device uint* LT, const device uint* M, Cmd c, int px, int py) {
    uint at = metal::min(c_slot(c), MEDIALIM - 7u);
    uint yoff = M[at + 2u], coff = M[at + 3u], ys = M[at + 4u], cs = M[at + 5u];
    int vw = c_vw(c), vh = c_vh(c);
    metal::int2 t = vd_pos(c, px, py);
    uint x0 = vd_ix(t.x, 0, vw), x1 = vd_ix(t.x, 1, vw), y0 = vd_ix(t.y, 0, vh), y1 = vd_ix(t.y, 1, vh);
    uint y = vd_bil(fx_byte(M, yoff + y0 * ys + x0), fx_byte(M, yoff + y0 * ys + x1), fx_byte(M, yoff + y1 * ys + x0), fx_byte(M, yoff + y1 * ys + x1), t.x, t.y);
    int cw = (vw + 1) >> 1, ch = (vh + 1) >> 1, ux = (t.x - 128) >> 1, uy = (t.y - 128) >> 1;
    uint u0 = vd_ix(ux, 0, cw), u1 = vd_ix(ux, 1, cw), v0 = vd_ix(uy, 0, ch), v1 = vd_ix(uy, 1, ch);
    uint cb = vd_bil(fx_byte(M, coff + v0 * cs + 2u * u0), fx_byte(M, coff + v0 * cs + 2u * u1), fx_byte(M, coff + v1 * cs + 2u * u0), fx_byte(M, coff + v1 * cs + 2u * u1), ux, uy);
    uint cr = vd_bil(fx_byte(M, coff + v0 * cs + 2u * u0 + 1u), fx_byte(M, coff + v0 * cs + 2u * u1 + 1u), fx_byte(M, coff + v1 * cs + 2u * u0 + 1u), fx_byte(M, coff + v1 * cs + 2u * u1 + 1u), ux, uy);
    return vd_rgb(y, cb, cr);
}
"#;
const FX_DOWN: &str = r#"
// one block (32 >> k pixels a side) of level k of a layer from level k - 1: a thread per pixel
kernel void k_down(device uint* POOL [[buffer(13)]], const device uint* LT [[buffer(14)]], const device uint* JOBS [[buffer(17)]],
                   metal::uint3 wg [[threadgroup_position_in_grid]], metal::uint3 tp [[thread_position_in_threadgroup]]) {
    const device uint* J = JOBS + wg.x * 4u;
    uint lid = J[0], k = J[1], bs = 5u - k;
    if (tp.x >= (1u << bs) || tp.y >= (1u << bs)) { return; }
    int x = (int(J[2]) << bs) + int(tp.x), y = (int(J[3]) << bs) + int(tp.y);
    metal::int4 r = fx_rect(LT, lid, k);
    if (x < r.x || x >= r.z || y < r.y || y >= r.w) { return; }
    int sw = bd_dim(FRAMEW, k - 1u), sh = bd_dim(FRAMEH, k - 1u);
    metal::uint4 rows[4];
    for (int j = 0; j < 4; j++) {
        int sy = metal::clamp(2 * y - 1 + j, 0, sh - 1);
        rows[j] = dn_row(POOL[fx_at(LT, lid, k - 1u, metal::clamp(2 * x - 1, 0, sw - 1), sy)], POOL[fx_at(LT, lid, k - 1u, metal::clamp(2 * x, 0, sw - 1), sy)],
                         POOL[fx_at(LT, lid, k - 1u, metal::clamp(2 * x + 1, 0, sw - 1), sy)], POOL[fx_at(LT, lid, k - 1u, metal::clamp(2 * x + 2, 0, sw - 1), sy)]);
    }
    POOL[fx_at(LT, lid, k, x, y)] = dn_fin(rows[0], rows[1], rows[2], rows[3]);
}
"#;

/// The whole shader: naga's MSL of the leaves, the archetypes' functions, the five kernels.
/// `tab`: the corner tables' first word in the arena.
pub fn source(descs: &[&Desc], lays: &[(usize, usize)], tab: usize) -> String {
    source_fx(descs, lays, tab, (1, 1), 8)
}
/// As `source`, for a frame of `frame` pixels and a media pool of `media_words` words (the layer and media
/// glue bake them in, as the column bases are).
pub fn source_fx(descs: &[&Desc], lays: &[(usize, usize)], tab: usize, frame: (u32, u32), media_words: usize) -> String {
    let lays: Vec<Lay> = lays.iter().map(|&(base, words)| Lay { base, words }).collect();
    let mut s = String::from(emit::LEAVES_MSL);
    s.push_str("\n// ---- generated per application (bough-paint emit.rs / metal.rs) ----\n");
    s.push_str(
        r#"// the corner tables: two header words (the key a table is filled for), then its entries
static inline uint tab_h(const device uint* T, uint base, uint i) { return base >= TABLIM ? 0xFFFFFFFFu : T[base + i]; }
static inline uint tab_v(const device uint* T, uint base, uint idx) {
    if (idx == 0xFFFFFFFEu) { return 0u; }
    if (idx == 0xFFFFFFFFu) { return 0xFFFFFFFFu; }
    return (T[base + 2u + (idx >> 1u)] >> ((idx & 1u) * 16u)) & 0xFFFFu;
}
"#,
    );
    s.push_str(FX_GLUE);
    for (k, d) in descs.iter().enumerate() {
        emit::archetype(emit::Lang::Msl, k, d, &mut s);
    }
    let all: u64 = descs.iter().fold(0, |m, d| m | d.reads);
    let load_u = |pre: char, at: usize| -> String { (0..64).filter(|i| all >> i & 1 != 0).map(|i| format!("    uint {pre}{i} = META[{}u]; (void){pre}{i};\n", at + i)).collect() };
    s.push_str(
        r#"
static inline uint atlas_at(const device uint* A, uint t, uint lim) {
    if (t == 0xFFFFFFFFu) { return 0u; }
    uint i = metal::min(t, lim);
    return (A[i >> 2u] >> ((i & 3u) * 8u)) & 255u;
}
static inline metal::int4 unpack_win(uint a, uint b) {
    return metal::int4(int(a << 16u) >> 16, int(a) >> 16, int(b << 16u) >> 16, int(b) >> 16);
}
static inline uint pack_lo(metal::int4 w) { return (uint(w.x) & 0xFFFFu) | (uint(w.y) << 16u); }
static inline uint pack_hi(metal::int4 w) { return (uint(w.z) & 0xFFFFu) | (uint(w.w) << 16u); }
// the tiles a pixel window touches, as atomic bits
static inline void mark(device metal::atomic_uint* DMG, metal::int4 w, uint TX, uint TY) {
    uint x0 = uint(metal::max(w.x, 0)) >> 5u, y0 = uint(metal::max(w.y, 0)) >> 5u;
    uint x1 = metal::min((uint(metal::max(w.z, 0)) + 31u) >> 5u, TX), y1 = metal::min((uint(metal::max(w.w, 0)) + 31u) >> 5u, TY);
    if (x0 >= x1) { return; }
    for (uint y = y0; y < y1; y++) {
        uint a = y * TX + x0, b = y * TX + x1;
        for (uint wd = a >> 5u; wd <= (b - 1u) >> 5u; wd++) {
            uint lo = metal::max(a, wd * 32u) - wd * 32u, hi = metal::min(b, wd * 32u + 32u) - wd * 32u;
            uint m = (hi == 32u ? 0xFFFFFFFFu : ((1u << hi) - 1u)) & ~((1u << lo) - 1u);
            metal::atomic_fetch_or_explicit(&DMG[wd], m, metal::memory_order_relaxed);
        }
    }
}
// the same for the record with key z, left out of the tiles whose list is cut at a cover above it (their
// header says so, and the cover is the list's first entry): nothing of the record shows there
static inline void markv(device metal::atomic_uint* DMG, metal::int4 w, uint TX, uint TY, uint z, const device uint* TILES, const device uint* LIST) {
    uint x0 = uint(metal::max(w.x, 0)) >> 5u, y0 = uint(metal::max(w.y, 0)) >> 5u;
    uint x1 = metal::min((uint(metal::max(w.z, 0)) + 31u) >> 5u, TX), y1 = metal::min((uint(metal::max(w.w, 0)) + 31u) >> 5u, TY);
    if (x0 >= x1) { return; }
    for (uint y = y0; y < y1; y++) {
        for (uint x = x0; x < x1; x++) {
            uint t = y * TX + x;
            uint fl = (TILES[2u * t + 1u] & 0x40000000u) != 0u ? LIST[TILES[2u * t]] : 0u;
            if (z >= fl) { metal::atomic_fetch_or_explicit(&DMG[t >> 5u], 1u << (t & 31u), metal::memory_order_relaxed); }
        }
    }
}
"#,
    );

    // ---- shade: `k_shade` (a whole tile program per damaged tile, into the surface) and `k_seg` (the
    // ---- staged frame's jobs: a range of a tile's list, from a stored prefix or from nothing, into the
    // ---- surface or into a layer's level 0) are one body ----
    for seg in [false, true] {
        s.push_str(if seg { "\nkernel void k_seg(" } else { "\nkernel void k_shade(" });
        s.push_str(
            r#"const device uint* COLS [[buffer(0)]], const device uint* META [[buffer(1)]], const device uint* ORDER [[buffer(2)]],
                    const device uint* WINS [[buffer(3)]], const device uint* TILES [[buffer(4)]], const device uint* LIST [[buffer(5)]],
                    const device uint* TL [[buffer(6)]], device uint* OUT [[buffer(7)]], const device uint* ATLAS [[buffer(8)]],
                    device uint* POOL [[buffer(13)]], const device uint* LT [[buffer(14)]], const device uint* MEDIA [[buffer(15)]],
                    const device uint* LAYOF [[buffer(16)]], const device uint* JOBS [[buffer(17)]],
                    metal::uint3 wg [[threadgroup_position_in_grid]], metal::uint3 tp [[thread_position_in_threadgroup]]) {
    uint fw = META[0], fh = META[1], stride = META[2], TX = META[3], aw = META[6], alim = META[7];
    const device uint* TAB = COLS + TABBASE; (void)TAB; (void)POOL; (void)LT; (void)MEDIA; (void)LAYOF; (void)JOBS; (void)TL;
"#,
        );
        s.push_str(&load_u('u', M_UNEW));
        s.push_str(if seg {
            r#"    const device uint* J = JOBS + wg.x * 8u;
    uint t = J[0], from = J[1], to = J[2], init = J[3], dest = J[4];
"#
        } else {
            r#"    uint t = TL[4u + wg.x], from = 0u, to = TILES[2u * t + 1u] & 0x3FFFFFFFu, init = 0xFFFFFFFFu, dest = 0xFFFFFFFFu;
"#
        });
        s.push_str(
            r#"    uint ux = (t % TX) * 32u + 2u * tp.x, uy = (t / TX) * 32u + 2u * tp.y;
    int x = int(ux), y = int(uy);
    uint off = TILES[2u * t];
    metal::uint4 a0 = metal::uint4(0u), a1 = metal::uint4(0u), a2 = metal::uint4(0u), a3 = metal::uint4(0u);
    if (init != 0xFFFFFFFFu) {
        // the stored prefix of a layer that holds this whole tile
        if (ux < fw && uy < fh) { a0 = x_unstore(POOL[fx_at(LT, init, 0u, x, y)]); }
        if (ux + 1u < fw && uy < fh) { a1 = x_unstore(POOL[fx_at(LT, init, 0u, x + 1, y)]); }
        if (ux < fw && uy + 1u < fh) { a2 = x_unstore(POOL[fx_at(LT, init, 0u, x, y + 1)]); }
        if (ux + 1u < fw && uy + 1u < fh) { a3 = x_unstore(POOL[fx_at(LT, init, 0u, x + 1, y + 1)]); }
    }
    // the occlusion cut at pixel grain: start at the topmost entry whose opaque cover holds this 2 x 2 block
    uint start = from;
    for (uint i = to; i > from; i--) {
        uint z = LIST[off + i - 1u];
        metal::int4 c = unpack_win(WINS[4u * z + 2u], WINS[4u * z + 3u]);
        if (c.x <= x && c.y <= y && c.z >= x + 2 && c.w >= y + 2) { start = i - 1u; break; }
    }
    for (uint i = start; i < to; i++) {
        uint z = LIST[off + i];
        metal::int4 wn = unpack_win(WINS[4u * z], WINS[4u * z + 1u]);
        if (x + 1 < wn.x || x >= wn.z || y + 1 < wn.y || y >= wn.w) { continue; }
        uint o = ORDER[z], idx = o & IDXMASK;
        switch (o >> IDXBITS) {
"#,
        );
        for (k, d) in descs.iter().enumerate() {
            write!(s, "        case {k}u: {{ {}", record(k, &lays[k])).unwrap();
            let (w, u) = (loads(d), unis(d, 'u'));
            let covs: Vec<String> = [(0, 0), (1, 0), (0, 1), (1, 1)].iter().map(|(dx, dy)| if emit::has_glyph(d.code) { format!("atlas_at(ATLAS, a{k}_texel(rect, x + {dx}, y + {dy}, aw{w}{u}), alim)") } else { "0u".into() }).collect();
            let fx = emit::has_fx(d.code);
            writeln!(s, "            a{k}_shade4({}{}a0, a1, a2, a3, rect, x, y, {}{}{w}{u});", if emit::has_tab(d.code) { "TAB, " } else { "" }, if fx { "POOL, LT, MEDIA, " } else { "" }, covs.join(", "), if fx { ", LAYOF[z]" } else { "" }).unwrap();
            s.push_str("        } break;\n");
        }
        s.push_str(
            r#"        default: break;
        }
    }
    if (dest == 0xFFFFFFFFu) {
        if (ux < fw && uy < fh) { OUT[uy * stride + ux] = x_store(a0); }
        if (ux + 1u < fw && uy < fh) { OUT[uy * stride + ux + 1u] = x_store(a1); }
        if (ux < fw && uy + 1u < fh) { OUT[(uy + 1u) * stride + ux] = x_store(a2); }
        if (ux + 1u < fw && uy + 1u < fh) { OUT[(uy + 1u) * stride + ux + 1u] = x_store(a3); }
    } else {
        // a prefix job: the pixels of the layer's level 0 (its window is cut to the frame)
        metal::int4 r = fx_rect(LT, dest, 0u);
        if (x_in(r, x, y)) { POOL[fx_at(LT, dest, 0u, x, y)] = x_store(a0); }
        if (x_in(r, x + 1, y)) { POOL[fx_at(LT, dest, 0u, x + 1, y)] = x_store(a1); }
        if (x_in(r, x, y + 1)) { POOL[fx_at(LT, dest, 0u, x, y + 1)] = x_store(a2); }
        if (x_in(r, x + 1, y + 1)) { POOL[fx_at(LT, dest, 0u, x + 1, y + 1)] = x_store(a3); }
    }
}
"#,
        );
    }
    s.push_str(FX_DOWN);

    // ---- pick: against the block the surface shows (the old one) ----
    s.push_str(
        r#"
kernel void k_pick(const device uint* COLS [[buffer(0)]], device uint* META [[buffer(1)]], const device uint* ORDER [[buffer(2)]],
                   const device uint* WINS [[buffer(3)]], const device uint* TILES [[buffer(4)]], const device uint* LIST [[buffer(5)]],
                   const device uint* ATLAS [[buffer(8)]], device uint* PICK [[buffer(10)]],
                   const device uint* POOL [[buffer(13)]], const device uint* LT [[buffer(14)]], const device uint* MEDIA [[buffer(15)]], const device uint* LAYOF [[buffer(16)]]) {
    uint fw = META[0], fh = META[1], TX = META[3], aw = META[6], alim = META[7];
    const device uint* TAB = COLS + TABBASE; (void)TAB; (void)POOL; (void)LT; (void)MEDIA; (void)LAYOF;
"#,
    );
    s.push_str(&load_u('u', M_UOLD));
    s.push_str(
        r#"    int x = int(META[10]), y = int(META[11]);
    uint id = 0xFFFFFFFFu;
    if (x >= 0 && y >= 0 && uint(x) < fw && uint(y) < fh) {
        uint t = (uint(y) >> 5u) * TX + (uint(x) >> 5u);
        uint off = TILES[2u * t], n = TILES[2u * t + 1u] & 0x3FFFFFFFu;
        bool found = false;
        for (uint i = n; i > 0u && !found; i--) {
            uint z = LIST[off + i - 1u];
            metal::int4 wn = unpack_win(WINS[4u * z], WINS[4u * z + 1u]);
            if (x < wn.x || x >= wn.z || y < wn.y || y >= wn.w) { continue; }
            uint o = ORDER[z], idx = o & IDXMASK;
            switch (o >> IDXBITS) {
"#,
    );
    for (k, d) in descs.iter().enumerate() {
        write!(s, "            case {k}u: {{ {}", record(k, &lays[k])).unwrap();
        let (w, u) = (loads(d), unis(d, 'u'));
        let cov = if emit::has_glyph(d.code) { format!("atlas_at(ATLAS, a{k}_texel(rect, x, y, aw{w}{u}), alim)") } else { "0u".into() };
        let fx = emit::has_fx(d.code);
        writeln!(s, "                if (a{k}_shade({}{}metal::uint4(0u), rect, x, y, {cov}{}{w}{u}).w != 0u) {{ found = true; id = {}; }}", if emit::has_tab(d.code) { "TAB, " } else { "" }, if fx { "POOL, LT, MEDIA, " } else { "" }, if fx { ", LAYOF[z]" } else { "" }, if d.hit { format!("a{k}_pick(rect{w}{u})") } else { "0xFFFFFFFFu".into() }).unwrap();
        s.push_str("            } break;\n");
    }
    write!(
        s,
        r#"            default: break;
            }}
        }}
    }}
    META[{}u] = id;
    PICK[0] = id;
}}
"#,
        M_UNEW + U_PICK
    )
    .unwrap();

    // ---- the window pass, one kernel per column ----
    for (k, d) in descs.iter().enumerate() {
        write!(
            s,
            r#"
kernel void k_win{k}(const device uint* COLS [[buffer(0)]], const device uint* META [[buffer(1)]], device uint* WINS [[buffer(3)]],
                    const device uint* TILES [[buffer(4)]], const device uint* LIST [[buffer(5)]],
                    device metal::atomic_uint* DMG [[buffer(9)]], const device uint* TOUCH [[buffer(11)]], metal::uint3 gid [[thread_position_in_grid]]) {{
    uint idx = gid.x;
    if (idx >= META[{}u]) {{ return; }}
    uint TX = META[3], TY = META[4];
"#,
            M_LEN + k
        )
        .unwrap();
        s.push_str(&load_u('u', M_UNEW));
        s.push_str(&load_u('o', M_UOLD));
        write!(s, "    {}", record(k, &lays[k])).unwrap();
        let (w, u, o) = (loads(d), unis(d, 'u'), unis(d, 'o'));
        write!(
            s,
            r#"    uint z = pg[256u + r];
    metal::int4 wn = a{k}_win(rect{w}{u});
    metal::int4 cv = {};
    metal::int4 ow = unpack_win(WINS[4u * z], WINS[4u * z + 1u]);
    metal::int4 oc = unpack_win(WINS[4u * z + 2u], WINS[4u * z + 3u]);
    bool moved = metal::any(wn != ow) || metal::any(cv != oc);
    if (moved) {{
        WINS[4u * z] = pack_lo(wn); WINS[4u * z + 1u] = pack_hi(wn); WINS[4u * z + 2u] = pack_lo(cv); WINS[4u * z + 3u] = pack_hi(cv);
        markv(DMG, ow, TX, TY, z, TILES, LIST);
        markv(DMG, wn, TX, TY, z, TILES, LIST);
    }} else {{
        metal::int4 cw = (((TOUCH[z >> 5u] >> (z & 31u)) & 1u) != 0u) ? wn : a{k}_chgw(rect{w}{o}{u});
        markv(DMG, cw, TX, TY, z, TILES, LIST);
    }}
}}
"#,
            if d.opaque_core { format!("a{k}_cover(rect{w}{u})") } else { "metal::int4(0)".into() }
        )
        .unwrap();
    }

    // ---- bin, compact ----
    s.push_str(
        r#"
kernel void k_bin(const device uint* META [[buffer(1)]], const device uint* WINS [[buffer(3)]], device uint* TILES [[buffer(4)]],
                  device uint* LIST [[buffer(5)]], const device uint* DMG [[buffer(9)]], device metal::atomic_uint* PICK [[buffer(10)]],
                  metal::uint3 gid [[thread_position_in_grid]]) {
    uint t = gid.x, TX = META[3], TY = META[4];
    if (t >= TX * TY) { return; }
    if (((DMG[t >> 5u] >> (t & 31u)) & 1u) == 0u) { return; }
    int ox = int(t % TX) * 32, oy = int(t / TX) * 32;
    uint nz = META[5], cap = META[8], base = t * cap, n = 0u;
    bool cut = META[9] != 0u, cutat = false;
    for (uint z = 0u; z < nz; z++) {
        metal::int4 w = unpack_win(WINS[4u * z], WINS[4u * z + 1u]);
        if (w.x >= ox + 32 || w.z <= ox || w.y >= oy + 32 || w.w <= oy) { continue; }
        metal::int4 c = unpack_win(WINS[4u * z + 2u], WINS[4u * z + 3u]);
        if (cut && c.x < c.z && c.x <= ox && c.y <= oy && c.z >= ox + 32 && c.w >= oy + 32) { n = 0u; cutat = true; }
        if (n < cap) { LIST[base + n] = z; }
        n++;
    }
    TILES[2u * t] = base;
    TILES[2u * t + 1u] = metal::min(n, cap) | (n > cap ? 0x80000000u : 0u) | (cutat ? 0x40000000u : 0u);
    // a tile with more entries than a slab holds: counted, and redrawn by the CPU kernel (it is exact)
    if (n > cap) { metal::atomic_fetch_add_explicit(&PICK[1], 1u, metal::memory_order_relaxed); }
}
kernel void k_compact(const device uint* META [[buffer(1)]], device uint* TL [[buffer(6)]], device uint* DMG [[buffer(9)]]) {
    uint nt = META[3] * META[4], n = 0u;
    for (uint wd = 0u; wd * 32u < nt; wd++) {
        uint m = DMG[wd];
        DMG[wd] = 0u;
        while (m != 0u) {
            uint b = metal::ctz(m);
            m &= m - 1u;
            TL[4u + n] = wd * 32u + b;
            n++;
        }
    }
    TL[0] = n; TL[1] = 1u; TL[2] = 1u; TL[3] = n;
}
"#,
    );
    s.push_str(
        r#"
// fill one corner table: a thread per entry, the leaf that defines it
kernel void k_tab(device uint* COLS [[buffer(0)]], const device uint* JOB [[buffer(12)]], metal::uint3 gid [[thread_position_in_grid]]) {
    uint base = JOB[0], n = JOB[3], wpr = (n + 1u) >> 1u;
    if (gid.x >= wpr || gid.y >= n) { return; }
    COLS[TABBASE + base + 2u + gid.y * wpr + gid.x] = x_cfillw(int(JOB[1]), int(JOB[2]), gid.x, gid.y);
}
"#,
    );
    s.replace("FRAMEW", &format!("{}u", frame.0)).replace("FRAMEH", &format!("{}u", frame.1)).replace("MEDIALIM", &format!("{}u", media_words.max(8) - 1)).replace("IDXMASK", &format!("{IDX_MASK}u")).replace("IDXBITS", &format!("{IDX_BITS}u")).replace("TABBASE", &format!("{tab}u")).replace("TABLIM", &format!("{}u", crate::scene::TAB_WORDS - 2))
}

/// Times of one GPU frame, seconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct GpuTime {
    /// Commit to completion as the calling thread sees it.
    pub wall: f64,
    /// `GPUEndTime - GPUStartTime`.
    pub gpu: f64,
}

pub struct Gpu {
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    shade: Pso,
    pick: Pso,
    bin: Pso,
    compact: Pso,
    tab: Pso,
    seg: Pso,
    down: Pso,
    win: Vec<Pso>,
    /// The arena this executor is bound to (calls that take a scene check it is that scene).
    arena: *const u32,
    /// 0 columns (the arena, no copy), 1 meta, 2 order, 3 windows, 4 tile headers, 5 lists, 6 tile list,
    /// 7 output, 8 atlas, 9 damage, 10 pick, 11 rewritten records (a bit per key), 12 table jobs; the staged
    /// frame's: 13 the layers' pool (this executor's own retained copy), 14 the layer table, 15 the media
    /// pool (the scene's, no copy), 16 layer of key, 17 jobs.
    bufs: [Option<Buf>; 18],
    /// The scene's media pool this executor wrapped.
    media: *const u32,
    /// The scene's bins and layer table as last mirrored (`staged`).
    seen_bins: Option<u64>,
    seen_lay: Option<u64>,
    /// The scene has layer-owning columns.
    layered: bool,
    /// What the last `staged` frame ran: prefix jobs, pyramid blocks, output tiles, dispatches.
    pub staged_stats: [u32; 4],
    /// Keys rewritten since the last frame, and the columns they are in.
    touched: Vec<u32>,
    touched_cols: u64,
    nt: usize,
    tx: usize,
    cap: usize,
    reads: Vec<u64>,
    geom_reads: Vec<u64>,
    lens: Vec<usize>,
    nz: usize,
    /// The surface shows nothing yet (or the scene changed): the next frame redraws everything.
    first: bool,
    /// The block the surface shows.
    prev: U,
    /// `BOUGH_GPU_SPLIT=1` (a measurement switch, never a result's configuration): every pass of a frame
    /// in its own command buffer, waited for, and its GPU seconds kept here: pick, window passes, bin,
    /// compact, shade.
    pub split: bool,
    pub pass_s: [f64; 5],
    /// Seconds Metal took to compile the source and build the pipelines.
    pub compile_s: f64,
    pub source_bytes: usize,
}

impl Gpu {
    /// Generate, compile, and wrap the scene's arena. `cap`: entries a tile's slab holds (`frame`'s bins).
    /// `None` without a Metal device.
    ///
    /// # Safety
    /// The executor binds the scene's arena without a copy and keeps reading and writing it (columns,
    /// corner tables) from the GPU: the scene must outlive the executor, and nothing may write the arena
    /// while a call on this executor is running (every call here waits for its command buffer, so between
    /// calls the arena is the scene's again). `Driver` owns both and is the safe form.
    pub unsafe fn new(scene: &Scene, out_words: usize, cap: usize) -> Option<Gpu> {
        let descs: Vec<&Desc> = scene.cols.iter().map(|c| c.desc).collect();
        let lays: Vec<(usize, usize)> = scene.cols.iter().map(|c| (c.base, c.desc.words)).collect();
        // SAFETY: the caller's contract is this function's.
        unsafe { Gpu::with_source(scene, out_words, cap, &source_fx(&descs, &lays, scene.tab_region().0, (scene.fw, scene.fh), scene.media.words().len())) }
    }
    /// As `new` with the source given (the gates pass mutated or naga-translated text).
    ///
    /// # Safety
    /// As `new`.
    pub unsafe fn with_source(scene: &Scene, out_words: usize, cap: usize, src: &str) -> Option<Gpu> {
        assert!(scene.cols.len() <= 64, "more than 64 columns");
        let device = MTLCreateSystemDefaultDevice()?;
        let queue = device.newCommandQueue()?;
        let t = std::time::Instant::now();
        let lib = device.newLibraryWithSource_options_error(&NSString::from_str(src), None).unwrap_or_else(|e| panic!("generated msl: {e}"));
        let pso = |name: &str| -> Pso {
            let f = lib.newFunctionWithName(&NSString::from_str(name)).unwrap_or_else(|| panic!("{name}"));
            device.newComputePipelineStateWithFunction_error(&f).unwrap_or_else(|e| panic!("{name}: {e}"))
        };
        let (shade, pick, bin, compact, tab) = (pso("k_shade"), pso("k_pick"), pso("k_bin"), pso("k_compact"), pso("k_tab"));
        let (seg, down) = (pso("k_seg"), pso("k_down"));
        let win: Vec<Pso> = (0..scene.cols.len()).map(|k| pso(&format!("k_win{k}"))).collect();
        let compile_s = t.elapsed().as_secs_f64();
        let nt = scene.tx * scene.ty;
        let mut g = Gpu {
            device,
            queue,
            shade,
            pick,
            bin,
            compact,
            tab,
            seg,
            down,
            media: scene.media.arena().ptr(),
            seen_bins: None,
            seen_lay: None,
            staged_stats: [0; 4],
            layered: scene.cols.iter().any(|c| c.desc.layer),
            win,
            arena: scene.arena.ptr(),
            bufs: Default::default(),
            nt,
            tx: scene.tx,
            cap,
            reads: scene.cols.iter().map(|c| c.desc.reads).collect(),
            geom_reads: scene.cols.iter().map(|c| c.desc.geom_reads).collect(),
            lens: Vec::new(),
            nz: 0,
            first: true,
            prev: U::default(),
            touched: Vec::new(),
            touched_cols: 0,
            split: std::env::var("BOUGH_GPU_SPLIT").is_ok(),
            pass_s: [0.0; 5],
            compile_s,
            source_bytes: src.len(),
        };
        // SAFETY: the arena is VM-page aligned, a whole number of pages, owned by the scene, which the
        // caller keeps alive and does not write while a frame is in flight (every frame here is waited for).
        let cols = unsafe { g.device.newBufferWithBytesNoCopy_length_options_deallocator(std::ptr::NonNull::new(scene.arena.ptr() as *mut std::ffi::c_void)?, scene.arena.bytes(), MTLResourceOptions::StorageModeShared, None) }?;
        g.bufs[0] = Some(cols);
        // SAFETY: the media pool's arena is page aligned and whole pages; the same contract as the columns
        // (the scene outlives the executor; a source writes only leased slots, which no record names).
        let media = unsafe { g.device.newBufferWithBytesNoCopy_length_options_deallocator(std::ptr::NonNull::new(scene.media.arena().ptr() as *mut std::ffi::c_void)?, scene.media.arena().bytes(), MTLResourceOptions::StorageModeShared, None) }?;
        g.bufs[15] = Some(media);
        for i in [12, 13, 14, 16, 17] {
            g.buf(i, 64);
        }
        g.buf(1, (M_LEN + scene.cols.len() + 16) * 4);
        g.buf(4, nt * 2 * 4);
        g.buf(6, (TL_HDR + nt) * 4);
        g.buf(7, out_words * 4);
        g.buf(9, (nt.div_ceil(32) + 1) * 4);
        g.buf(10, 64);
        let m = g.words_mut(1);
        m[..10].copy_from_slice(&[scene.fw, scene.fh, 0, scene.tx as u32, scene.ty as u32, 0, scene.aw, 0, cap as u32, scene.cut as u32]);
        g.sync(scene);
        Some(g)
    }
    fn buf(&mut self, i: usize, bytes: usize) -> &Buf {
        let need = bytes.max(16);
        if self.bufs[i].as_ref().is_none_or(|b| b.length() < need) {
            let cap = need.next_power_of_two();
            self.bufs[i] = Some(self.device.newBufferWithLength_options(cap, MTLResourceOptions::StorageModeShared).expect("buffer"));
        }
        self.bufs[i].as_ref().unwrap()
    }
    fn words_mut(&mut self, i: usize) -> &mut [u32] {
        let b = self.bufs[i].as_ref().expect("buffer");
        // SAFETY: shared storage owned by this executor; no frame is in flight (each is waited for) and
        // `&mut self` excludes other access from this side.
        unsafe { std::slice::from_raw_parts_mut(b.contents().as_ptr() as *mut u32, b.length() / 4) }
    }
    fn words(&self, i: usize) -> &[u32] {
        let b = self.bufs[i].as_ref().expect("buffer");
        // SAFETY: as `words_mut`, shared.
        unsafe { std::slice::from_raw_parts(b.contents().as_ptr() as *const u32, b.length() / 4) }
    }
    /// Take the scene's mounts: the paint order, the columns' lengths, the atlas. (The columns themselves are
    /// already here: they are the arena.) Called after records are pushed; the next frame redraws everything.
    pub fn sync(&mut self, scene: &Scene) {
        assert!(std::ptr::eq(self.arena, scene.arena.ptr()), "another scene than the one bound");
        self.nz = scene.order.len();
        self.lens = scene.cols.iter().map(|c| c.len).collect();
        self.buf(2, self.nz.max(1) * 4);
        let nz = self.nz;
        self.words_mut(2)[..nz].copy_from_slice(&scene.order);
        self.buf(3, self.nz.max(1) * 16);
        self.words_mut(3)[..nz * 4].fill(0);
        self.buf(5, (self.nt * self.cap).max(scene.bins.list.len()).max(1) * 4);
        self.buf(11, (self.nz / 32 + 1) * 4);
        self.words_mut(11).fill(0);
        self.touched.clear();
        self.touched_cols = 0;
        self.buf(8, scene.atlas.len() + 8);
        let b = self.bufs[8].as_ref().unwrap();
        // SAFETY: the buffer holds at least `atlas.len()` bytes; no frame is in flight.
        unsafe { std::ptr::copy_nonoverlapping(scene.atlas.as_ptr(), b.contents().as_ptr() as *mut u8, scene.atlas.len()) };
        let lens = self.lens.clone();
        let (nz, alim) = (self.nz as u32, (scene.atlas.len() as u32).saturating_sub(1));
        let m = self.words_mut(1);
        (m[5], m[7]) = (nz, alim);
        for (k, l) in lens.iter().enumerate() {
            m[M_LEN + k] = *l as u32;
        }
        self.first = true;
    }
    fn bind(&self, enc: &ProtocolObject<dyn MTLComputeCommandEncoder>, out: Option<&Buf>) {
        for i in 0..18 {
            let b = if i == 7 { out.or(self.bufs[7].as_ref()) } else { self.bufs[i].as_ref() };
            // SAFETY: each buffer is alive for the frame and bound at the index the source reads.
            unsafe { enc.setBuffer_offset_atIndex(b.map(|b| &**b), 0, i) };
        }
    }

    /// Fill the scene's pending corner tables on the GPU (one dispatch per table, waited for), then name
    /// their keys in the headers. Returns seconds of GPU time and the entries written. Without this call
    /// the CPU fills them at the scene's next `update`, and until either happens shadows take the closed
    /// form: tables change time, never pixels.
    pub fn fill_tabs(&mut self, scene: &mut Scene) -> (f64, usize) {
        assert!(std::ptr::eq(self.arena, scene.arena.ptr()), "another scene than the one bound");
        let jobs = scene.tab_jobs().to_vec();
        if jobs.is_empty() {
            return (0.0, 0);
        }
        self.buf(12, jobs.len() * 16);
        let jw = self.words_mut(12);
        for (k, j) in jobs.iter().enumerate() {
            jw[4 * k..4 * k + 4].copy_from_slice(&[j.base, j.r as u32, j.w as u32, j.n]);
        }
        let cmd = self.queue.commandBuffer().expect("command buffer");
        let enc = cmd.computeCommandEncoder().expect("encoder");
        self.bind(&enc, None);
        enc.setComputePipelineState(&self.tab);
        let mut entries = 0;
        for (k, j) in jobs.iter().enumerate() {
            // SAFETY: buffer 12 holds four words per job; the offset is inside it.
            unsafe { enc.setBuffer_offset_atIndex(self.bufs[12].as_deref(), 16 * k, 12) };
            enc.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: (j.n as usize).div_ceil(16), height: (j.n as usize).div_ceil(8), depth: 1 }, MTLSize { width: 8, height: 8, depth: 1 });
            entries += (j.n * j.n) as usize;
        }
        enc.endEncoding();
        cmd.commit();
        cmd.waitUntilCompleted();
        scene.tabs_filled();
        (cmd.GPUEndTime() - cmd.GPUStartTime(), entries)
    }

    /// Stage A: shade `tiles` from the bins and window lane the CPU scene holds (after `Scene::update`),
    /// under `u`. Only the generated shade functions are on trial.
    pub fn shade_with(&mut self, scene: &Scene, u: &U, tiles: &[u32], stride: usize) -> GpuTime {
        assert!(std::ptr::eq(self.arena, scene.arena.ptr()), "another scene than the one bound");
        assert!(tiles.iter().all(|&t| (t as usize) < self.nt) && tiles.len() <= self.nt);
        self.buf(5, scene.bins.list.len().max(1) * 4);
        self.words_mut(5)[..scene.bins.list.len()].copy_from_slice(&scene.bins.list);
        let th = self.words_mut(4);
        for t in 0..scene.tx * scene.ty {
            (th[2 * t], th[2 * t + 1]) = (scene.bins.off[t], scene.bins.off[t + 1] - scene.bins.off[t]);
        }
        let w = self.words_mut(3);
        for z in 0..scene.win.len() {
            let (a, c) = (scene.win[z], scene.cover[z]);
            let p = |v: [i16; 4]| ((v[0] as u16 as u32) | (v[1] as u16 as u32) << 16, (v[2] as u16 as u32) | (v[3] as u16 as u32) << 16);
            ((w[4 * z], w[4 * z + 1]), (w[4 * z + 2], w[4 * z + 3])) = (p(a), p(c));
        }
        let tl = self.words_mut(6);
        tl[TL_HDR..TL_HDR + tiles.len()].copy_from_slice(tiles);
        let m = self.words_mut(1);
        m[2] = stride as u32;
        m[M_UNEW..M_UNEW + 64].copy_from_slice(&u.w);
        let t = std::time::Instant::now();
        let cmd = self.queue.commandBuffer().expect("command buffer");
        let enc = cmd.computeCommandEncoder().expect("encoder");
        self.bind(&enc, None);
        enc.setComputePipelineState(&self.shade);
        if !tiles.is_empty() {
            enc.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: tiles.len(), height: 1, depth: 1 }, MTLSize { width: 16, height: 16, depth: 1 });
        }
        enc.endEncoding();
        cmd.commit();
        cmd.waitUntilCompleted();
        // the CPU-provided lanes are not the resident ones: a later `frame` starts over
        self.first = true;
        GpuTime { wall: t.elapsed().as_secs_f64(), gpu: cmd.GPUEndTime() - cmd.GPUStartTime() }
    }

    /// A buffer at least `bytes` long that keeps what it held (the layers' pool grows when layers do).
    fn grow_keep(&mut self, i: usize, bytes: usize) {
        let old = self.bufs[i].clone();
        if old.as_ref().is_some_and(|b| b.length() >= bytes) {
            return;
        }
        self.bufs[i] = None;
        self.buf(i, bytes);
        if let Some(o) = old {
            let n = self.bufs[i].as_ref().unwrap();
            // SAFETY: two distinct shared buffers, the new one at least as long; no frame is in flight.
            unsafe { std::ptr::copy_nonoverlapping(o.contents().as_ptr() as *const u8, n.contents().as_ptr() as *mut u8, o.length()) };
        }
    }
    /// The staged frame: what `Scene::update` scheduled, executed here. The CPU scene decided the damage,
    /// the bins and the layers' stages; this runs them as dispatches over the scene's own records: per
    /// layer in paint order its prefix jobs (`k_seg` into the layer) and its pyramid blocks level by level
    /// (`k_down`), then the output tiles (`k_seg` into the surface). The layers' pool here is this
    /// executor's own and is retained: call this once for every `update` of the scene (each frame's stages
    /// repair what the previous ones left), with `tiles` the output tiles to draw (the frame's damage, or
    /// more for an older surface). This is the path for scenes with layers; `frame` refuses them.
    pub fn staged(&mut self, scene: &Scene, u: &U, tiles: &[u32], stride: usize) -> GpuTime {
        assert!(std::ptr::eq(self.arena, scene.arena.ptr()), "another scene than the one bound");
        assert!(std::ptr::eq(self.media, scene.media.arena().ptr()), "the scene's media pool was replaced after the executor was made");
        assert!(tiles.iter().all(|&t| (t as usize) < self.nt));
        if self.seen_bins != Some(scene.bins_epoch) {
            self.buf(5, scene.bins.list.len().max(1) * 4);
            self.words_mut(5)[..scene.bins.list.len()].copy_from_slice(&scene.bins.list);
            let th = self.words_mut(4);
            for t in 0..scene.tx * scene.ty {
                (th[2 * t], th[2 * t + 1]) = (scene.bins.off[t], scene.bins.off[t + 1] - scene.bins.off[t]);
            }
            let w = self.words_mut(3);
            for z in 0..scene.win.len() {
                let (a, c) = (scene.win[z], scene.cover[z]);
                let p = |v: [i16; 4]| ((v[0] as u16 as u32) | (v[1] as u16 as u32) << 16, (v[2] as u16 as u32) | (v[3] as u16 as u32) << 16);
                ((w[4 * z], w[4 * z + 1]), (w[4 * z + 2], w[4 * z + 3])) = (p(a), p(c));
            }
            self.seen_bins = Some(scene.bins_epoch);
        }
        if self.seen_lay != Some(scene.layers.epoch) {
            let mut tab = Vec::new();
            scene.layers.table(&mut tab);
            self.buf(14, tab.len().max(16) * 4);
            self.words_mut(14)[..tab.len()].copy_from_slice(&tab);
            self.buf(16, self.nz.max(16) * 4);
            let nz = self.nz;
            let lo = self.words_mut(16);
            lo[..nz].fill(u32::MAX);
            lo[..scene.layers.of.len().min(nz)].copy_from_slice(&scene.layers.of[..scene.layers.of.len().min(nz)]);
            self.seen_lay = Some(scene.layers.epoch);
        }
        self.grow_keep(13, scene.layers.used.max(16) * 4);
        // the jobs: (pipeline is seg, first word, count) per dispatch
        let mut jobs: Vec<u32> = Vec::new();
        let mut stages: Vec<(bool, usize, usize)> = Vec::new();
        let mut st = [0u32; 4];
        let seg = |jobs: &mut Vec<u32>, ti: u32, upto: u32| {
            let (from, to, init, _) = scene.seg(ti, upto);
            jobs.extend([ti, from as u32, to as u32, init, upto, 0, 0, 0]);
        };
        for w in &scene.plan.work {
            if !w.prefix.is_empty() {
                stages.push((true, jobs.len(), w.prefix.len()));
                w.prefix.iter().for_each(|&ti| seg(&mut jobs, ti, w.lid));
                st[0] += w.prefix.len() as u32;
            }
            for (k, blocks) in &w.blocks {
                stages.push((false, jobs.len(), blocks.len()));
                blocks.iter().for_each(|&(bx, by)| jobs.extend([w.lid, *k, bx as u32, by as u32]));
                st[1] += blocks.len() as u32;
            }
        }
        if !tiles.is_empty() {
            stages.push((true, jobs.len(), tiles.len()));
            tiles.iter().for_each(|&ti| seg(&mut jobs, ti, crate::layer::NONE));
            st[2] = tiles.len() as u32;
        }
        st[3] = stages.len() as u32;
        self.staged_stats = st;
        self.buf(17, jobs.len().max(16) * 4);
        self.words_mut(17)[..jobs.len()].copy_from_slice(&jobs);
        let m = self.words_mut(1);
        m[2] = stride as u32;
        m[M_UNEW..M_UNEW + 64].copy_from_slice(&u.w);
        let t = std::time::Instant::now();
        let cmd = self.queue.commandBuffer().expect("command buffer");
        let enc = cmd.computeCommandEncoder().expect("encoder");
        self.bind(&enc, None);
        for &(is_seg, at, n) in &stages {
            enc.setComputePipelineState(if is_seg { &self.seg } else { &self.down });
            // SAFETY: buffer 17 holds every stage's jobs; `at` words in is inside it and word aligned.
            unsafe { enc.setBuffer_offset_atIndex(self.bufs[17].as_deref(), at * 4, 17) };
            enc.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: n, height: 1, depth: 1 }, MTLSize { width: 16, height: 16, depth: 1 });
        }
        enc.endEncoding();
        cmd.commit();
        cmd.waitUntilCompleted();
        // the resident lanes were not kept up: a later `frame` starts over
        self.first = true;
        GpuTime { wall: t.elapsed().as_secs_f64(), gpu: cmd.GPUEndTime() - cmd.GPUStartTime() }
    }

    /// A GPU-resident frame. The CPU's whole part is here: store the block and the pointer, encode the fixed
    /// list of dispatches, wait. Returns the times and the pick (the id under the pointer in the frame the
    /// surface showed; `u[U_PICK]` is ignored and replaced by it).
    pub fn frame(&mut self, u: &U, pointer: Option<(i32, i32)>, stride: usize) -> (GpuTime, u32) {
        self.run(u, pointer, stride, true)
    }
    /// Bring the resident lanes and bins to block `u` without drawing: for a surface that already shows
    /// `u` because the CPU kernel drew the frames in between (it writes the same words).
    pub fn catch_up(&mut self, u: &U, stride: usize) {
        let mut u = *u;
        let pick = u.w[U_PICK];
        // `run` keeps the previous pick word; the block being caught up to carries its own
        self.prev.w[U_PICK] = pick;
        u.w[U_PICK] = pick;
        self.run(&u, None, stride, false);
    }
    /// The record with key `z` was rewritten in the arena (`Scene::set`): the next frame's window pass
    /// damages where it was and where it is. The CPU's whole part of a record write on this side is this bit.
    pub fn touch(&mut self, z: u32) {
        assert!((z as usize) < self.nz);
        let col = self.words(2)[z as usize] >> IDX_BITS;
        self.words_mut(11)[z as usize >> 5] |= 1 << (z & 31);
        self.touched.push(z);
        self.touched_cols |= 1 << col;
    }
    fn run(&mut self, u: &U, pointer: Option<(i32, i32)>, stride: usize, shade: bool) -> (GpuTime, u32) {
        assert!(!self.layered, "a scene with layers runs the staged frame (`Gpu::staged`): the resident window pass and bins do not schedule layers");
        let first = self.first;
        let mut new = *u;
        new.w[U_PICK] = self.prev.w[U_PICK];
        let mut mask = if first { u64::MAX } else { self.prev.diff(&new, 64) };
        if pointer.is_some() {
            mask |= 1 << U_PICK;
        }
        let prev = self.prev;
        let (nt, lens) = (self.nt, self.lens.clone());
        let m = self.words_mut(1);
        m[2] = stride as u32;
        m[M_UNEW..M_UNEW + 64].copy_from_slice(&new.w);
        m[M_UOLD..M_UOLD + 64].copy_from_slice(&prev.w);
        if let Some((x, y)) = pointer {
            (m[10], m[11]) = (x as u32, y as u32);
        }
        if first {
            // everything is damaged; the window lane starts empty so every record's window "moves"
            let d = self.words_mut(9);
            for t in 0..nt {
                d[t / 32] |= 1 << (t % 32);
            }
        }
        self.words_mut(10)[1] = 0;
        let t = std::time::Instant::now();
        let split = self.split;
        let mut pass_s = [0.0f64; 5];
        let mut gpu_s = 0.0;
        let mut cmd = self.queue.commandBuffer().expect("command buffer");
        let mut enc = cmd.computeCommandEncoder().expect("encoder");
        self.bind(&enc, None);
        // with `split`, close the pass just encoded and open the next
        macro_rules! pass {
            ($k:expr, $last:expr) => {
                if split || $last {
                    enc.endEncoding();
                    cmd.commit();
                    cmd.waitUntilCompleted();
                    let s = cmd.GPUEndTime() - cmd.GPUStartTime();
                    pass_s[$k] = s;
                    gpu_s += s;
                    if !$last {
                        cmd = self.queue.commandBuffer().expect("command buffer");
                        enc = cmd.computeCommandEncoder().expect("encoder");
                        self.bind(&enc, None);
                    }
                }
            };
        }
        if pointer.is_some() && !first {
            enc.setComputePipelineState(&self.pick);
            enc.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: 1, height: 1, depth: 1 }, MTLSize { width: 1, height: 1, depth: 1 });
        }
        pass!(0, false);
        let mut geom = first;
        for (k, pso) in self.win.iter().enumerate() {
            let touched = self.touched_cols >> k & 1 != 0;
            if self.reads[k] & mask == 0 && !first && !touched || lens[k] == 0 {
                continue;
            }
            geom |= self.geom_reads[k] & mask != 0 || touched;
            enc.setComputePipelineState(pso);
            enc.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: lens[k].div_ceil(64), height: 1, depth: 1 }, MTLSize { width: 64, height: 1, depth: 1 });
        }
        pass!(1, false);
        if geom {
            enc.setComputePipelineState(&self.bin);
            enc.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: nt.div_ceil(64), height: 1, depth: 1 }, MTLSize { width: 64, height: 1, depth: 1 });
        }
        pass!(2, false);
        enc.setComputePipelineState(&self.compact);
        enc.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: 1, height: 1, depth: 1 }, MTLSize { width: 1, height: 1, depth: 1 });
        pass!(3, false);
        if shade {
            enc.setComputePipelineState(&self.shade);
            // SAFETY: buffer 6 starts with the three dispatch counts `k_compact` writes earlier in this encoder.
            unsafe { enc.dispatchThreadgroupsWithIndirectBuffer_indirectBufferOffset_threadsPerThreadgroup(self.bufs[6].as_ref().unwrap(), 0, MTLSize { width: 16, height: 16, depth: 1 }) };
        }
        pass!(4, true);
        self.pass_s = pass_s;
        let time = GpuTime { wall: t.elapsed().as_secs_f64(), gpu: gpu_s };
        let touched = std::mem::take(&mut self.touched);
        for &z in &touched {
            self.words_mut(11)[z as usize >> 5] = 0;
        }
        self.touched = touched;
        self.touched.clear();
        self.touched_cols = 0;
        // the one word read back
        let pick = self.words(1)[M_UNEW + U_PICK];
        new.w[U_PICK] = pick;
        self.prev = new;
        self.first = false;
        (time, pick)
    }
    /// Forget what the surface shows: the next frame redraws everything.
    pub fn invalidate(&mut self) {
        let nz = self.nz;
        self.words_mut(3)[..nz * 4].fill(0);
        self.first = true;
    }
    /// Tiles the last frame shaded, and tiles whose slab overflowed in its bin pass.
    pub fn last(&self) -> (u32, u32) {
        (self.words(6)[3], self.words(10)[1])
    }
    /// The tiles whose slab overflowed in the last bin pass (their lists are truncated: the CPU redraws them).
    pub fn overflowed(&self, out: &mut Vec<u32>) {
        out.clear();
        let th = self.words(4);
        out.extend((0..self.nt as u32).filter(|&t| th[2 * t as usize + 1] >> 31 != 0));
    }
    pub fn tile_list(&self) -> &[u32] {
        let n = self.words(6)[3] as usize;
        &self.words(6)[TL_HDR..TL_HDR + n]
    }
    /// The output buffer's first `words` words.
    pub fn out(&self, words: usize) -> &[u32] {
        &self.words(7)[..words]
    }
    pub fn out_mut(&mut self, words: usize) -> &mut [u32] {
        &mut self.words_mut(7)[..words]
    }
    pub fn tx(&self) -> usize {
        self.tx
    }
}
