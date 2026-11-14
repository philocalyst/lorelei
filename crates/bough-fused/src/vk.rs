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
use ash::vk;
use crate::frame::{cert_words, view_masks, vm_page, write_header, write_n_dir, Port};
use bough_frame::region::*;
use std::ffi::{c_void, CStr};
use std::ptr::NonNull;

/// Bindings of set 0 (shaders/fused.rs): region head, 11 columns, bins, counts, 4 surfaces, certificates, atlases.
const NBIND: u32 = 21;
/// The slot uniform's stride (every device's `minUniformBufferOffsetAlignment` is at most 256).
const UBO_STRIDE: u64 = 256;

fn err(what: &str, e: impl std::fmt::Debug) -> String {
    format!("{what}: {e:?}")
}

/// A buffer this presenter allocated, persistently mapped when host-visible.
struct Buf {
    buf: vk::Buffer,
    mem: vk::DeviceMemory,
    size: u64,
    ptr: Option<NonNull<u8>>,
}

/// Caller memory imported as a buffer (`VK_EXT_external_memory_host`): a region view or a surface.
struct Import {
    buf: vk::Buffer,
    mem: vk::DeviceMemory,
    ptr: *mut u8,
    size: u64,
}

/// The device state of a presenter: instance, device, queue, the pipelines, built on another thread from process
/// entry while the main thread mounts (`prepare_in_background`).
pub struct Prep {
    instance: ash::Instance,
    device: ash::Device,
    queue: vk::Queue,
    qfam: u32,
    host: ash::ext::external_memory_host::Device,
    host_align: u64,
    mem: vk::PhysicalDeviceMemoryProperties,
    max_range: u64,
    dsl: [vk::DescriptorSetLayout; 2],
    layout: vk::PipelineLayout,
    /// fs_copy, fs_bin, fs_tile.
    pipes: [vk::Pipeline; 3],
    cache: vk::PipelineCache,
    cache_path: Option<std::path::PathBuf>,
    /// The device's name, its driver, the API version, the subgroup size and whether full subgroups are required.
    pub info: String,
    pub software: bool,
    pub ns: u64,
}

// SAFETY: Vulkan handles are plain values; the instance and device function tables are immutable. A `Prep` moves from
// the thread that built it to the one that uses it, which is then the only one to touch it.
unsafe impl Send for Prep {}

impl Prep {
    /// Instance, device and pipelines; `Err` when there is no Vulkan loader or no device that can run the kernels
    /// (Vulkan 1.2 timeline semaphores, subgroup arithmetic in compute, `VK_EXT_external_memory_host`).
    /// `BOUGH_VK_DEVICE`: an index or a substring of the device's name; a software device (lavapipe) is taken only
    /// when `software` allows it (nothing is built for it otherwise: no device, no pipelines).
    /// `BOUGH_VK_VALIDATE=1`: the Khronos validation layer, if installed.
    pub fn new(software: bool) -> Result<Prep, String> {
        let t0 = std::time::Instant::now();
        let entry = entry()?;
        // 1.3: what the device then offers of 1.3 (full subgroups, subgroup size control) is core; a 1.2 device still works
        let app = vk::ApplicationInfo::default().application_name(c"bough").engine_name(c"bough").api_version(vk::make_api_version(0, 1, 3, 0));
        // SAFETY: plain enumeration.
        let have_ext = unsafe { entry.enumerate_instance_extension_properties(None) }.unwrap_or_default();
        let has = |n: &CStr| have_ext.iter().any(|e| e.extension_name_as_c_str() == Ok(n));
        let mut exts: Vec<*const i8> = Vec::new();
        let mut flags = vk::InstanceCreateFlags::empty();
        // MoltenVK is a portability implementation: enumerated only when asked for
        if has(ash::khr::portability_enumeration::NAME) {
            exts.push(ash::khr::portability_enumeration::NAME.as_ptr());
            flags |= vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR;
        }
        if has(ash::khr::get_physical_device_properties2::NAME) {
            exts.push(ash::khr::get_physical_device_properties2::NAME.as_ptr());
        }
        let validate = std::env::var("BOUGH_VK_VALIDATE").is_ok_and(|v| v == "1");
        // SAFETY: plain enumeration.
        let layers_have = unsafe { entry.enumerate_instance_layer_properties() }.unwrap_or_default();
        let vl = c"VK_LAYER_KHRONOS_validation";
        let layers: Vec<*const i8> = if validate && layers_have.iter().any(|l| l.layer_name_as_c_str() == Ok(vl)) { vec![vl.as_ptr()] } else { vec![] };
        if validate && layers.is_empty() {
            eprintln!("bough: BOUGH_VK_VALIDATE=1 but the validation layer is not installed");
        }
        let ici = vk::InstanceCreateInfo::default().application_info(&app).enabled_extension_names(&exts).enabled_layer_names(&layers).flags(flags);
        // SAFETY: a well-formed create info; the strings outlive the call.
        let instance = unsafe { entry.create_instance(&ici, None) }.map_err(|e| err("vkCreateInstance", e))?;
        match Self::with_instance(instance, t0, software) {
            Ok(p) => Ok(p),
            Err((instance, e)) => {
                // SAFETY: nothing was created from the instance that outlives it.
                unsafe { instance.destroy_instance(None) };
                Err(e)
            }
        }
    }

