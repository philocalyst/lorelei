//! The Vulkan device layer of bough's GPU executors (`ash`; Linux, and macOS through MoltenVK): what bough-fused's
//! presenter and bough-kernel's executor share, so each holds only its kernels, its bindings and its frame.
//!
//! * **The loader** is opened once and never closed ([`entry`]): a driver's threads can outlive the instance that
//!   started them. On Linux the default search runs only where a hardware driver can exist ([`hardware_possible`]: a
//!   DRM render node, or NVIDIA's device files), so a machine without a GPU loads no Vulkan library at all.
//! * **The device** ([`Device`]): the best device that has what the kernels need (timeline semaphores,
//!   `VK_EXT_external_memory_host`, subgroup arithmetic if asked), discrete first; a software device (lavapipe) only
//!   when asked for; a compute-only queue family when there is one; full subgroups where the device can; pipelines
//!   through a pipeline cache kept on disk per device, driver and executor.
//! * **Memory**: buffers device-local or host-visible and persistently mapped ([`Device::buffer`]); caller memory
//!   imported without a copy ([`Device::import`]: a window's `wl_shm` buffer, the façade's region).
//! * **The pre-armed stream** ([`Stream`]): frames are submitted ahead, frame f waiting on a timeline semaphore for
//!   2f - 1 and signalling 2f; a frame is released by one host signal (Metal's `MTLSharedEvent.signaledValue`), so
//!   nothing is recorded or submitted on the critical path.
//! * **Completion as an fd** ([`Waiter`], Linux): a window loop polls it beside the compositor's socket.
//! * **Devices built on another thread** ([`Pending`]) from process entry, as Metal's are.
#![cfg(any(target_os = "linux", target_os = "macos"))]
pub use ash;
pub use ash::vk;
use std::ffi::{CStr, c_void};
use std::ptr::NonNull;

pub fn err(what: &str, e: impl std::fmt::Debug) -> String {
    format!("{what}: {e:?}")
}

/// A buffer this device allocated, persistently mapped when host-visible.
pub struct Buf {
    pub buf: vk::Buffer,
    pub mem: vk::DeviceMemory,
    pub size: u64,
    pub ptr: Option<NonNull<u8>>,
}

impl Buf {
    /// A placeholder that owns nothing (for `mem::replace` in a drop).
    pub const NONE: Buf = Buf { buf: vk::Buffer::null(), mem: vk::DeviceMemory::null(), size: 0, ptr: None };
    /// The mapping's base (host-visible buffers only).
    pub fn host(&self) -> *mut u8 {
        self.ptr.expect("a host-visible buffer").as_ptr()
    }
}

/// Caller memory imported as a buffer (`VK_EXT_external_memory_host`).
pub struct Import {
    pub buf: vk::Buffer,
    pub mem: vk::DeviceMemory,
    pub ptr: *mut u8,
    pub size: u64,
}

impl Import {
    pub const NONE: Import = Import { buf: vk::Buffer::null(), mem: vk::DeviceMemory::null(), ptr: std::ptr::null_mut(), size: 0 };
}

/// What an executor's kernels need of a device beyond the baseline (timeline semaphores, host-memory import).
#[derive(Clone, Copy, Debug, Default)]
pub struct Needs {
    /// Subgroup `BASIC | ARITHMETIC` operations in compute (the fused kernels' prefix sums).
    pub subgroups: bool,
}

/// Instance, device and queue of one executor.
pub struct Device {
    pub instance: ash::Instance,
    pub device: ash::Device,
    pub queue: vk::Queue,
    pub qfam: u32,
    host: ash::ext::external_memory_host::Device,
    /// `minImportedHostPointerAlignment`: an import's address and length are multiples of it.
    pub host_align: u64,
    mem: vk::PhysicalDeviceMemoryProperties,
    /// `maxStorageBufferRange`.
    pub max_range: u64,
    pub cache: vk::PipelineCache,
    cache_path: Option<std::path::PathBuf>,
    /// Pipelines may require full subgroups (`REQUIRE_FULL_SUBGROUPS`).
    pub full_subgroups: bool,
    pub subgroup: u32,
    /// The device's name, its driver, the API version, the subgroup size and whether full subgroups are required.
    pub info: String,
    /// A software device (lavapipe).
    pub software: bool,
    /// Nanoseconds the device took to build.
    pub ns: u64,
}

