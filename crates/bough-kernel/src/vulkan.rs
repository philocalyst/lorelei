//! The GPU executor on Vulkan (Linux; macOS through MoltenVK, for comparisons with Metal on the same GPU): naga's
//! SPIR-V of `kernel.rs` (build.rs), one workgroup per scheduled tile, on bough-vk's device layer (the one bough-fused's
//! presenter runs on). The same API as `metal.rs`, so the exactness gate and the app's backend drive either; the bytes
//! are the CPU executors' by construction, so the backend switches executors per frame with no seam.
//!
//! * **Buffers**: commands, programs, the atlas and the image atlas are host-visible, coherent and persistently mapped
//!   (device-local too where the device has such memory: unified memory, resizable BAR): the CPU writes them in place.
//! * **The window's words** (`wrap`): the output is the caller's memory imported as device memory
//!   (`VK_EXT_external_memory_host`): the kernel writes the pixels of the `wl_shm` buffer the compositor reads, with no
//!   copy, as the Metal executor writes an IOSurface's base address.
//! * **One pre-armed frame**: the frame's command buffer is recorded once per set of targets: an indirect dispatch for
//!   every target (and the executor's own output), each reading its workgroup count from a mapped arguments buffer. The
//!   next frame is submitted while the GPU draws this one, waiting on a timeline semaphore (bough-vk's `Stream`); a
//!   frame is: write the commands and the program, set the chosen target's count (the others' are zero), one host
//!   signal, one wait. No recording, no descriptor update and no queue submission on the frame's path.
//! * **Measured costs**: the device's round trip and its time per tile are measured when it is built (`calibrate`:
//!   empty frames, then a probe scene of list rows), for the backend's choice between this executor and the CPU's; the
//!   backend corrects them from the frames it runs. A software device (lavapipe) measures as slow as it is.
//! * **Built on another thread** from process entry (`prepare_in_background`), never waited for by a frame the CPU
//!   can draw (`poll_prepared`).
use crate::cpu::{Costs, Prog};
use crate::stage::Scene;
use bough_vk::{Buf, Device, Import, Needs, Pending, Stream, err, vk};

pub use bough_vk::{no_hardware, spirv_words};

/// Targets bound at once (a window's ring is at most 4; a resize forgets them).
const MAX_TARGETS: usize = 8;
/// The own output's descriptor set and dispatch (after the targets').
const OWN: usize = MAX_TARGETS;
/// Bytes per indirect dispatch's arguments (x, y, z, padding).
const ARGS: u64 = 16;

/// Times of one GPU frame, in seconds (wall only: release to completion as the calling thread sees it).
#[derive(Clone, Copy, Debug, Default)]
pub struct GpuTime {
    pub wall: f64,
    pub gpu: f64,
}

/// A target this executor wrapped (`Gpu::wrap`): which of its bound targets, and the memory, so a target evicted for
/// another is imported again when it is drawn.
pub struct Target {
    owner: std::sync::Arc<()>,
    slot: usize,
    epoch: u64,
    ptr: *mut u8,
    len: usize,
}

struct Bound {
    imp: Import,
    epoch: u64,
    used: u64,
}

pub struct Gpu {
    atlas_bytes: usize,
    image_bytes: usize,
    admitted_scene: Scene,
    admitted_prog: Option<Prog>,
    owner: std::sync::Arc<()>,
    /// The device, its driver and API version.
    pub info: String,
    /// A software device (lavapipe).
    pub software: bool,
    costs: Costs,
    dsl: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    pipe: vk::Pipeline,
    /// 0 commands, 1 programs, 2 atlas, 3 the own output, 4 image atlas.
    bufs: [Option<Buf>; 5],
    /// Indirect arguments, one per target and one for the own output.
    args: Buf,
    targets: [Option<Bound>; MAX_TARGETS],
    epoch: u64,
    dpool: vk::DescriptorPool,
    sets: Vec<vk::DescriptorSet>,
    cpool: vk::CommandPool,
    cb: vk::CommandBuffer,
    s: Stream,
    /// The bindings changed since the frame was recorded.
    stale: bool,
    frames: u64,
    d: Device,
}

// SAFETY: every Vulkan object here belongs to `d`, which moves with it; the mapped pointers are this executor's own
// memory. A `Gpu` is built on one thread and then used by one thread at a time (`&mut self` for all GPU work).
unsafe impl Send for Gpu {}

