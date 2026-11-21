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
    if (lid == 0xFFFFFFFFu) { return a; }
    if (x_in(fx_rect(LT, lid, 0u), x, y)) { a = x_flat(a); }
    if (!x_in(c_win(c), x, y)) { return a; }
    metal::uint4 bl = bd_read(POOL, LT, lid, c_lvl(c), x, y);
    if (c_frac(c) != 0u) { bl = x_mixv(bl, bd_read(POOL, LT, lid, c_lvl(c) + 1u, x, y), c_frac(c)); }
    return p_bdrop(a, c, x, y, bl);
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

/// The resident plan (`Gpu::resident`): every (layer, level, block) has a slot, in stage order (rank, then
/// level); a frame is the same dispatches whatever changed. Level 0 of a rank: a compaction kernel (a thread
/// per slot) decides which prefix tiles are due and appends their jobs, and the tile program runs them
/// through an indirect dispatch. Each level above: one dispatch with a thread per pixel of every slot, which
/// returns at once unless a block beneath that its taps touch changed. Every write compares first and stamps
/// its block with the frame's generation; a change at a level the record reads marks the tiles it reaches
/// (per-tile keys for the layers above, output damage where the record shows).
/// Buffers: 19 slot -> layer | level << 24; 20 layer * 8 + level -> first slot; 21 changed stamps; 22 forced
/// stamps (the CPU's: storage that holds nothing yet); 23 per tile (least event key, greatest + 1, output
/// damage generation, floor); 24 per stage (job count, 1, 1, first slot, slots, level), seen at 24 as the
/// indirect arguments and at 27 as the stage; 17 the stage's jobs; 26 (generation, since, everything, output).
const FX_RES: &str = r#"
// ---- the resident plan ----
static inline uint rs_capw(const device uint* LT, uint lid, uint k) { return (((LT[lid * 32u + 6u + k * 4u] & 0xFFFFu) + (32u >> k) - 1u) >> (5u - k)) + 1u; }
static inline uint rs_caph(const device uint* LT, uint lid, uint k) { return (((LT[lid * 32u + 6u + k * 4u] >> 16u) + (32u >> k) - 1u) >> (5u - k)) + 1u; }
// slot i of (layer, level) is the block at (i % capw, i / capw) from the first block of the level's window
static inline bool rs_block(const device uint* LT, uint lid, uint k, uint i, thread metal::int2& b) {
    metal::int4 r = fx_rect(LT, lid, k);
    if (r.x >= r.z || r.y >= r.w || k > LT[lid * 32u + 3u]) { return false; }
    uint bs = 5u - k, cw = rs_capw(LT, lid, k);
    b = metal::int2((r.x >> bs) + int(i % cw), (r.y >> bs) + int(i / cw));
    return (b.x << bs) < r.z && (b.y << bs) < r.w;
}
static inline uint rs_slot(const device uint* LT, const device uint* LB, uint lid, uint k, int bx, int by) {
    metal::int4 r = fx_rect(LT, lid, k);
    if (r.x >= r.z || r.y >= r.w) { return 0xFFFFFFFFu; }
    uint bs = 5u - k;
    int ix = bx - (r.x >> bs), iy = by - (r.y >> bs), cw = int(rs_capw(LT, lid, k));
    if (ix < 0 || iy < 0 || ix >= cw || iy >= int(rs_caph(LT, lid, k))) { return 0xFFFFFFFFu; }
    return LB[lid * 8u + k] + uint(iy * cw + ix);
}
// pixels `p` of level k of a layer changed: if the record reads that level, the tiles its read of them
// reaches are events of the record's key (for the layers above) and output damage where the record shows
static void rs_mark(device metal::atomic_uint* KT, const device uint* LT, uint lid, uint k, metal::int4 p, uint TX, uint TY, uint gen) {
    const device uint* row = LT + lid * 32u;
    if (k != row[1] && k != row[3]) { return; }
    int h = int(1u << k);
    metal::int4 w = metal::int4(int(row[28] << 16u) >> 16, int(row[28]) >> 16, int(row[29] << 16u) >> 16, int(row[29]) >> 16);
    int x0 = metal::max(((p.x - 1) * (2 * h) + h) >> 1, w.x), y0 = metal::max(((p.y - 1) * (2 * h) + h) >> 1, w.y);
    int x1 = metal::min((p.z * (2 * h) + h) >> 1, w.z), y1 = metal::min((p.w * (2 * h) + h) >> 1, w.w);
    if (x0 >= x1 || y0 >= y1) { return; }
    uint z = row[0];
    uint tx0 = uint(metal::max(x0, 0)) >> 5u, ty0 = uint(metal::max(y0, 0)) >> 5u, tx1 = metal::min((uint(metal::max(x1, 0)) + 31u) >> 5u, TX), ty1 = metal::min((uint(metal::max(y1, 0)) + 31u) >> 5u, TY);
    for (uint ty = ty0; ty < ty1; ty++) {
        for (uint tx = tx0; tx < tx1; tx++) {
            uint t = ty * TX + tx;
            metal::atomic_fetch_min_explicit(&KT[4u * t], z, metal::memory_order_relaxed); // reach lowers the tile key
            metal::atomic_fetch_max_explicit(&KT[4u * t + 1u], z + 1u, metal::memory_order_relaxed);
            if (z >= metal::atomic_load_explicit(&KT[4u * t + 3u], metal::memory_order_relaxed)) { metal::atomic_store_explicit(&KT[4u * t + 2u], gen, metal::memory_order_relaxed); }
        }
    }
}
// `Scene::seg`: the entries of tile t beneath key zb, started at the topmost layer record whose level 0
// holds the whole tile. (from, to, init)
static metal::uint3 rs_seg(const device uint* TILES, const device uint* LIST, const device uint* LT, const device uint* LAYOF, uint t, uint zb, uint TX, uint fw, uint fh) {
    uint off = TILES[2u * t], to = TILES[2u * t + 1u] & 0x3FFFFFFFu;
    int ox = int(t % TX) * 32, oy = int(t / TX) * 32, x1 = metal::min(ox + 32, int(fw)), y1 = metal::min(oy + 32, int(fh));
    while (to > 0u && LIST[off + to - 1u] >= zb) { to--; }
    for (uint j = to; j > 0u; j--) {
        uint lid = LAYOF[LIST[off + j - 1u]];
        if (lid == 0xFFFFFFFFu) { continue; }
        metal::int4 h = fx_rect(LT, lid, 0u);
        if (h.x <= ox && h.y <= oy && h.z >= x1 && h.w >= y1) { return metal::uint3(j - 1u, to, lid); }
    }
    return metal::uint3(0u, to, 0xFFFFFFFFu);
}
"#;
const FX_RES_KERNELS: &str = r#"
// level 0: a prefix tile is due when an event beneath its record touched the tile (and no opaque cover of
// the whole tile lies between every event and the record), or by rule
kernel void k_rc0(const device uint* META [[buffer(1)]], const device uint* WINS [[buffer(3)]], const device uint* TILES [[buffer(4)]], const device uint* LIST [[buffer(5)]],
                  const device uint* LT [[buffer(14)]], const device uint* LAYOF [[buffer(16)]], device uint* JOBS [[buffer(17)]], const device uint* SL [[buffer(19)]],
                  const device uint* LB [[buffer(20)]], device metal::atomic_uint* CHG [[buffer(21)]], const device uint* FORCE [[buffer(22)]], device metal::atomic_uint* KT [[buffer(23)]],
                  device metal::atomic_uint* ARGS [[buffer(24)]], const device uint* RP [[buffer(26)]], const device uint* STG [[buffer(27)]], metal::uint3 gid [[thread_position_in_grid]]) {
    if (gid.x >= STG[1]) { return; }
    uint s = STG[0] + gid.x, gen = RP[0], lid = SL[s] & 0xFFFFFFu, TX = META[3];
    metal::int2 b;
    if (!rs_block(LT, lid, 0u, s - LB[lid * 8u], b)) { return; }
    uint t = uint(b.y) * TX + uint(b.x), zl = LT[lid * 32u];
    bool forced = RP[2] != 0u || FORCE[s] == gen;
    if (!forced && metal::atomic_load_explicit(&KT[4u * t], metal::memory_order_relaxed) >= zl) { return; } // no event beneath
    metal::uint3 sg = rs_seg(TILES, LIST, LT, LAYOF, t, zl, TX, META[0], META[1]);
    if (!forced) {
        uint k1 = metal::atomic_load_explicit(&KT[4u * t + 1u], metal::memory_order_relaxed), off = TILES[2u * t];
        int ox = int(t % TX) * 32, oy = int(t / TX) * 32;
        for (uint j = sg.y; j > 0u; j--) {
            uint z = LIST[off + j - 1u];
            if (z < k1) { break; }
            metal::int4 c = unpack_win(WINS[4u * z + 2u], WINS[4u * z + 3u]);
            if (c.x <= ox && c.y <= oy && c.z >= ox + 32 && c.w >= oy + 32) { return; } // every event is under this cover
        }
    } else {
        metal::atomic_store_explicit(&CHG[s], gen, metal::memory_order_relaxed);
    }
    uint j = metal::atomic_fetch_add_explicit(&ARGS[0], 1u, metal::memory_order_relaxed);
    device uint* J = JOBS + j * 8u;
    J[0] = t; J[1] = sg.x; J[2] = sg.y; J[3] = sg.z; J[4] = lid; J[5] = s; J[6] = uint(forced);
}
// level k, a thread per pixel of every slot's block: a pixel is due when a block beneath that its taps
// touch changed, or by rule; it compares before it writes, stamps its block changed, and (at a level the
// record reads) marks the tiles its change reaches
kernel void k_downr(const device uint* META [[buffer(1)]], device uint* POOL [[buffer(13)]], const device uint* LT [[buffer(14)]], const device uint* SL [[buffer(19)]],
                    const device uint* LB [[buffer(20)]], device metal::atomic_uint* CHG [[buffer(21)]], const device uint* FORCE [[buffer(22)]], device metal::atomic_uint* KT [[buffer(23)]],
                    const device uint* RP [[buffer(26)]], const device uint* STG [[buffer(27)]], metal::uint3 wg [[threadgroup_position_in_grid]], metal::uint3 tp [[thread_position_in_threadgroup]]) {
    uint s = STG[0] + wg.x, gen = RP[0], lid = SL[s] & 0xFFFFFFu, k = STG[2], bs = 5u - k;
    metal::int2 b;
    if (!rs_block(LT, lid, k, s - LB[lid * 8u + k], b)) { return; }
    int x = (b.x << bs) + int(tp.x), y = (b.y << bs) + int(tp.y);
    metal::int4 r = fx_rect(LT, lid, k);
    if (x < r.x || x >= r.z || y < r.y || y >= r.w) { return; }
    bool forced = RP[2] != 0u || FORCE[s] == gen, due = forced;
    int s1 = int(bs) + 1;
    for (int by = (2 * y - 1) >> s1; by <= (2 * y + 2) >> s1 && !due; by++) {
        for (int bx = (2 * x - 1) >> s1; bx <= (2 * x + 2) >> s1 && !due; bx++) { // the blocks beneath that the taps touch
            uint q = rs_slot(LT, LB, lid, k - 1u, bx, by);
            due = q != 0xFFFFFFFFu && metal::atomic_load_explicit(&CHG[q], metal::memory_order_relaxed) == gen;
        }
    }
    if (!due) { return; }
    int sw = bd_dim(FRAMEW, k - 1u), sh = bd_dim(FRAMEH, k - 1u);
    metal::uint4 rows[4];
    for (int j = 0; j < 4; j++) {
        int sy = metal::clamp(2 * y + j - 1, 0, sh - 1);
        rows[j] = dn_row(POOL[fx_at(LT, lid, k - 1u, metal::clamp(2 * x - 1, 0, sw - 1), sy)], POOL[fx_at(LT, lid, k - 1u, metal::clamp(2 * x, 0, sw - 1), sy)],
                         POOL[fx_at(LT, lid, k - 1u, metal::clamp(2 * x + 1, 0, sw - 1), sy)], POOL[fx_at(LT, lid, k - 1u, metal::clamp(2 * x + 2, 0, sw - 1), sy)]);
    }
    uint at = fx_at(LT, lid, k, x, y), w = dn_fin(rows[0], rows[1], rows[2], rows[3]);
    bool ch = POOL[at] != w;
    if (ch) { POOL[at] = w; }
    if (ch || forced) {
        metal::atomic_store_explicit(&CHG[s], gen, metal::memory_order_relaxed); // block stamps changed
        rs_mark(KT, LT, lid, k, metal::int4(x, y, x + 1, y + 1), META[3], META[4], gen);
    }
}
// the output: the tiles damaged after the frame the surface last showed
kernel void k_rcout(const device uint* META [[buffer(1)]], const device uint* TILES [[buffer(4)]], const device uint* LIST [[buffer(5)]], const device uint* LT [[buffer(14)]],
                    const device uint* LAYOF [[buffer(16)]], device uint* JOBS [[buffer(17)]], device metal::atomic_uint* KT [[buffer(23)]], device metal::atomic_uint* ARGS [[buffer(24)]],
                    const device uint* RP [[buffer(26)]], metal::uint3 gid [[thread_position_in_grid]]) {
    uint t = gid.x, TX = META[3];
    if (t >= TX * META[4]) { return; }
    if (RP[2] == 0u && metal::atomic_load_explicit(&KT[4u * t + 2u], metal::memory_order_relaxed) <= RP[1]) { return; }
    metal::uint3 sg = rs_seg(TILES, LIST, LT, LAYOF, t, 0xFFFFFFFFu, TX, META[0], META[1]);
    uint j = metal::atomic_fetch_add_explicit(&ARGS[0], 1u, metal::memory_order_relaxed);
    device uint* J = JOBS + j * 8u;
    J[0] = t; J[1] = sg.x; J[2] = sg.y; J[3] = sg.z; J[4] = 0xFFFFFFFFu; J[5] = 0u; J[6] = 0u;
}
// the frame's events are spent
kernel void k_rfin(const device uint* META [[buffer(1)]], device metal::atomic_uint* KT [[buffer(23)]], metal::uint3 gid [[thread_position_in_grid]]) {
    if (gid.x >= META[3] * META[4]) { return; }
    metal::atomic_store_explicit(&KT[4u * gid.x], 0xFFFFFFFFu, metal::memory_order_relaxed);
    metal::atomic_store_explicit(&KT[4u * gid.x + 1u], 0u, metal::memory_order_relaxed);
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
// the palette tables: the PALWORDS words that end where the corner tables begin (`Code::Pal`; 0 past the end)
static inline uint pal_at(const device uint* T, uint i) { return i < PALWORDS ? *(T - int(PALWORDS) + int(i)) : 0u; }
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

    s.push_str(FX_RES);
    // ---- shade: `k_shade` (a whole tile program per damaged tile, into the surface) and `k_seg` (the
    // ---- staged frame's jobs: a range of a tile's list, from a stored prefix or from nothing, into the
    // ---- surface or into a layer's level 0) are one body ----
    for variant in 0..3 {
        // 0: `k_shade`; 1: `k_seg`; 2: `k_segr` (the resident plan's: a prefix job compares before it writes
        // and stamps its block changed)
        let seg = variant > 0;
        s.push_str(["\nkernel void k_shade(", "\nkernel void k_seg(", "\nkernel void k_segr(device metal::atomic_uint* CHG [[buffer(21)]], device metal::atomic_uint* KT [[buffer(23)]], const device uint* RP [[buffer(26)]], "][variant]);
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
"#,
        );
        s.push_str(if variant == 2 {
            r#"        bool ch = false;
        if (x_in(r, x, y)) { uint at = fx_at(LT, dest, 0u, x, y), w = x_store(a0); if (POOL[at] != w) { POOL[at] = w; ch = true; } }
        if (x_in(r, x + 1, y)) { uint at = fx_at(LT, dest, 0u, x + 1, y), w = x_store(a1); if (POOL[at] != w) { POOL[at] = w; ch = true; } }
        if (x_in(r, x, y + 1)) { uint at = fx_at(LT, dest, 0u, x, y + 1), w = x_store(a2); if (POOL[at] != w) { POOL[at] = w; ch = true; } }
        if (x_in(r, x + 1, y + 1)) { uint at = fx_at(LT, dest, 0u, x + 1, y + 1), w = x_store(a3); if (POOL[at] != w) { POOL[at] = w; ch = true; } }
        if (ch || J[6] != 0u) {
            metal::atomic_store_explicit(&CHG[J[5]], RP[0], metal::memory_order_relaxed); // prefix stamps changed
            rs_mark(KT, LT, dest, 0u, metal::int4(x, y, x + 2, y + 2), TX, META[4], RP[0]);
        }
    }
}
"#
        } else {
            r#"        if (x_in(r, x, y)) { POOL[fx_at(LT, dest, 0u, x, y)] = x_store(a0); }
        if (x_in(r, x + 1, y)) { POOL[fx_at(LT, dest, 0u, x + 1, y)] = x_store(a1); }
        if (x_in(r, x, y + 1)) { POOL[fx_at(LT, dest, 0u, x, y + 1)] = x_store(a2); }
        if (x_in(r, x + 1, y + 1)) { POOL[fx_at(LT, dest, 0u, x + 1, y + 1)] = x_store(a3); }
    }
}
"#
        });
    }
    s.push_str(FX_DOWN);
    s.push_str(FX_RES_KERNELS);

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
    const device uint* TAB = COLS + TABBASE; (void)TAB;
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
    // an archetype with a palette read: its geometry functions take the table pointer too
    for (k, d) in descs.iter().enumerate() {
        if emit::has_pal(d.code) {
            for f in ["texel", "win", "cover", "pick", "chgw"] {
                s = s.replace(&format!("a{k}_{f}(rect"), &format!("a{k}_{f}(TAB, rect"));
            }
        }
    }
    s.replace("PALWORDS", &format!("{}u", crate::scene::PAL_WORDS)).replace("FRAMEW", &format!("{}u", frame.0)).replace("FRAMEH", &format!("{}u", frame.1)).replace("MEDIALIM", &format!("{}u", media_words.max(8) - 1)).replace("IDXMASK", &format!("{IDX_MASK}u")).replace("IDXBITS", &format!("{IDX_BITS}u")).replace("TABBASE", &format!("{tab}u")).replace("TABLIM", &format!("{}u", crate::scene::TAB_WORDS - 2))
}

