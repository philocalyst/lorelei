//! What both presenters write into the region besides the façade's columns: the kernels' header (`FHdr`, as Metal's),
//! the port table that replaces Metal's bind table (`P_*` in shaders/fused.rs), and the certificate planes' size.
use bough_frame::region::*;

/// The region header the kernels read (`FHdr` of shader.metal, region offset 0).
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FHdr {
    viewport: [f32; 2],
    tiles_x: u32,
    tiles_y: u32,
    inst_off: u32,
    spaces_off: u32,
    clips_off: u32,
    dir_id_off: u32,
    dir_ink_off: u32,
    n_dir: u32,
    pages_off: u32,
    gx_off: u32,
    gent_off: u32,
    runs_off: u32,
    toks_off: u32,
    tok_stride: u32,
    bin_cap: u32,
    debug: u32,
    masks: [u32; 7],
    xforms_off: u32,
}
const _: () = assert!(size_of::<FHdr>() <= FBIND_OFF);

/// The port table (region offset `FBIND_OFF`, where Metal keeps its bind table): what the portable kernels read in
/// place of resource ids (`P_*` in shaders/fused.rs).
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Port {
    pub atlas_w: u32,
    pub atlas_h: u32,
    /// Surface row stride in words.
    pub stride: u32,
    /// Words of one surface's certificate plane.
    pub cert_plane: u32,
    pub img_w: u32,
    pub img_h: u32,
    /// Words of the smallest surface binding (every surface index is clamped below it).
    pub surf_words: u32,
}

/// Words of a certificate plane for a `w` x `h` surface: per 32 px tile, 8 bands of (word, row mask).
pub fn cert_words(w: usize, h: usize) -> usize {
    w.div_ceil(32) * h.div_ceil(32) * 16
}

/// The views' masks from their committed capacities (0 before the first frame).
pub fn view_masks(cap: &[u32; NGROUPS]) -> [u32; 7] {
    let m = |c: u32| c.saturating_sub(1);
    [m(cap[G_INST]), m(cap[G_PAGE]), m(cap[G_GLYPH]), m(cap[G_RUN]), m(cap[G_SPACE]), m(cap[G_CLIP]), m(cap[G_STRIKE])]
}

/// Write the header and the port table (the GPU is idle).
///
/// # Safety
/// `region` holds `HDR_BYTES` writable bytes no GPU work reads now.
pub unsafe fn write_header(region: *mut u8, l: &Layout, masks: [u32; 7], bin_cap: u32, port: Port) {
    let h = FHdr {
        viewport: [l.w as f32, l.h as f32],
        tiles_x: l.tiles_x(),
        tiles_y: l.tiles_y(),
        inst_off: l.inst_off,
        spaces_off: l.spaces_off,
        clips_off: l.clips_off,
        dir_id_off: l.dir_id_off,
        dir_ink_off: l.dir_ink_off,
        n_dir: 0,
        pages_off: l.pages_off,
        gx_off: l.gx_off,
        gent_off: l.gent_off,
        runs_off: l.runs_off,
        toks_off: l.toks_off,
        tok_stride: l.tok_stride,
        bin_cap,
        debug: 0,
        masks,
        xforms_off: l.xforms_off,
    };
    // SAFETY: the caller's contract; the header and the port table lie inside HDR_BYTES (const-asserted).
    unsafe {
        (region as *mut FHdr).write(h);
        (region.add(FBIND_OFF) as *mut Port).write(port);
    }
}

/// Write the frame's directory length into the header (the GPU is idle).
///
/// # Safety
/// As `write_header`.
pub unsafe fn write_n_dir(region: *mut u8, n_dir: u32) {
    // SAFETY: the caller's contract; `n_dir` is header word 9.
    unsafe { (region as *mut u32).add(9).write(n_dir) };
}

/// The calling platform's VM page.
pub fn vm_page() -> usize {
    // SAFETY: sysconf has no preconditions.
    (unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize).max(4096)
}
