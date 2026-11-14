//! The fused presenter on Vulkan (`ash`; Linux, and macOS through MoltenVK: `BOUGH_VK_LIB=.../libMoltenVK.dylib`): bough-metal's `fused.rs` with the Metal
//! objects replaced by their Vulkan counterparts, the frame contract (region, header, slots) unchanged.
//!
//! * **The region without a copy.** As `Fused::new_views`: the region's head (header and frame slots) and each
//!   GPU-read column's used prefix are the region's own pages, imported with `VK_EXT_external_memory_host` (Metal's
//!   `newBufferWithBytesNoCopy`), each its own storage binding; a column's capacity stays address space and only what
//!   the masks can reach is imported. `ensure_columns` grows a view to the next power of two, as Metal's does.
//! * **The window's buffers are the surfaces.** A surface is the memory the window host gave (a `wl_shm` buffer of the
//!   compositor's memfd, or plain memory offscreen), imported the same way: the tile pass writes the words the
//!   compositor reads, as Metal writes an IOSurface. Bins are device-local; counts, certificates and the atlases are
//!   host-visible (written or read by the CPU only while the GPU is idle, as in Metal).
//! * **One pre-armed stream for every frame.** Frames are submitted ahead, two command buffers (one per frame slot)
//!   recorded once: frame f waits on the timeline semaphore for 2f - 1, runs copy, bin, barrier, tile over slot
//!   `f % 2` with sizes read from the slot (indirect dispatch), and signals 2f. A frame is: write its slot, signal
//!   2f - 1 from the host (`vkSignalSemaphore`; Metal's `signaledValue`): no recording and no queue submission on
//!   the critical path. `sync` is one read of the semaphore's counter when the GPU is done.
//! * **Drains** replace Metal's residency-set drains: anything that changes what the recorded frames bind (a column
//!   view grows, a surface is imported, the bins grow, a resize) first runs the armed frames as empty frames, then
//!   rewrites the descriptors and re-records.
//! * **Pipelines from build-time SPIR-V**, created through a pipeline cache kept on disk (`$XDG_CACHE_HOME/bough`),
//!   with full subgroups required where the device can (the kernels' prefix sums take any size). The device is
//!   built on another thread from process entry (`prepare_in_background`), as Metal's.
use crate::frame::{Port, cert_words, view_masks, vm_page, write_header, write_n_dir};
use bough_frame::region::*;
use bough_vk::{Buf, Device, Import, Needs, Pending, Stream, err, vk};

/// Bindings of set 0 (shaders/fused.rs): region head, 11 columns, bins, counts, 4 surfaces, certificates, atlases.
const NBIND: u32 = 21;
/// The slot uniform's stride (every device's `minUniformBufferOffsetAlignment` is at most 256).
const UBO_STRIDE: u64 = 256;

/// The device of a presenter (bough-vk) and the fused kernels' pipelines, built on another thread from process entry
/// while the main thread mounts (`prepare_in_background`).
pub struct Prep {
    dsl: [vk::DescriptorSetLayout; 2],
    layout: vk::PipelineLayout,
    /// fs_copy, fs_bin, fs_tile.
    pipes: [vk::Pipeline; 3],
    dev: Device,
}

impl std::ops::Deref for Prep {
    type Target = Device;
    fn deref(&self) -> &Device {
        &self.dev
    }
}

impl Prep {
    /// The device (subgroup arithmetic in compute: the kernels' prefix sums) and the pipelines; `Err` when there is no
    /// Vulkan loader or no device that can run the kernels. A software device (lavapipe) is taken only when `software`
    /// allows it (nothing is built for it otherwise: no device, no pipelines).
    pub fn new(software: bool) -> Result<Prep, String> {
        let dev = Device::new(software, Needs { subgroups: true }, "fused")?;
        let device = &dev.device;
        // set 0: 21 storage buffers; set 1: the slot (a dynamic uniform)
        let b0: Vec<vk::DescriptorSetLayoutBinding> = (0..NBIND)
            .map(|b| vk::DescriptorSetLayoutBinding::default().binding(b).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE))
            .collect();
        let b1 = [vk::DescriptorSetLayoutBinding::default().binding(0).descriptor_type(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE)];
        // SAFETY: well-formed create infos on the live device (destroyed by `Prep`'s drop, before the device's).
        let dsl = unsafe {
            [
                device.create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().bindings(&b0), None).map_err(|e| err("set layout", e))?,
                device.create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().bindings(&b1), None).map_err(|e| err("set layout", e))?,
            ]
        };
        // SAFETY: as above.
        let layout = unsafe { device.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&dsl), None) }.map_err(|e| err("pipeline layout", e))?;
        // the kernels' prefix sums take any subgroup size: full subgroups where the device can
        let pipes = dev.pipelines(layout, &[(crate::spirv::FS_COPY, c"fs_copy"), (crate::spirv::FS_BIN, c"fs_bin"), (crate::spirv::FS_TILE, c"fs_tile")], true)?;
        Ok(Prep { dsl, layout, pipes: [pipes[0], pipes[1], pipes[2]], dev })
    }
}

