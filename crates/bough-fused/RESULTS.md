# bough-fused: the fused presenter on Vulkan and OpenGL (2026-11-14, branch `lead/vk-gl`)

The façade's production GPU path is bough-metal's fused presenter: three Metal kernels (`fs_copy`, `fs_bin`, `fs_tile`
in `crates/bough-metal/src/shader.metal`) that repair a frame's damaged 32 px tiles from the region the CPU writes in
place. This crate runs the same kernels, ported, on Vulkan and on OpenGL, and the façade uses them on Linux
(`crates/bough/src/backend/gpu.rs`). Nothing in the frame contract changed: region, header, slots, payload encodings,
the bin layout and the certificates' decisions are bough-metal's (INTERFACE.md).

## 1. What exists

**One kernel source** (`src/shaders/fused.rs`, wgsl-rs). build.rs turns it into WGSL, validates it with naga
(`ValidationFlags::all()`), and writes per entry point:

* SPIR-V 1.3 for Vulkan, with the array strides naga puts on workgroup arrays removed (Vulkan 1.3's SPIR-V
  environment forbids them: VUID-StandaloneSpirv-None-10684);
* GLSL 4.30 core for OpenGL in two forms: with `KHR_shader_subgroup` arithmetic, and subgroup-free (every invocation a
  one-lane subgroup, the ordered prefix sums as an 8-step workgroup-memory scan) for drivers without it (llvmpipe).
  The GL forms read the region as one buffer (build.rs rewrites the column readers `c_*` to read it at the header's
  offsets); the Vulkan form reads each column through its own binding.

What the port changes against the Metal kernels, and why:

| Metal | Portable kernels | Why |
|---|---|---|
| columns through GPU addresses (`FCols`) | one storage binding per column | no pointers in WGSL; Vulkan binds the same views Metal makes |
| surfaces are IOSurface textures (`FBind` resource ids) | storage buffers of BGRA8 words (`S0..S3`) | the window's `wl_shm` buffers can then be the surfaces (host-pointer import) |
| glyph and image atlases are textures | words (R8 four to a word, RGBA8) | no image layouts, no staging; the CPU writes them in place |
| a certificate holds a half4 | a certificate holds the stored BGRA8 word | a FILL writes back exactly the stored bytes, nothing converted |
| compaction over 8 simdgroups of 32 | over `num_subgroups` subgroups of any size | lavapipe's subgroups are 8 lanes, MoltenVK's 32 |
| a band certified by one simdgroup | by workgroup memory (32 invocations, 2 barriers) | a band is not a subgroup at other sizes |
| half accumulation | f32 accumulation | portability; see §4 for the cost |
| transformed records stage their inverse | recompute it from the space at shading | keeps the tile pass's workgroup memory at 12 KB (16 KB is Vulkan's guaranteed minimum) |

**The Vulkan presenter** (`src/vk.rs`, ash; Linux, and macOS through MoltenVK), point for point against `bough_metal::fused`:

* *Region without a copy:* the region's head (header, slots) and each GPU-read column's used prefix are the façade's
  own pages, imported with `VK_EXT_external_memory_host` (Metal: `newBufferWithBytesNoCopy`); `ensure_columns` grows a
  view to the next power of two as Metal's `new_views` does. A column's capacity stays address space.
* *Surfaces without a copy:* a surface is the window's buffer (a `wl_shm` buffer of the compositor's memfd) or memory
  offscreen, imported the same way: the tile pass writes the words the compositor reads (Metal: an IOSurface).
* *One pre-armed stream:* the two frame command buffers (one per slot) are recorded once; frames are submitted ahead,
  each waiting on a timeline semaphore for 2f - 1 and signalling 2f; a frame is a slot write and one host signal
  (`vkSignalSemaphore`; Metal: `MTLSharedEvent.signaledValue`), and the next frames' submission happens after the
  release, while the GPU draws. Dispatch sizes come from the slot (indirect dispatch), so recorded frames never change.