    fn with_instance(instance: ash::Instance, t0: std::time::Instant, software: bool) -> Result<Prep, (ash::Instance, String)> {
        // SAFETY: plain enumeration on a live instance.
        let pdevs = match unsafe { instance.enumerate_physical_devices() } {
            Ok(p) if !p.is_empty() => p,
            Ok(_) => return Err((instance, "no Vulkan device".into())),
            Err(e) => return Err((instance, err("vkEnumeratePhysicalDevices", e))),
        };
        let want = std::env::var("BOUGH_VK_DEVICE").ok();
        // candidates that can run the kernels, best first
        let mut cands: Vec<(u32, vk::PhysicalDevice, String, vk::PhysicalDeviceType)> = Vec::new();
        for (i, &pd) in pdevs.iter().enumerate() {
            // SAFETY: a device of this instance.
            let p = unsafe { instance.get_physical_device_properties(pd) };
            let name = p.device_name_as_c_str().map(|c| c.to_string_lossy().into_owned()).unwrap_or_default();
            if p.api_version < vk::make_api_version(0, 1, 2, 0) {
                continue;
            }
            // SAFETY: as above.
            let exts = unsafe { instance.enumerate_device_extension_properties(pd) }.unwrap_or_default();
            if !exts.iter().any(|e| e.extension_name_as_c_str() == Ok(ash::ext::external_memory_host::NAME)) {
                continue;
            }
            let mut sg = vk::PhysicalDeviceSubgroupProperties::default();
            let mut p2 = vk::PhysicalDeviceProperties2::default().push_next(&mut sg);
            // SAFETY: as above; Vulkan 1.1+ (checked).
            unsafe { instance.get_physical_device_properties2(pd, &mut p2) };
            let ops = vk::SubgroupFeatureFlags::BASIC | vk::SubgroupFeatureFlags::ARITHMETIC;
            if !sg.supported_stages.contains(vk::ShaderStageFlags::COMPUTE) || !sg.supported_operations.contains(ops) {
                continue;
            }
            let named = want.as_deref().is_some_and(|w| w == i.to_string() || name.to_lowercase().contains(&w.to_lowercase()));
            if want.is_some() && !named {
                continue;
            }
            let rank = match p.device_type {
                vk::PhysicalDeviceType::DISCRETE_GPU => 0,
                vk::PhysicalDeviceType::INTEGRATED_GPU => 1,
                vk::PhysicalDeviceType::VIRTUAL_GPU => 2,
                vk::PhysicalDeviceType::CPU => 4,
                _ => 3,
            };
            cands.push((rank, pd, name, p.device_type));
        }
        cands.sort_by_key(|c| c.0);
        let Some((_, pdev, name, dtype)) = cands.into_iter().next() else {
            return Err((instance, "no Vulkan device with timeline semaphores, compute subgroup arithmetic and VK_EXT_external_memory_host".into()));
        };
        if dtype == vk::PhysicalDeviceType::CPU && !software {
            return Err((instance, format!("the only Vulkan device is a software one ({name})")));
        }
        match Self::with_device(&instance, pdev, name, dtype, t0) {
            Ok(mut p) => {
                p.instance = instance;
                Ok(p)
            }
            Err(e) => Err((instance, e)),
        }
    }

