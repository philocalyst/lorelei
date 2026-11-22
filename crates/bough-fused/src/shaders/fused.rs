/// The fused presenter's three kernels (`fs_copy`, `fs_bin`, `fs_tile`), ported from bough-metal's shader.metal to
/// wgsl-rs so that one Rust source becomes SPIR-V (Vulkan) and GLSL (OpenGL) through naga (build.rs). The
/// arithmetic, the cull hierarchy, the bin layout, the paint order and the certificates' decisions are the Metal
/// kernels'; what differs is what a portable API can bind:
///
/// * **Columns are bindings, not GPU addresses.** Metal reads every column through the region's column table
///   (`FCols`); here each column is its own storage binding `C_*`, which the Vulkan presenter binds to a host-pointer
///   view of the column's used prefix (the same views as `Fused::new_views`) and which build.rs rewrites, for GL, into
///   reads of the one region buffer at the header's offsets (`c_*` are the only readers of `C_*`).
/// * **Surfaces are storage buffers of BGRA8 words** (`S0..S3`, row stride `P_STRIDE` words): the Vulkan presenter
///   binds the window's `wl_shm` buffers there through `VK_EXT_external_memory_host` (the GPU writes the pixels the
///   compositor reads, as Metal writes an IOSurface). The glyph atlas (R8) and the image atlas (RGBA8) are words too.
/// * **Certificates hold the stored word**: a band's colour is the 32-bit BGRA8 word its rows hold, and a set row bit
///   means all 32 pixels of that row hold exactly that word, so a FILL writes it back bit for bit (Metal keeps the
///   half4 the word was converted from; here nothing is converted).
/// * **Any subgroup size**: the ordered compaction loops over `num_subgroups` (Metal's simdgroups are always 32 wide,
///   lavapipe's are 8 or 16); a band's certificate is decided from workgroup memory, not from one subgroup.
/// * **f32 accumulation** (Metal blends in half).
///
/// Bounds by construction as in Metal: every index read from region data is ANDed with its column's mask, every loop
/// bound read from it is min-ed with a capacity, every surface index is clamped to the surface.
#[allow(unused_assignments)]
#[wgsl_rs::wgsl]
pub mod fused {
    use wgsl_rs::std::*;

    // the region's head: header (`FHdr` @0), the port table (@128), the frame slots
    storage!(group(0), binding(0), POOL: RuntimeArray<u32>);
    // the GPU-read columns (`bough_frame::COL_*` order)
    storage!(group(0), binding(1), C_INST: RuntimeArray<u32>);
    storage!(group(0), binding(2), C_SPACES: RuntimeArray<u32>);
    storage!(group(0), binding(3), C_CLIPS: RuntimeArray<u32>);
    storage!(group(0), binding(4), C_DIR_ID: RuntimeArray<u32>);
    storage!(group(0), binding(5), C_DIR_INK: RuntimeArray<u32>);
    storage!(group(0), binding(6), C_PAGES: RuntimeArray<u32>);
    storage!(group(0), binding(7), C_GX: RuntimeArray<u32>);
    storage!(group(0), binding(8), C_GENT: RuntimeArray<u32>);
    storage!(group(0), binding(9), C_RUNS: RuntimeArray<u32>);
    storage!(group(0), binding(10), C_TOKS: RuntimeArray<u32>);
    storage!(group(0), binding(11), C_XFORMS: RuntimeArray<u32>);
    storage!(group(0), binding(12), read_write, BINS: RuntimeArray<Vec4u>);
    storage!(group(0), binding(13), read_write, COUNTS: RuntimeArray<Atomic<u32>>);
    storage!(group(0), binding(14), read_write, S0: RuntimeArray<u32>);
    storage!(group(0), binding(15), read_write, S1: RuntimeArray<u32>);
    storage!(group(0), binding(16), read_write, S2: RuntimeArray<u32>);
    storage!(group(0), binding(17), read_write, S3: RuntimeArray<u32>);
    storage!(group(0), binding(18), read_write, CERT: RuntimeArray<u32>);
    storage!(group(0), binding(19), ATLAS: RuntimeArray<u32>);
    storage!(group(0), binding(20), IMG: RuntimeArray<u32>);
    // x: the frame slot's byte offset in the region
    uniform!(group(1), binding(0), SLOT: Vec4u);

    // per-subgroup counts of the ordered compaction (64: subgroups of 4 lanes or more)
    workgroup!(SGC: [u32; 64]);
    workgroup!(FLO: [u32; 64]);
    // fs_bin
    workgroup!(DL: [u32; 256]);
    workgroup!(GG: [u32; 256]);
    workgroup!(HL: [u32; 256]);
    workgroup!(HK: [Vec2u; 256]);
    workgroup!(HC: [u32; 256]);
    workgroup!(SP: [u32; 256]);
    workgroup!(SD: [u32; 256]);
    workgroup!(SH: [u32; 256]);
    workgroup!(IP: [u32; 256]);
    // fs_tile: the hits of a step, the staged records (structure of arrays), the band verdicts
    workgroup!(HT: [u32; 256]);
    workgroup!(RQ: [Vec4f; 128]);
    workgroup!(RCORE: [Vec4f; 128]);
    workgroup!(RB: [Vec4f; 128]);
    workgroup!(RCOL: [Vec2u; 128]);
    workgroup!(RPAR: [Vec4u; 128]);
    workgroup!(RXS: [f32; 128]);
    workgroup!(BW: [u32; 256]);
    // fs_copy: per band of the tile, the decision and the colour
    workgroup!(TGK: [u32; 8]);
    workgroup!(TGC: [u32; 8]);

    pub const F_RUN: u32 = 0x80000000u32;
    pub const F_IRUN: u32 = 0x40000000u32;
    pub const CERT_FROZEN: u32 = 0x100u32;
    /// Words of a page (1472 B) and of its columns' starts (bough-core `Page`).
    pub const PG_WORDS: u32 = 368u32;
    pub const PG_INK: u32 = 32u32;
    pub const PG_GINK: u32 = 160u32;
    pub const PG_PAYLOAD: u32 = 192u32;
    pub const PG_CLIP: u32 = 320u32;
    pub const PG_OFFL: u32 = 359u32;
    pub const PG_XF: u32 = 362u32;
    /// Header words (`FHdr`).
    pub const H_VW: u32 = 0u32;
    pub const H_VH: u32 = 1u32;
    pub const H_TILES_X: u32 = 2u32;
    pub const H_TILES_Y: u32 = 3u32;
    pub const H_N_DIR: u32 = 9u32;
    pub const H_TOK_STRIDE: u32 = 15u32;
    pub const H_BIN_CAP: u32 = 16u32;
    pub const H_DEBUG: u32 = 17u32;
    pub const H_M_INST: u32 = 18u32;
    pub const H_M_PAGE: u32 = 19u32;
    pub const H_M_GLYPH: u32 = 20u32;
    pub const H_M_RUN: u32 = 21u32;
    pub const H_M_SPACE: u32 = 22u32;
    pub const H_M_CLIP: u32 = 23u32;
    pub const H_M_STRIKE: u32 = 24u32;
    /// The port table (region bytes 128..256, where Metal keeps its bind table).
    pub const P_ATLAS_W: u32 = 32u32;
    pub const P_ATLAS_H: u32 = 33u32;
    pub const P_STRIDE: u32 = 34u32;
    pub const P_CERT_PLANE: u32 = 35u32;
    pub const P_IMG_W: u32 = 36u32;
    pub const P_IMG_H: u32 = 37u32;
    pub const P_SURF_WORDS: u32 = 38u32;

    pub fn hw(i: u32) -> u32 {
        get!(POOL)[i]
    }
    // The column readers: the only readers of `C_*` (build.rs's GL form reads the region at the header's offsets).
    pub fn c_inst(i: u32) -> u32 {
        get!(C_INST)[i]
    }
    pub fn c_spaces(i: u32) -> u32 {
        get!(C_SPACES)[i]
    }
    pub fn c_clips(i: u32) -> u32 {
        get!(C_CLIPS)[i]
    }
    pub fn c_dir_id(i: u32) -> u32 {
        get!(C_DIR_ID)[i]
    }
    pub fn c_dir_ink(i: u32) -> u32 {
        get!(C_DIR_INK)[i]
    }
    pub fn c_pages(i: u32) -> u32 {
        get!(C_PAGES)[i]
    }
    pub fn c_gx(i: u32) -> u32 {
        get!(C_GX)[i]
    }
    pub fn c_gent(i: u32) -> u32 {
        get!(C_GENT)[i]
    }
    pub fn c_runs(i: u32) -> u32 {
        get!(C_RUNS)[i]
    }
    pub fn c_toks(i: u32) -> u32 {
        get!(C_TOKS)[i]
    }
    pub fn c_xforms(i: u32) -> u32 {
        get!(C_XFORMS)[i]
    }