impl Drop for Prep {
    fn drop(&mut self) {
        // SAFETY: the owner waited for the device to be idle; the pipelines and layouts were created here, from the
        // device that drops after this.
        unsafe {
            let _ = self.dev.device.device_wait_idle();
            for &p in &self.pipes {
                self.dev.device.destroy_pipeline(p, None);
            }
            self.dev.device.destroy_pipeline_layout(self.layout, None);
            for &l in &self.dsl {
                self.dev.device.destroy_descriptor_set_layout(l, None);
            }
        }
    }
}

/// Could a hardware Vulkan device exist here (bough-vk)?
pub use bough_vk::hardware_possible;

static PREP: Pending<Prep> = Pending::new();

/// Start building the device and the pipelines on another thread; the next presenter takes them. `software`: a
/// software device may be the one (without it, a machine with no render node starts nothing).
pub fn prepare_in_background(software: bool) {
    if !software && bough_vk::no_hardware() {
        return;
    }
    PREP.start("bough-vk-prep", move || Prep::new(software));
}

/// The prepared device (waiting for its thread), or a new one.
pub fn take_prep(software: bool) -> Result<Prep, String> {
    match PREP.take() {
        Some(r) => r,
        None if !software && bough_vk::no_hardware() => Err("no hardware Vulkan device (no render node, or none found before)".into()),
        None => Prep::new(software),
    }
}

/// Counters.
#[derive(Default, Clone, Copy, Debug)]
pub struct Stats {
    pub gpu_frames: u64,
    pub tiles: u64,
    /// Armed frames run empty because what the frames bind changed.
    pub drains: u64,
    pub view_growths: u32,
    pub bin_growths: u32,
    pub imports: u64,
}

/// One surface: the caller memory it is imported from.
struct Surf {
    imp: Import,
    /// Bytes per row.
    bpr: usize,
}

pub struct Fused {
    #[cfg(target_os = "linux")]
    waiter: Option<bough_vk::Waiter>,
    d: Prep,
    /// The region's head (header and frame slots): a view of the region's first pages.
    head: Import,
    region: *mut u8,
    region_len: usize,
    _keep: Box<dyn std::any::Any>,
    views: [Option<Import>; NCOLS],
    cap: [u32; NGROUPS],
    bins: Buf,
    counts: Buf,
    pub bin_cap: u32,
    cert: Buf,
    atlas: Buf,
    atlas_dims: (u32, u32),
    img: Option<Buf>,
    /// Bound where nothing else is yet (an unbound descriptor is invalid).
    dummy: Buf,
    ubo: Buf,
    surfs: [Option<Surf>; MAX_SURFACES],
    pub w: usize,
    pub h: usize,
    lay: Layout,
    port: Port,
    dpool: vk::DescriptorPool,
    set0: vk::DescriptorSet,
    set1: vk::DescriptorSet,
    cpool: vk::CommandPool,
    /// The two frame command buffers (slot 0, slot 1), recorded once per binding generation.
    frames: [vk::CommandBuffer; 2],
    /// One-shot work: overflow redraws, reference renders.
    aux: vk::CommandBuffer,
    aux_fence: vk::Fence,
    s: Stream,
    batch: u64,
    /// The bindings changed since the frames were recorded.
    stale: bool,
    /// The last GPU frame: (slot, target, rows, tiles), redrawn if its bins overflowed.
    last_frame: Option<(usize, usize, u32, u32)>,
    pub stats: Stats,
}