* *Drains* replace Metal's residency drains: anything that changes what the recorded frames bind (a view, a surface,
  the bins, the layout, a resize) first runs the armed frames out as empty ones, then rewrites the descriptors and
  re-records.
* *Bins sized by need:* an overflowing row records its need; the CPU grows the bins and redraws the frame before it is
  presented (Metal's `check_bins`).
* *Presentation without waiting (windowed):* a waiter thread waits on the timeline semaphore and writes an eventfd;
  the Wayland loop polls it with the compositor's socket and commits the frame the moment it is drawn
  (`App::present_fd`, `present_ready`; Metal: the publisher thread). Offscreen, frames are waited for.
* *Start-up:* the device is built on another thread from process entry (`prepare_in_background`, as Metal's); the
  pipelines come from build-time SPIR-V through a pipeline cache kept on disk (`$XDG_CACHE_HOME/bough`); full
  subgroups are required where the device can (`REQUIRE_FULL_SUBGROUPS`); a compute-only queue family is preferred.
  A process that found no hardware device does not search again.
* `BOUGH_VK_VALIDATE=1` runs under the Khronos validation layer; `BOUGH_VK_DEVICE` picks a device;
  `BOUGH_VK_LIB` opens a Vulkan library directly (MoltenVK's dylib without a loader).

**The OpenGL presenter** (`src/gl.rs`, Linux, EGL; no window of its own): the region is one persistently mapped,
coherent buffer (`ARB_buffer_storage`) the façade carves its columns in (GL cannot import host memory, so the GL
region has the configured capacities, as the wgpu-hal Metal port's); frames are three indirect dispatches and a fence;
surfaces are persistently mapped buffers whose changed tiles (the damage and the scroll copy's rect) are copied into
the window's buffer after the fence; the context is made surfaceless (`EGL_MESA_platform_surfaceless`) on another
thread from process entry and handed over; program binaries are cached on disk per renderer and source.

**The façade** (`crates/bough/src/backend/gpu.rs`, `app.rs`): on Linux `BOUGH_GPU` (or `Config::executor`) picks
`vulkan`, `gl` or `cpu`; unset, it is Vulkan on a hardware device, otherwise the CPU executor (bough's SIMD executor
beats a software rasteriser running these kernels, §3). On macOS `BOUGH_GPU=vulkan` runs the Vulkan executor headless
through MoltenVK in place of the Metal presenter (for gates on a real GPU); the Metal presenter stays the default.

## 2. How it was checked

No machine here has a Linux GPU (ilo-prod and mothership have BMC display chips), so the drivers were emulated or
translated: **lavapipe** (Mesa 26.2.6's software Vulkan, LLVM 21, 8 threads, subgroups of 8) and **llvmpipe** (Mesa's
software OpenGL 4.6, no `KHR_shader_subgroup`) on ilo-prod, and **MoltenVK 1.4.2** (Vulkan on Metal) on an M3 Pro, the
only real GPU available. Windows were never shown on the Mac; on Linux the windowed runs used fakeland (zortax's headless
Wayland compositor, pixman renderer).

* **`comparisons/bough-zgui/examples/gpu_gate.rs`** on zgui's laid-out gallery (1600 x 1000, about 2,400 nodes: text
  in several faces, rounded boxes, borders, linear and radial gradients, shadows, vector icons, the turning spinner
  image, pulses, the text field, scrolling): each step drives the document and checks the presented surface against a
  full redraw by the same kernels. **Every step exact** on lavapipe, llvmpipe, MoltenVK and Metal.
* **`crates/bough/tests/gpu_target.rs`**: the CPU executor's gate (retained frames equal a full redraw at every ring
  age through scrolls, hovers and structural changes), bins of 2 entries forced to grow and the frame redrawn, and three
  resizes. **6 of 6 pass** on ilo-prod (Vulkan on lavapipe under the validation layer, **0 validation errors**; GL on
  llvmpipe); **3 of 3 Vulkan tests pass** on the M3 Pro through MoltenVK.
* **Against Metal's production kernels** (the same gallery frames, raw BGRA): Vulkan on lavapipe, GL on llvmpipe and
  Vulkan on MoltenVK each differ from Metal in 1,482–1,909 of 1,600,000 pixels (about 0.1 %), **every one by one
  level in one channel** (f32 against half rounding). Vulkan and GL agree with each other on those frames. Against the
  CPU executor: 1,116–3,343 pixels, at most 2 levels.
* **Windowed under fakeland** (1280 x 800 at 60 Hz, the zgx lab with streaming text and a scrolling list, 6 s): every
  executor commits and presents correctly (screenshots checked).

## 3. Measured

| | executor | frame p50 | p90 | notes |
|---|---|---:|---:|---|
| gallery, ilo-prod | CPU executor | 1.67–1.93 ms | 3.3–4.0 ms | 6 drawn frames of 12 steps, cores 8–15 at nice 19, host at load 15–20 |
| | Vulkan, lavapipe | 50–61 ms | 53–79 ms | a software rasteriser running GPU kernels |
| | GL, llvmpipe | 108 ms | 119 ms | subgroup-free form |
| gallery, M3 Pro | Metal (direct) | 1.32–1.33 ms | 3.0–3.9 ms | 18 drawn frames of 48 steps; Mac at load 58 |
| | Vulkan, MoltenVK | 2.44–2.57 ms | 3.2–3.4 ms | same runs |

| windowed, fakeland | fps | missed vblanks | display latency p50 |
|---|---:|---:|---:|
| CPU executor | 59.8–60.0 | 0–1 | 7.06 ms |
| Vulkan (lavapipe), waited | 58.8 | 7 | |
| Vulkan (lavapipe), presented when signalled | 58.7 | 8 | 5.69 ms |
| GL (llvmpipe) | 31–33 | 160–171 | |

Start-up, to a presenter ready to draw: Vulkan with a warm pipeline cache 353–482 ms on lavapipe (5.4 s cold, under the
validation layer) and 34–59 ms on MoltenVK (5.3 s cold); lavapipe also compiles its shaders at the first dispatch
(3–4 s cold, then from Mesa's disk cache), which left the first windowed run's screen black at 4 s.

## 4. Open

* **No real Linux GPU has run it** (radv, anv, NVIDIA; GL on any hardware). The default executor on Linux is
  Vulkan on hardware, so the first run on a GPU desktop is its first test there; `BOUGH_GPU=cpu` restores the CPU
  executor.
* **Zero copy into a GPU compositor:** surfaces are `wl_shm` (the compositor uploads them). The next step is dmabuf
  surfaces (`VK_EXT_external_memory_dma_buf`, `zwp_linux_dmabuf_v1`) with explicit sync
  (`wp_linux_drm_syncobj_v1`: commit at release, the compositor waits on the GPU); lavapipe cannot test it.
* **Half precision:** the port accumulates in f32; Metal's half doubles ALU throughput on Apple GPUs and is part of
  the 1.9x gap on the M3 Pro (MoltenVK's translation is the rest, unmeasured separately). An f16 variant
  (`shaderFloat16`) is the first thing to try on a real GPU.
* **Band certificates by subgroup** where subgroups are 32 wide (Metal's form) instead of workgroup memory.
* **GL presents synchronously** (no waiter: a GL context is bound to one thread); its region has the configured
  capacities (no views).
* **One source for Metal too:** the Metal presenter still runs the hand-written MSL; the portable kernels produce MSL
  through naga as well (bough-gpu's path) and could replace them once they are as fast.
* **Calm trims** (Metal marks idle surfaces purgeable): the window's buffers belong to the host on Linux; bins and
  certificates are not trimmed.