    pub fn surf_load(s: u32, i: u32) -> u32 {
        let j = min(i, hw(P_SURF_WORDS) - 1u32);
        if s == 0u32 {
            return get!(S0)[j];
        }
        if s == 1u32 {
            return get!(S1)[j];
        }
        if s == 2u32 {
            return get!(S2)[j];
        }
        get!(S3)[j]
    }
    pub fn surf_store(s: u32, i: u32, v: u32) {
        let j = min(i, hw(P_SURF_WORDS) - 1u32);
        if s == 0u32 {
            get_mut!(S0)[j] = v;
        } else if s == 1u32 {
            get_mut!(S1)[j] = v;
        } else if s == 2u32 {
            get_mut!(S2)[j] = v;
        } else {
            get_mut!(S3)[j] = v;
        }
    }

    /// An `NRect` (i16 x 4: x0, y0, -x1, -y1) from its two words, as i32 lanes.
    pub fn unpack_rect(a: u32, b: u32) -> Vec4i {
        vec4i(((a << 16u32) as i32) >> 16u32, (a as i32) >> 16u32, ((b << 16u32) as i32) >> 16u32, (b as i32) >> 16u32)
    }
    pub fn pack2(x: i32, y: i32) -> u32 {
        ((x as u32) & 65535u32) | ((y as u32) << 16u32)
    }
    /// The core's `Thresh::matches`: all(r < th).
    pub fn lt4(r: Vec4i, th: Vec4i) -> bool {
        r.x() < th.x() && r.y() < th.y() && r.z() < th.z() && r.w() < th.w()
    }
    /// Saturating i16 subtraction, lane-wise.
    pub fn subsat(a: Vec4i, b: Vec4i) -> Vec4i {
        clamp(a - b, vec4i(-32768i32, -32768i32, -32768i32, -32768i32), vec4i(32767i32, 32767i32, 32767i32, 32767i32))
    }
    pub fn pg_rect(w: u32) -> Vec4i {
        unpack_rect(c_pages(w), c_pages(w + 1u32))
    }
    pub fn clip_rect(c: u32) -> Vec4f {
        let w = 2u32 * (c & hw(H_M_CLIP));
        let r = unpack_rect(c_clips(w), c_clips(w + 1u32));
        vec4f(r.x() as f32, r.y() as f32, -(r.z() as f32), -(r.w() as f32))
    }
    pub fn space(s: u32) -> Vec2f {
        let w = 2u32 * (s & hw(H_M_SPACE));
        vec2f((c_spaces(w) as i32) as f32, (c_spaces(w + 1u32) as i32) as f32)
    }
    /// A straight RGBA8 word (r in the low byte) premultiplied.
    pub fn premul(c: u32) -> Vec4f {
        let v = unpack4x8unorm(c);
        vec4f(v.x() * v.w(), v.y() * v.w(), v.z() * v.w(), v.w())
    }
    pub fn qink(qa: Vec2f, qb: Vec2f) -> Vec4i {
        vec4i(floor(qa.x()) as i32, floor(qa.y()) as i32, -(ceil(qb.x()) as i32), -(ceil(qb.y()) as i32))
    }
    /// The ink margin beyond an instance's box: none for a solid quad, 1 px for an SDF edge, or the instance's own.
    pub fn margin(kind: u32, flags: u32) -> f32 {
        if kind == 0u32 {
            return 0.0;
        }
        if flags != 0u32 {
            return flags as f32;
        }
        1.0
    }

    /// A transformed page (bough-core `Page::xm/xv`): its stored inks are untransformed.
    pub fn pg_xf(pg: u32) -> bool {
        let mut any = false;
        let mut k = 0u32;
        while k < 6u32 {
            if bitcast_f32(c_pages(pg + PG_XF + k)) != 0.0 {
                any = true;
            }
            k += 1u32;
        }
        any
    }
    /// The page-relative ink `rel` of a transformed page, as its absolute bounding box grown by a pixel.
    pub fn pg_xink(pg: u32, rel: Vec4i) -> Vec4i {
        if rel.x() >= -rel.z() || rel.y() >= -rel.w() {
            return rel;
        }
        let m = vec4f(
            bitcast_f32(c_pages(pg + PG_XF)) + 1.0,
            bitcast_f32(c_pages(pg + PG_XF + 1u32)),
            bitcast_f32(c_pages(pg + PG_XF + 2u32)),
            bitcast_f32(c_pages(pg + PG_XF + 3u32)) + 1.0,
        );
        let o = pg_rect(pg + PG_OFFL);
        let t = vec2f(bitcast_f32(c_pages(pg + PG_XF + 4u32)) + (o.x() as f32), bitcast_f32(c_pages(pg + PG_XF + 5u32)) + (o.y() as f32));
        let a = vec2f(rel.x() as f32, rel.y() as f32);
        let b = vec2f(-(rel.z() as f32), -(rel.w() as f32));
        let p0 = vec2f(m.x() * a.x() + m.y() * a.y(), m.z() * a.x() + m.w() * a.y()) + t;
        let p1 = vec2f(m.x() * b.x() + m.y() * a.y(), m.z() * b.x() + m.w() * a.y()) + t;
        let p2 = vec2f(m.x() * a.x() + m.y() * b.y(), m.z() * a.x() + m.w() * b.y()) + t;
        let p3 = vec2f(m.x() * b.x() + m.y() * b.y(), m.z() * b.x() + m.w() * b.y()) + t;
        let lo = clamp(floor(min(min(p0, p1), min(p2, p3))) - vec2f(1.0, 1.0), vec2f(-30000.0, -30000.0), vec2f(30000.0, 30000.0));
        let hi = clamp(ceil(max(max(p0, p1), max(p2, p3))) + vec2f(1.0, 1.0), vec2f(-30000.0, -30000.0), vec2f(30000.0, 30000.0));
        vec4i(lo.x() as i32, lo.y() as i32, -(hi.x() as i32), -(hi.y() as i32))
    }

    /// A space's affine (transformed subtrees): `on` = 0 for the identity.
    #[derive(Clone, Copy, Wgsl)]
    pub struct Xf {
        pub m: Vec4f,
        pub k: Vec2f,
        pub t: Vec2f,
        pub on: u32,
    }
    pub fn xf(s: u32) -> Xf {
        let w = 8u32 * (s & hw(H_M_SPACE));
        let dm = vec4f(bitcast_f32(c_xforms(w)), bitcast_f32(c_xforms(w + 1u32)), bitcast_f32(c_xforms(w + 2u32)), bitcast_f32(c_xforms(w + 3u32)));
        let c = vec2f(bitcast_f32(c_xforms(w + 4u32)), bitcast_f32(c_xforms(w + 5u32)));
        let t = vec2f(bitcast_f32(c_xforms(w + 6u32)), bitcast_f32(c_xforms(w + 7u32)));
        let on = dm.x() != 0.0 || dm.y() != 0.0 || dm.z() != 0.0 || dm.w() != 0.0 || t.x() != 0.0 || t.y() != 0.0;
        Xf { m: dm + vec4f(1.0, 0.0, 0.0, 1.0), k: space(s) + c, t: t, on: select(0u32, 1u32, on) }
    }
    pub fn xmul(m: Vec4f, v: Vec2f) -> Vec2f {
        vec2f(m.x() * v.x() + m.y() * v.y(), m.z() * v.x() + m.w() * v.y())
    }
    /// The transformed box [a, b] (untransformed absolute px): its bounding box, grown by a pixel.
    pub fn xbox(a: Vec2f, b: Vec2f, x: Xf) -> Vec4f {
        let p0 = x.k + x.t + xmul(x.m, a - x.k);
        let p1 = x.k + x.t + xmul(x.m, vec2f(b.x(), a.y()) - x.k);
        let p2 = x.k + x.t + xmul(x.m, vec2f(a.x(), b.y()) - x.k);
        let p3 = x.k + x.t + xmul(x.m, b - x.k);
        let lo = min(min(p0, p1), min(p2, p3)) - vec2f(1.0, 1.0);
        let hi = max(max(p0, p1), max(p2, p3)) + vec2f(1.0, 1.0);
        vec4f(lo.x(), lo.y(), hi.x(), hi.y())
    }