impl Fused {
    /// A presenter over a region whose columns the GPU sees through views of their used prefixes (bough-metal
    /// `Fused::new_views`): the head view covers the region's first `base` bytes (header and frame slots); each
    /// column's view is made by `ensure_columns`. `surfaces`: the ring's size.
    ///
    /// # Safety
    /// `ptr` is page-aligned, valid for `len` bytes and stays valid while `keep` lives; every column of the layouts
    /// later passed to `set_layout` lies inside the region and starts on a VM page boundary.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn new_views(d: Prep, ptr: *mut u8, len: usize, keep: Box<dyn std::any::Any>, base: usize, w: usize, h: usize, bin_cap: u32) -> Result<Fused, String> {
        let base = base.next_multiple_of(vm_page()).min(len);
        let usage_head = vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::INDIRECT_BUFFER;
        // SAFETY: the caller's contract: the region's first `base` bytes, alive while `keep` is.
        let head = unsafe { d.import(ptr, base, usage_head) }?;
        let ty = h.div_ceil(32);
        let st = vk::BufferUsageFlags::STORAGE_BUFFER;
        let bins = d.buffer(ty as u64 * bin_cap as u64 * 16, st, false)?;
        let counts = d.buffer((ty as u64 + 1) * 4, st, true)?;
        let cert = d.buffer(MAX_SURFACES as u64 * cert_words(w, h) as u64 * 4, st, true)?;
        let atlas = d.buffer(1024 * 512, st, true)?;
        let dummy = d.buffer(256, st, true)?;
        let ubo = d.buffer(3 * UBO_STRIDE, vk::BufferUsageFlags::UNIFORM_BUFFER, true)?;
        let dev = &d.device;
        // SAFETY: well-formed create infos on the live device.
        let (dpool, set0, set1, cpool, frames, aux, aux_fence) = unsafe {
            let sizes = [
                vk::DescriptorPoolSize { ty: vk::DescriptorType::STORAGE_BUFFER, descriptor_count: NBIND },
                vk::DescriptorPoolSize { ty: vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC, descriptor_count: 1 },
            ];
            let dpool = dev.create_descriptor_pool(&vk::DescriptorPoolCreateInfo::default().max_sets(2).pool_sizes(&sizes), None).map_err(|e| err("descriptor pool", e))?;
            let sets = dev.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(dpool).set_layouts(&d.dsl)).map_err(|e| err("descriptor sets", e))?;
            let cpool = dev
                .create_command_pool(&vk::CommandPoolCreateInfo::default().queue_family_index(d.qfam).flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER), None)
                .map_err(|e| err("command pool", e))?;
            let cbs = dev
                .allocate_command_buffers(&vk::CommandBufferAllocateInfo::default().command_pool(cpool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(3))
                .map_err(|e| err("command buffers", e))?;
            let fence = dev.create_fence(&vk::FenceCreateInfo::default(), None).map_err(|e| err("fence", e))?;
            (dpool, sets[0], sets[1], cpool, [cbs[0], cbs[1]], cbs[2], fence)
        };
        let s = Stream::new(&d)?;
        let mut f = Fused {
            #[cfg(target_os = "linux")]
            waiter: None,
            d,
            head,
            region: ptr,
            region_len: len,
            _keep: keep,
            views: Default::default(),
            cap: [0; NGROUPS],
            bins,
            counts,
            bin_cap,
            cert,
            atlas,
            atlas_dims: (1024, 512),
            img: None,
            dummy,
            ubo,
            surfs: Default::default(),
            w,
            h,
            lay: Layout::default(),
            port: Port::default(),
            dpool,
            set0,
            set1,
            cpool,
            frames,
            aux,
            aux_fence,
            s,
            batch: 2,
            stale: true,
            last_frame: None,
            stats: Stats::default(),
        };
        f.port = f.port_now();
        Ok(f)
    }

    pub fn info(&self) -> &str {
        &self.d.info
    }

    pub fn software(&self) -> bool {
        self.d.software
    }

    fn port_now(&self) -> Port {
        let stride = self.surfs.iter().flatten().map(|s| s.bpr / 4).next().unwrap_or(self.w) as u32;
        let words = self.surfs.iter().flatten().map(|s| (s.imp.size / 4) as u32).min().unwrap_or(1).max(1);
        Port {
            atlas_w: self.atlas_dims.0,
            atlas_h: self.atlas_dims.1,
            stride,
            cert_plane: cert_words(self.w, self.h) as u32,
            img_w: if self.img.is_some() { IMG_W } else { 1 },
            img_h: if self.img.is_some() { IMG_H } else { 1 },
            surf_words: words,
        }
    }

    /// Write the kernels' header (offsets, window size, bin capacity, masks) and the port table.
    pub fn set_layout(&mut self, l: Layout) {
        // the armed frames dispatch from the old slots' offsets: run them out with the old layout first
        self.drain();
        self.lay = l;
        self.port = self.port_now();
        // SAFETY: the region starts with HDR_BYTES reserved for the header; the GPU is idle (synced).
        unsafe { write_header(self.region, &l, view_masks(&self.cap), self.bin_cap, self.port) };
        // SAFETY: the slots' offsets name the head view (the region's first `base` bytes).
        let u = self.ubo.ptr.unwrap().as_ptr();
        for (k, &o) in l.slot.iter().enumerate() {
            // SAFETY: the uniform buffer holds 3 strides; host-coherent; the GPU is idle.
            unsafe { (u.add(k * UBO_STRIDE as usize) as *mut [u32; 4]).write([o, 0, 0, 0]) };
        }
        self.stale = true;
    }

    /// Make the views of the region's columns hold at least `need[g]` elements of every column of group `g` (seven
    /// compares in the common case; a growth imports the next power of two, page-rounded, and drains).
    #[inline]
    pub fn ensure_columns(&mut self, need: &[usize; NGROUPS]) {
        if (0..NGROUPS).all(|g| need[g] <= self.cap[g] as usize && self.cap[g] != 0) {
            return;
        }
        self.grow_views(need);
    }

    #[cold]
    #[inline(never)]
    fn grow_views(&mut self, need: &[usize; NGROUPS]) {
        self.drain();
        let page = vm_page().max(self.d.host_align as usize);
        let l = self.lay;
        let mut cap = self.cap;
        let mut changed = [false; NGROUPS];
        for g in 0..NGROUPS {
            // the largest capacity every column of the group can hold (its reservation)
            let limit = l.cols.iter().filter(|c| c.group as usize == g && c.elem > 0).map(|c| (c.reserve / c.elem) as usize).min().unwrap_or(0);
            let want = need[g].max(1).next_power_of_two();
            if cap[g] != 0 && want <= cap[g] as usize {
                continue;
            }
            // at least double, starting from one page's worth
            let first = |c: &ColumnDesc| (page / c.elem.max(1) as usize).max(1);
            let floor = l.cols.iter().filter(|c| c.group as usize == g && c.elem > 0).map(first).min().unwrap_or(1).next_power_of_two() / 2;
            let new = want.max(cap[g] as usize * 2).max(floor.max(1)).min(limit.max(1));
            if new as u32 != cap[g] {
                cap[g] = new as u32;
                changed[g] = true;
            }
        }
        for (k, c) in l.cols.iter().enumerate() {
            let g = c.group as usize;
            if !changed[g] || c.elem == 0 {
                continue;
            }
            let bytes = (cap[g] as usize * c.elem as usize)
                .next_multiple_of(page)
                .min((c.reserve as usize).next_multiple_of(page))
                .min(self.region_len - c.off as usize)
                .min(self.d.max_range as usize & !(page - 1));
            // SAFETY: `[off, off + bytes)` lies inside the region (clamped to its length), on a page boundary (the
            // layout's contract), alive while the presenter holds the region's keep-alive.
            match unsafe { self.d.import(self.region.add(c.off as usize), bytes, vk::BufferUsageFlags::STORAGE_BUFFER) } {
                Ok(v) => {
                    if let Some(o) = self.views[k].replace(v) {
                        self.d.drop_import(o);
                    }
                    self.stats.imports += 1;
                }
                Err(e) => panic!("bough-fused: column {k} view of {bytes} bytes: {e}"),
            }
        }
        self.cap = cap;
        self.stats.view_growths += 1;
        // SAFETY: header field `masks` (words 18..25); the GPU is idle (drained).
        unsafe { (self.region.add(18 * 4) as *mut [u32; 7]).write(view_masks(&cap)) };
        self.stale = true;
    }

    /// Surface `i` is `len` bytes of caller memory at `ptr`, rows of `bpr` bytes (a window buffer, or memory
    /// offscreen). A new address re-imports it (its certificates are cleared: nothing is known of its pixels).
    ///
    /// # Safety
    /// `ptr` is page-aligned and valid for `len` bytes until the next `bind_surface` of `i` with another address,
    /// `resize`, or the presenter's drop; nothing else writes it while a frame drawing it is in flight.
    pub unsafe fn bind_surface(&mut self, i: usize, ptr: *mut u8, len: usize, bpr: usize) {
        if let Some(s) = &self.surfs[i] {
            if s.imp.ptr == ptr && s.imp.size as usize >= len && s.bpr == bpr {
                return;
            }
        }
        self.drain();
        // SAFETY: the caller's contract.
        let imp = unsafe { self.d.import(ptr, len, vk::BufferUsageFlags::STORAGE_BUFFER) }.unwrap_or_else(|e| panic!("bough-fused: surface {i}: {e}"));
        if let Some(o) = self.surfs[i].replace(Surf { imp, bpr }) {
            self.d.drop_import(o.imp);
        }
        self.stats.imports += 1;
        self.forget(i);
        self.port = self.port_now();
        // SAFETY: the port table; the GPU is idle (drained).
        unsafe { (self.region.add(FBIND_OFF) as *mut Port).write(self.port) };
        self.stale = true;
    }

    /// Nothing is known of surface `i`'s pixels any more (something else wrote it): clear its certificates.
    pub fn forget(&mut self, i: usize) {
        self.sync();
        let n = cert_words(self.w, self.h);
        // SAFETY: plane `i` of the host-visible certificate buffer; the GPU is idle.
        unsafe { std::ptr::write_bytes((self.cert.ptr.unwrap().as_ptr() as *mut u32).add(i * n), 0, n) };
    }

    /// Copy the glyph atlas (R8, `width` x `height`) when it changed. Returns whether it was copied.
    pub fn sync_atlas(&mut self, width: u32, height: u32, pixels: &[u8], dirty: bool) -> bool {
        let grown = (width, height) != self.atlas_dims;
        if !dirty && !grown {
            return false;
        }
        self.sync();
        let bytes = (width as u64 * height as u64).next_multiple_of(4);
        if bytes > self.atlas.size {
            self.drain();
            let b = self.d.buffer(bytes, vk::BufferUsageFlags::STORAGE_BUFFER, true).expect("atlas");
            let old = std::mem::replace(&mut self.atlas, b);
            self.d.free(old);
            self.stale = true;
        }
        let n = pixels.len().min(self.atlas.size as usize);
        // SAFETY: the host-visible atlas holds `size` bytes; the GPU is idle.
        unsafe { std::ptr::copy_nonoverlapping(pixels.as_ptr(), self.atlas.ptr.unwrap().as_ptr(), n) };
        if grown {
            self.atlas_dims = (width, height);
            self.port = self.port_now();
            // SAFETY: the port table; the GPU is idle.
            unsafe { (self.region.add(FBIND_OFF) as *mut Port).write(self.port) };
        }
        true
    }

    /// Copy rows `rows` of the image atlas (premultiplied RGBA8, `IMG_W` wide).
    pub fn sync_images(&mut self, pixels: &[u8], rows: std::ops::Range<u32>) {
        self.sync();
        if self.img.is_none() {
            self.drain();
            self.img = Some(self.d.buffer(IMG_W as u64 * IMG_H as u64 * 4, vk::BufferUsageFlags::STORAGE_BUFFER, true).expect("image atlas"));
            self.port = self.port_now();
            // SAFETY: the port table; the GPU is idle.
            unsafe { (self.region.add(FBIND_OFF) as *mut Port).write(self.port) };
            self.stale = true;
        }
        let row = IMG_W as usize * 4;
        let (a, b) = (rows.start as usize * row, (rows.end as usize * row).min(pixels.len()));
        if a < b {
            // SAFETY: the host-visible image atlas holds IMG_W * IMG_H * 4 bytes; the GPU is idle.
            unsafe { std::ptr::copy_nonoverlapping(pixels[a..b].as_ptr(), self.img.as_ref().unwrap().ptr.unwrap().as_ptr().add(a), b - a) };
        }
    }

    fn descriptors(&mut self) {
        let dummy = vk::DescriptorBufferInfo { buffer: self.dummy.buf, offset: 0, range: vk::WHOLE_SIZE };
        let whole = |b: vk::Buffer| vk::DescriptorBufferInfo { buffer: b, offset: 0, range: vk::WHOLE_SIZE };
        let mut infos: Vec<vk::DescriptorBufferInfo> = Vec::with_capacity(NBIND as usize);
        infos.push(whole(self.head.buf));
        for v in &self.views {
            infos.push(v.as_ref().map_or(dummy, |v| whole(v.buf)));
        }
        infos.push(whole(self.bins.buf));
        infos.push(whole(self.counts.buf));
        for s in &self.surfs {
            infos.push(s.as_ref().map_or(dummy, |s| whole(s.imp.buf)));
        }
        infos.push(whole(self.cert.buf));
        infos.push(whole(self.atlas.buf));
        infos.push(self.img.as_ref().map_or(dummy, |b| whole(b.buf)));
        debug_assert_eq!(infos.len(), NBIND as usize);
        let ubo = [vk::DescriptorBufferInfo { buffer: self.ubo.buf, offset: 0, range: 16 }];
        let mut writes: Vec<vk::WriteDescriptorSet> = infos
            .iter()
            .enumerate()
            .map(|(b, i)| vk::WriteDescriptorSet::default().dst_set(self.set0).dst_binding(b as u32).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).buffer_info(std::slice::from_ref(i)))
            .collect();
        writes.push(vk::WriteDescriptorSet::default().dst_set(self.set1).dst_binding(0).descriptor_type(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC).buffer_info(&ubo));
        // SAFETY: no pending work uses the sets (the caller drained).
        unsafe { self.d.device.update_descriptor_sets(&writes, &[]) };
    }

    /// Record the frame of slot `k` into `cb`: copy, bin, barrier, tile, with sizes from the slot (`indirect`) or
    /// the given ones.
    fn record(&self, cb: vk::CommandBuffer, k: usize, sizes: Option<(u32, u32, u32)>) {
        let dev = &self.d.device;
        let slot = self.lay.slot[k] as u64;
        let mb = |src_s, src_a, dst_s, dst_a| (vk::MemoryBarrier::default().src_access_mask(src_a).dst_access_mask(dst_a), src_s, dst_s);
        let barrier = |(m, s, d): (vk::MemoryBarrier, vk::PipelineStageFlags, vk::PipelineStageFlags)| {
            // SAFETY: recording into a command buffer in the recording state.
            unsafe { dev.cmd_pipeline_barrier(cb, s, d, vk::DependencyFlags::empty(), &[m], &[], &[]) };
        };
        let cs = vk::PipelineStageFlags::COMPUTE_SHADER;
        // SAFETY: `cb` is in the initial state, allocated from this presenter's pool; every object bound is alive
        // until the next re-record (drained before any is destroyed).
        unsafe {
            dev.begin_command_buffer(
                cb,
                &vk::CommandBufferBeginInfo::default().flags(if sizes.is_none() { vk::CommandBufferUsageFlags::SIMULTANEOUS_USE } else { vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT }),
            )
            .unwrap();
            // the slot, header and region the host wrote before releasing the frame
            barrier(mb(vk::PipelineStageFlags::HOST, vk::AccessFlags::HOST_WRITE, cs | vk::PipelineStageFlags::DRAW_INDIRECT, vk::AccessFlags::SHADER_READ | vk::AccessFlags::INDIRECT_COMMAND_READ));
            dev.cmd_bind_descriptor_sets(cb, vk::PipelineBindPoint::COMPUTE, self.d.layout, 0, &[self.set0, self.set1], &[(k as u64 * UBO_STRIDE) as u32]);
            // the scroll copy (0 workgroups without one): disjoint from the tiles the repair writes, so no barrier
            dev.cmd_bind_pipeline(cb, vk::PipelineBindPoint::COMPUTE, self.d.pipes[0]);
            match sizes {
                None => dev.cmd_dispatch_indirect(cb, self.head.buf, slot + 32),
                Some((nc, _, _)) => dev.cmd_dispatch(cb, nc, 1, 1),
            }
            dev.cmd_bind_pipeline(cb, vk::PipelineBindPoint::COMPUTE, self.d.pipes[1]);
            match sizes {
                None => dev.cmd_dispatch_indirect(cb, self.head.buf, slot),
                Some((_, nr, _)) => dev.cmd_dispatch(cb, nr, 1, 1),
            }
            barrier(mb(cs, vk::AccessFlags::SHADER_WRITE, cs, vk::AccessFlags::SHADER_READ));
            dev.cmd_bind_pipeline(cb, vk::PipelineBindPoint::COMPUTE, self.d.pipes[2]);
            match sizes {
                None => dev.cmd_dispatch_indirect(cb, self.head.buf, slot + 12),
                Some((_, _, nt)) => dev.cmd_dispatch(cb, nt, 1, 1),
            }
            // the pixels, certificates and counts, for the host (and the compositor) once the frame signals
            barrier(mb(cs, vk::AccessFlags::SHADER_WRITE, vk::PipelineStageFlags::HOST, vk::AccessFlags::HOST_READ));
            dev.end_command_buffer(cb).unwrap();
        }
    }

    /// Re-record the frame command buffers after the bindings changed (the stream is drained).
    fn refresh(&mut self) {
        if !self.stale {
            return;
        }
        debug_assert_eq!(self.s.armed_upto, self.s.released);
        self.descriptors();
        for k in 0..2 {
            // SAFETY: not pending (drained); reset individually (RESET_COMMAND_BUFFER pool).
            unsafe { self.d.device.reset_command_buffer(self.frames[k], vk::CommandBufferResetFlags::empty()) }.unwrap();
            self.record(self.frames[k], k, None);
        }
        self.stale = false;
    }

    /// Submit frames up to `upto`, each waiting for its release (the stream's `arm` over the two slots' buffers).
    fn arm(&mut self, upto: u64) {
        if self.s.armed_upto >= upto {
            return;
        }
        if self.stale {
            self.drain();
            self.refresh();
        }
        let frames = self.frames;
        self.s.arm(&self.d, upto, |f| frames[(f % 2) as usize]);
    }

    /// Wait for the GPU's last frame (the CPU is about to write the region); redraw it if a bin overflowed. When the
    /// GPU is done (the common case) this is one read of the semaphore's counter.
    #[inline]
    pub fn sync(&mut self) {
        self.s.idle(&self.d);
        self.check_bins();
    }

    /// Run every armed frame as an empty one: what the frames bind may then change.
    fn drain(&mut self) {
        self.sync();
        if self.s.armed_upto == self.s.released {
            return;
        }
        self.stats.drains += 1;
        // the slots' dispatch sizes go to zero for the drain (both slots: armed frames alternate), then come back
        let sb = Layout::slot_bytes(self.lay.w, self.lay.h);
        let keep: Vec<Vec<u8>> = (0..2)
            .map(|k| {
                let mut v = vec![0u8; sb];
                // SAFETY: slot k lies inside the head view; the GPU is idle.
                unsafe { std::ptr::copy_nonoverlapping(self.region.add(self.lay.slot[k] as usize), v.as_mut_ptr(), sb) };
                v
            })
            .collect();
        for k in 0..2 {
            // SAFETY: as above: the three dispatches' x sizes (words 0, 3, 8) of slot k.
            unsafe {
                let s = self.region.add(self.lay.slot[k] as usize) as *mut u32;
                *s = 0;
                *s.add(3) = 0;
                *s.add(8) = 0;
            }
        }
        self.s.run_out(&self.d);
        for (k, v) in keep.iter().enumerate() {
            // SAFETY: as above.
            unsafe { std::ptr::copy_nonoverlapping(v.as_ptr(), self.region.add(self.lay.slot[k] as usize), sb) };
        }
    }

    fn check_bins(&mut self) {
        let ty = self.lay.tiles_y() as usize;
        // SAFETY: `counts` holds ty + 1 u32s in host-coherent memory; the GPU is idle (the caller waited).
        let need = unsafe { *(self.counts.ptr.unwrap().as_ptr() as *const u32).add(ty) };
        if need == 0 {
            return;
        }
        // SAFETY: as above.
        unsafe { *(self.counts.ptr.unwrap().as_ptr() as *mut u32).add(ty) = 0 };
        let Some((k, _target, nr, nt)) = self.last_frame else { return };
        let cap = need.next_power_of_two().max(self.bin_cap * 2);
        self.replace_bins(cap);
        self.stats.bin_growths += 1;
        // the overflowed frame again, from its slot (which names the target), before anyone sees it
        let nc = {
            // SAFETY: slot k's copy size (word 8); the GPU is idle.
            unsafe { *(self.region.add(self.lay.slot[k] as usize) as *const u32).add(8) }
        };
        self.run_aux(k, (nc, nr, nt));
        self.check_bins();
    }

    /// New row bins of `cap` entries per row (drains).
    fn replace_bins(&mut self, cap: u32) {
        self.drain();
        let ty = self.lay.tiles_y().max(1) as u64;
        let b = self.d.buffer(ty * cap as u64 * 16, vk::BufferUsageFlags::STORAGE_BUFFER, false).expect("bins");
        let old = std::mem::replace(&mut self.bins, b);
        self.d.free(old);
        self.bin_cap = cap;
        // SAFETY: header word 16 (`bin_cap`); the GPU is idle.
        unsafe { (self.region as *mut u32).add(16).write(cap) };
        self.stale = true;
    }

    /// Replace the row bins by ones of `cap` entries per row (tests: forces the grow-and-redraw path).
    pub fn reset_bins(&mut self, cap: u32) {
        self.replace_bins(cap);
    }

    /// One frame of slot `k` with the given dispatch sizes, outside the stream, waited for.
    fn run_aux(&mut self, k: usize, sizes: (u32, u32, u32)) {
        self.drain();
        self.refresh();
        let dev = &self.d.device;
        // SAFETY: the aux command buffer is not pending (waited below each time); the stream is drained.
        unsafe { dev.reset_command_buffer(self.aux, vk::CommandBufferResetFlags::empty()) }.unwrap();
        self.record(self.aux, k, Some(sizes));
        let cbs = [self.aux];
        // SAFETY: as above.
        unsafe {
            dev.queue_submit(self.d.queue, &[vk::SubmitInfo::default().command_buffers(&cbs)], self.aux_fence).expect("vkQueueSubmit");
            dev.wait_for_fences(&[self.aux_fence], true, 5_000_000_000).expect("aux frame");
            dev.reset_fences(&[self.aux_fence]).unwrap();
        }
    }

    /// Draw surface `target`: copy the OR-ed tile rows into the next frame slot (and the scroll copy, if any) and
    /// release the pre-armed frame. Returns the tiles repaired.
    pub fn draw(&mut self, target: usize, rows: &[u128], full: bool, n_dir: u32, copy: &SlotCopy) -> u32 {
        self.sync();
        if self.stale {
            // what the frames bind changed (a surface, a view, the bins): the armed frames run out empty, then the
            // frames are recorded again, before this frame's slot is written
            self.drain();
            self.refresh();
        }
        let f = self.s.released + 1;
        let k = (f % 2) as usize;
        // SAFETY: the region is laid out by `lay` (slots in the head view); the GPU is idle (synced).
        let (nt, nr) = unsafe {
            write_n_dir(self.region, n_dir);
            write_slot(self.region, &self.lay, k, target as u32, rows, full, copy)
        };
        self.last_frame = Some((k, target, nr, nt));
        // normally armed already; the release first, then the next frames' submission while the GPU draws this one
        self.arm(f);
        self.s.release(&self.d);
        self.arm(f + self.batch);
        self.stats.gpu_frames += 1;
        self.stats.tiles += nt as u64;
        nt
    }

    /// The fd that becomes readable when a frame `notify_drawn` named is drawn (Linux): poll it with the window's
    /// connection, then present (the eventfd is drained by `drawn`).
    #[cfg(target_os = "linux")]
    pub fn completion_fd(&mut self) -> std::os::fd::RawFd {
        let (device, tl) = (self.d.device.clone(), self.s.tl);
        // SAFETY: the stream's semaphore, destroyed only after the waiter is dropped (`Drop`); only released frames'
        // values are sent.
        let w = self.waiter.get_or_insert_with(|| unsafe { bough_vk::Waiter::new(device, tl) });
        w.fd()
    }

    /// Have `completion_fd` become readable once the last released frame is drawn.
    #[cfg(target_os = "linux")]
    pub fn notify_drawn(&mut self) {
        self.completion_fd();
        if let Some(w) = &self.waiter {
            w.notify(2 * self.s.released);
        }
    }

    /// The last released frame is drawn (one read of the semaphore's counter); drains the completion fd.
    pub fn drawn(&mut self) -> bool {
        #[cfg(target_os = "linux")]
        if let Some(w) = &self.waiter {
            w.drain();
        }
        self.s.drawn(&self.d)
    }

    /// Frames armed per arming (the stream's depth).
    pub fn set_batch(&mut self, k: usize) {
        self.batch = k.clamp(1, 16) as u64;
    }

    /// The window is now `w` x `h` px: bins, counts and certificates for its tiles; the surfaces are the caller's
    /// (it binds the new ones before the next frame). The caller then writes the new layout and draws a full frame.
    pub fn resize(&mut self, w: usize, h: usize) {
        if (w, h) == (self.w, self.h) {
            return;
        }
        self.drain();
        let ty = h.div_ceil(32) as u64;
        let st = vk::BufferUsageFlags::STORAGE_BUFFER;
        let bins = self.d.buffer(ty * self.bin_cap as u64 * 16, st, false).expect("bins");
        let counts = self.d.buffer((ty + 1) * 4, st, true).expect("counts");
        let cert = self.d.buffer(MAX_SURFACES as u64 * cert_words(w, h) as u64 * 4, st, true).expect("certificates");
        for (o, n) in [(&mut self.bins, bins), (&mut self.counts, counts), (&mut self.cert, cert)] {
            let old = std::mem::replace(o, n);
            // SAFETY: drained above: no pending work uses the old buffers.
            unsafe {
                self.d.device.destroy_buffer(old.buf, None);
                self.d.device.free_memory(old.mem, None);
            }
        }
        for s in self.surfs.iter_mut() {
            if let Some(s) = s.take() {
                self.d.drop_import(s.imp);
            }
        }
        (self.w, self.h) = (w, h);
        self.last_frame = None;
        self.stale = true;
    }

    /// A from-scratch full redraw of the current state into fresh memory (the same kernels): its BGRA bytes.
    pub fn reference_pixels(&mut self, n_dir: u32) -> Vec<u8> {
        self.drain();
        let (w, h) = (self.w, self.h);
        let bpr = self.port.stride as usize * 4;
        let len = (bpr * h).next_multiple_of(vm_page().max(self.d.host_align as usize));
        let layout = std::alloc::Layout::from_size_align(len, vm_page().max(self.d.host_align as usize)).unwrap();
        // SAFETY: a non-zero size; freed below with the same layout after the GPU is done with it.
        let mem = unsafe { std::alloc::alloc_zeroed(layout) };
        assert!(!mem.is_null());
        // the scratch takes surface 3's binding and certificate plane for one frame; both come back after
        let saved = self.surfs[3].take();
        let n = cert_words(w, h);
        let cert3: Vec<u32> = {
            // SAFETY: plane 3 of the host-visible certificate buffer; the GPU is idle (drained).
            unsafe { std::slice::from_raw_parts((self.cert.ptr.unwrap().as_ptr() as *const u32).add(3 * n), n).to_vec() }
        };
        // SAFETY: `mem` is page aligned and `len` bytes, alive until the import is dropped below.
        let imp = unsafe { self.d.import(mem, len, vk::BufferUsageFlags::STORAGE_BUFFER) }.expect("reference surface");
        self.surfs[3] = Some(Surf { imp, bpr });
        let port = self.port;
        self.port = self.port_now();
        // SAFETY: the port table and frame slot 2; the GPU is idle.
        let (nt, nr) = unsafe {
            (self.region.add(FBIND_OFF) as *mut Port).write(self.port);
            write_n_dir(self.region, n_dir);
            write_slot(self.region, &self.lay, 2, 3, &[], true, &SlotCopy::NONE)
        };
        self.stale = true;
        // a reference whose bins overflowed is no reference: grow them and draw it again (the presented frames grow
        // theirs in `check_bins`)
        let ty = self.lay.tiles_y() as usize;
        loop {
            self.run_aux(2, (0, nr, nt));
            // SAFETY: `counts` holds ty + 1 u32s in host-coherent memory; the GPU is idle (waited in run_aux).
            let need = unsafe { std::mem::replace(&mut *(self.counts.ptr.unwrap().as_ptr() as *mut u32).add(ty), 0) };
            if need == 0 {
                break;
            }
            self.replace_bins(need.next_power_of_two().max(self.bin_cap * 2));
            self.stats.bin_growths += 1;
        }
        let mut out = vec![0u8; w * h * 4];
        for y in 0..h {
            // SAFETY: the scratch holds `h` rows of `bpr >= 4 w` bytes; the GPU is done (waited in run_aux).
            unsafe { std::ptr::copy_nonoverlapping(mem.add(y * bpr), out.as_mut_ptr().add(y * w * 4), w * 4) };
        }
        let s = self.surfs[3].take().unwrap();
        self.d.drop_import(s.imp);
        self.surfs[3] = saved;
        self.port = port;
        // SAFETY: as above; the GPU is idle.
        unsafe {
            (self.region.add(FBIND_OFF) as *mut Port).write(self.port);
            std::ptr::copy_nonoverlapping(cert3.as_ptr(), (self.cert.ptr.unwrap().as_ptr() as *mut u32).add(3 * n), n);
            std::alloc::dealloc(mem, layout);
        }
        self.stale = true;
        out
    }

    /// Bytes of GPU memory this presenter allocated itself (bins, counts, certificates, atlases): the region and the
    /// surfaces are the caller's.
    pub fn own_bytes(&self) -> u64 {
        self.bins.size + self.counts.size + self.cert.size + self.atlas.size + self.img.as_ref().map_or(0, |b| b.size) + self.dummy.size + self.ubo.size
    }

    /// Bins overflowed in the last frame (it was redrawn).
    pub fn bin_growths(&self) -> u32 {
        self.stats.bin_growths
    }
}

