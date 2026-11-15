# bough-fused: the fused presenter on Vulkan and OpenGL (2026-11-14, branch `lead/vk-gl`)

The façade's production GPU path on macOS is bough-metal's fused presenter: three Metal kernels (`fs_copy`, `fs_bin`,
`fs_tile` in `crates/bough-metal/src/shader.metal`) that repair a frame's damaged 32 px tiles from the region the CPU
writes in place. This crate runs the same kernels, ported, on Vulkan and on OpenGL; the façade runs them when asked
(`BOUGH_GPU=fused-vulkan|fused-gl`, `crates/bough/src/backend/gpu.rs`). Nothing in the frame contract changed: region,
header, slots, payload encodings, the bin layout and the certificates' decisions are bough-metal's (INTERFACE.md).

**Linux's default is not this crate** (§5, round 2): it is the mixed model, the CPU executors and, on a hardware device,
bough-kernel's Vulkan executor, chosen per frame by measured costs. Its Vulkan device layer, `crates/bough-vk`, is the one
this crate's presenter runs on.

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
  A process that found no hardware device does not search again, and on Linux the default does not search at all
  without a DRM render node (or NVIDIA's device files), so a machine without a GPU loads no Vulkan library. The
  library, once opened, is never closed: a driver's threads (llvmpipe's rasterisers) outlive the instance that
  started them, and closing it under them crashed `tests/capacity` after a probe found only lavapipe.