/// Times of one GPU frame, seconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct GpuTime {
    /// Commit to completion as the calling thread sees it.
    pub wall: f64,
    /// `GPUEndTime - GPUStartTime`.
    pub gpu: f64,
}

/// The resident plan's CPU side (`Gpu::resident`).
#[derive(Default)]
struct Res {
    /// The lanes and tile lists in buffers 3 to 5 are this path's mirror of the scene's.
    mirrored: bool,
    /// Per tile: its slab in buffer 5 (first word, capacity), and the words used.
    slab: Vec<(u32, u32)>,
    used: u32,
    /// The layers' storage the slots were made for.
    alloc: Option<u64>,
    rank: Vec<u32>,
    /// Layer * 8 + level -> first slot.
    lb: Vec<u32>,
    /// (level, or `u32::MAX` for a rank's reach pass; slots; first slot), in order.
    stages: Vec<(u32, u32, u32)>,
    nslots: usize,
    era: u32,
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
    bufs: [Option<Buf>; 28],
    /// The resident plan's kernels and state (`resident`).
    rk: [Pso; 5],
    res: Res,
    /// The scene's layer pool this executor wrapped (null: none yet).
    pool: *const u32,
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
        let rk = [pso("k_rc0"), pso("k_segr"), pso("k_downr"), pso("k_rcout"), pso("k_rfin")];
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
            rk,
            res: Res::default(),
            media: scene.media.arena().ptr(),
            pool: std::ptr::null(),
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
        for i in [12, 13, 14, 16, 17, 19, 20, 21, 22, 23, 24, 25, 26, 27] {
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
        self.buf(5, (self.nt * self.cap).max(scene.bins.total()).max(1) * 4);
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
        for i in 0..28 {
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
        let (list, off) = scene.bins.csr();
        self.buf(5, list.len().max(1) * 4);
        self.words_mut(5)[..list.len()].copy_from_slice(&list);
        let th = self.words_mut(4);
        for t in 0..scene.tx * scene.ty {
            (th[2 * t], th[2 * t + 1]) = (off[t], off[t + 1] - off[t]);
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

    /// The staged frame: what `Scene::update` scheduled, executed here. The CPU scene decided the damage,
    /// the bins and the layers' stages; this runs them as dispatches over the scene's own records: per
    /// layer in paint order its prefix jobs (`k_seg` into the layer) and its pyramid blocks level by level
    /// (`k_down`), then the output tiles (`k_seg` into the surface). The layers' pool is the scene's own
    /// memory (wrapped, not copied), so a frame may run here or on the CPU kernel, whichever the chooser
    /// picks. (Historical wording follows.) The layers' pool here is this
    /// executor's own and is retained: call this once for every `update` of the scene (each frame's stages
    /// repair what the previous ones left), with `tiles` the output tiles to draw (the frame's damage, or
    /// more for an older surface). This is the path for scenes with layers; `frame` refuses them.
    pub fn staged(&mut self, scene: &Scene, u: &U, tiles: &[u32], stride: usize) -> GpuTime {
        assert!(std::ptr::eq(self.arena, scene.arena.ptr()), "another scene than the one bound");
        assert!(std::ptr::eq(self.media, scene.media.arena().ptr()), "the scene's media pool was replaced after the executor was made");
        assert!(tiles.iter().all(|&t| (t as usize) < self.nt));
        if self.seen_bins != Some(scene.bins_epoch) {
            self.res.mirrored = false;
            let (list, off) = scene.bins.csr();
            self.buf(5, list.len().max(1) * 4);
            self.words_mut(5)[..list.len()].copy_from_slice(&list);
            let th = self.words_mut(4);
            for t in 0..scene.tx * scene.ty {
                (th[2 * t], th[2 * t + 1]) = (off[t], off[t + 1] - off[t]);
            }
            let w = self.words_mut(3);
            for z in 0..scene.win.len() {
                // a layer record acts over its halo too (it flattens there): its window lane is the union
                let h = scene.halo[z];
                let a0 = scene.win[z];
                let a = if h[0] >= h[2] { a0 } else if a0[0] >= a0[2] { h } else { [a0[0].min(h[0]), a0[1].min(h[1]), a0[2].max(h[2]), a0[3].max(h[3])] };
                let c = scene.cover[z];
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
        // the layers' pool is the scene's own memory: wrap it once (it never moves)
        if let Some(a) = scene.layers.pool.arena() {
            if !std::ptr::eq(self.pool, a.ptr()) {
                // SAFETY: page-aligned whole pages owned by the scene, which outlives the executor; the CPU
                // writes it only between this executor's calls (each waits for the GPU).
                self.bufs[13] = unsafe { self.device.newBufferWithBytesNoCopy_length_options_deallocator(std::ptr::NonNull::new(a.ptr() as *mut std::ffi::c_void).expect("pool"), a.bytes(), MTLResourceOptions::StorageModeShared, None) };
                self.pool = a.ptr();
            }
        }
        // the jobs of a plan and an output tile list: (pipeline is seg, first word, count) per dispatch
        let build = |plan: &crate::layer::Plan, tiles: &[u32]| -> (Vec<u32>, Vec<(bool, usize, usize)>, [u32; 4]) {
        let mut jobs: Vec<u32> = Vec::new();
        let mut stages: Vec<(bool, usize, usize)> = Vec::new();
        let mut st = [0u32; 4];
        let seg = |jobs: &mut Vec<u32>, ti: u32, upto: u32| {
            let (from, to, init, _) = scene.seg(ti, upto);
            jobs.extend([ti, from as u32, to as u32, init, upto, 0, 0, 0]);
        };
        // Layers whose halos share no tile with a lower layer that also has work this frame are independent:
        // they take the same rank and run in one dispatch per stage.
        let work = &plan.work;
        let tiles_of = |lid: u32| scene.layers.list[lid as usize].lv[0].r;
        let mut rank = vec![0usize; work.len()];
        for i in 0..work.len() {
            let a = tiles_of(work[i].lid);
            for j in 0..i {
                let b = tiles_of(work[j].lid);
                if a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3] {
                    rank[i] = rank[i].max(rank[j] + 1);
                }
            }
        }
        for r in 0..=rank.iter().copied().max().unwrap_or(0) {
            let of_rank = || work.iter().zip(&rank).filter(move |(_, k)| **k == r).map(|(w, _)| w);
            let (at, mut n) = (jobs.len(), 0);
            for w in of_rank() {
                w.prefix.iter().for_each(|&ti| seg(&mut jobs, ti, w.lid));
                n += w.prefix.len();
            }
            if n > 0 {
                stages.push((true, at, n));
                st[0] += n as u32;
            }
            for k in 1..=crate::ops::ops::BD_LMAX {
                let (at, mut n) = (jobs.len(), 0);
                for w in of_rank() {
                    for (_, blocks) in w.blocks.iter().filter(|(lv, _)| *lv == k) {
                        blocks.iter().for_each(|&(bx, by)| jobs.extend([w.lid, k, bx as u32, by as u32]));
                        n += blocks.len();
                    }
                }
                if n > 0 {
                    stages.push((false, at, n));
                    st[1] += n as u32;
                }
            }
        }
        if !tiles.is_empty() {
            stages.push((true, jobs.len(), tiles.len()));
            tiles.iter().for_each(|&ti| seg(&mut jobs, ti, crate::layer::NONE));
            st[2] = tiles.len() as u32;
        }
        st[3] = stages.len() as u32;
        (jobs, stages, st)
        };
        // (a broad change is every layer whole: enumerated here; the resident plan says it in one flag)
        let plan_full;
        let plan = if scene.plan.full {
            plan_full = scene.layers.full_plan(scene.tx);
            &plan_full
        } else {
            &scene.plan
        };
        let (jobs, stages, st) = build(plan, tiles);
        self.staged_stats = st;
        self.buf(17, jobs.len().max(16) * 4);
        self.words_mut(17)[..jobs.len()].copy_from_slice(&jobs);
        let jobs_buf = 17;
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
            unsafe { enc.setBuffer_offset_atIndex(self.bufs[jobs_buf].as_deref(), at * 4, 17) };
            enc.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: n, height: 1, depth: 1 }, MTLSize { width: 16, height: 16, depth: 1 });
        }
        enc.endEncoding();
        cmd.commit();
        cmd.waitUntilCompleted();
        // the resident lanes were not kept up: a later `frame` starts over
        self.first = true;
        GpuTime { wall: t.elapsed().as_secs_f64(), gpu: cmd.GPUEndTime() - cmd.GPUStartTime() }
    }

    /// The resident frame for a scene with layers: the same dispatches whatever changed, with the schedule
    /// decided on the GPU (see `FX_RES`). The CPU's part is the damage: per tile the least and greatest key
    /// of this frame's events and a generation stamp for `tiles` (the scene's output damage), stamps for
    /// blocks whose storage holds nothing yet, and the tile lists, lanes and table rows that changed.
    /// Call once for every `update` of the scene (not `update_eager`; the scene may have `route_off` set),
    /// in place of `prepare` + `shade` or `staged`. `since`: draw the tiles damaged after that generation
    /// (`None`: repair the layers, draw nothing); pass the generation this call returned when the surface
    /// was last drawn. Returns the time and this frame's generation.
    pub fn resident(&mut self, scene: &mut Scene, u: &U, tiles: &[u32], since: Option<u32>, stride: usize) -> (GpuTime, u32) {
        assert!(std::ptr::eq(self.arena, scene.arena.ptr()), "another scene than the one bound");
        assert!(std::ptr::eq(self.media, scene.media.arena().ptr()), "the scene's media pool was replaced after the executor was made");
        assert!(scene.order.len() == self.nz, "records were pushed: call `sync`");
        let (nt, tx, ty) = (self.nt, scene.tx, scene.ty);
        let whole = !self.res.mirrored;
        // ---- the lanes: every key once, then the keys that changed ----
        let lanes = scene.take_lane_dirty();
        let put = |w: &mut [u32], scene: &Scene, z: usize| {
            let (h, a0, c) = (scene.halo[z], scene.win[z], scene.cover[z]);
            let a = if h[0] >= h[2] { a0 } else if a0[0] >= a0[2] { h } else { [a0[0].min(h[0]), a0[1].min(h[1]), a0[2].max(h[2]), a0[3].max(h[3])] };
            let p = |v: [i16; 4]| ((v[0] as u16 as u32) | (v[1] as u16 as u32) << 16, (v[2] as u16 as u32) | (v[3] as u16 as u32) << 16);
            ((w[4 * z], w[4 * z + 1]), (w[4 * z + 2], w[4 * z + 3])) = (p(a), p(c));
        };
        match lanes {
            Some(keys) if !whole => {
                let w = self.words_mut(3);
                keys.iter().for_each(|&z| put(w, scene, z as usize));
            }
            _ => {
                let w = self.words_mut(3);
                (0..scene.win.len()).for_each(|z| put(w, scene, z));
            }
        }
        // ---- the tile lists: a slab per tile, rewritten where the bins changed ----
        let changed = scene.bins.take_changed();
        self.buf(23, nt * 16);
        if whole || self.res.slab.len() != nt {
            if self.res.slab.len() != nt {
                self.words_mut(23).chunks_exact_mut(4).for_each(|k| k.copy_from_slice(&[u32::MAX, 0, 0, 0]));
            }
            self.res_lists(scene, None);
        } else if !changed.is_empty() && !cfg!(feature = "mutant_slab_stale") {
            self.res_lists(scene, Some(&changed));
        }
        self.seen_bins = None;
        // ---- the layers: the table rows that changed; slots when storage or the stage order changed ----
        if let Some(a) = scene.layers.pool.arena() {
            if !std::ptr::eq(self.pool, a.ptr()) {
                // SAFETY: page-aligned whole pages owned by the scene, which outlives the executor; the CPU
                // writes it only between this executor's calls (each waits for the GPU).
                self.bufs[13] = unsafe { self.device.newBufferWithBytesNoCopy_length_options_deallocator(std::ptr::NonNull::new(a.ptr() as *mut std::ffi::c_void).expect("pool"), a.bytes(), MTLResourceOptions::StorageModeShared, None) };
                self.pool = a.ptr();
            }
        }
        let moved = scene.layers.take_moved();
        let mut reslot = self.res.alloc != Some(scene.layers.alloc_epoch) || whole;
        let mut row = Vec::new();
        match &moved {
            Some(m) if !reslot => {
                for &lid in m {
                    scene.layers.row(lid, &mut row);
                    self.words_mut(14)[lid as usize * crate::layer::ROW..][..row.len()].copy_from_slice(&row);
                    // the stage order still holds if the layer's rank is between those of its neighbours
                    let (r, me) = (scene.layers.list[lid as usize].lv[0].r, self.res.rank[lid as usize]);
                    for y in (r[1] >> 5).max(0)..((r[3] + 31) >> 5).min(ty as i32) {
                        for x in (r[0] >> 5).max(0)..((r[2] + 31) >> 5).min(tx as i32) {
                            for &lz in scene.bins.layers(y as usize * tx + x as usize) {
                                let o = scene.layers.of[lz as usize];
                                reslot |= (o < lid && self.res.rank[o as usize] >= me) || (o > lid && self.res.rank[o as usize] <= me);
                            }
                        }
                    }
                }
            }
            _ => reslot = true,
        }
        if reslot {
            self.res_slots(scene);
        }
        if reslot {
            self.buf(16, self.nz.max(16) * 4);
            let nz = self.nz;
            let lo = self.words_mut(16);
            lo[..nz].fill(u32::MAX);
            lo[..scene.layers.of.len().min(nz)].copy_from_slice(&scene.layers.of[..scene.layers.of.len().min(nz)]);
        }
        self.seen_lay = None;
        // ---- this frame's generation; what is due by rule; the events; the output damage ----
        self.res.era += 1;
        let era = self.res.era;
        let all = scene.plan.full;
        let dues = scene.take_dues();
        if !all && !cfg!(feature = "mutant_forced") {
            let lb = std::mem::take(&mut self.res.lb);
            let force = self.words_mut(22);
            for (lid, due) in &dues {
                let l = &scene.layers.list[*lid as usize];
                for k in 0..=l.top as usize {
                    let (v, bs) = (&l.lv[k], crate::layer::bshift(k as u32));
                    if v.empty() {
                        continue;
                    }
                    let capw = ((v.cw + (1 << bs) - 1) >> bs) + 1;
                    let (bx0, by0, base) = (v.r[0] >> bs, v.r[1] >> bs, lb[*lid as usize * 8 + k]);
                    let whole_level = [v.r];
                    let rects: &[[i32; 4]] = if due.fresh { &whole_level } else { &due.entered[k] };
                    for e in rects {
                        let e = crate::layer::cut(*e, v.r);
                        if e[0] >= e[2] || e[1] >= e[3] {
                            continue;
                        }
                        let m = (1 << bs) - 1;
                        for by in e[1] >> bs..(e[3] + m) >> bs {
                            for bx in e[0] >> bs..(e[2] + m) >> bs {
                                force[(base + (by - by0) as u32 * capw + (bx - bx0) as u32) as usize] = era;
                            }
                        }
                    }
                }
            }
            self.res.lb = lb;
        }
        let kt = self.words_mut(23);
        for &(w, z) in scene.events() {
            for y in (w[1] >> 5).max(0)..((w[3] + 31) >> 5).min(ty as i32) {
                for x in (w[0] >> 5).max(0)..((w[2] + 31) >> 5).min(tx as i32) {
                    let t = (y as usize * tx + x as usize) * 4;
                    (kt[t], kt[t + 1]) = (kt[t].min(z), kt[t + 1].max(z + 1));
                }
            }
        }
        for &t in tiles {
            kt[t as usize * 4 + 2] = era;
        }
        let nstages = self.res.stages.len();
        let args = self.words_mut(24);
        for i in 0..=nstages {
            args[8 * i] = 0;
        }
        self.words_mut(26)[..4].copy_from_slice(&[era, since.unwrap_or(era), all as u32, since.is_some() as u32]);
        let m = self.words_mut(1);
        m[2] = stride as u32;
        m[M_UNEW..M_UNEW + 64].copy_from_slice(&u.w);
        // ---- the same commands every frame ----
        let t = std::time::Instant::now();
        // Every buffer and pipeline the commands name is owned by `self` (or wraps the scene's memory, which
        // outlives `self`) and the command buffer is waited for before this call returns, so nothing it
        // references can be released while it runs: it need not retain them.
        let cmd = self.queue.commandBufferWithUnretainedReferences().expect("command buffer");
        let enc = cmd.computeCommandEncoder().expect("encoder");
        self.bind(&enc, None);
        let g16 = MTLSize { width: 16, height: 16, depth: 1 };
        let g64 = MTLSize { width: 64, height: 1, depth: 1 };
        let args_buf = self.bufs[24].as_ref().unwrap();
        // SAFETY (the buffer offsets below): buffer 24 holds eight words per stage and for one more (count,
        // 1, 1, first slot, slots, level): index 24 sees a stage's count, index 27 its slots. Buffer 25 holds
        // eight words for every slot of the largest stage and for every tile.
        unsafe {
            enc.setBuffer_offset_atIndex(self.bufs[25].as_deref(), 0, 17);
            enc.setBuffer_offset_atIndex(Some(args_buf), 12, 27);
        }
        for (i, st) in self.res.stages.iter().enumerate() {
            unsafe {
                enc.setBufferOffset_atIndex(32 * i, 24);
                enc.setBufferOffset_atIndex(32 * i + 12, 27);
            }
            if st.0 == 0 {
                enc.setComputePipelineState(&self.rk[0]);
                enc.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: (st.1 as usize).div_ceil(64), height: 1, depth: 1 }, g64);
                enc.setComputePipelineState(&self.rk[1]);
                unsafe { enc.dispatchThreadgroupsWithIndirectBuffer_indirectBufferOffset_threadsPerThreadgroup(args_buf, 32 * i, g16) };
            } else {
                // a threadgroup per slot, a thread per pixel of its block
                let side = 32usize >> st.0;
                enc.setComputePipelineState(&self.rk[2]);
                enc.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: st.1 as usize, height: 1, depth: 1 }, MTLSize { width: side, height: side, depth: 1 });
            }
        }
        if since.is_some() {
            unsafe { enc.setBufferOffset_atIndex(32 * nstages, 24) };
            enc.setComputePipelineState(&self.rk[3]);
            enc.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: nt.div_ceil(64), height: 1, depth: 1 }, g64);
            enc.setComputePipelineState(&self.rk[1]);
            unsafe { enc.dispatchThreadgroupsWithIndirectBuffer_indirectBufferOffset_threadsPerThreadgroup(args_buf, 32 * nstages, g16) };
        }
        enc.setComputePipelineState(&self.rk[4]);
        enc.dispatchThreadgroups_threadsPerThreadgroup(MTLSize { width: nt.div_ceil(64), height: 1, depth: 1 }, g64);
        enc.endEncoding();
        cmd.commit();
        cmd.waitUntilCompleted();
        // the other paths' resident lanes were not kept up
        self.first = true;
        (GpuTime { wall: t.elapsed().as_secs_f64(), gpu: cmd.GPUEndTime() - cmd.GPUStartTime() }, era)
    }
    /// What the last `resident` frame ran, read from the stages' counters after the fact (a report; nothing
    /// is scheduled from it): prefix jobs, pyramid blocks, output tiles, dispatches encoded.
    pub fn resident_stats(&self) -> [u32; 4] {
        let args = self.words(24);
        let mut st = [0u32; 4];
        for (i, s) in self.res.stages.iter().enumerate() {
            if s.0 == 0 {
                st[0] += args[8 * i];
            }
        }
        st[2] = args[8 * self.res.stages.len()];
        st[3] = self.resident_dispatches() as u32;
        st
    }
    /// Dispatches a resident frame encodes (0 before the first one).
    pub fn resident_dispatches(&self) -> usize {
        if self.res.stages.is_empty() { 0 } else { self.res.stages.iter().map(|s| if s.0 == 0 { 2 } else { 1 }).sum::<usize>() + 3 }
    }
    /// Ranks in the resident plan's stage order, and its slots.
    pub fn resident_shape(&self) -> (u32, usize) {
        (self.res.rank.iter().copied().max().map_or(0, |r| r + 1), self.res.nslots)
    }
    /// The slots: every (layer, level, block its storage can hold), ordered by (rank, level).
    fn res_slots(&mut self, scene: &Scene) {
        let ls = &scene.layers;
        let rank = ls.ranks(scene.tx, scene.ty);
        let mut tab = Vec::new();
        ls.table(&mut tab);
        let nl = ls.list.len();
        let mut by_rank: Vec<u32> = (0..nl as u32).collect();
        by_rank.sort_by_key(|&l| rank[l as usize]);
        let (mut sl, mut lb, mut stages): (Vec<u32>, Vec<u32>, Vec<(u32, u32, u32)>) = (Vec::new(), vec![0; nl * 8], Vec::new());
        let mut i = 0;
        let mut most = 0;
        while i < nl {
            let j = i + by_rank[i..].iter().take_while(|&&l| rank[l as usize] == rank[by_rank[i] as usize]).count();
            for k in 0..crate::layer::LEVELS {
                let at = sl.len();
                for &lid in &by_rank[i..j] {
                    let l = &ls.list[lid as usize];
                    if k as u32 > l.top {
                        continue;
                    }
                    let (v, bs) = (&l.lv[k], crate::layer::bshift(k as u32));
                    let n = (((v.cw + (1 << bs) - 1) >> bs) + 1) * (((v.ch + (1 << bs) - 1) >> bs) + 1);
                    lb[lid as usize * 8 + k] = sl.len() as u32;
                    sl.extend(std::iter::repeat_n(lid | (k as u32) << 24, n as usize));
                }
                if sl.len() > at {
                    stages.push((k as u32, (sl.len() - at) as u32, at as u32));
                    most = most.max(sl.len() - at);
                }
            }
            i = j;
        }
        let ns = sl.len();
        self.buf(14, tab.len().max(16) * 4);
        self.words_mut(14)[..tab.len()].copy_from_slice(&tab);
        self.buf(19, ns.max(4) * 4);
        self.words_mut(19)[..ns].copy_from_slice(&sl);
        self.buf(20, lb.len().max(4) * 4);
        self.words_mut(20)[..lb.len()].copy_from_slice(&lb);
        for b in [21, 22] {
            self.buf(b, ns.max(4) * 4);
            self.words_mut(b).fill(0);
        }
        self.buf(24, (stages.len() + 1) * 32);
        let sw = self.words_mut(24);
        sw.chunks_exact_mut(8).for_each(|a| a.copy_from_slice(&[0, 1, 1, 0, 0, 0, 0, 0]));
        for (i, s) in stages.iter().enumerate() {
            sw[8 * i + 3..8 * i + 6].copy_from_slice(&[s.2, s.1, s.0]);
        }
        self.buf(25, most.max(self.nt).max(1) * 32);
        self.res.alloc = Some(ls.alloc_epoch);
        (self.res.rank, self.res.lb, self.res.stages, self.res.nslots) = (rank, lb, stages, ns);
    }
    /// The tile lists as slabs in buffer 5: all of them (`None`), or the tiles that changed (rewritten in
    /// place while they fit, moved to the end otherwise; everything is repacked when the end is reached).
    fn res_lists(&mut self, scene: &Scene, changed: Option<&[u32]>) {
        let nt = self.nt;
        if let Some(ch) = changed {
            let mut slab = std::mem::take(&mut self.res.slab);
            let mut used = self.res.used;
            let cap = self.words(5).len() as u32;
            let mut ok = true;
            for &t in ch {
                let l = scene.bins.tile(t as usize);
                if l.len() as u32 > slab[t as usize].1 {
                    let c = l.len() as u32 * 3 / 2 + 4;
                    if used + c > cap {
                        ok = false;
                        break;
                    }
                    slab[t as usize] = (used, c);
                    used += c;
                }
                let off = slab[t as usize].0 as usize;
                self.words_mut(5)[off..off + l.len()].copy_from_slice(l);
                let th = self.words_mut(4);
                (th[2 * t as usize], th[2 * t as usize + 1]) = (off as u32, l.len() as u32);
                self.words_mut(23)[4 * t as usize + 3] = scene.bins.floor[t as usize];
            }
            (self.res.slab, self.res.used) = (slab, used);
            if ok {
                return;
            }
        }
        let total = scene.bins.total();
        self.buf(5, (total * 2 + nt * 8) * 4);
        let mut slab = Vec::with_capacity(nt);
        let mut used = 0u32;
        for t in 0..nt {
            let l = scene.bins.tile(t);
            let c = l.len() as u32 * 3 / 2 + 4;
            self.words_mut(5)[used as usize..used as usize + l.len()].copy_from_slice(l);
            let th = self.words_mut(4);
            (th[2 * t], th[2 * t + 1]) = (used, l.len() as u32);
            self.words_mut(23)[4 * t + 3] = scene.bins.floor[t];
            slab.push((used, c));
            used += c;
        }
        (self.res.slab, self.res.used, self.res.mirrored) = (slab, used, true);
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