impl Drop for Fused {
    fn drop(&mut self) {
        self.drain();
        // the waiter's thread waits on the semaphore: joined before the semaphore goes
        #[cfg(target_os = "linux")]
        drop(self.waiter.take());
        // SAFETY: drained: nothing is pending; every object below was created by this presenter.
        unsafe {
            let _ = self.d.device.device_wait_idle();
            let dev = &self.d.device;
            self.s.destroy(&self.d);
            dev.destroy_fence(self.aux_fence, None);
            dev.destroy_command_pool(self.cpool, None);
            dev.destroy_descriptor_pool(self.dpool, None);
        }
        for b in [&mut self.bins, &mut self.counts, &mut self.cert, &mut self.atlas, &mut self.dummy, &mut self.ubo] {
            let b = std::mem::replace(b, Buf::NONE);
            self.d.free(b);
        }
        if let Some(b) = self.img.take() {
            self.d.free(b);
        }
        for v in self.views.iter_mut() {
            if let Some(v) = v.take() {
                self.d.drop_import(v);
            }
        }
        for s in self.surfs.iter_mut() {
            if let Some(s) = s.take() {
                self.d.drop_import(s.imp);
            }
        }
        let head = std::mem::replace(&mut self.head, Import::NONE);
        self.d.drop_import(head);
    }
}