    /// Glyph `slot` of run `run` (bough-text's pen formula): geo = (p0, size), info = (colour, atlas uv packed);
    /// geo.z = -1 when the glyph has no bitmap.
    #[derive(Clone, Copy, Wgsl)]
    pub struct Glyph {
        pub geo: Vec4f,
        pub info: Vec2u,
    }
    pub fn glyph(run: u32, slot: u32) -> Glyph {
        let rw = 8u32 * (run & hw(H_M_RUN));
        let s = slot & hw(H_M_GLYPH);
        let x = c_gx(s) as i32;
        let gw = c_gent(s >> 1u32);
        let e = (gw >> ((s & 1u32) * 16u32)) & 255u32;
        let sw = c_runs(rw + 4u32);
        let strike = (sw >> 16u32) & hw(H_M_STRIKE);
        let ew = (strike * hw(H_TOK_STRIDE)) / 4u32 + e * 8u32;
        if (c_toks(ew + 5u32) & 65535u32) == 0u32 {
            return Glyph { geo: vec4f(0.0, 0.0, -1.0, -1.0), info: vec2u(0u32, 0u32) };
        }
        let dx = x - (c_runs(rw + 7u32) as i32);
        let scale = bitcast_f32(c_runs(rw + 2u32));
        let ox = bitcast_f32(c_runs(rw));
        let oy = bitcast_f32(c_runs(rw + 1u32));
        let px = floor(fma(dx as f32, scale, ox + 0.5)) + bitcast_f32(c_toks(ew));
        let sp = space(sw & 65535u32);
        Glyph {
            geo: vec4f(px + sp.x(), oy + bitcast_f32(c_toks(ew + 1u32)) + sp.y(), bitcast_f32(c_toks(ew + 2u32)), bitcast_f32(c_toks(ew + 3u32))),
            info: vec2u(c_runs(rw + 3u32), c_toks(ew + 4u32)),
        }
    }
    pub fn run_space(run: u32) -> u32 {
        c_runs(8u32 * (run & hw(H_M_RUN)) + 4u32) & 65535u32
    }

    /// Ordered exclusive prefix sum of `v` across the workgroup: (this invocation's sum of those before it, the
    /// total). The compaction of a hit is this with v = 0 or 1.
    pub fn prefix(v: u32, lane: u32, sg: u32, nsg: u32) -> Vec2u {
        let pre = subgroup_exclusive_add(v);
        let cnt = subgroup_add(v);
        if lane == 0u32 {
            get_mut!(SGC)[sg as usize] = cnt;
        }
        workgroup_barrier();
        let mut off = 0u32;
        let mut total = 0u32;
        let mut s = 0u32;
        while s < nsg {
            let x = get!(SGC)[s as usize];
            if s < sg {
                off += x;
            }
            total += x;
            s += 1u32;
        }
        vec2u(off + pre, total)
    }
    pub fn compact(hit: bool, lane: u32, sg: u32, nsg: u32) -> Vec2u {
        prefix(select(0u32, 1u32, hit), lane, sg, nsg)
    }

    pub fn bin_put(at: u32, cap: u32, base: u32, v: Vec4u) {
        if at < cap {
            get_mut!(BINS)[base + at] = v;
        }
    }

    /// The bin pass: one workgroup per damaged tile row. Cull the directory -> 8-record groups -> records against the
    /// row's damaged span (transformed pages by their transformed boxes), then append the hits to the row's bin in
    /// paint order, hits expanded to segments (an instance, an instance run, one glyph run) and segments to items
    /// (an instance, a glyph) by prefix sums, 256 items per step whatever runs they belong to.
    #[compute]
    #[workgroup_size(256, 1, 1)]
    pub fn fs_bin(
        #[builtin(workgroup_id)] wg: Vec3u,
        #[builtin(local_invocation_index)] li: u32,
        #[builtin(subgroup_invocation_id)] lane: u32,
        #[builtin(subgroup_id)] sg: u32,
        #[builtin(num_subgroups)] nsg: u32,
    ) {
        let rw = get!(SLOT).x() / 4u32 + 16u32 + 8u32 * wg.x();
        let ty = get!(POOL)[rw];
        let span = get!(POOL)[rw + 1u32];
        let tx0 = span & 255u32;
        let tx1 = (span >> 8u32) & 255u32;
        let th = vec4i((tx1 * 32u32) as i32, (ty * 32u32 + 32u32) as i32, -((tx0 * 32u32) as i32), -((ty * 32u32) as i32));
        let cap = hw(H_BIN_CAP);
        let base = ty * cap;
        let m_page = hw(H_M_PAGE);
        let m_inst = hw(H_M_INST);
        let m_run = hw(H_M_RUN);
        let m_glyph = hw(H_M_GLYPH);
        // the directory columns hold 2x pages (blocks with holes)
        let ndir = min(hw(H_N_DIR), 2u32 * (m_page + 1u32));
        let mut nb = 0u32;
        let mut d0 = 0u32;
        while d0 < ndir {
            let di = d0 + li;
            let mut dh = false;
            if di < ndir {
                dh = lt4(unpack_rect(c_dir_ink(2u32 * di), c_dir_ink(2u32 * di + 1u32)), th);
            }
            workgroup_barrier();
            let dc = compact(dh, lane, sg, nsg);
            if dh {
                get_mut!(DL)[dc.x() as usize] = c_dir_id(di) & m_page;
            }
            workgroup_barrier();
            let nd = dc.y();
            let mut p0 = 0u32;
            while p0 < nd {
                let pk = p0 + (li >> 3u32);
                let g = li & 7u32;
                let mut gh = false;
                let mut gid = 0u32;
                if pk < nd {
                    let pid = get!(DL)[pk as usize];
                    gid = pid * 8u32 + g;
                    let pg = pid * PG_WORDS;
                    let gink = pg_rect(pg + PG_GINK + 2u32 * g);
                    if pg_xf(pg) {
                        gh = lt4(pg_xink(pg, gink), th);
                    } else {
                        gh = lt4(gink, subsat(th, pg_rect(pg + PG_OFFL)));
                    }
                }
                workgroup_barrier();
                let gc = compact(gh, lane, sg, nsg);
                if gh {
                    get_mut!(GG)[gc.x() as usize] = gid;
                }
                workgroup_barrier();
                let ng = gc.y();
                let mut q0 = 0u32;
                while q0 < ng {
                    let gk = q0 + (li >> 3u32);
                    let s = li & 7u32;
                    let mut rh = false;
                    let mut ii = 0u32;
                    let mut cid = 0u32;
                    let mut ink = vec4i(0i32, 0i32, 0i32, 0i32);
                    if gk < ng {
                        let gidv = get!(GG)[gk as usize];
                        let pid = gidv >> 3u32;
                        let sl = (gidv & 7u32) * 8u32 + s;
                        let pg = pid * PG_WORDS;
                        let rel = pg_rect(pg + PG_INK + 2u32 * sl);
                        if pg_xf(pg) {
                            ink = pg_xink(pg, rel);
                            rh = lt4(ink, th);
                        } else {
                            let offl = pg_rect(pg + PG_OFFL);
                            rh = lt4(rel, subsat(th, offl));
                            ink = rel + offl;
                        }
                        ii = c_pages(pg + PG_PAYLOAD + sl);
                        cid = ((c_pages(pg + PG_CLIP + (sl >> 1u32)) >> ((sl & 1u32) * 16u32)) & 65535u32) & hw(H_M_CLIP);
                    }
                    workgroup_barrier();
                    let rc = compact(rh, lane, sg, nsg);
                    if rh {
                        let at = rc.x() as usize;
                        get_mut!(HL)[at] = ii;
                        get_mut!(HK)[at] = vec2u(pack2(ink.x(), ink.y()), pack2(ink.z(), ink.w()));
                        get_mut!(HC)[at] = cid;
                    }
                    workgroup_barrier();
                    let nr = rc.y();
                    // a hit's segments (low 20 bits) and whether it is a run (bit 20)
                    let mut v = 0u32;
                    if li < nr {
                        let p = get!(HL)[li as usize];
                        if (p & F_RUN) != 0u32 {
                            v = (((p >> 20u32) & 2047u32) + 1u32) | (1u32 << 20u32);
                        } else if (p & F_IRUN) != 0u32 {
                            v = 1u32 | (1u32 << 20u32);
                        } else {
                            v = 1u32;
                        }
                    }
                    let vp = prefix(v, lane, sg, nsg);
                    let nseg = vp.y() & 1048575u32;
                    if (vp.y() >> 20u32) == 0u32 {
                        // no run among the hits: the hits are the items
                        if li < nr {
                            let hk = get!(HK)[li as usize];
                            bin_put(nb + li, cap, base, vec4u(get!(HL)[li as usize] & m_inst, hk.x(), hk.y(), get!(HC)[li as usize] << 20u32));
                        }
                        nb += nr;
                    } else {
                        if li < nr {
                            get_mut!(SP)[li as usize] = vp.x() & 1048575u32;
                        }
                        workgroup_barrier();
                        let mut s0 = 0u32;
                        while s0 < nseg {
                            // this invocation's segment: its hit (the last whose first segment is <= it), then what it is
                            let sgi = s0 + li;
                            let mut c = 0u32;
                            let mut desc = 0u32;
                            let mut hit = 0u32;
                            if sgi < nseg {
                                let mut lo = 0u32;
                                let mut hi = nr;
                                while hi - lo > 1u32 {
                                    let mid = (lo + hi) >> 1u32;
                                    if get!(SP)[mid as usize] <= sgi {
                                        lo = mid;
                                    } else {
                                        hi = mid;
                                    }
                                }
                                hit = lo;
                                let p = get!(HL)[lo as usize];
                                if (p & F_RUN) != 0u32 {
                                    let run = ((p & 1048575u32) + (sgi - get!(SP)[lo as usize])) & m_run;
                                    c = min(c_runs(8u32 * run + 6u32), m_glyph + 1u32);
                                    desc = run | F_RUN;
                                } else if (p & F_IRUN) != 0u32 {
                                    c = ((p >> 18u32) & 4095u32) + 1u32;
                                    desc = p;
                                } else {
                                    c = 1u32;
                                    desc = p & m_inst;
                                }
                            }
                            // the previous chunk's readers of SD, SH, IP are done
                            workgroup_barrier();
                            let cp = prefix(c, lane, sg, nsg);
                            let nitem = cp.y();
                            get_mut!(SD)[li as usize] = desc;
                            get_mut!(SH)[li as usize] = hit;
                            get_mut!(IP)[li as usize] = cp.x();
                            let ns = min(nseg - s0, 256u32);
                            let mut t0 = 0u32;
                            while t0 < nitem {
                                // SD, SH, IP are written; the previous step's readers of SGC are done
                                workgroup_barrier();
                                let t = t0 + li;
                                let mut ok = false;
                                let mut e = vec4u(0u32, 0u32, 0u32, 0u32);
                                if t < nitem {
                                    // this invocation's item: its segment (the last whose first item is <= t), then its place in it
                                    let mut lo = 0u32;
                                    let mut hi = ns;
                                    while hi - lo > 1u32 {
                                        let mid = (lo + hi) >> 1u32;
                                        if get!(IP)[mid as usize] <= t {
                                            lo = mid;
                                        } else {
                                            hi = mid;
                                        }
                                    }
                                    let u = t - get!(IP)[lo as usize];
                                    let d = get!(SD)[lo as usize];
                                    let hx = get!(SH)[lo as usize];
                                    let cid2 = get!(HC)[hx as usize];
                                    if (d & (F_RUN | F_IRUN)) == 0u32 {
                                        ok = true;
                                        let hk = get!(HK)[hx as usize];
                                        e = vec4u(d, hk.x(), hk.y(), cid2 << 20u32);
                                    } else {
                                        let cl = clip_rect(cid2);
                                        let mut qa = vec2f(0.0, 0.0);
                                        let mut qb = vec2f(0.0, 0.0);
                                        let mut spc = 0u32;
                                        let mut have = false;
                                        if (d & F_RUN) != 0u32 {
                                            let run = d & 2147483647u32;
                                            let slot = c_runs(8u32 * run + 5u32) + u;
                                            let gl = glyph(run, slot);
                                            if gl.geo.z() >= 0.0 {
                                                qa = gl.geo.xy() - vec2f(1.0, 1.0);
                                                qb = gl.geo.xy() - vec2f(1.0, 1.0) + (gl.geo.zw() + vec2f(2.0, 2.0));
                                                spc = run_space(run);
                                                have = true;
                                            }
                                            e = vec4u(d, 0u32, 0u32, (cid2 << 20u32) | (slot & m_glyph));
                                        } else {
                                            let idx = ((d & 262143u32) + u) & m_inst;
                                            let iw = 12u32 * idx;
                                            let k10 = c_inst(iw + 10u32);
                                            let q0v = vec2f(bitcast_f32(c_inst(iw)), bitcast_f32(c_inst(iw + 1u32))) + space(k10 & 65535u32);
                                            let size = vec2f(bitcast_f32(c_inst(iw + 2u32)), bitcast_f32(c_inst(iw + 3u32)));
                                            let m = margin((k10 >> 16u32) & 255u32, k10 >> 24u32);
                                            qa = q0v - vec2f(m, m);
                                            qb = q0v + size + vec2f(m, m);
                                            spc = k10 & 65535u32;
                                            have = true;
                                            e = vec4u(idx, 0u32, 0u32, cid2 << 20u32);
                                        }
                                        if have {
                                            let x = xf(spc);
                                            if x.on != 0u32 {
                                                let bb = xbox(qa, qb, x);
                                                qa = bb.xy();
                                                qb = bb.zw();
                                            }
                                            qa = clamp(qa, cl.xy(), cl.zw());
                                            qb = clamp(qb, cl.xy(), cl.zw());
                                            if qa.x() < qb.x() && qa.y() < qb.y() {
                                                let gi = qink(qa, qb);
                                                ok = lt4(gi, th);
                                                e = vec4u(e.x(), pack2(gi.x(), gi.y()), pack2(gi.z(), gi.w()), e.w());
                                            }
                                        }
                                    }
                                }
                                let ac = compact(ok, lane, sg, nsg);
                                if ok {
                                    bin_put(nb + ac.x(), cap, base, e);
                                }
                                nb += ac.y();
                                t0 += 256u32;
                            }
                            s0 += 256u32;
                        }
                    }
                    workgroup_barrier();
                    q0 += 32u32;
                }
                p0 += 32u32;
            }
            d0 += 256u32;
        }
        if li == 0u32 {
            atomic_store(&get!(COUNTS)[ty], min(nb, cap));
            // overflow: the largest row count the CPU must grow the bins to (it redraws the frame then)
            if nb > cap {
                atomic_max(&get!(COUNTS)[hw(H_TILES_Y)], nb);
            }
        }
    }