    fn with_device(instance: &ash::Instance, pdev: vk::PhysicalDevice, name: String, dtype: vk::PhysicalDeviceType, t0: std::time::Instant) -> Result<Prep, String> {
        // SAFETY: queries on a device of a live instance.
        let (props, qfams, mem) = unsafe { (instance.get_physical_device_properties(pdev), instance.get_physical_device_queue_family_properties(pdev), instance.get_physical_device_memory_properties(pdev)) };
        // a compute-only family (an async compute queue) if there is one, else any family with compute
        let qfam = qfams
            .iter()
            .position(|q| q.queue_flags.contains(vk::QueueFlags::COMPUTE) && !q.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .or_else(|| qfams.iter().position(|q| q.queue_flags.contains(vk::QueueFlags::COMPUTE)))
            .ok_or("no compute queue")? as u32;
        // features: timeline semaphores (1.2), subgroup size control (1.3 or the extension)
        let mut f12 = vk::PhysicalDeviceVulkan12Features::default();
        let mut ssc = vk::PhysicalDeviceSubgroupSizeControlFeatures::default();
        let mut f2 = vk::PhysicalDeviceFeatures2::default().push_next(&mut f12).push_next(&mut ssc);
        // SAFETY: as above.
        unsafe { instance.get_physical_device_features2(pdev, &mut f2) };
        if f12.timeline_semaphore == 0 {
            return Err("no timeline semaphores".into());
        }
        // SAFETY: as above.
        let exts = unsafe { instance.enumerate_device_extension_properties(pdev) }.unwrap_or_default();
        let has = |n: &CStr| exts.iter().any(|e| e.extension_name_as_c_str() == Ok(n));
        let mut dexts: Vec<*const i8> = vec![ash::ext::external_memory_host::NAME.as_ptr()];
        let portability = c"VK_KHR_portability_subset";
        if has(portability) {
            dexts.push(portability.as_ptr());
        }
        let api13 = props.api_version >= vk::make_api_version(0, 1, 3, 0);
        let ssc_ext = !api13 && has(ash::ext::subgroup_size_control::NAME);
        if ssc_ext {
            dexts.push(ash::ext::subgroup_size_control::NAME.as_ptr());
        }
        let full_sg = (api13 || ssc_ext) && ssc.compute_full_subgroups != 0;
        let prio = [1.0f32];
        let qci = [vk::DeviceQueueCreateInfo::default().queue_family_index(qfam).queue_priorities(&prio)];
        let mut en12 = vk::PhysicalDeviceVulkan12Features::default().timeline_semaphore(true);
        let mut en_ssc = vk::PhysicalDeviceSubgroupSizeControlFeatures::default().subgroup_size_control(ssc.subgroup_size_control != 0).compute_full_subgroups(full_sg);
        let mut dci = vk::DeviceCreateInfo::default().queue_create_infos(&qci).enabled_extension_names(&dexts).push_next(&mut en12);
        if full_sg {
            dci = dci.push_next(&mut en_ssc);
        }
        // SAFETY: a well-formed create info naming features the device reported.
        let device = unsafe { instance.create_device(pdev, &dci, None) }.map_err(|e| err("vkCreateDevice", e))?;
        // SAFETY: the queue created above.
        let queue = unsafe { device.get_device_queue(qfam, 0) };
        let host = ash::ext::external_memory_host::Device::new(instance, &device);
        let mut hp = vk::PhysicalDeviceExternalMemoryHostPropertiesEXT::default();
        let mut sg = vk::PhysicalDeviceSubgroupProperties::default();
        let mut p2 = vk::PhysicalDeviceProperties2::default().push_next(&mut hp).push_next(&mut sg);
        // SAFETY: as above.
        unsafe { instance.get_physical_device_properties2(pdev, &mut p2) };
        let host_align = hp.min_imported_host_pointer_alignment.max(1);
        // set 0: 21 storage buffers; set 1: the slot (a dynamic uniform)
        let b0: Vec<vk::DescriptorSetLayoutBinding> = (0..NBIND)
            .map(|b| vk::DescriptorSetLayoutBinding::default().binding(b).descriptor_type(vk::DescriptorType::STORAGE_BUFFER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE))
            .collect();
        let b1 = [vk::DescriptorSetLayoutBinding::default().binding(0).descriptor_type(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC).descriptor_count(1).stage_flags(vk::ShaderStageFlags::COMPUTE)];
        // SAFETY: well-formed create infos on the live device.
        let dsl = unsafe {
            [
                device.create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().bindings(&b0), None).map_err(|e| err("set layout", e))?,
                device.create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().bindings(&b1), None).map_err(|e| err("set layout", e))?,
            ]
        };
        // SAFETY: as above.
        let layout = unsafe { device.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&dsl), None) }.map_err(|e| err("pipeline layout", e))?;
        // the pipeline cache: per device and driver, under the user's cache directory
        let cache_path = std::env::var_os("XDG_CACHE_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".cache"))).map(|d| {
            let uuid: String = props.pipeline_cache_uuid.iter().map(|b| format!("{b:02x}")).collect();
            d.join("bough").join(format!("vk-{:04x}-{:04x}-{}-{uuid}.bin", props.vendor_id, props.device_id, props.driver_version))
        });
        let initial = cache_path.as_ref().and_then(|p| std::fs::read(p).ok()).unwrap_or_default();
        // SAFETY: as above; a stale or foreign blob is rejected by the driver's header check, not trusted.
        let cache = unsafe { device.create_pipeline_cache(&vk::PipelineCacheCreateInfo::default().initial_data(&initial), None) }
            .or_else(|_| unsafe { device.create_pipeline_cache(&vk::PipelineCacheCreateInfo::default(), None) })
            .map_err(|e| err("pipeline cache", e))?;
        let words = |b: &[u8]| -> Vec<u32> { b.chunks_exact(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect() };
        let srcs = [(crate::spirv::FS_COPY, c"fs_copy"), (crate::spirv::FS_BIN, c"fs_bin"), (crate::spirv::FS_TILE, c"fs_tile")];
        let mut modules = Vec::new();
        for (spv, _) in &srcs {
            let w = words(spv);
            // SAFETY: SPIR-V produced and validated by naga at build time.
            modules.push(unsafe { device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&w), None) }.map_err(|e| err("shader module", e))?);
        }
        let stage_flags = if full_sg { vk::PipelineShaderStageCreateFlags::REQUIRE_FULL_SUBGROUPS } else { vk::PipelineShaderStageCreateFlags::empty() };
        let cis: Vec<vk::ComputePipelineCreateInfo> = srcs
            .iter()
            .zip(&modules)
            .map(|((_, name), &m)| vk::ComputePipelineCreateInfo::default().layout(layout).stage(vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::COMPUTE).module(m).name(name).flags(stage_flags)))
            .collect();
        // SAFETY: as above.
        let pipes = unsafe { device.create_compute_pipelines(cache, &cis, None) }.map_err(|(_, e)| err("compute pipelines", e))?;
        for m in modules {
            // SAFETY: the pipelines are created; the modules are no longer needed.
            unsafe { device.destroy_shader_module(m, None) };
        }
        let drv = {
            let mut dp = vk::PhysicalDeviceDriverProperties::default();
            let mut p2 = vk::PhysicalDeviceProperties2::default().push_next(&mut dp);
            // SAFETY: as above (1.2).
            unsafe { instance.get_physical_device_properties2(pdev, &mut p2) };
            dp.driver_info_as_c_str().map(|c| c.to_string_lossy().into_owned()).unwrap_or_default()
        };
        let v = props.api_version;
        let info = format!(
            "{name} ({drv}; Vulkan {}.{}.{}; subgroup {}{}; host import align {host_align})",
            vk::api_version_major(v),
            vk::api_version_minor(v),
            vk::api_version_patch(v),
            sg.subgroup_size,
            if full_sg { ", full" } else { "" }
        );
        Ok(Prep {
            // replaced by the caller with the owning handle (this one is a copy of its function table)
            instance: instance.clone(),
            device,
            queue,
            qfam,
            host,
            host_align,
            mem,
            max_range: props.limits.max_storage_buffer_range as u64,
            dsl,
            layout,
            pipes: [pipes[0], pipes[1], pipes[2]],
            cache,
            cache_path,
            info,
            software: dtype == vk::PhysicalDeviceType::CPU,
            ns: t0.elapsed().as_nanos() as u64,
        })
    }

    fn mem_type(&self, bits: u32, want: vk::MemoryPropertyFlags, prefer: vk::MemoryPropertyFlags) -> Option<u32> {
        let n = self.mem.memory_type_count as usize;
        let ok = |i: usize, f: vk::MemoryPropertyFlags| bits & (1 << i) != 0 && self.mem.memory_types[i].property_flags.contains(f);
        (0..n).find(|&i| ok(i, want | prefer)).or_else(|| (0..n).find(|&i| ok(i, want))).map(|i| i as u32)
    }

    /// A buffer of `size` bytes: device-local (`host` false) or host-visible and coherent, persistently mapped
    /// (device-local too when the device has such memory: resizable BAR, unified memory).
    fn buffer(&self, size: u64, usage: vk::BufferUsageFlags, host: bool) -> Result<Buf, String> {
        let size = size.max(16);
        let d = &self.device;
        // SAFETY: well-formed create infos on the live device; the memory is bound once, mapped once.
        unsafe {
            let buf = d.create_buffer(&vk::BufferCreateInfo::default().size(size).usage(usage).sharing_mode(vk::SharingMode::EXCLUSIVE), None).map_err(|e| err("buffer", e))?;
            let req = d.get_buffer_memory_requirements(buf);
            let (want, prefer) = if host {
                (vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT, vk::MemoryPropertyFlags::DEVICE_LOCAL)
            } else {
                (vk::MemoryPropertyFlags::DEVICE_LOCAL, vk::MemoryPropertyFlags::empty())
            };
            let t = self.mem_type(req.memory_type_bits, want, prefer).or_else(|| self.mem_type(req.memory_type_bits, vk::MemoryPropertyFlags::empty(), vk::MemoryPropertyFlags::empty())).ok_or("no memory type")?;
            let mem = d.allocate_memory(&vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(t), None).map_err(|e| err("allocate", e))?;
            d.bind_buffer_memory(buf, mem, 0).map_err(|e| err("bind", e))?;
            let ptr = if host { Some(NonNull::new(d.map_memory(mem, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty()).map_err(|e| err("map", e))? as *mut u8).ok_or("map")?) } else { None };
            if let Some(p) = ptr {
                std::ptr::write_bytes(p.as_ptr(), 0, size as usize);
            }
            Ok(Buf { buf, mem, size, ptr })
        }
    }

    fn free(&self, b: Buf) {
        // SAFETY: the caller guarantees no pending GPU work uses the buffer (it drained or synced).
        unsafe {
            self.device.destroy_buffer(b.buf, None);
            self.device.free_memory(b.mem, None);
        }
    }

    /// Import `size` bytes of caller memory at `ptr` (page aligned, a whole number of the device's import alignment)
    /// as a storage buffer.
    ///
    /// # Safety
    /// The memory stays mapped and is not freed while the import lives.
    unsafe fn import(&self, ptr: *mut u8, size: usize, usage: vk::BufferUsageFlags) -> Result<Import, String> {
        let a = self.host_align as usize;
        if !(ptr as usize).is_multiple_of(a) {
            return Err(format!("host import: {ptr:?} is not aligned to {a}"));
        }
        let size = (size.max(1).next_multiple_of(a)) as u64;
        let ht = vk::ExternalMemoryHandleTypeFlags::HOST_ALLOCATION_EXT;
        let d = &self.device;
        // SAFETY: the caller's contract; well-formed create infos on the live device.
        unsafe {
            let mut hp = vk::MemoryHostPointerPropertiesEXT::default();
            let r = (self.host.fp().get_memory_host_pointer_properties_ext)(d.handle(), ht, ptr as *const c_void, &mut hp);
            if r != vk::Result::SUCCESS {
                return Err(err("vkGetMemoryHostPointerPropertiesEXT", r));
            }
            let mut ext = vk::ExternalMemoryBufferCreateInfo::default().handle_types(ht);
            let buf = d.create_buffer(&vk::BufferCreateInfo::default().size(size).usage(usage).sharing_mode(vk::SharingMode::EXCLUSIVE).push_next(&mut ext), None).map_err(|e| err("import buffer", e))?;
            let req = d.get_buffer_memory_requirements(buf);
            let bits = req.memory_type_bits & hp.memory_type_bits;
            let Some(t) = self.mem_type(bits, vk::MemoryPropertyFlags::empty(), vk::MemoryPropertyFlags::HOST_COHERENT) else {
                d.destroy_buffer(buf, None);
                return Err("host import: no memory type".into());
            };
            let mut imp = vk::ImportMemoryHostPointerInfoEXT::default().handle_type(ht).host_pointer(ptr as *mut c_void);
            let mem = match d.allocate_memory(&vk::MemoryAllocateInfo::default().allocation_size(size).memory_type_index(t).push_next(&mut imp), None) {
                Ok(m) => m,
                Err(e) => {
                    d.destroy_buffer(buf, None);
                    return Err(err("host import", e));
                }
            };
            d.bind_buffer_memory(buf, mem, 0).map_err(|e| err("bind import", e))?;
            Ok(Import { buf, mem, ptr, size })
        }
    }

    fn drop_import(&self, i: Import) {
        // SAFETY: the caller guarantees no pending GPU work uses it.
        unsafe {
            self.device.destroy_buffer(i.buf, None);
            self.device.free_memory(i.mem, None);
        }
    }
}