impl Gpu {
    fn validate_dispatch(&self, tiles: usize, out_words: usize) {
        let p = self
            .admitted_prog
            .as_ref()
            .expect("upload a program before rendering");
        assert!(
            (p.words[6] as usize) < self.atlas_bytes.max(1),
            "glyph atlas bounds"
        );
        assert!(
            (p.words[8] as usize) < (self.image_bytes / 4).max(1),
            "image atlas bounds"
        );
        assert!(tiles <= p.tiles(), "invalid dispatch size");
        assert!(
            out_words >= p.validate(&self.admitted_scene),
            "short output target"
        );
    }

    /// The kernel's pipeline from `spirv` (normally `crate::KERNEL_SPIRV`; the gates pass mutated words) on the best
    /// Vulkan device, a software one included; `None` without one.
    pub fn new(spirv: &[u8]) -> Option<Gpu> {
        Gpu::build(spirv, true).ok()
    }

    /// As `new`; a software device only when `software` allows it.
    pub fn build(spirv: &[u8], software: bool) -> Result<Gpu, String> {
        let d = Device::new(software, Needs::default(), "kernel")?;
        let dev = &d.device;
        let b: Vec<vk::DescriptorSetLayoutBinding> = (0..5)
            .map(|b| {
                vk::DescriptorSetLayoutBinding::default()
                    .binding(b)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::COMPUTE)
            })
            .collect();
        // SAFETY (this block): well-formed create infos on the live device; each object is destroyed by `Drop`, before
        // the device.
        let (dsl, layout, dpool, sets, cpool, cb) = unsafe {
            let dsl = dev
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&b),
                    None,
                )
                .map_err(|e| err("set layout", e))?;
            let layout = dev
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .set_layouts(std::slice::from_ref(&dsl)),
                    None,
                )
                .map_err(|e| err("pipeline layout", e))?;
            let n = (MAX_TARGETS + 1) as u32;
            let sizes = [vk::DescriptorPoolSize {
                ty: vk::DescriptorType::STORAGE_BUFFER,
                descriptor_count: 5 * n,
            }];
            let dpool = dev
                .create_descriptor_pool(
                    &vk::DescriptorPoolCreateInfo::default()
                        .max_sets(n)
                        .pool_sizes(&sizes),
                    None,
                )
                .map_err(|e| err("descriptor pool", e))?;
            let layouts = vec![dsl; n as usize];
            let sets = dev
                .allocate_descriptor_sets(
                    &vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(dpool)
                        .set_layouts(&layouts),
                )
                .map_err(|e| err("descriptor sets", e))?;
            let cpool = dev
                .create_command_pool(
                    &vk::CommandPoolCreateInfo::default()
                        .queue_family_index(d.qfam)
                        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                    None,
                )
                .map_err(|e| err("command pool", e))?;
            let cb = dev
                .allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::default()
                        .command_pool(cpool)
                        .level(vk::CommandBufferLevel::PRIMARY)
                        .command_buffer_count(1),
                )
                .map_err(|e| err("command buffer", e))?[0];
            (dsl, layout, dpool, sets, cpool, cb)
        };
        let pipe = d.pipelines(layout, &[(spirv, c"fs_ktile")], false)?[0];
        let args = d.buffer(
            (MAX_TARGETS as u64 + 1) * ARGS,
            vk::BufferUsageFlags::INDIRECT_BUFFER | vk::BufferUsageFlags::STORAGE_BUFFER,
            true,
        )?;
        let s = Stream::new(&d)?;
        let mut g = Gpu {
            info: d.info.clone(),
            software: d.software,
            costs: crate::cpu::COSTS,
            dsl,
            layout,
            pipe,
            atlas_bytes: 0,
            image_bytes: 0,
            admitted_scene: Scene::default(),
            admitted_prog: None,
            owner: std::sync::Arc::new(()),
            bufs: [None, None, None, None, None],
            args,
            targets: Default::default(),
            epoch: 0,
            dpool,
            sets,
            cpool,
            cb,
            s,
            stale: true,
            frames: 0,
            d,
        };
        for i in [0, 1, 2, 4] {
            g.buf(i, 16);
        }
        g.zero_args();
        Ok(g)
    }

    /// The measured constants of the backend's choice (`calibrate`; Apple's until then).
    pub fn costs(&self) -> &Costs {
        &self.costs
    }
    pub fn costs_mut(&mut self) -> &mut Costs {
        &mut self.costs
    }

    /// Measure this device: its round trip (the median of eight empty frames) and its time per unit of work, from a
    /// probe of list rows with text (`scenes::list`, 512 x 512: 256 tiles), held to the CPU-calibrated ratio of time
    /// per tile to time per entry. A frame after the GPU idled is taken as four round trips until the backend has
    /// seen one.
    pub fn calibrate(&mut self) {
        let mut v: Vec<f64> = (0..8).map(|_| self.frame(OWN, 0).wall).collect();
        v.sort_by(f64::total_cmp);
        let fixed = (v[3] * 1e9) as u64;
        let (fw, fh) = (512u32, 512u32);
        let scene = crate::scenes::list(fw, fh, 48, true, None);
        let (apx, ipx) = (crate::scenes::atlas(), crate::scenes::images());
        let atlas = crate::cpu::Atlas::new(&apx, crate::scenes::ATLAS_W)
            .with_images(&ipx, crate::scenes::IMG_W);
        let mut prog = Prog::default();
        crate::cpu::Binner::default().bin(
            &scene,
            fw,
            fh,
            fw,
            &atlas,
            0,
            &crate::cpu::Damage::All,
            true,
            &mut prog,
        );
        self.upload(&scene, &atlas, &prog);
        let (tiles, entries) = (prog.tiles() as u64, prog.entries() as u64);
        let mut w: Vec<f64> = (0..3)
            .map(|_| self.render(tiles as usize, (fw * fh) as usize).wall)
            .collect();
        w.sort_by(f64::total_cmp);
        let wall = (w[1] * 1e9) as u64;
        let c = crate::cpu::COSTS;
        let unit = c.gpu_tile * tiles + c.gpu_entry * entries;
        let work = wall.saturating_sub(fixed).max(1);
        self.costs = Costs {
            gpu_fixed_warm: fixed,
            gpu_fixed_cold: fixed * 4,
            gpu_tile: (c.gpu_tile * work).div_ceil(unit),
            gpu_entry: (c.gpu_entry * work).div_ceil(unit),
            wake: c.wake,
        };
        // the probe's buffers go back to the frame's sizes as frames need them; its output is dropped
        self.drop_own_out();
    }

    /// Buffer `i`, at least `bytes` long (grown to the next power of two; contents not kept). A growth runs the armed
    /// frame out first (it binds the old buffer).
    fn buf(&mut self, i: usize, bytes: usize) -> &Buf {
        let need = (bytes.max(16) as u64).next_multiple_of(4);
        if self.bufs[i].as_ref().is_none_or(|b| b.size < need) {
            self.run_out();
            let cap = need.next_power_of_two();
            let b = self
                .d
                .buffer(cap, vk::BufferUsageFlags::STORAGE_BUFFER, true)
                .expect("bough-kernel: a Vulkan buffer");
            if let Some(old) = self.bufs[i].replace(b) {
                self.d.free(old);
            }
            self.stale = true;
        }
        self.bufs[i].as_ref().unwrap()
    }
    fn put<T: Copy>(&mut self, i: usize, data: &[T]) {
        let n = std::mem::size_of_val(data);
        let p = self.buf(i, n).host();
        // SAFETY: a coherent mapping of at least `n` bytes; the GPU is idle (every frame is waited for) and the armed
        // frame reads nothing before its release.
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr() as *const u8, p, n) };
    }

    /// Upload the scene, both atlases and the program.
    pub fn upload(&mut self, scene: &Scene, atlas: &crate::cpu::Atlas, prog: &Prog) {
        prog.validate(scene);
        self.admitted_scene = scene.clone();
        self.admitted_prog = Some(prog.clone());
        self.put(0, &scene.words);
        self.put(1, &prog.words);
        self.upload_atlas(atlas.px);
        self.upload_images(atlas.img);
    }
    /// Rewrite only the program.
    pub fn upload_prog(&mut self, prog: &Prog) {
        prog.validate(&self.admitted_scene);
        self.admitted_prog = Some(prog.clone());
        self.put(1, &prog.words);
    }
    /// This frame's commands and program only.
    pub fn upload_frame(&mut self, scene: &Scene, prog: &Prog) {
        prog.validate(scene);
        self.admitted_scene = scene.clone();
        self.admitted_prog = Some(prog.clone());
        self.put(0, &scene.words);
        self.put(1, &prog.words);
    }
    pub fn upload_atlas(&mut self, atlas: &[u8]) {
        self.atlas_bytes = atlas.len();
        if atlas.is_empty() {
            self.put(2, &[0u32]);
        } else {
            self.put(2, atlas);
        }
    }
    /// The image atlas (premultiplied RGBA8 bytes).
    pub fn upload_images(&mut self, img: &[u8]) {
        self.image_bytes = img.len();
        if img.len() < 4 {
            self.put(4, &[0u32]);
        } else {
            self.put(4, img);
        }
    }

    /// The caller's memory as an output (the window's `wl_shm` pages), imported as device memory: the kernel writes
    /// the words the compositor reads. `None` when `ptr` or `len` is not a multiple of the device's import alignment
    /// or the device refuses the import.
    ///
    /// # Safety
    /// `ptr..ptr + len` stays mapped and writable until `forget_targets` or the executor's drop (an evicted target is
    /// imported again from the same memory when it is drawn), and nothing else writes it while a frame drawing it is
    /// in flight (`render_to` waits for completion).
    pub unsafe fn wrap(&mut self, ptr: *mut u8, len: usize) -> Option<Target> {
        let a = self.d.host_align as usize;
        if !(ptr as usize).is_multiple_of(a) || !len.is_multiple_of(a) || len == 0 {
            return None;
        }
        if let Some(slot) = (0..MAX_TARGETS).find(|&j| {
            self.targets[j]
                .as_ref()
                .is_some_and(|b| b.imp.ptr == ptr && b.imp.size as usize == len)
        }) {
            let epoch = self.targets[slot].as_ref().unwrap().epoch;
            return Some(Target {
                owner: self.owner.clone(),
                slot,
                epoch,
                ptr,
                len,
            });
        }
        // SAFETY: the caller's contract.
        unsafe { self.bind(ptr, len) }.map(|(slot, epoch)| Target {
            owner: self.owner.clone(),
            slot,
            epoch,
            ptr,
            len,
        })
    }

    /// Import `ptr` into a free slot, or the least recently drawn one's (runs the armed frame out first).
    unsafe fn bind(&mut self, ptr: *mut u8, len: usize) -> Option<(usize, u64)> {
        let slot = (0..MAX_TARGETS)
            .find(|&j| self.targets[j].is_none())
            .unwrap_or_else(|| {
                (0..MAX_TARGETS)
                    .min_by_key(|&j| self.targets[j].as_ref().map_or(0, |b| b.used))
                    .unwrap()
            });
        // SAFETY: the caller's contract.
        let imp = unsafe {
            self.d
                .import(ptr, len, vk::BufferUsageFlags::STORAGE_BUFFER)
        }
        .ok()?;
        self.run_out();
        if let Some(old) = self.targets[slot].take() {
            self.d.drop_import(old.imp);
        }
        self.epoch += 1;
        self.targets[slot] = Some(Bound {
            imp,
            epoch: self.epoch,
            used: self.frames,
        });
        self.stale = true;
        Some((slot, self.epoch))
    }

    /// Let go of every wrapped target (their memory is about to change hands: a resize).
    pub fn forget_targets(&mut self) {
        self.owner = std::sync::Arc::new(());
        self.run_out();
        for t in self.targets.iter_mut() {
            if let Some(b) = t.take() {
                self.d.drop_import(b.imp);
            }
        }
        self.stale = true;
    }

    /// Run the uploaded program with `target` as the output (its stride is the program's).
    pub fn render_to(&mut self, tiles: usize, target: &Target) -> GpuTime {
        assert!(
            std::sync::Arc::ptr_eq(&self.owner, &target.owner),
            "foreign target"
        );
        self.validate_dispatch(tiles, target.len / 4);
        let live = self.targets[target.slot]
            .as_ref()
            .is_some_and(|b| b.epoch == target.epoch);
        let slot = if live {
            target.slot
        } else {
            // SAFETY: `wrap`'s contract: the memory stays valid until `forget_targets` (after which no target of
            // before is drawn: the backend forgets its handles with them).
            unsafe { self.bind(target.ptr, target.len) }
                .expect("bough-kernel: re-importing an evicted target")
                .0
        };
        self.targets[slot].as_mut().unwrap().used = self.frames;
        self.frame(slot, tiles as u32)
    }

    /// Run the uploaded program: `tiles` workgroups into the own output buffer of `out_words` words.
    pub fn render(&mut self, tiles: usize, out_words: usize) -> GpuTime {
        self.validate_dispatch(tiles, out_words);
        self.buf(3, out_words.checked_mul(4).expect("byte count overflow"));
        self.frame(OWN, tiles as u32)
    }

    fn zero_args(&mut self) {
        // SAFETY: the host-visible arguments buffer holds MAX_TARGETS + 1 entries; the GPU reads it only after a
        // release (the caller's frame is the only writer).
        unsafe {
            let a = self.args.host() as *mut [u32; 4];
            for j in 0..=MAX_TARGETS {
                a.add(j).write([0, 1, 1, 0]);
            }
        }
    }

    /// Run the armed frame (if any) with every count zero: what the frame binds may then change.
    fn run_out(&mut self) {
        if self.s.armed_upto == self.s.released {
            self.s.idle(&self.d);
            return;
        }
        self.zero_args();
        self.s.run_out(&self.d);
    }

    fn descriptors(&mut self) {
        let whole = |b: vk::Buffer| vk::DescriptorBufferInfo {
            buffer: b,
            offset: 0,
            range: vk::WHOLE_SIZE,
        };
        let common: Vec<vk::DescriptorBufferInfo> = [0, 1, 2, 3, 4]
            .iter()
            .map(|&i| whole(self.bufs[i].as_ref().map_or(self.args.buf, |b| b.buf)))
            .collect();
        let mut infos: Vec<[vk::DescriptorBufferInfo; 5]> = Vec::new();
        for j in 0..=MAX_TARGETS {
            let out = if j == OWN {
                self.bufs[3].as_ref().map(|b| b.buf)
            } else {
                self.targets[j].as_ref().map(|b| b.imp.buf)
            };
            let mut i = [common[0], common[1], common[2], common[3], common[4]];
            // an unbound slot's set names the own output (or any buffer): its dispatch is never non-empty
            i[3] = whole(out.unwrap_or(common[3].buffer));
            infos.push(i);
        }
        let writes: Vec<vk::WriteDescriptorSet> = infos
            .iter()
            .enumerate()
            .flat_map(|(j, i)| (0..5).map(move |b| (j, b, i)))
            .map(|(j, b, i)| {
                vk::WriteDescriptorSet::default()
                    .dst_set(self.sets[j])
                    .dst_binding(b as u32)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(std::slice::from_ref(&i[b]))
            })
            .collect();
        // SAFETY: no pending work uses the sets (the caller ran the stream out).
        unsafe { self.d.device.update_descriptor_sets(&writes, &[]) };
    }

    /// Record the frame: the host's writes made visible, one indirect dispatch per bound target and the own output,
    /// the shader's writes made visible to the host.
    fn record(&self) {
        let dev = &self.d.device;
        let cs = vk::PipelineStageFlags::COMPUTE_SHADER;
        let host_in = vk::MemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::HOST_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ | vk::AccessFlags::INDIRECT_COMMAND_READ);
        let host_out = vk::MemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::SHADER_WRITE)
            .dst_access_mask(vk::AccessFlags::HOST_READ);
        // SAFETY: the command buffer is not pending (the stream ran out); every object it names lives until the next
        // re-record or the executor's drop, each preceded by a run-out.
        unsafe {
            dev.reset_command_buffer(self.cb, vk::CommandBufferResetFlags::empty())
                .unwrap();
            dev.begin_command_buffer(
                self.cb,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::SIMULTANEOUS_USE),
            )
            .unwrap();
            dev.cmd_pipeline_barrier(
                self.cb,
                vk::PipelineStageFlags::HOST,
                cs | vk::PipelineStageFlags::DRAW_INDIRECT,
                vk::DependencyFlags::empty(),
                &[host_in],
                &[],
                &[],
            );
            dev.cmd_bind_pipeline(self.cb, vk::PipelineBindPoint::COMPUTE, self.pipe);
            for j in 0..=MAX_TARGETS {
                let bound = if j == OWN {
                    self.bufs[3].is_some()
                } else {
                    self.targets[j].is_some()
                };
                if !bound {
                    continue;
                }
                dev.cmd_bind_descriptor_sets(
                    self.cb,
                    vk::PipelineBindPoint::COMPUTE,
                    self.layout,
                    0,
                    &[self.sets[j]],
                    &[],
                );
                dev.cmd_dispatch_indirect(self.cb, self.args.buf, j as u64 * ARGS);
            }
            dev.cmd_pipeline_barrier(
                self.cb,
                cs,
                vk::PipelineStageFlags::HOST,
                vk::DependencyFlags::empty(),
                &[host_out],
                &[],
                &[],
            );
            dev.end_command_buffer(self.cb).unwrap();
        }
    }

    /// One frame: `tiles` workgroups into output `slot`, released from the armed stream and waited for.
    fn frame(&mut self, slot: usize, tiles: u32) -> GpuTime {
        let t = std::time::Instant::now();
        if self.stale {
            self.run_out();
            self.descriptors();
            self.record();
            self.stale = false;
        }
        self.zero_args();
        // SAFETY: entry `slot` of the arguments buffer (as `zero_args`).
        unsafe {
            (self.args.host() as *mut [u32; 4])
                .add(slot)
                .write([tiles, 1, 1, 0])
        };
        let f = self.s.released + 1;
        let cb = self.cb;
        // normally armed by the previous frame; then the release, then the next frame's submission while this one draws
        self.s.arm(&self.d, f, |_| cb);
        self.s.release(&self.d);
        self.s.arm(&self.d, f + 1, |_| cb);
        self.s.idle(&self.d);
        self.frames += 1;
        let wall = t.elapsed().as_secs_f64();
        GpuTime { wall, gpu: wall }
    }

    fn drop_own_out(&mut self) {
        if let Some(b) = self.bufs[3].take() {
            self.run_out();
            self.d.free(b);
            self.stale = true;
        }
    }

    /// The own output buffer's first `words` words.
    pub fn out(&self, words: usize) -> &[u32] {
        let b = self.bufs[3].as_ref().expect("render first");
        assert!(b.size as usize >= words.checked_mul(4).expect("byte count overflow"));
        // SAFETY: a coherent mapping of at least `words` words; the GPU is idle.
        unsafe { std::slice::from_raw_parts(b.host() as *const u32, words) }
    }
    /// Fill the own output with `words` (tests: a sentinel that unscheduled tiles must keep).
    pub fn set_out(&mut self, words: &[u32]) {
        self.put(3, words);
    }
    /// Zero the own output.
    pub fn clear_out(&mut self, words: usize) {
        let p = self
            .buf(3, words.checked_mul(4).expect("byte count overflow"))
            .host();
        // SAFETY: as `put`.
        unsafe { std::ptr::write_bytes(p, 0, words.checked_mul(4).expect("byte count overflow")) };
    }
}