    /// Stage instance `i` as record `at` (the edge form of shader.metal's `f_stage_rec`): x = hit, y = covers the
    /// whole tile at `o` opaquely.
    pub fn stage_inst(at: u32, i: u32, cl: Vec4f, o: Vec2u) -> Vec2u {
        let iw = 12u32 * (i & hw(H_M_INST));
        let k10 = c_inst(iw + 10u32);
        let spc = k10 & 65535u32;
        let kind = (k10 >> 16u32) & 255u32;
        let p0 = vec2f(bitcast_f32(c_inst(iw)), bitcast_f32(c_inst(iw + 1u32))) + space(spc);
        let size = vec2f(bitcast_f32(c_inst(iw + 2u32)), bitcast_f32(c_inst(iw + 3u32)));
        let p1 = p0 + size;
        let m = margin(kind, k10 >> 24u32);
        let mut qa = p0 - vec2f(m, m);
        let mut qb = p1 + vec2f(m, m);
        let x = xf(spc);
        let mut xs = 0.0;
        if x.on != 0u32 {
            let bb = xbox(qa, qb, x);
            qa = bb.xy();
            qb = bb.zw();
            // the transform's scale (sqrt |det M|): SDF distances in the untransformed frame are xs times smaller
            xs = max(sqrt(abs(x.m.x() * x.m.w() - x.m.y() * x.m.z())), 1e-6);
        }
        get_mut!(RXS)[at as usize] = xs;
        qa = clamp(qa, cl.xy(), cl.zw());
        qb = clamp(qb, cl.xy(), cl.zw());
        let color = c_inst(iw + 4u32);
        let radius_in = bitcast_f32(c_inst(iw + 6u32));
        let bw = bitcast_f32(c_inst(iw + 7u32));
        // CSS clamps a corner radius to the box (a pill's 9999 px is half its height); an image's word is its angle
        let mut rad = min(radius_in, 0.5 * min(size.x(), size.y()));
        if kind == 4u32 {
            rad = radius_in;
        }
        get_mut!(RQ)[at as usize] = vec4f(qa.x(), qa.y(), qb.x(), qb.y());
        get_mut!(RB)[at as usize] = vec4f(p0.x(), p0.y(), p1.x(), p1.y());
        get_mut!(RCOL)[at as usize] = vec2u(color, c_inst(iw + 5u32));
        let uvw = c_inst(iw + 8u32);
        // uv.z low 8 bits: gradient form (0 linear, 1 radial, 2 conic) or shadow form (0 outer, 1 inset)
        let pad = (c_inst(iw + 9u32) & 255u32) << 8u32;
        get_mut!(RPAR)[at as usize] = vec4u(bitcast_u32(rad), bitcast_u32(bw), uvw, kind | pad | (spc << 16u32));
        let mut k = max(rad, 0.5 + bw) + 1.0;
        if kind == 1u32 {
            k = max(rad, 0.5) + 1.0;
        }
        let cb0 = max(p0 + vec2f(k, k), qa);
        let cb1 = min(p1 - vec2f(k, k), qb);
        let mut core = vec4f(cb0.x(), cb0.y(), cb1.x(), cb1.y());
        if kind == 0u32 {
            core = vec4f(qa.x(), qa.y(), qb.x(), qb.y());
        }
        if kind >= 3u32 || x.on != 0u32 {
            core = vec4f(0.0, 0.0, 0.0, 0.0);
        }
        get_mut!(RCORE)[at as usize] = core;
        let ox = o.x() as f32;
        let oy = o.y() as f32;
        let cover = x.on == 0u32 && kind == 0u32 && (color >> 24u32) == 255u32 && qa.x() <= ox + 0.5 && qa.y() <= oy + 0.5 && qb.x() > ox + 31.5 && qb.y() > oy + 31.5;
        let hit = qa.x() < qb.x() && qa.y() < qb.y();
        vec2u(select(0u32, 1u32, hit), select(0u32, 1u32, cover))
    }