// SAFETY: Vulkan handles are plain values and the function tables are immutable. A `Device` moves from the thread
// that built it to the one that uses it, which is then the only one to touch it (the queue is externally
// synchronised by that ownership).
unsafe impl Send for Device {}

impl Device {
    /// The best device that has what `needs` asks for; `Err` when there is no Vulkan loader or no such device.
    /// `BOUGH_VK_DEVICE`: an index or a substring of the device's name; a software device (lavapipe) is taken only
    /// when `software` allows it. `BOUGH_VK_VALIDATE=1`: the Khronos validation layer, if installed. `tag` names the
    /// executor in the pipeline cache's file (each executor keeps its own).
    pub fn new(software: bool, needs: Needs, tag: &str) -> Result<Device, String> {
        let t0 = std::time::Instant::now();
        let entry = entry()?;
        // 1.3: what the device then offers of 1.3 (full subgroups, subgroup size control) is core; a 1.2 device still works
        let app = vk::ApplicationInfo::default().application_name(c"bough").engine_name(c"bough").api_version(vk::make_api_version(0, 1, 3, 0));
        // SAFETY: plain enumeration.
        let have_ext = unsafe { entry.enumerate_instance_extension_properties(None) }.unwrap_or_default();
        let has = |n: &CStr| have_ext.iter().any(|e| e.extension_name_as_c_str() == Ok(n));
        let mut exts: Vec<*const core::ffi::c_char> = Vec::new();
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
        let layers: Vec<*const core::ffi::c_char> = if validate && layers_have.iter().any(|l| l.layer_name_as_c_str() == Ok(vl)) { vec![vl.as_ptr()] } else { vec![] };
        if validate && layers.is_empty() {
            eprintln!("bough: BOUGH_VK_VALIDATE=1 but the validation layer is not installed");
        }
        let ici = vk::InstanceCreateInfo::default().application_info(&app).enabled_extension_names(&exts).enabled_layer_names(&layers).flags(flags);
        // SAFETY: a well-formed create info; the strings outlive the call.
        let instance = unsafe { entry.create_instance(&ici, None) }.map_err(|e| err("vkCreateInstance", e))?;
        match Self::with_instance(instance, t0, software, needs, tag) {
            Ok(p) => Ok(p),
            Err((instance, e)) => {
                // SAFETY: nothing was created from the instance that outlives it.
                unsafe { instance.destroy_instance(None) };
                Err(e)
            }
        }
    }