impl Drop for Gpu {
    fn drop(&mut self) {
        self.run_out();
        // SAFETY: the stream ran out and the device is idle; each object was created by this executor, and is
        // destroyed once, before the device (the last field).
        unsafe {
            let _ = self.d.device.device_wait_idle();
            self.s.destroy(&self.d);
            self.d.device.destroy_command_pool(self.cpool, None);
            self.d.device.destroy_descriptor_pool(self.dpool, None);
            self.d.device.destroy_pipeline(self.pipe, None);
            self.d.device.destroy_pipeline_layout(self.layout, None);
            self.d.device.destroy_descriptor_set_layout(self.dsl, None);
        }
        for b in self.bufs.iter_mut().filter_map(|b| b.take()) {
            self.d.free(b);
        }
        self.d.free(std::mem::replace(&mut self.args, Buf::NONE));
        for t in self.targets.iter_mut().filter_map(|t| t.take()) {
            self.d.drop_import(t.imp);
        }
    }
}

static PREP: Pending<Gpu> = Pending::new();

/// Start building the executor (device, pipeline, measured costs) on another thread. `software`: a software device
/// may be the one; without it, a machine with no render node (or where a search found no hardware device) starts
/// nothing.
pub fn prepare_in_background(software: bool) {
    if !software && bough_vk::no_hardware() {
        return;
    }
    PREP.start("bough-kernel-vk", move || {
        let mut g = Gpu::build(crate::KERNEL_SPIRV, software)?;
        g.calibrate();
        Ok(g)
    });
}

/// The executor `prepare_in_background` is building: `Some` once it is built (or failed), never waiting; `None` while
/// it builds or when none was started.
pub fn poll_prepared() -> Option<Result<Gpu, String>> {
    if PREP.ready() { PREP.take() } else { None }
}

/// A build was started and not yet taken.
pub fn preparing() -> bool {
    PREP.started()
}

/// The executor, waiting for the build that was started, or built now (calibrated either way).
pub fn take_prepared(software: bool) -> Result<Gpu, String> {
    match PREP.take() {
        Some(r) => r,
        None if !software && bough_vk::no_hardware() => {
            Err("no hardware Vulkan device (no render node, or none found before)".into())
        }
        None => {
            let mut g = Gpu::build(crate::KERNEL_SPIRV, software)?;
            g.calibrate();
            Ok(g)
        }
    }
}