    /// Stage glyph `slot` of run `run` as record `at`; returns whether it meets anything.
    pub fn stage_glyph(at: u32, run: u32, slot: u32, cl: Vec4f) -> bool {
        let gl = glyph(run, slot);
        if gl.geo.z() < 0.0 {
            return false;
        }
        let p0 = gl.geo.xy();
        let mut qa = p0 - vec2f(1.0, 1.0);
        let mut qb = p0 - vec2f(1.0, 1.0) + (gl.geo.zw() + vec2f(2.0, 2.0));
        let spc = run_space(run);
        let x = xf(spc);
        let mut xs = 0.0;
        if x.on != 0u32 {
            let bb = xbox(qa, qb, x);
            qa = bb.xy();
            qb = bb.zw();
            xs = max(sqrt(abs(x.m.x() * x.m.w() - x.m.y() * x.m.z())), 1e-6);
        }
        get_mut!(RXS)[at as usize] = xs;
        qa = clamp(qa, cl.xy(), cl.zw());
        qb = clamp(qb, cl.xy(), cl.zw());
        get_mut!(RQ)[at as usize] = vec4f(qa.x(), qa.y(), qb.x(), qb.y());
        get_mut!(RB)[at as usize] = vec4f(p0.x(), p0.y(), p0.x() + gl.geo.z(), p0.y() + gl.geo.w());
        get_mut!(RCOL)[at as usize] = vec2u(gl.info.x(), 0u32);
        get_mut!(RPAR)[at as usize] = vec4u(0u32, 0u32, gl.info.y(), 3u32 | (spc << 16u32));
        get_mut!(RCORE)[at as usize] = vec4f(0.0, 0.0, 0.0, 0.0);
        qa.x() < qb.x() && qa.y() < qb.y()
    }

    /// Rounded-box SDF in edge form (the length written out as one fma under a correctly rounded root, as Metal's).
    pub fn sd(c: Vec2f, b: Vec4f, r: f32) -> f32 {
        let q = max(c - b.zw(), b.xy() - c) + vec2f(r, r);
        let m = max(q, vec2f(0.0, 0.0));
        sqrt(fma(m.y(), m.y(), m.x() * m.x())) + min(max(q.x(), q.y()), 0.0) - r
    }

    /// Glyph atlas coverage at texel (x, y) (R8, packed four to a word).
    pub fn atlas_r(x: u32, y: u32) -> f32 {
        let i = y * hw(P_ATLAS_W) + x;
        let w = get!(ATLAS)[i >> 2u32];
        ((w >> ((i & 3u32) * 8u32)) & 255u32) as f32 * (1.0 / 255.0)
    }
    /// Image atlas texel (premultiplied RGBA8).
    pub fn img_rgba(p: Vec2i) -> Vec4f {
        let x = clamp(p.x(), 0i32, (hw(P_IMG_W) as i32) - 1i32) as u32;
        let y = clamp(p.y(), 0i32, (hw(P_IMG_H) as i32) - 1i32) as u32;
        unpack4x8unorm(get!(IMG)[y * hw(P_IMG_W) + x])
    }

    /// The base kinds, untransformed (`f_shade`): solid, rounded, border, glyph.
    pub fn shade_base(k: u32, c: Vec2f) -> Vec4f {
        let q = get!(RQ)[k as usize];
        if !(c.x() >= q.x() && c.x() < q.z() && c.y() >= q.y() && c.y() < q.w()) {
            return vec4f(0.0, 0.0, 0.0, 0.0);
        }
        let par = get!(RPAR)[k as usize];
        let kind = par.w() & 255u32;
        let col = get!(RCOL)[k as usize];
        let pm = premul(col.x());
        let b = get!(RB)[k as usize];
        if kind == 0u32 {
            return pm;
        }
        if kind == 1u32 {
            let d = sd(c, b, bitcast_f32(par.x()));
            return pm * clamp(0.5 - d, 0.0, 1.0);
        }
        if kind == 2u32 {
            let d = sd(c, b, bitcast_f32(par.x()));
            let outer = clamp(0.5 - d, 0.0, 1.0);
            let inner = clamp(0.5 - (d + bitcast_f32(par.y())), 0.0, 1.0);
            return fma(pm, vec4f(1.0, 1.0, 1.0, 1.0) * max(outer - inner, 0.0), premul(col.y()) * inner);
        }
        let t = c - b.xy();
        let mut cov = 0.0;
        if t.x() >= 0.0 && t.y() >= 0.0 && t.x() < b.z() - b.x() && t.y() < b.w() - b.y() {
            let ux = min((par.z() & 65535u32) + (t.x() as u32), hw(P_ATLAS_W) - 1u32);
            let uy = min((par.z() >> 16u32) + (t.y() as u32), hw(P_ATLAS_H) - 1u32);
            cov = atlas_r(ux, uy);
        }
        pm * cov
    }

    /// A base kind in a transformed space (`f_shade_t`): `u` is the pixel centre in the untransformed frame.
    pub fn shade_t(k: u32, u: Vec2f) -> Vec4f {
        let s = get!(RXS)[k as usize];
        let par = get!(RPAR)[k as usize];
        let kind = par.w() & 255u32;
        let col = get!(RCOL)[k as usize];
        let pm = premul(col.x());
        let b = get!(RB)[k as usize];
        let r = bitcast_f32(par.x());
        if kind == 0u32 {
            let d = sd(u, b, 0.0) * s;
            return pm * clamp(0.5 - d, 0.0, 1.0);
        }
        if kind == 1u32 {
            let d = sd(u, b, r) * s;
            return pm * clamp(0.5 - d, 0.0, 1.0);
        }
        if kind == 2u32 {
            let d = sd(u, b, r);
            let outer = clamp(0.5 - d * s, 0.0, 1.0);
            let inner = clamp(0.5 - (d + bitcast_f32(par.y())) * s, 0.0, 1.0);
            return pm * max(outer - inner, 0.0) + premul(col.y()) * inner;
        }
        // the glyph's mask, bilinear (texels outside the glyph's rect are empty)
        let sz = b.zw() - b.xy();
        let t = u - b.xy() - vec2f(0.5, 0.5);
        let f0 = floor(t);
        let fr = t - f0;
        let i0 = vec2i(f0.x() as i32, f0.y() as i32);
        let uv = vec2i((par.z() & 65535u32) as i32, (par.z() >> 16u32) as i32);
        let lim = vec2i((hw(P_ATLAS_W) as i32) - 1i32, (hw(P_ATLAS_H) as i32) - 1i32);
        let mut cov = 0.0;
        let mut kk = 0u32;
        while kk < 4u32 {
            let dx = (kk & 1u32) as i32;
            let dy = (kk >> 1u32) as i32;
            let q = i0 + vec2i(dx, dy);
            if !(q.x() < 0i32 || q.y() < 0i32 || (q.x() as f32) >= sz.x() || (q.y() as f32) >= sz.y()) {
                let wx = select(1.0 - fr.x(), fr.x(), dx == 1i32);
                let wy = select(1.0 - fr.y(), fr.y(), dy == 1i32);
                let a = clamp(uv + q, vec2i(0i32, 0i32), lim);
                cov += atlas_r(a.x() as u32, a.y() as u32) * (wx * wy);
            }
            kk += 1u32;
        }
        pm * cov
    }