    fn with_instance(instance: ash::Instance, t0: std::time::Instant, software: bool, needs: Needs, tag: &str) -> Result<Device, (ash::Instance, String)> {
        // SAFETY: plain enumeration on a live instance.
        let pdevs = match unsafe { instance.enumerate_physical_devices() } {
            Ok(p) if !p.is_empty() => p,
            Ok(_) => {
                NO_HARDWARE.store(true, std::sync::atomic::Ordering::Relaxed);
                return Err((instance, "no Vulkan device".into()));
            }
            Err(e) => return Err((instance, err("vkEnumeratePhysicalDevices", e))),
        };
        let want = std::env::var("BOUGH_VK_DEVICE").ok();
        // candidates that can run the kernels, best first
        let mut cands: Vec<(u32, vk::PhysicalDevice, String, vk::PhysicalDeviceType)> = Vec::new();
        let mut any_hardware = false;
        for (i, &pd) in pdevs.iter().enumerate() {
            // SAFETY: a device of this instance.
            let p = unsafe { instance.get_physical_device_properties(pd) };
            let name = p.device_name_as_c_str().map(|c| c.to_string_lossy().into_owned()).unwrap_or_default();
            any_hardware |= p.device_type != vk::PhysicalDeviceType::CPU;
            if p.api_version < vk::make_api_version(0, 1, 2, 0) {
                continue;
            }
            // SAFETY: as above.
            let exts = unsafe { instance.enumerate_device_extension_properties(pd) }.unwrap_or_default();
            if !exts.iter().any(|e| e.extension_name_as_c_str() == Ok(ash::ext::external_memory_host::NAME)) {
                continue;
            }
            if needs.subgroups {
                let mut sg = vk::PhysicalDeviceSubgroupProperties::default();
                let mut p2 = vk::PhysicalDeviceProperties2::default().push_next(&mut sg);
                // SAFETY: as above; Vulkan 1.1+ (checked).
                unsafe { instance.get_physical_device_properties2(pd, &mut p2) };
                let ops = vk::SubgroupFeatureFlags::BASIC | vk::SubgroupFeatureFlags::ARITHMETIC;
                if !sg.supported_stages.contains(vk::ShaderStageFlags::COMPUTE) || !sg.supported_operations.contains(ops) {
                    continue;
                }
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
        if !any_hardware {
            NO_HARDWARE.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        cands.sort_by_key(|c| c.0);
        let Some((_, pdev, name, dtype)) = cands.into_iter().next() else {
            return Err((instance, "no Vulkan device with timeline semaphores and VK_EXT_external_memory_host (and compute subgroup arithmetic, where asked)".into()));
        };
        if dtype == vk::PhysicalDeviceType::CPU && !software {
            return Err((instance, format!("the only Vulkan device is a software one ({name})")));
        }
        match Self::with_device(&instance, pdev, name, dtype, t0, tag) {
            Ok(mut p) => {
                p.instance = instance;
                Ok(p)
            }
            Err(e) => Err((instance, e)),
        }
    }

    fn with_device(instance: &ash::Instance, pdev: vk::PhysicalDevice, name: String, dtype: vk::PhysicalDeviceType, t0: std::time::Instant, tag: &str) -> Result<Device, String> {
        // SAFETY: queries on a device of a live instance.
        let (props, qfams, mem) =
            unsafe { (instance.get_physical_device_properties(pdev), instance.get_physical_device_queue_family_properties(pdev), instance.get_physical_device_memory_properties(pdev)) };
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
        let mut dexts: Vec<*const core::ffi::c_char> = vec![ash::ext::external_memory_host::NAME.as_ptr()];
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
        let mut dp = vk::PhysicalDeviceDriverProperties::default();
        let mut p2 = vk::PhysicalDeviceProperties2::default().push_next(&mut hp).push_next(&mut sg).push_next(&mut dp);
        // SAFETY: as above (1.2).
        unsafe { instance.get_physical_device_properties2(pdev, &mut p2) };
        let host_align = hp.min_imported_host_pointer_alignment.max(1);
        let drv = dp.driver_info_as_c_str().map(|c| c.to_string_lossy().into_owned()).unwrap_or_default();
        // the pipeline cache: per device, driver and executor, under the user's cache directory
        let cache_path = std::env::var_os("XDG_CACHE_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".cache"))).map(|d| {
            let uuid: String = props.pipeline_cache_uuid.iter().map(|b| format!("{b:02x}")).collect();
            d.join("bough").join(format!("vk-{tag}-{:04x}-{:04x}-{}-{uuid}.bin", props.vendor_id, props.device_id, props.driver_version))
        });
        let initial = cache_path.as_ref().and_then(|p| std::fs::read(p).ok()).unwrap_or_default();
        // SAFETY: as above; a stale or foreign blob is rejected by the driver's header check, not trusted.
        let cache = unsafe { device.create_pipeline_cache(&vk::PipelineCacheCreateInfo::default().initial_data(&initial), None) }
            .or_else(|_| unsafe { device.create_pipeline_cache(&vk::PipelineCacheCreateInfo::default(), None) })
            .map_err(|e| err("pipeline cache", e))?;
        let v = props.api_version;
        let info = format!(
            "{name} ({drv}; Vulkan {}.{}.{}; subgroup {}{}; host import align {host_align})",
            vk::api_version_major(v),
            vk::api_version_minor(v),
            vk::api_version_patch(v),
            sg.subgroup_size,
            if full_sg { ", full" } else { "" }
        );
        Ok(Device {
            // replaced by the caller with the owning handle (this one is a copy of its function table)
            instance: instance.clone(),
            device,
            queue,
            qfam,
            host,
            host_align,
            mem,
            max_range: props.limits.max_storage_buffer_range as u64,
            cache,
            cache_path,
            full_subgroups: full_sg,
            subgroup: sg.subgroup_size,
            info,
            software: dtype == vk::PhysicalDeviceType::CPU,
            ns: t0.elapsed().as_nanos() as u64,
        })
    }

    /// Compute pipelines of `layout` from build-time SPIR-V (`(bytes, entry point)`), through the pipeline cache.
    /// `full`: require full subgroups where the device can (kernels whose subgroup operations take any size).
    pub fn pipelines(&self, layout: vk::PipelineLayout, mods: &[(&[u8], &CStr)], full: bool) -> Result<Vec<vk::Pipeline>, String> {
        let mut modules = Vec::new();
        for (spv, _) in mods {
            let w = spirv_words(spv);
            // SAFETY: SPIR-V produced and validated by naga at build time.
            match unsafe { self.device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&w), None) } {
                Ok(m) => modules.push(m),
                Err(e) => {
                    for m in modules {
                        // SAFETY: created above, unused.
                        unsafe { self.device.destroy_shader_module(m, None) };
                    }
                    return Err(err("shader module", e));
                }
            }
        }
        let flags = if full && self.full_subgroups { vk::PipelineShaderStageCreateFlags::REQUIRE_FULL_SUBGROUPS } else { vk::PipelineShaderStageCreateFlags::empty() };
        let cis: Vec<vk::ComputePipelineCreateInfo> = mods
            .iter()
            .zip(&modules)
            .map(|((_, name), &m)| {
                vk::ComputePipelineCreateInfo::default().layout(layout).stage(vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::COMPUTE).module(m).name(name).flags(flags))
            })
            .collect();
        // SAFETY: well-formed create infos on the live device.
        let r = unsafe { self.device.create_compute_pipelines(self.cache, &cis, None) };
        for m in modules {
            // SAFETY: the pipelines are created (or failed); the modules are no longer needed.
            unsafe { self.device.destroy_shader_module(m, None) };
        }
        r.map_err(|(_, e)| err("compute pipelines", e))
    }

    fn mem_type(&self, bits: u32, want: vk::MemoryPropertyFlags, prefer: vk::MemoryPropertyFlags) -> Option<u32> {
        let n = self.mem.memory_type_count as usize;
        let ok = |i: usize, f: vk::MemoryPropertyFlags| bits & (1 << i) != 0 && self.mem.memory_types[i].property_flags.contains(f);
        (0..n).find(|&i| ok(i, want | prefer)).or_else(|| (0..n).find(|&i| ok(i, want))).map(|i| i as u32)
    }

    /// A buffer of `size` bytes: device-local (`host` false) or host-visible and coherent, persistently mapped and
    /// zeroed (device-local too when the device has such memory: resizable BAR, unified memory).
    pub fn buffer(&self, size: u64, usage: vk::BufferUsageFlags, host: bool) -> Result<Buf, String> {
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
            let Some(t) = self.mem_type(req.memory_type_bits, want, prefer).or_else(|| self.mem_type(req.memory_type_bits, vk::MemoryPropertyFlags::empty(), vk::MemoryPropertyFlags::empty())) else {
                d.destroy_buffer(buf, None);
                return Err("no memory type".into());
            };
            let mem = match d.allocate_memory(&vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(t), None) {
                Ok(m) => m,
                Err(e) => {
                    d.destroy_buffer(buf, None);
                    return Err(err("allocate", e));
                }
            };
            d.bind_buffer_memory(buf, mem, 0).map_err(|e| err("bind", e))?;
            let ptr = if host { Some(NonNull::new(d.map_memory(mem, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty()).map_err(|e| err("map", e))? as *mut u8).ok_or("map")?) } else { None };
            if let Some(p) = ptr {
                std::ptr::write_bytes(p.as_ptr(), 0, size as usize);
            }
            Ok(Buf { buf, mem, size, ptr })
        }
    }

    /// Free a buffer of this device.
    pub fn free(&self, b: Buf) {
        if b.buf == vk::Buffer::null() {
            return;
        }
        // SAFETY: the caller guarantees no pending GPU work uses the buffer (it drained or synced).
        unsafe {
            self.device.destroy_buffer(b.buf, None);
            self.device.free_memory(b.mem, None);
        }
    }

    /// Import `size` bytes of caller memory at `ptr` (aligned to `host_align`; the length is rounded up to it) as a
    /// buffer.
    ///
    /// # Safety
    /// The memory stays mapped, rounded length included, and is not freed while the import lives.
    pub unsafe fn import(&self, ptr: *mut u8, size: usize, usage: vk::BufferUsageFlags) -> Result<Import, String> {
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
            let buf =
                d.create_buffer(&vk::BufferCreateInfo::default().size(size).usage(usage).sharing_mode(vk::SharingMode::EXCLUSIVE).push_next(&mut ext), None).map_err(|e| err("import buffer", e))?;
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
            if let Err(e) = d.bind_buffer_memory(buf, mem, 0) {
                d.destroy_buffer(buf, None);
                d.free_memory(mem, None);
                return Err(err("bind import", e));
            }
            Ok(Import { buf, mem, ptr, size })
        }
    }

    /// Release an import (the caller's memory stays the caller's).
    pub fn drop_import(&self, i: Import) {
        if i.buf == vk::Buffer::null() {
            return;
        }
        // SAFETY: the caller guarantees no pending GPU work uses it.
        unsafe {
            self.device.destroy_buffer(i.buf, None);
            self.device.free_memory(i.mem, None);
        }
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: the owner destroyed what it created from the device (pipelines, pools, buffers) after waiting for
        // it to be idle; the cache, the device and the instance were created here.
        unsafe {
            let _ = self.device.device_wait_idle();
            if let (Some(p), Ok(data)) = (&self.cache_path, self.device.get_pipeline_cache_data(self.cache)) {
                if let Some(dir) = p.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(p, data);
            }
            self.device.destroy_pipeline_cache(self.cache, None);
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

/// SPIR-V bytes as words (`include_bytes!` is only byte aligned).
pub fn spirv_words(bytes: &[u8]) -> Vec<u32> {
    assert!(bytes.len().is_multiple_of(4), "SPIR-V is a whole number of words");
    bytes.chunks_exact(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect()
}

/// The Vulkan library, opened once and never closed: a driver's threads (llvmpipe's rasterisers, shader-cache
/// queues) can outlive the instance that started them, and closing the library under them is a crash at exit or at the
/// next probe. `BOUGH_VK_LIB`: a Vulkan library to open instead of the loader (an ICD that exports
/// `vkGetInstanceProcAddr`, e.g. MoltenVK's dylib).
pub fn entry() -> Result<&'static ash::Entry, String> {
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

/// Set once a search found no hardware device (or no Vulkan at all): later default searches in this process answer
/// at once (a machine without a GPU pays the loader and the enumeration once, not at every executor).
static NO_HARDWARE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A default (hardware-only) search is pointless here: no render node, or a search already found no hardware device.
pub fn no_hardware() -> bool {
    !hardware_possible() || NO_HARDWARE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Something built on another thread from process entry (a device, its pipelines, its measured costs) that the
/// executor made later takes.
pub struct Pending<T>(std::sync::Mutex<Option<std::thread::JoinHandle<Result<T, String>>>>);

impl<T: Send + 'static> Pending<T> {
    pub const fn new() -> Pending<T> {
        Pending(std::sync::Mutex::new(None))
    }
    /// Start `f` on a thread named `name` (replacing nothing: a build already started is kept).
    pub fn start(&self, name: &str, f: impl FnOnce() -> Result<T, String> + Send + 'static) {
        let mut g = self.0.lock().unwrap();
        if g.is_none() {
            *g = Some(std::thread::Builder::new().name(name.into()).spawn(f).expect("bough-vk thread"));
        }
    }
    /// A build is running or finished and not yet taken.
    pub fn started(&self) -> bool {
        self.0.lock().unwrap().is_some()
    }
    /// The build is finished (taking it will not block).
    pub fn ready(&self) -> bool {
        self.0.lock().unwrap().as_ref().is_some_and(|h| h.is_finished())
    }
    /// The build's result, waiting for it; `None` if none was started.
    pub fn take(&self) -> Option<Result<T, String>> {
        let h = self.0.lock().unwrap().take()?;
        Some(h.join().unwrap_or_else(|_| Err("bough-vk build thread panicked".into())))
    }
}

impl<T: Send + 'static> Default for Pending<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// The pre-armed stream of one executor: frame f (from 1) waits on the timeline semaphore for 2f - 1 and signals 2f.
/// Frames are submitted ahead (`arm`) and released from the host (`release`), so a frame costs one host signal; the
/// next frames' submission happens after the release, while the GPU draws.
pub struct Stream {
    pub tl: vk::Semaphore,
    /// The last frame released.
    pub released: u64,
    /// The last frame submitted.
    pub armed_upto: u64,
}

impl Stream {
    pub fn new(d: &Device) -> Result<Stream, String> {
        let mut tt = vk::SemaphoreTypeCreateInfo::default().semaphore_type(vk::SemaphoreType::TIMELINE).initial_value(0);
        // SAFETY: a well-formed create info on the live device.
        let tl = unsafe { d.device.create_semaphore(&vk::SemaphoreCreateInfo::default().push_next(&mut tt), None) }.map_err(|e| err("timeline semaphore", e))?;
        Ok(Stream { tl, released: 0, armed_upto: 0 })
    }

    /// The semaphore's value: 2f once frame f is drawn.
    pub fn counter(&self, d: &Device) -> u64 {
        // SAFETY: this stream's semaphore.
        unsafe { d.device.get_semaphore_counter_value(self.tl) }.unwrap_or(0)
    }

    /// Wait until the semaphore reaches `v` (one read of the counter when it already has).
    pub fn wait(&self, d: &Device, v: u64) {
        if self.counter(d) >= v {
            return;
        }
        let (sems, vals) = ([self.tl], [v]);
        // SAFETY: this stream's semaphore; the value is signalled by a submitted, released frame. (30 s: a software
        // device compiles its shaders at the first dispatch, seconds on a loaded host.)
        let r = unsafe { d.device.wait_semaphores(&vk::SemaphoreWaitInfo::default().semaphores(&sems).values(&vals), 30_000_000_000) };
        if let Err(e) = r {
            panic!("bough-vk: GPU frame {} did not finish within 30 s: {e:?}", v / 2);
        }
    }

    fn signal(&self, d: &Device, v: u64) {
        // SAFETY: the value is above the counter (frames are released in order).
        unsafe { d.device.signal_semaphore(&vk::SemaphoreSignalInfo::default().semaphore(self.tl).value(v)) }.expect("vkSignalSemaphore");
    }

    /// Submit frames `armed_upto + 1 ..= upto`, frame f running `cb(f)` (executable, `SIMULTANEOUS_USE`), each
    /// waiting for its release.
    pub fn arm(&mut self, d: &Device, upto: u64, cb: impl Fn(u64) -> vk::CommandBuffer) {
        if self.armed_upto >= upto {
            return;
        }
        let from = self.armed_upto + 1;
        let n = (upto - self.armed_upto) as usize;
        let waits: Vec<u64> = (from..=upto).map(|f| 2 * f - 1).collect();
        let sigs: Vec<u64> = (from..=upto).map(|f| 2 * f).collect();
        let cbs: Vec<vk::CommandBuffer> = (from..=upto).map(cb).collect();
        let sem = [self.tl];
        let stage = [vk::PipelineStageFlags::COMPUTE_SHADER | vk::PipelineStageFlags::DRAW_INDIRECT];
        let mut tsi: Vec<vk::TimelineSemaphoreSubmitInfo> =
            (0..n).map(|i| vk::TimelineSemaphoreSubmitInfo::default().wait_semaphore_values(&waits[i..i + 1]).signal_semaphore_values(&sigs[i..i + 1])).collect();
        let subs: Vec<vk::SubmitInfo> = tsi
            .iter_mut()
            .enumerate()
            .map(|(i, t)| vk::SubmitInfo::default().wait_semaphores(&sem).wait_dst_stage_mask(&stage).command_buffers(&cbs[i..i + 1]).signal_semaphores(&sem).push_next(t))
            .collect();
        // SAFETY: the command buffers are executable and SIMULTANEOUS_USE (the caller's contract); every wait has a
        // host signal to come (a release, or `run_out`).
        unsafe { d.device.queue_submit(d.queue, &subs, vk::Fence::null()) }.expect("vkQueueSubmit");
        self.armed_upto = upto;
    }

    /// Release the next frame (it must be armed); returns its number.
    pub fn release(&mut self, d: &Device) -> u64 {
        let f = self.released + 1;
        debug_assert!(self.armed_upto >= f, "released a frame that was not armed");
        self.signal(d, 2 * f - 1);
        self.released = f;
        f
    }

    /// Wait for the last released frame.
    pub fn idle(&self, d: &Device) {
        self.wait(d, 2 * self.released);
    }

    /// The last released frame is drawn.
    pub fn drawn(&self, d: &Device) -> bool {
        self.counter(d) >= 2 * self.released
    }

    /// Release and wait for every armed frame (the caller made them draw nothing): what the frames bind may then
    /// change. Returns whether any ran.
    pub fn run_out(&mut self, d: &Device) -> bool {
        self.idle(d);
        if self.armed_upto == self.released {
            return false;
        }
        for f in self.released + 1..=self.armed_upto {
            self.signal(d, 2 * f - 1);
            self.wait(d, 2 * f);
        }
        self.released = self.armed_upto;
        true
    }

    /// Destroy the semaphore (the stream is run out and the device idle).
    pub fn destroy(&self, d: &Device) {
        // SAFETY: no pending work waits on or signals it (the caller's contract).
        unsafe { d.device.destroy_semaphore(self.tl, None) };
    }
}

/// GPU completion as a pollable fd (Linux): a thread waits on a stream's semaphore for the values it is sent and
/// writes an eventfd when each is reached, so a window's event loop can commit a frame the moment the GPU finishes it
/// without the input thread waiting (the role of Metal's publisher thread; Wayland's objects stay on the loop's thread).
#[cfg(target_os = "linux")]
pub struct Waiter {
    fd: std::os::fd::OwnedFd,
    tx: Option<std::sync::mpsc::Sender<u64>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "linux")]
impl Waiter {
    /// # Safety
    /// `tl` is a timeline semaphore of `device` that outlives this waiter (it joins its thread when dropped), and every
    /// value sent is one that will be signalled.
    pub unsafe fn new(device: ash::Device, tl: vk::Semaphore) -> Waiter {
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
                    // SAFETY: the caller's contract: the semaphore outlives this thread, and the value comes.
                    let _ = unsafe { device.wait_semaphores(&vk::SemaphoreWaitInfo::default().semaphores(&sems).values(&vals), 10_000_000_000) };
                    let one: u64 = 1;
                    // SAFETY: the eventfd lives as long as the `Waiter` (which joins this thread first).
                    unsafe { libc::write(wfd, (&one as *const u64).cast(), 8) };
                }
            })
            .expect("waiter thread");
        Waiter { fd, tx: Some(tx), thread: Some(thread) }
    }
    pub fn fd(&self) -> std::os::fd::RawFd {
        use std::os::fd::AsRawFd;
        self.fd.as_raw_fd()
    }
    /// Make the fd readable once the semaphore reaches `v`.
    pub fn notify(&self, v: u64) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(v);
        }
    }
    /// Read the fd empty (it is non-blocking).
    pub fn drain(&self) {
        let mut v: u64 = 0;
        // SAFETY: a non-blocking eventfd this waiter owns.
        unsafe { libc::read(self.fd(), (&mut v as *mut u64).cast(), 8) };
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

/// A page of the virtual memory system (16 KiB on Apple silicon, 4 KiB elsewhere).
pub fn vm_page() -> usize {
    // SAFETY: a plain query.
    let p = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if p > 0 { p as usize } else { 4096 }
}