* `BOUGH_VK_VALIDATE=1` runs under the Khronos validation layer; `BOUGH_VK_DEVICE` picks a device;
  `BOUGH_VK_LIB` opens a Vulkan library directly (MoltenVK's dylib without a loader).

**The OpenGL presenter** (`src/gl.rs`, Linux, EGL; no window of its own): the region is one persistently mapped,
coherent buffer (`ARB_buffer_storage`) the façade carves its columns in (GL cannot import host memory, so the GL
region has the configured capacities, as the wgpu-hal Metal port's); frames are three indirect dispatches and a fence;
surfaces are persistently mapped buffers whose changed tiles (the damage and the scroll copy's rect) are copied into
the window's buffer after the fence; the context is made surfaceless (`EGL_MESA_platform_surfaceless`) on another
thread from process entry and handed over; program binaries are cached on disk per renderer and source.

**The façade** (`crates/bough/src/backend/gpu.rs`, `app.rs`, `kernel_backend.rs`), by `BOUGH_GPU` (or
`Config::executor`): unset, the mixed model (on Linux its GPU is a hardware Vulkan device where one exists, §5);
`vulkan`, the mixed model with a Vulkan device, software ones included; `cpu`, the mixed model without a GPU;
`fused-vulkan`, `fused-gl`, this crate's presenters, every frame on the GPU. On macOS the Metal presenter stays the
default; `fused-vulkan` runs this crate's Vulkan presenter headless through MoltenVK, and `vulkan` with
`BOUGH_REPAIR=kernel` runs the mixed model with the Vulkan executor through MoltenVK (for comparisons on one GPU).

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
* **The whole `bough` suite on Linux** (19 test targets, 45 tests; `tests/startup.rs` imports bough-metal and
  never built on Linux) passes with the executors in the default mode, `gpu_target` included; the Metal pixel gates
  on macOS (`scroll_box`, `far`, `seals`, `cpu_target`, `capacity`, `counter`, `select_all`) pass unchanged.
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

* **No real Linux GPU has run it** (radv, anv, NVIDIA; GL on any hardware). Since round 2 (§5) Linux's default is the
  mixed model; a hardware Vulkan device joins it only when a frame's measured costs favour it, so on a GPU desktop the
  first such frame is the executor's first test there; `BOUGH_GPU=cpu` keeps every frame on the CPU.
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

## 5. Round 2: Vulkan inside the mixed model (predictions frozen 2026-11-14, before measuring)

The owner's direction after §1-4: Metal's speed, every benchmark number held, tested on ilo-prod, and the mixed
CPU/GPU model kept. Measured first (M3 Pro, gallery gate, p50): the mixed model (`BOUGH_REPAIR=kernel`, the costs
choosing among one core, the pool and Metal) 0.54 ms; the pool alone 0.57; the Metal presenter 1.32; the mixed model
with Metal forced 1.79; the fused Vulkan port through MoltenVK 2.33. So the fused port as Linux's default (§1) replaced
the fastest path with a 4x slower one on this workload. The change: Linux keeps the mixed model by default, and a
hardware Vulkan device joins it as its GPU executor, running bough-kernel's integer kernel (the CPU executors' bytes,
so a switch leaves no seam) on bough-vk, the device layer now shared with the fused port; the fused presenters are
opt-in (`BOUGH_GPU=fused-vulkan|fused-gl`).

Predictions and kill lines:

* **P1** (M3 Pro, gallery, the mixed model's GPU forced): Vulkan through MoltenVK p50 within 1.10x of Metal's. Kill:
  above 1.25x.
* **P2** (same, the costs choosing): the mixed model with Vulkan within 1.10x of the mixed model with Metal. Kill:
  above 1.25x.
* **P3**: every gate step exact with the GPU forced and with the executors switched frame by frame (the CPU
  definition's bytes), on MoltenVK and on lavapipe.
* **P4** (ilo-prod, no render node): the gui-bench `bough` row from this branch equals main's within the spread of two
  runs of main's binary, in fixed and unthrottled timing; no Vulkan library is loaded.
* **P5** (ilo-prod, lavapipe allowed in the costs' choice): the gallery sends no frame to it (its measured costs lose
  to the pool's), so its frame times equal the CPU executors'.

### 5.1 What was built

* `crates/bough-vk`: the device layer both executors use. The loader is opened once, behind the render-node gate. It
  also holds the device search, host-memory imports, mapped buffers, pipelines through a disk cache per executor
  (`vk-kernel-*`, `vk-fused-*`), the pre-armed timeline-semaphore stream (`Stream`), the completion waiter and builds
  on another thread (`Pending`). This crate's presenter now runs on it.
* `crates/bough-kernel/src/vulkan.rs`, rewritten on bough-vk (wgpu-hal is gone from bough-kernel). One command buffer
  is recorded per set of targets, with an indirect dispatch per target (up to 8 plus its own output), each reading its
  workgroup count from a mapped arguments buffer. The next frame is submitted while the GPU draws this one. A frame is a
  copy of its commands and program into mapped memory, the chosen target's count, one host signal and one wait. Costs
  are measured when the executor is built (`calibrate`: eight empty round trips, then a probe of list rows, 512 x 512 px
  in 256 tiles) and corrected a quarter of the way toward each GPU frame's wall time. It builds on macOS too, through
  MoltenVK.
* `crates/bough/src/kernel_backend.rs`: the GPU of the choice is Apple's (`COSTS`, unchanged) or a Vulkan device's
  measured costs. A device still being built is never waited for (the CPU draws). Without a render node nothing is
  built and every decision is main's.
* Fixed on the way, each by a new test:
  - The kernel backend forgets its GPU targets on every resize. On macOS the Metal presenter's new IOSurfaces came
    back at old addresses, so the GPU kept writing the old pages.
  - This crate's reference render grows overflowed bins and draws again. Bins overflowing on a frame after the first
    had never been tested; the presenter's own redraw was already right, the oracle was not.

### 5.2 Measured

**M3 Pro, gallery gate** (96 steps, 36 drawn frames, p50; the Mac at load 35-70, so runs alternate). The gate now calls
`app::prepare` at entry, as applications do.

| | Metal | Vulkan (MoltenVK) | ratio |
|---|---:|---:|---:|
| mixed model, GPU forced | 2.16, 2.20, 2.42 ms | 1.64, 1.58, 1.54 ms | 0.71 (**P1 holds**) |
| mixed model, the costs choosing | 0.79, 0.94, 0.91 ms | 0.84, 0.91, 0.95 ms | 1.03 (**P2 holds**) |
| the CPU executors only | 0.84, 0.89, 0.96 ms | | |
| Metal presenter (macOS default) / fused Vulkan port | 1.33 ms (48 steps, as in §3) | 2.51-2.55 ms | |

With the costs choosing, both GPUs get none of the gallery's 50 frames (13 on one core, 37 on the pool). Its repairs
are small enough that the pool wins. Before the gate called `prepare`, the Vulkan device was built during the timed
frames and cost 0.2-0.4 ms per frame. The Vulkan device's calibrated costs on the M3 Pro: a 25-65 us round trip (Metal's
written constant: 150 us) and 0.5-1.1 us per tile (Metal's: 0.18).

**ilo-prod, functional** (commit `d8ee4a7ade`; every step exact, 0 validation errors):

| | result |
|---|---|
| `bough` suite, default | 19 targets, 48 tests, all pass |
| `tests/gpu_target.rs` under the validation layer | 9 of 9 (mixed model on lavapipe, GPU forced and switching per frame; fused Vulkan and GL) |
| bough-kernel `exact.rs` on lavapipe | 0 differing words; rounding mutant caught in 4 of 4 scenes |
| gallery, default (no render node) | 1.52 ms p50; GPU never built |
| gallery, mixed model with lavapipe forced | 11.55 ms (the fused Vulkan port: 63.1; fused GL: 106.1) |
| gallery, lavapipe allowed in the choice | 0 of 15 frames to it (**P5 holds** for the choice); 2.09 ms p50 under load 20, its build overlapping the timed frames |

**ilo-prod, gui-bench's `bough` row (P4).** main `17635c1c98` (binary `a7f8a95c`, 55.9 MB) against this branch at
`d8ee4a7ade` (binary `24ff909c`, 46.7 MB: bough-kernel no longer links wgpu-hal). Setup:

* Runner: round 8's patched gui-bench runner, a private build of the identical source, pinned as round 8's final runs
  (`taskset -c 0-7 nice -n 10`). 46 cells per run.
* Order: main and branch alternating (A1 B1 A2 B2) in fixed 60 Hz timing, then again unthrottled
  (`gb-ab.sh`, `tools/gbcmp.py`, `gbnull.py`, `gbkey.py`, `gbrun.py` in the lane).
* Host: load 17-26 throughout from another user's builds. Round 8's final runs had a quiet host, so these absolute
  numbers are not comparable to those.

The test of a difference is a null comparison: the same statistic for the two splits that mix the binaries
(A1,B1 | A2,B2 and A1,B2 | A2,B1), which can differ only by noise and drift.

| | fixed | unthrottled |
|---|---|---|
| metrics in all four runs | 2,950 | 3,744 |
| worse in every run by over 10 %: branch / null splits | 54 / 28, 43 | 91 / 104, 58 |
| cells with errors | the same in both binaries (round 8's known 10,000-row failures) | the same |
| end-to-end latency (`total_p50`), ratio (>1 worse) / nulls | 0.978 / 0.993, 1.013 | 0.910 / 1.106, 1.058 |
| `input_to_commit_p50` | 0.930 / 0.969, 1.059 | 0.910 / 1.106, 1.059 |
| CPU per second | 0.995 / 0.988, 1.026 | 0.962 / 0.987, 0.992 |
| frames per second | 1.005 / 1.005, 0.995 | 1.059 / 0.949, 0.980 |
| start latency | 1.024 / 1.220, 0.979 | 1.049 / 0.912, 0.949 |

Ratios are geometric means over cells, direction-adjusted. One main run (A2, fixed) also lost 8 cells to the harness's
dbus session failing to start.

Notes on the table:

* **Unthrottled frames per second and start latency** follow each run's load, not its binary. Relative to A1 (load
  26.0): B1 (23.5) 0.958 and 0.998; A2 (17.1) 1.034 and 0.863; B2 (21.8) 1.006 and 0.955.
* **CPU per frame**, the measure least sensitive to load: B1 0.955, A2 0.946, B2 0.937 of A1's.
* **Memory first looked worse**: peak RSS was 1.2 MB higher per cell unthrottled (0.2 MB fixed). All of it was the
  executable's resident code pages: main had 3.5 MB of its 6.1 MB of code resident, the branch 4.7 MB of its 4.9 MB.
  That is page-cache state, not code. With both binaries dropped from the page cache and loaded alike, the totals were
  14.77 and 14.90 MB for main against 14.88 and 14.94 MB for the branch (the `rows` scenario at 10 rows, unthrottled).
* **No Vulkan library** is mapped in the branch's process (no render node). **P4 holds** on this host, with the caveat
  of its load.

**The final commit** (`d5cefc2f2e`; the device made lazy after the A/B runs, which changes nothing without a render
node), on ilo-prod:

* `tests/gpu_target.rs` 9 of 9 and `exact.rs` under the validation layer.
* The gallery exact in every mode. With the costs choosing, no device is built at all, because no frame pays for one.
* Memory against main, both binaries dropped from the page cache first (`rows`, 1,000 rows, unthrottled): 19.32 and
  19.20 MB against 19.26 and 19.16 MB.
* Under fakeland with lavapipe pinned (`BOUGH_KERNEL_TRACE=1`), the window's frame (1,000 tiles, the whole window) was
  drawn by the Vulkan executor into the compositor's `wl_shm` buffer. Its screenshot is byte-identical to the CPU
  executor's.

### 5.3 Open

* **A real Linux GPU** (radv, anv, NVIDIA): the calibrated costs and the share of frames the choice gives it.
* **The atlas in device-local memory** on discrete GPUs without resizable BAR (S). Today it is host-visible, read over
  PCIe; the calibration measures that cost, so the choice already accounts for it.
* **OpenGL inside the mixed model** (S-M): the integer kernel as GLSL, and a copy of the repaired tiles, since GL cannot
  import host memory. Today OpenGL exists only as the opt-in fused presenter.
* **The fused port at the Metal presenter's speed** (2.5 against 1.33 ms on the M3 Pro): f16 needs half-precision types
  in wgsl-rs first; band certificates by subgroup.
* **A quiet-host re-run of the gui-bench A/B**: these were measured at load 17-26.