    pub fn erf2(x: Vec2f) -> Vec2f {
        let s = sign(x);
        let a = min(abs(x), vec2f(2.8284271, 2.8284271));
        let mut y = vec2f(1.0, 1.0) + (vec2f(0.278393, 0.278393) + (vec2f(0.230389, 0.230389) + vec2f(0.078108, 0.078108) * (a * a)) * a) * a;
        y = y * y;
        let r = s - s / (y * y);
        vec2f(select(r.x(), s.x(), a.x() >= 2.8284271), select(r.y(), s.y(), a.y() >= 2.8284271))
    }
    pub fn shadow_x(x: f32, y: f32, sigma: f32, corner: f32, hs: Vec2f) -> f32 {
        let delta = min(hs.y() - corner - abs(y), 0.0);
        let curved = hs.x() - corner + sqrt(max(0.0, corner * corner - delta * delta));
        let integral = vec2f(0.5, 0.5) + erf2((vec2f(x, x) + vec2f(-curved, curved)) * (0.70710678 / sigma)) * 0.5;
        integral.y() - integral.x()
    }
    /// A rounded box's Gaussian shadow (Evan Wallace's closed form along x, 4 samples along y), in edge form.
    pub fn shadow(p: Vec2f, hs: Vec2f, sigma: f32, corner: f32) -> f32 {
        let low = p.y() - hs.y();
        let high = p.y() + hs.y();
        let start = clamp(-3.0 * sigma, low, high);
        let end = clamp(3.0 * sigma, low, high);
        let step = (end - start) * 0.25;
        let mut y = start + step * 0.5;
        let mut v = 0.0;
        let mut i = 0u32;
        while i < 4u32 {
            let g = exp(-(y * y) / (2.0 * sigma * sigma)) / (2.5066283 * sigma);
            v += shadow_x(p.x(), p.y() - y, sigma, corner, hs) * g * step;
            y += step;
            i += 1u32;
        }
        v
    }

    /// Everything else (`f_shade_x`): transformed records, images, gradients, shadows.
    pub fn shade_x(k: u32, c0: Vec2f) -> Vec4f {
        let q = get!(RQ)[k as usize];
        if !(c0.x() >= q.x() && c0.x() < q.z() && c0.y() >= q.y() && c0.y() < q.w()) {
            return vec4f(0.0, 0.0, 0.0, 0.0);
        }
        let par = get!(RPAR)[k as usize];
        let kind = par.w() & 255u32;
        let xs = get!(RXS)[k as usize];
        let mut c = c0;
        if xs != 0.0 {
            // the pixel centre in the untransformed frame: u = K + M^-1 (c - K - t), from the record's space
            let x = xf(par.w() >> 16u32);
            let det = x.m.x() * x.m.w() - x.m.y() * x.m.z();
            let mut id = 0.0;
            if abs(det) > 1e-12 {
                id = 1.0 / det;
            }
            let d = c0 - (x.k + x.t);
            c = x.k + vec2f(x.m.w() * d.x() - x.m.y() * d.y(), -x.m.z() * d.x() + x.m.x() * d.y()) * id;
            if kind < 4u32 {
                return shade_t(k, c);
            }
        } else if kind < 4u32 {
            return shade_base(k, c);
        }
        let b = get!(RB)[k as usize];
        let col = get!(RCOL)[k as usize];
        let hs = (b.zw() - b.xy()) * 0.5;
        let p = c - (b.xy() + hs);
        let r = bitcast_f32(par.x());
        let bw = bitcast_f32(par.y());
        let uv = vec2i((par.z() & 65535u32) as i32, (par.z() >> 16u32) as i32);
        if kind == 4u32 {
            // an image, rotated about its centre by `r` rad, bilinear
            let cs = cos(r);
            let sn = sin(r);
            let t = vec2f(p.x() * cs + p.y() * sn, -p.x() * sn + p.y() * cs) + hs - vec2f(0.5, 0.5);
            let sz = hs * 2.0;
            if t.x() <= -1.0 || t.y() <= -1.0 || t.x() >= sz.x() || t.y() >= sz.y() {
                return vec4f(0.0, 0.0, 0.0, 0.0);
            }
            let f0 = floor(t);
            let fr = t - f0;
            let i0 = vec2i(f0.x() as i32, f0.y() as i32);
            let mut acc = vec4f(0.0, 0.0, 0.0, 0.0);
            let mut kk = 0u32;
            while kk < 4u32 {
                let dx = (kk & 1u32) as i32;
                let dy = (kk >> 1u32) as i32;
                let qq = i0 + vec2i(dx, dy);
                if !(qq.x() < 0i32 || qq.y() < 0i32 || qq.x() >= (sz.x() as i32) || qq.y() >= (sz.y() as i32)) {
                    let wx = select(1.0 - fr.x(), fr.x(), dx == 1i32);
                    let wy = select(1.0 - fr.y(), fr.y(), dy == 1i32);
                    acc = acc + img_rgba(uv + qq) * (wx * wy);
                }
                kk += 1u32;
            }
            return acc * unpack4x8unorm(col.x()).w();
        }
        let pad = (par.w() >> 8u32) & 255u32;
        if kind == 5u32 {
            // a 256-texel premultiplied ramp: linear (0), radial (1), or conic (2) in a rounded box
            let mut t = 0.0;
            if pad == 0u32 {
                let dir = vec2f(sin(bw), -cos(bw));
                let len = abs(2.0 * hs.x() * dir.x()) + abs(2.0 * hs.y() * dir.y());
                t = dot(p, dir) / max(len, 1e-6) + 0.5;
            } else if pad == 1u32 {
                let gc = vec2f(bitcast_f32(col.x()), bitcast_f32(col.y()));
                let g = (gc - vec2f(0.5, 0.5)) * 2.0 * hs;
                t = length(p - g) / max(length(abs(g) + hs), 1e-6);
            } else {
                let gc = vec2f(bitcast_f32(col.x()), bitcast_f32(col.y()));
                let g = (gc - vec2f(0.5, 0.5)) * 2.0 * hs;
                let d = p - g;
                let ang = atan2(d.x(), -d.y()) - bw;
                t = fract(ang * 0.15915494309189535);
            }
            let x = clamp(t, 0.0, 1.0) * 255.0;
            let i = min(floor(x), 254.0);
            let f = x - i;
            let c0v = img_rgba(uv + vec2i(i as i32, 0i32));
            let c1v = img_rgba(uv + vec2i((i as i32) + 1i32, 0i32));
            let mut sc = 1.0;
            if xs != 0.0 {
                sc = xs;
            }
            let cov = clamp(0.5 - sd(c, b, r) * sc, 0.0, 1.0);
            return mix(c0v, c1v, vec4f(f, f, f, f)) * cov;
        }
        let sh = clamp(shadow(p, hs, max(bw, 1e-3), r), 0.0, 1.0);
        if pad == 1u32 {
            let mut sc = 1.0;
            if xs != 0.0 {
                sc = xs;
            }
            let cov = clamp(0.5 - sd(c, b, r) * sc, 0.0, 1.0);
            return premul(col.x()) * ((1.0 - sh) * cov);
        }
        premul(col.x()) * sh
    }

    /// Source-over of `s` onto `a` with one rounding.
    pub fn over(a: Vec4f, s: Vec4f) -> Vec4f {
        fma(a, vec4f(1.0, 1.0, 1.0, 1.0) * (1.0 - s.w()), s)
    }
    /// A premultiplied RGBA colour as the surface's BGRA8 word.
    pub fn bgra(a: Vec4f) -> u32 {
        pack4x8unorm(vec4f(a.z(), a.y(), a.x(), a.w()))
    }