impl Drop for Prep {
    fn drop(&mut self) {
        // SAFETY: the owner waited for the device to be idle; every object below was created here.
        unsafe {
            let _ = self.device.device_wait_idle();
            if let (Some(p), Ok(data)) = (&self.cache_path, self.device.get_pipeline_cache_data(self.cache)) {
                if let Some(dir) = p.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(p, data);
            }
            for &p in &self.pipes {
                self.device.destroy_pipeline(p, None);
            }
            self.device.destroy_pipeline_cache(self.cache, None);
            self.device.destroy_pipeline_layout(self.layout, None);
            for &l in &self.dsl {
                self.device.destroy_descriptor_set_layout(l, None);
            }
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

/// The Vulkan library, opened once and never closed: a driver's threads (llvmpipe's rasterisers, shader-cache
/// queues) can outlive the instance that started them, and closing the library under them is a crash at exit or at the
/// next probe. `BOUGH_VK_LIB`: a Vulkan library to open instead of the loader (an ICD that exports
/// `vkGetInstanceProcAddr`, e.g. MoltenVK's dylib).
fn entry() -> Result<&'static ash::Entry, String> {
    static ENTRY: std::sync::OnceLock<Result<ash::Entry, String>> = std::sync::OnceLock::new();
    ENTRY
        .get_or_init(|| {
            // SAFETY: loading a Vulkan library (no other preconditions); it is never unloaded.
            match std::env::var_os("BOUGH_VK_LIB") {
                Some(p) => unsafe { ash::Entry::load_from(p) },
                None => unsafe { ash::Entry::load() },
            }
            .map_err(|e| err("Vulkan loader", e))
        })
        .as_ref()
        .map_err(|e| e.clone())
}

static PREP: std::sync::Mutex<Option<std::thread::JoinHandle<Result<Prep, String>>>> = std::sync::Mutex::new(None);

/// Could a hardware Vulkan device exist here? On Linux every hardware driver has a DRM render node (or NVIDIA's own
/// device files); without one the default search is skipped and no Vulkan library is loaded at all.
pub fn hardware_possible() -> bool {
    #[cfg(target_os = "linux")]
    {
        let render = std::fs::read_dir("/dev/dri").is_ok_and(|d| d.flatten().any(|e| e.file_name().to_string_lossy().starts_with("renderD")));
        render || std::path::Path::new("/dev/nvidia0").exists()
    }
    #[cfg(not(target_os = "linux"))]
    true
}

/// Start building the device (instance, device, queue, pipelines) on another thread; the next presenter takes it.
/// `software`: a software device may be the one.
pub fn prepare_in_background(software: bool) {
    if !software && !hardware_possible() {
        return;
    }
    let h = std::thread::Builder::new().name("bough-vk-prep".into()).spawn(move || Prep::new(software)).expect("vk prep thread");
    *PREP.lock().unwrap() = Some(h);
}

/// Set once a search for a hardware device found none: later default searches in this process answer at once (a
/// machine without a GPU pays the loader and the enumeration once, not at every app).
static NO_HARDWARE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The prepared device (waiting for its thread), or a new one.
pub fn take_prep(software: bool) -> Result<Prep, String> {
    use std::sync::atomic::Ordering::Relaxed;
    let r = match PREP.lock().unwrap().take() {
        Some(h) => h.join().map_err(|_| "vk prep thread panicked".to_string())?,
        None if !software && NO_HARDWARE.load(Relaxed) => Err("no hardware Vulkan device (searched before)".into()),
        None if !software && !hardware_possible() => Err("no GPU render node".into()),
        None => Prep::new(software),
    };
    if r.is_err() && !software {
        NO_HARDWARE.store(true, Relaxed);
    }
    r
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

/// GPU completion as a pollable fd (Linux): a thread waits on the timeline semaphore for the values it is sent and
/// writes an eventfd when each is reached, so a window's event loop can commit a frame the moment the GPU finishes it
/// without the input thread waiting (the role of Metal's publisher thread; Wayland's objects stay on the loop's thread).
#[cfg(target_os = "linux")]
struct Waiter {
    fd: std::os::fd::OwnedFd,
    tx: Option<std::sync::mpsc::Sender<u64>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "linux")]
impl Waiter {
    fn new(device: ash::Device, tl: vk::Semaphore) -> Waiter {
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
        // SAFETY: a fresh eventfd this function owns.
        let fd = unsafe { OwnedFd::from_raw_fd(libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK)) };
        let wfd = fd.as_raw_fd();
        let (tx, rx) = std::sync::mpsc::channel::<u64>();
        let thread = std::thread::Builder::new()
            .name("bough-vk-waiter".into())
            .spawn(move || {
                while let Ok(v) = rx.recv() {
                    let (sems, vals) = ([tl], [v]);
                    // SAFETY: the presenter's semaphore, alive until this thread is joined (the presenter's drop);
                    // every value sent is a released frame's, so the wait ends.
                    let _ = unsafe { device.wait_semaphores(&vk::SemaphoreWaitInfo::default().semaphores(&sems).values(&vals), 10_000_000_000) };
                    let one: u64 = 1;
                    // SAFETY: the eventfd lives as long as the `Waiter` (which joins this thread first).
                    unsafe { libc::write(wfd, (&one as *const u64).cast(), 8) };
                }
            })
            .expect("waiter thread");
        Waiter { fd, tx: Some(tx), thread: Some(thread) }
    }
}

#[cfg(target_os = "linux")]
impl Drop for Waiter {
    fn drop(&mut self) {
        drop(self.tx.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub struct Fused {
    #[cfg(target_os = "linux")]
    waiter: Option<Waiter>,
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
    tl: vk::Semaphore,
    released: u64,
    armed_upto: u64,
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
        let (dpool, set0, set1, cpool, frames, aux, aux_fence, tl) = unsafe {
            let sizes = [vk::DescriptorPoolSize { ty: vk::DescriptorType::STORAGE_BUFFER, descriptor_count: NBIND }, vk::DescriptorPoolSize { ty: vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC, descriptor_count: 1 }];
            let dpool = dev.create_descriptor_pool(&vk::DescriptorPoolCreateInfo::default().max_sets(2).pool_sizes(&sizes), None).map_err(|e| err("descriptor pool", e))?;
            let sets = dev.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(dpool).set_layouts(&d.dsl)).map_err(|e| err("descriptor sets", e))?;
            let cpool = dev
                .create_command_pool(&vk::CommandPoolCreateInfo::default().queue_family_index(d.qfam).flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER), None)
                .map_err(|e| err("command pool", e))?;
            let cbs = dev.allocate_command_buffers(&vk::CommandBufferAllocateInfo::default().command_pool(cpool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(3)).map_err(|e| err("command buffers", e))?;
            let fence = dev.create_fence(&vk::FenceCreateInfo::default(), None).map_err(|e| err("fence", e))?;
            let mut tt = vk::SemaphoreTypeCreateInfo::default().semaphore_type(vk::SemaphoreType::TIMELINE).initial_value(0);
            let tl = dev.create_semaphore(&vk::SemaphoreCreateInfo::default().push_next(&mut tt), None).map_err(|e| err("timeline semaphore", e))?;
            (dpool, sets[0], sets[1], cpool, [cbs[0], cbs[1]], cbs[2], fence, tl)
        };
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
            tl,
            released: 0,
            armed_upto: 0,
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
            dev.begin_command_buffer(cb, &vk::CommandBufferBeginInfo::default().flags(if sizes.is_none() { vk::CommandBufferUsageFlags::SIMULTANEOUS_USE } else { vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT })).unwrap();
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
        debug_assert_eq!(self.armed_upto, self.released);
        self.descriptors();
        for k in 0..2 {
            // SAFETY: not pending (drained); reset individually (RESET_COMMAND_BUFFER pool).
            unsafe { self.d.device.reset_command_buffer(self.frames[k], vk::CommandBufferResetFlags::empty()) }.unwrap();
            self.record(self.frames[k], k, None);
        }
        self.stale = false;
    }

    /// Submit frames `armed_upto + 1 ..= upto`, each waiting for its release.
    fn arm(&mut self, upto: u64) {
        if self.armed_upto >= upto {
            return;
        }
        if self.stale {
            self.drain();
            self.refresh();
        }
        let from = self.armed_upto + 1;
        let n = (upto - self.armed_upto) as usize;
        let waits: Vec<u64> = (from..=upto).map(|f| 2 * f - 1).collect();
        let sigs: Vec<u64> = (from..=upto).map(|f| 2 * f).collect();
        let cbs: Vec<vk::CommandBuffer> = (from..=upto).map(|f| self.frames[(f % 2) as usize]).collect();
        let sem = [self.tl];
        let stage = [vk::PipelineStageFlags::COMPUTE_SHADER | vk::PipelineStageFlags::DRAW_INDIRECT];
        let mut tsi: Vec<vk::TimelineSemaphoreSubmitInfo> = (0..n).map(|i| vk::TimelineSemaphoreSubmitInfo::default().wait_semaphore_values(&waits[i..i + 1]).signal_semaphore_values(&sigs[i..i + 1])).collect();
        let subs: Vec<vk::SubmitInfo> = tsi
            .iter_mut()
            .enumerate()
            .map(|(i, t)| vk::SubmitInfo::default().wait_semaphores(&sem).wait_dst_stage_mask(&stage).command_buffers(&cbs[i..i + 1]).signal_semaphores(&sem).push_next(t))
            .collect();
        // SAFETY: the frame command buffers are executable and SIMULTANEOUS_USE; every wait has a host signal to come.
        unsafe { self.d.device.queue_submit(self.d.queue, &subs, vk::Fence::null()) }.expect("vkQueueSubmit");
        self.armed_upto = upto;
    }

    fn counter(&self) -> u64 {
        // SAFETY: the presenter's own semaphore.
        unsafe { self.d.device.get_semaphore_counter_value(self.tl) }.unwrap_or(0)
    }

    fn wait_value(&self, v: u64) {
        if self.counter() >= v {
            return;
        }
        let sems = [self.tl];
        let vals = [v];
        // SAFETY: the presenter's own semaphore; the value is signalled by a submitted, released frame.
        let r = unsafe { self.d.device.wait_semaphores(&vk::SemaphoreWaitInfo::default().semaphores(&sems).values(&vals), 5_000_000_000) };
        if let Err(e) = r {
            panic!("bough-fused: GPU frame {} did not finish within 5 s: {e:?}", v / 2);
        }
    }

    fn signal(&self, v: u64) {
        // SAFETY: the value is above the counter (frames are released in order).
        unsafe { self.d.device.signal_semaphore(&vk::SemaphoreSignalInfo::default().semaphore(self.tl).value(v)) }.expect("vkSignalSemaphore");
    }

    /// Wait for the GPU's last frame (the CPU is about to write the region); redraw it if a bin overflowed. When the
    /// GPU is done (the common case) this is one read of the semaphore's counter.
    #[inline]
    pub fn sync(&mut self) {
        self.wait_value(2 * self.released);
        self.check_bins();
    }

    /// Run every armed frame as an empty one: what the frames bind may then change.
    fn drain(&mut self) {
        self.sync();
        if self.armed_upto == self.released {
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
        for f in self.released + 1..=self.armed_upto {
            self.signal(2 * f - 1);
            self.wait_value(2 * f);
        }
        self.released = self.armed_upto;
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
        let f = self.released + 1;
        let k = (f % 2) as usize;
        // SAFETY: the region is laid out by `lay` (slots in the head view); the GPU is idle (synced).
        let (nt, nr) = unsafe {
            write_n_dir(self.region, n_dir);
            write_slot(self.region, &self.lay, k, target as u32, rows, full, copy)
        };
        self.last_frame = Some((k, target, nr, nt));
        // normally armed already; the release first, then the next frames' submission while the GPU draws this one
        self.arm(f);
        self.signal(2 * f - 1);
        self.released = f;
        self.arm(f + self.batch);
        self.stats.gpu_frames += 1;
        self.stats.tiles += nt as u64;
        nt
    }

    /// The fd that becomes readable when a frame `notify_drawn` named is drawn (Linux): poll it with the window's
    /// connection, then present (the eventfd is drained by `drawn`).
    #[cfg(target_os = "linux")]
    pub fn completion_fd(&mut self) -> std::os::fd::RawFd {
        use std::os::fd::AsRawFd;
        let w = self.waiter.get_or_insert_with(|| Waiter::new(self.d.device.clone(), self.tl));
        w.fd.as_raw_fd()
    }

    /// Have `completion_fd` become readable once the last released frame is drawn.
    #[cfg(target_os = "linux")]
    pub fn notify_drawn(&mut self) {
        self.completion_fd();
        if let Some(tx) = self.waiter.as_ref().and_then(|w| w.tx.as_ref()) {
            let _ = tx.send(2 * self.released);
        }
    }

    /// The last released frame is drawn (one read of the semaphore's counter); drains the completion fd.
    pub fn drawn(&mut self) -> bool {
        #[cfg(target_os = "linux")]
        if let Some(w) = &self.waiter {
            use std::os::fd::AsRawFd;
            let mut v: u64 = 0;
            // SAFETY: a non-blocking eventfd this presenter owns.
            unsafe { libc::read(w.fd.as_raw_fd(), (&mut v as *mut u64).cast(), 8) };
        }
        self.counter() >= 2 * self.released
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
        self.run_aux(2, (0, nr, nt));
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
            dev.destroy_semaphore(self.tl, None);
            dev.destroy_fence(self.aux_fence, None);
            dev.destroy_command_pool(self.cpool, None);
            dev.destroy_descriptor_pool(self.dpool, None);
        }
        for b in [&mut self.bins, &mut self.counts, &mut self.cert, &mut self.atlas, &mut self.dummy, &mut self.ubo] {
            let b = std::mem::replace(b, Buf { buf: vk::Buffer::null(), mem: vk::DeviceMemory::null(), size: 0, ptr: None });
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
        let head = std::mem::replace(&mut self.head, Import { buf: vk::Buffer::null(), mem: vk::DeviceMemory::null(), ptr: std::ptr::null_mut(), size: 0 });
        self.d.drop_import(head);
    }
}