    /// The tile pass: one workgroup per damaged tile (a binary search over the rows' prefix counts and a select of
    /// the k-th set bit); 16 x 16 invocations x 2 x 2 pixels. Each step tests 256 bin entries against the tile,
    /// compacts the hits in paint order and stages them 128 at a time; the last opaque record covering the whole
    /// tile starts the shading; each invocation marks the staged records whose window meets its 2 x 2 block and
    /// walks its own set bits. Then the pixels are stored and every band of the tile is certified (one word iff all
    /// 128 of its pixels hold it) or cleared.
    #[compute]
    #[workgroup_size(256, 1, 1)]
    pub fn fs_tile(
        #[builtin(workgroup_id)] wg: Vec3u,
        #[builtin(local_invocation_index)] li: u32,
        #[builtin(subgroup_invocation_id)] lane: u32,
        #[builtin(subgroup_id)] sg: u32,
        #[builtin(num_subgroups)] nsg: u32,
    ) {
        let dbg = hw(H_DEBUG);
        if dbg == 2u32 {
            return;
        }
        // one-dimensional workgroup (subgroup builtins require it); invocation (x, y) of the 16 x 16 block
        let lt = vec2u(li & 15u32, li >> 4u32);
        let s = get!(SLOT).x() / 4u32;
        let tgt = get!(POOL)[s + 6u32] & 3u32;
        let nrows = get!(POOL)[s + 7u32];
        let rows = s + 16u32;
        let i = wg.x();
        let mut lo = 0u32;
        let mut hi = nrows;
        while hi - lo > 1u32 {
            let mid = (lo + hi) >> 1u32;
            if get!(POOL)[rows + 8u32 * mid + 2u32] <= i {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let rw = rows + 8u32 * lo;
        let mut k = i - get!(POOL)[rw + 2u32];
        let mut w = 0u32;
        while w < 3u32 {
            let c = count_one_bits(get!(POOL)[rw + 4u32 + w]);
            if k < c {
                break;
            }
            k -= c;
            w += 1u32;
        }
        let mut v = get!(POOL)[rw + 4u32 + w];
        let mut jj = 0u32;
        while jj < k {
            v = v & (v - 1u32);
            jj += 1u32;
        }
        let tile = vec2u(w * 32u32 + count_trailing_zeros(v), get!(POOL)[rw]);
        let o = tile * 32u32;
        let th = vec4i((o.x() + 32u32) as i32, (o.y() + 32u32) as i32, -(o.x() as i32), -(o.y() as i32));
        let c00 = vec2f((o.x() + 2u32 * lt.x()) as f32 + 0.5, (o.y() + 2u32 * lt.y()) as f32 + 0.5);
        let blk = vec4f(c00.x(), c00.y(), c00.x() + 1.0, c00.y() + 1.0);
        let c10 = c00 + vec2f(1.0, 0.0);
        let c01 = c00 + vec2f(0.0, 1.0);
        let c11 = c00 + vec2f(1.0, 1.0);
        let mut a0 = vec4f(0.0, 0.0, 0.0, 0.0);
        let mut a1 = vec4f(0.0, 0.0, 0.0, 0.0);
        let mut a2 = vec4f(0.0, 0.0, 0.0, 0.0);
        let mut a3 = vec4f(0.0, 0.0, 0.0, 0.0);
        let m_inst = hw(H_M_INST);
        let m_run = hw(H_M_RUN);
        let m_glyph = hw(H_M_GLYPH);
        let aw = hw(P_ATLAS_W);
        let ah = hw(P_ATLAS_H);
        let base = tile.y() * hw(H_BIN_CAP);
        let nb = min(atomic_load(&get!(COUNTS)[tile.y()]), hw(H_BIN_CAP));
        let mut b0 = 0u32;
        while b0 < nb {
            let bi = b0 + li;
            let mut ih = false;
            if bi < nb {
                let en = get!(BINS)[base + bi];
                ih = lt4(unpack_rect(en.y(), en.z()), th);
            }
            // the previous step's readers of HT and the records are done
            workgroup_barrier();
            let hc = compact(ih, lane, sg, nsg);
            if ih {
                get_mut!(HT)[hc.x() as usize] = bi;
            }
            let nh = hc.y();
            let mut h0 = 0u32;
            while h0 < nh {
                // HT is written; the previous chunk's readers of the records are done
                workgroup_barrier();
                let mut cover = false;
                let nr = min(nh - h0, 128u32);
                if li < nr {
                    let en = get!(BINS)[base + get!(HT)[(h0 + li) as usize]];
                    let cl = clip_rect(en.w() >> 20u32);
                    let mut rh = false;
                    if (en.x() & F_RUN) != 0u32 {
                        rh = stage_glyph(li, en.x() & m_run, en.w() & m_glyph, cl);
                    } else {
                        let st = stage_inst(li, en.x() & m_inst, cl, o);
                        rh = st.x() != 0u32;
                        cover = st.y() != 0u32;
                    }
                    if !rh {
                        // clipped away: an empty window, which no block meets
                        get_mut!(RQ)[li as usize] = vec4f(1e30, 1e30, -1e30, -1e30);
                        get_mut!(RCORE)[li as usize] = vec4f(0.0, 0.0, 0.0, 0.0);
                        cover = false;
                    }
                }
                let fl = subgroup_max(select(0u32, li + 1u32, cover));
                if lane == 0u32 {
                    get_mut!(FLO)[sg as usize] = fl;
                }
                workgroup_barrier();
                let mut k0 = 0u32;
                let mut s2 = 0u32;
                while s2 < nsg {
                    k0 = max(k0, get!(FLO)[s2 as usize]);
                    s2 += 1u32;
                }
                if k0 > 0u32 {
                    k0 -= 1u32;
                }
                let mut kend = nr;
                if dbg == 1u32 {
                    kend = k0;
                }
                // survivors first, then shading: mark the staged records whose window meets this 2 x 2 block (one
                // word per 32 records), then walk the set bits in paint order
                let mut w2 = k0 >> 5u32;
                while w2 * 32u32 < kend {
                    let mut m = 0u32;
                    let mut kk = max(k0, w2 * 32u32);
                    let ke = min(kend, w2 * 32u32 + 32u32);
                    while kk < ke {
                        let q = get!(RQ)[kk as usize];
                        if !(q.x() > blk.z() || q.z() <= blk.x() || q.y() > blk.w() || q.w() <= blk.y()) {
                            m = m | (1u32 << (kk & 31u32));
                        }
                        kk += 1u32;
                    }
                    while m != 0u32 {
                        let r = w2 * 32u32 + count_trailing_zeros(m);
                        m = m & (m - 1u32);
                        let core = get!(RCORE)[r as usize];
                        let par = get!(RPAR)[r as usize];
                        let kind = par.w() & 255u32;
                        let xs = get!(RXS)[r as usize];
                        if blk.x() >= core.x() && blk.z() < core.z() && blk.y() >= core.y() && blk.w() < core.w() {
                            let col = get!(RCOL)[r as usize];
                            let mut cv = premul(col.x());
                            if kind == 2u32 {
                                cv = premul(col.y());
                            }
                            a0 = over(a0, cv);
                            a1 = over(a1, cv);
                            a2 = over(a2, cv);
                            a3 = over(a3, cv);
                        } else if xs == 0.0 && kind == 3u32 {
                            // the untransformed glyph, inline (most of every frame's records)
                            let gq = get!(RQ)[r as usize];
                            let gb = get!(RB)[r as usize];
                            let pm = premul(get!(RCOL)[r as usize].x());
                            let ux = par.z() & 65535u32;
                            let uy = par.z() >> 16u32;
                            let gw = gb.z() - gb.x();
                            let gh = gb.w() - gb.y();
                            let mut j = 0u32;
                            while j < 4u32 {
                                let c = c00 + vec2f((j & 1u32) as f32, (j >> 1u32) as f32);
                                let t = c - gb.xy();
                                let mut sh = vec4f(0.0, 0.0, 0.0, 0.0);
                                if c.x() >= gq.x() && c.x() < gq.z() && c.y() >= gq.y() && c.y() < gq.w() && t.x() >= 0.0 && t.y() >= 0.0 && t.x() < gw && t.y() < gh {
                                    sh = pm * atlas_r(min(ux + (t.x() as u32), aw - 1u32), min(uy + (t.y() as u32), ah - 1u32));
                                }
                                if j == 0u32 {
                                    a0 = over(a0, sh);
                                } else if j == 1u32 {
                                    a1 = over(a1, sh);
                                } else if j == 2u32 {
                                    a2 = over(a2, sh);
                                } else {
                                    a3 = over(a3, sh);
                                }
                                j += 1u32;
                            }
                        } else if xs == 0.0 && kind < 3u32 {
                            a0 = over(a0, shade_base(r, c00));
                            a1 = over(a1, shade_base(r, c10));
                            a2 = over(a2, shade_base(r, c01));
                            a3 = over(a3, shade_base(r, c11));
                        } else {
                            a0 = over(a0, shade_x(r, c00));
                            a1 = over(a1, shade_x(r, c10));
                            a2 = over(a2, shade_x(r, c01));
                            a3 = over(a3, shade_x(r, c11));
                        }
                    }
                    w2 += 1u32;
                }
                h0 += 128u32;
            }
            b0 += 256u32;
        }
        let vw = bitcast_f32(hw(H_VW)) as u32;
        let vh = bitcast_f32(hw(H_VH)) as u32;
        let stride = hw(P_STRIDE);
        let p = vec2u(o.x() + 2u32 * lt.x(), o.y() + 2u32 * lt.y());
        let w0 = bgra(a0);
        let w1 = bgra(a1);
        let w2b = bgra(a2);
        let w3 = bgra(a3);
        if p.x() < vw && p.y() < vh {
            surf_store(tgt, p.y() * stride + p.x(), w0);
        }
        if p.x() + 1u32 < vw && p.y() < vh {
            surf_store(tgt, p.y() * stride + p.x() + 1u32, w1);
        }
        if p.x() < vw && p.y() + 1u32 < vh {
            surf_store(tgt, (p.y() + 1u32) * stride + p.x(), w2b);
        }
        if p.x() + 1u32 < vw && p.y() + 1u32 < vh {
            surf_store(tgt, (p.y() + 1u32) * stride + p.x() + 1u32, w3);
        }
        // Certify each band (32 x 4 px: the 32 invocations of two invocation rows): one word iff all 128 pixels
        // hold it. Every invocation leaves its block's word (or a mark that its four differ); the band's first
        // invocation decides and stores the verdict, so every band of a shaded tile is rewritten, never left stale.
        let same = w0 == w1 && w0 == w2b && w0 == w3;
        workgroup_barrier();
        get_mut!(BW)[li as usize] = w0;
        get_mut!(HT)[li as usize] = select(0u32, 1u32, same);
        workgroup_barrier();
        if lt.x() == 0u32 && (lt.y() & 1u32) == 0u32 && tile.x() < hw(H_TILES_X) && tile.y() < hw(H_TILES_Y) {
            let band = lt.y() >> 1u32;
            let first = li;
            let c = get!(BW)[first as usize];
            let mut one = true;
            let mut t = 0u32;
            while t < 32u32 {
                if get!(HT)[(first + t) as usize] == 0u32 || get!(BW)[(first + t) as usize] != c {
                    one = false;
                }
                t += 1u32;
            }
            let at = (tgt & 3u32) * hw(P_CERT_PLANE) + (tile.y() * hw(H_TILES_X) + tile.x()) * 16u32 + 2u32 * band;
            get_mut!(CERT)[at] = select(0u32, c, one);
            get_mut!(CERT)[at + 1u32] = select(0u32, 15u32, one);
        }
    }

    /// Scroll by copy (bough-metal RESULTS §15): one workgroup per 32 x 32 tile of the destination's tile span moves the
    /// tile's pixels inside the destination rect from the source surface at p - d, unless the frame's damaged rows
    /// mark the tile (the tile pass writes it whole). Invocation (x, y) of 32 x 8 is column x of band y (32 x 4 px).
    /// Each band's rows are SKIPped (destination certified the same word), FILLed (32 writes, no read) or MOVEd, by
    /// the certificates of both surfaces, decided by 8 invocations and handed to the tile through workgroup memory.
    #[compute]
    #[workgroup_size(256, 1, 1)]
    pub fn fs_copy(#[builtin(workgroup_id)] wg: Vec3u, #[builtin(local_invocation_index)] li: u32) {
        let lt = vec2u(li & 31u32, li >> 5u32);
        let s = get!(SLOT).x() / 4u32;
        let nrows = get!(POOL)[s + 7u32];
        let tgt = get!(POOL)[s + 6u32] & 3u32;
        let src = get!(POOL)[s + 11u32];
        let dst0 = get!(POOL)[s + 12u32];
        let dst1 = get!(POOL)[s + 13u32];
        let dw = get!(POOL)[s + 14u32];
        let cspan = get!(POOL)[s + 15u32];
        let rows = s + 16u32;
        let i = wg.x();
        let tw = max(cspan >> 16u32, 1u32);
        let tile = vec2u((cspan & 255u32) + i % tw, ((cspan >> 8u32) & 255u32) + i / tw);
        let tx = hw(H_TILES_X);
        let tys = hw(H_TILES_Y);
        // the repair's rows are sorted by tile row: is this tile one of them?
        let mut lo = 0u32;
        let mut hi = nrows;
        while lo < hi {
            let mid = (lo + hi) >> 1u32;
            if get!(POOL)[rows + 8u32 * mid] < tile.y() {
                lo = mid + 1u32;
            } else {
                hi = mid;
            }
        }
        let mut skip = tile.x() >= tx || tile.y() >= tys;
        if lo < nrows && get!(POOL)[rows + 8u32 * lo] == tile.y() && ((get!(POOL)[rows + 8u32 * lo + 4u32 + ((tile.x() >> 5u32) & 3u32)] >> (tile.x() & 31u32)) & 1u32) != 0u32 {
            skip = true;
        }
        if skip {
            return;
        }
        let d = vec2i(((dw << 16u32) as i32) >> 16u32, (dw as i32) >> 16u32);
        let r0 = vec2u(dst0 & 65535u32, dst0 >> 16u32);
        let r1 = vec2u(dst1 & 65535u32, dst1 >> 16u32);
        let org = tile * 32u32;
        let plane = hw(P_CERT_PLANE);
        if lt.y() == 0u32 && lt.x() < 8u32 {
            let band = lt.x();
            let y0 = org.y() + 4u32 * band;
            let xall = org.x() >= r0.x() && org.x() + 32u32 <= r1.x();
            let xany = org.x() < r1.x() && org.x() + 32u32 > r0.x();
            let dc = tgt * plane + (tile.y() * tx + tile.x()) * 16u32 + 2u32 * band;
            let oc = get!(CERT)[dc];
            let om = get!(CERT)[dc + 1u32];
            // the source rows y0 - dy .. + 3 lie in at most two source bands (A: row 0's, B: row 3's) of at most two
            // tile columns (0, 1); a destination row is one word iff its source row is certified in both columns
            // with the same word, and all 32 of its pixels are copied (`xall`)
            let sp = (src & 3u32) * plane;
            let sx = (org.x() as i32) - d.x();
            let sy = (y0 as i32) - d.y();
            let sx0 = min((max(sx, 0i32) as u32) >> 5u32, tx - 1u32);
            let sx1 = min((max(sx + 31i32, 0i32) as u32) >> 5u32, tx - 1u32);
            let ya = min(max(sy, 0i32) as u32, tys * 32u32 - 1u32);
            let yb = min(max(sy + 3i32, 0i32) as u32, tys * 32u32 - 1u32);
            let ia = (ya >> 5u32) * tx;
            let ib = (yb >> 5u32) * tx;
            let ba = (ya >> 2u32) & 7u32;
            let bb = (yb >> 2u32) & 7u32;
            let a0 = sp + (ia + sx0) * 16u32 + 2u32 * ba;
            let a1 = sp + (ia + sx1) * 16u32 + 2u32 * ba;
            let b0 = sp + (ib + sx0) * 16u32 + 2u32 * bb;
            let b1 = sp + (ib + sx1) * 16u32 + 2u32 * bb;
            let ca = get!(CERT)[a0];
            let cb = get!(CERT)[b0];
            let mut ma = get!(CERT)[a0 + 1u32] & get!(CERT)[a1 + 1u32];
            let mut mb = get!(CERT)[b0 + 1u32] & get!(CERT)[b1 + 1u32];
            if !xall || ca != get!(CERT)[a1] {
                ma = 0u32;
            }
            if !xall || cb != get!(CERT)[b1] {
                mb = 0u32;
            }
            let mut cc = 0u32;
            let mut have = false;
            let mut kk = 0u32;
            let mut ww = 0u32;
            let mut j = 0u32;
            while j < 4u32 {
                let y = y0 + j;
                if xany && y >= r0.y() && y < r1.y() {
                    ww = ww | (1u32 << j);
                    let ys = max((y as i32) - d.y(), 0i32) as u32;
                    let in_a = (ys >> 2u32) == (ya >> 2u32);
                    let mm = select(mb, ma, in_a);
                    if ((mm >> (ys & 3u32)) & 1u32) != 0u32 {
                        let cj = select(cb, ca, in_a);
                        if !have {
                            cc = cj;
                            have = true;
                        }
                        if cj == cc {
                            kk = kk | (1u32 << j);
                        }
                    }
                }
                j += 1u32;
            }
            let cn = select(oc, cc, kk != 0u32);
            // rows the destination already holds in word cn
            let keep = select(0u32, om, oc == cn);
            // moved rows lose their bit; rows off the rect keep theirs in the same word
            let mask = kk | (keep & (15u32 - (ww & 15u32)));
            if ww != 0u32 && (src & CERT_FROZEN) == 0u32 {
                get_mut!(CERT)[dc] = cn;
                get_mut!(CERT)[dc + 1u32] = mask;
            }
            get_mut!(TGK)[band as usize] = kk | (keep << 4u32) | (ww << 8u32);
            get_mut!(TGC)[band as usize] = cn;
        }
        workgroup_barrier();
        // this invocation: column lt.x of band lt.y, pixel rows y0 .. y0 + 3
        let kd = get!(TGK)[lt.y() as usize];
        let kf = kd & 15u32;
        let keep = (kd >> 4u32) & 15u32;
        let ww = kd >> 8u32;
        if (ww & (15u32 - (kf & keep))) == 0u32 {
            return;
        }
        let y0 = org.y() + 4u32 * lt.y();
        let fill = get!(TGC)[lt.y() as usize];
        let stride = hw(P_STRIDE);
        let mut j = 0u32;
        while j < 4u32 {
            if ((ww >> j) & 1u32) != 0u32 {
                let px = vec2u(org.x() + lt.x(), y0 + j);
                if ((kf >> j) & 1u32) != 0u32 {
                    if ((keep >> j) & 1u32) == 0u32 {
                        surf_store(tgt, px.y() * stride + px.x(), fill);
                    }
                } else if px.x() >= r0.x() && px.x() < r1.x() {
                    let sxp = ((px.x() as i32) - d.x()) as u32;
                    let syp = ((px.y() as i32) - d.y()) as u32;
                    surf_store(tgt, px.y() * stride + px.x(), surf_load(src & 3u32, syp * stride + sxp));
                }
            }
            j += 1u32;
        }
    }
}
