# Effects locality, round 2

The 20-tick attribution repeats the original one-card trace so it can be compared with round 1. The façade is pinned to the integer CPU1 executor. The typed-core fixture paints opaque rectangles with arithmetic placement and a single colour tween using `Soft`. Both cases are headless on an M3 Ultra. Raw instruction, timing and load records are in `bench/results/effects/r2/`.

## Façade one-card phase attribution

Calling-thread instructions per input tick. `BOUGH_KPROF=1` brackets cull, staging, binning and raster. The phase means remove separately profiled startup. The outer frame and update brackets use the same call sites at every size. Residual is the exact difference between the rounded outer total and the reported row sums. It includes ring and presentation bookkeeping, instrumentation, and separately sampled initial-frame variation. At 200 the initial diagnostics make the damage-plane subphase too noisy to separate, so it stays in the residual at all sizes.

| Phase | 200 | 1,000 | 5,000 |
|---|---:|---:|---:|
| Input and tick | 7,349 | 32,949 | 160,949 |
| Page and record cull | 1,505 | 13,187 | 63,226 |
| Commands staged from records | 30,113 | 159,577 | 800,147 |
| Build tile programs | 31,735 | 122,447 | 610,053 |
| Decode, shade and store | 2,225,858 | 10,940,061 | 52,938,070 |
| Residual | 120,352 | 113,925 | 126,499 |
| Total | 2,416,912 | 11,382,146 | 54,698,944 |

The one translucent card changes its position at the front of the same stack. Damaged tiles cover the same 1.23% of the surface on average, but all contributors in those tiles are staged, binned and blended. The 5,000-card case spends 96.8% of calling-thread instructions in raster. The next largest terms are command staging and binning. Both get more candidates from the dense stack even when the damaged area stays constant. Sample attribution places `bough_kernel::cpu::shade_tile` and its inlined edge closure on the CPU. The input slope comes from `Rt::step_transitions`, which calls the type-erased `is_active` for every registered operand. The measured slope is exactly 32 additional calling-thread instructions per dormant card across both size increments. The fixture registers four inactive operands per ordinary card, giving eight instructions per dormant operand in this trace. The new sorted active IDs preserve original applier order and remove the table scan.

| Input/tick only | 200 | 1,000 | 5,000 |
|---|---:|---:|---:|
| Before | 7,349 | 32,949 | 160,949 |
| After active IDs | 1,053 | 1,053 | 1,053 |
| End-to-end after | 2,406,851 | 11,348,217 | 54,465,549 |

## Typed software core phase attribution

The calling-thread phase brackets separately measure `tick`, input state writes and `settle`, spreading damage to ring histories, and `Soft::repair` with `World::paint`. The sum below includes these four bracketed phases. It excludes outer process-counter and clock overhead. Visits count records passed to the painter, so the first use of each surface contributes full-frame work.

| Phase | 200 | 1,000 | 5,000 |
|---|---:|---:|---:|
| Clock and sweeps | 141 | 141 | 141 |
| Retarget words and settle | 122 | 116 | 116 |
| Extend ring damage | 1,383 | 1,383 | 1,383 |
| Repair and paint | 757,025 | 3,092,332 | 14,807,702 |
| Total | 758,671 | 3,093,972 | 14,809,342 |
| Record visits | 8,522 | 42,442 | 212,042 |
| Total after rectangle coalescing | 167,156 | 460,819 | 1,926,249 |
| Record visits after | 201 | 1,001 | 5,001 |

`Soft::repair` used to call `World::paint` once per marked tile, including each tile in a first-use ring surface. `Store::paint` walks the paint order, then `ColRef::each` walks live bits and loads the rectangle and its record before the leaf clips it. The unsupplied state operands, pointer and exposed active set are outside the growing phase in this trace. This adapter does not run the `bough-span` executor. Rectangle coalescing eliminates redundant walks and retains exact clipping via an area certificate. Ring ages, a nonrectangular control, and the existing animation fixture cover the change. The remaining full-store walk makes the 1.2× end-to-end target an open structural change.

The first use of each ring buffer contributed three large redraws to the 20-step mean. The warm-ring subtrace starts at step 3 and isolates repeated repair of the small damage. The data below are calling-thread paint instructions and record visits per tick in that subtrace.

| Cards | Before paint instructions | After paint instructions | Before visits | After visits |
|---:|---:|---:|---:|---:|
| 200 | 60,963 | 20,455 | 804 | 201 |
| 1000 | 273,171 | 84,399 | 4,004 | 1,001 |
| 5000 | 1,334,427 | 404,173 | 20,004 | 5,001 |

## Structural locality design

Carry stable generational paint order and the minimum changed key with damage. Attribute a move to its old and new tile windows, and attribute clip, paint, resource and ordering edits to the same dependency route. Maintain each tile's ordered references incrementally on intersecting edits. Removing and inserting a changed reference must leave the relative order of unrelated records intact. A tree of immutable ordered runs keeps large insertions and removals from rewriting each tile's entire list.

Store a checkpoint at the last stable boundary common to the active records of a tile. Capture the kernel accumulator in its exact 4×Q15 representation before display conversion. Reuse the checkpoint only if the minimum changed key is above its boundary and its clip, referenced resources and scene generation still match. A lower edit invalidates the affected checkpoint and restarts from the last eligible one or from transparent. Keep the checkpoint generation separate from the framebuffer age. Repair an older framebuffer from the current published checkpoint and relevant intervening damage. A stationary folded prefix needs no command emission, binning or shading on the hot path. One full-viewport checkpoint at 640×400 costs 2,048,000 sample bytes plus metadata. Budget it, admit only after observed reuse, and retain the full renderer for misses, changing layer schedules and sparse workloads.

For `Soft`, store the typed place or validated column slice in the same tile index and enumerate only the tile candidates. The damage rectangle certificate can remain for full and rectangular repairs. Add an opaque-only dominance certificate after audited order traversal. A tile query alone remains proportional to all overlapping records, so a stable prefix or a certified opaque covering suffix is required for the dense fixture. Hand off the `Soft` path and differential proof to the incremental lane. The façade's command list, binning and prefix integration belong with `fx`.

A candidate for the next measured locality pass needs old/new displacement across tile boundaries, a lower-plane edit, resource replacement and clip change, an insertion on either side of the checkpoint, stacked fractional translucent content, sparse damage with holes, and all four surface ages. Mutants should reuse a stale lower prefix and miss the old ink on a move. Scope the 1.2× test to the repeated top-card operations with a warmed valid checkpoint and report the cold latency and bytes as separate cells.

## 240-tick paired software comparison

Both binaries ran the same trace in A/B/B/A order. Process instruction means include the warm ring phase, the output-ring first-use frames and the same retarget input. Each table entry combines two blocks of 240 samples.

| Cards | Before instructions / tick | After instructions / tick | Before p50/p95/p99 ms | After p50/p95/p99 ms |
|---:|---:|---:|---|---|
| 200 | 120,427 | 34,450 | 0.003/0.003/0.253 | 0.001/0.001/0.122 |
| 1000 | 510,289 | 117,480 | 0.011/0.012/0.856 | 0.004/0.004/0.220 |
| 5000 | 2,459,356 | 532,911 | 0.055/0.057/3.744 | 0.018/0.019/0.780 |

## Exact CPU shadow cache

The raster lane already had corner tables in `bough-paint` and a staged prefix for backdrop records in `fx` at `0a92f72e54`. This patch targets the façade's `bough-kernel` CPU1 and CPU pool path. It caches the literal integer kernel's Q15 coverage on the active corner coordinates and leaves the existing SIMD separable edges, flat interior and occlusion cut in use. The key includes both extents, the Q8 phase at each origin, the radius and derived blur words, and the root shift. Clip and colour affect the blend window and source colour, so coverage stays valid across them. The cache holds at most 2 MiB of sample arrays per worker, with bounded metadata. Large shapes and transformed shadows enter the unchanged general loop. Cold misses compute the same sample as before. The app creates no cache.

Shadow coverage reuse and the executor parity suite passed. The half-coverage mutant failed. The parity fixture changes colour, fractional phase, dimensions and clipping, and includes several overlapping shadows and work split across workers. The historical replay uses the restored 1,000/5,000-node canvas at scale 2. Both operands use the same binary. `BOUGH_SHADOW_CACHE_OFF=1` selects the control. Each value is the median of the two block p50s in ABBA order. Full frame means a full damage repair of the theme operation. First frame is the cold sample from each process. CPU pool measurements include caller wall time and the raw instructions are calling-thread only. This patch changes no GPU shaders or buffer formats, so the Metal speed column is outside the patch's execution path. The parity suite checks both CPU executors against the existing integer reference and Metal on this host.

| Executor | Nodes | Cell | Wall p50 ms before → after | Calling-thread instructions before → after |
|---|---:|---|---:|---:|
| cpu1 | 1,000 | Hover | 27.17 → 21.87 | 459.20M → 367.33M |
| cpu1 | 1,000 | Theme | 162.71 → 132.27 | 2784.96M → 2246.24M |
| cpu1 | 1,000 | Resize | 159.83 → 130.90 | 2745.65M → 2234.56M |
| cpu1 | 1,000 | First frame | 163.11 → 161.05 | 2780.32M → 2698.57M |
| cpu1 | 1,000 | Full frame | 164.05 → 131.65 | 2793.29M → 2255.52M |
| cpu1 | 5,000 | Hover | 587.76 → 574.12 | 9976.67M → 9734.14M |
| cpu1 | 5,000 | Theme | 967.74 → 941.77 | 16504.39M → 16048.22M |
| cpu1 | 5,000 | Resize | 714.75 → 693.56 | 12198.47M → 11769.08M |
| cpu1 | 5,000 | First frame | 969.26 → 967.85 | 16491.38M → 16412.96M |
| cpu1 | 5,000 | Full frame | 966.89 → 945.64 | 16511.68M → 16056.57M |
| cpun | 1,000 | Hover | 5.21 → 4.85 | 85.87M → 79.70M |
| cpun | 1,000 | Theme | 28.08 → 24.01 | 473.74M → 401.96M |
| cpun | 1,000 | Resize | 28.16 → 23.05 | 459.24M → 384.55M |
| cpun | 1,000 | First frame | 28.78 → 28.41 | 474.75M → 466.90M |
| cpun | 1,000 | Full frame | 28.77 → 26.29 | 478.63M → 427.61M |
| cpun | 5,000 | Hover | 102.71 → 101.39 | 1723.34M → 1703.19M |
| cpun | 5,000 | Theme | 169.19 → 165.46 | 2760.25M → 2756.66M |
| cpun | 5,000 | Resize | 126.48 → 122.38 | 2098.05M → 1944.11M |
| cpun | 5,000 | First frame | 170.53 → 169.68 | 2637.94M → 2717.75M |
| cpun | 5,000 | Full frame | 169.56 → 166.73 | 2837.10M → 2780.85M |

Warm 1,000-card CPU1 improves approximately 18–20% in the measured cells. The 5,000-card gains are roughly 2–3% on CPU1 in this run. In the 5,000-card CPU pool run the theme footprint rises from about 79 to 92 MiB while wall p50 improves from 169.19 to 165.46 ms. This is a trade-off for the owner to review before integration. Integer-position variants with widely separated edges could share more corner samples across box sizes. That requires proof of the exact far-edge saturation and Q8 parity conditions. Existing general coverage remains the fallback for transformed, thin and oversize shapes.

## Instruction and sampling cross-checks

On the archived `71ee3be` source, `cargo asm --offline -p bough --lib --asm step_transitions` shows the loop loading the trait method out of the operand vtable (`ldr x8, [x8,#40]`, then `blr x8`) before branching on the result. `cargo asm --offline -p bough-kernel --lib --asm bough_kernel::cpu::shade_tile 0` resolves the actual shade function. Its per-command and per-raster-row loops contain the branch to the corner and coverage handlers and the inlined row blends. `cargo asm --offline --manifest-path comparisons/bough-fx-bench/Cargo.toml --features typed-core --bin typed-core --asm paint_col 1` selects the opaque-record `Soft` monomorphisation. It enumerates a page's live bits (`rbit` and `clz`), loads the selected rectangle and reaches projection and clipping inside that loop. The raw assembler outputs are executable- and compiler-specific. These commands are reproducible on the pinned nightly and supplement the phase counts.

`sample PID 1 1 -file output` succeeded for the 5,000-card façade process. The hottest leaf bins included the shade closure (42 samples) and the tile function (27). The short typed-core sampling command exited before writing an analysis file. A longer replay captured 687 samples in the opaque `Fill` instantiation below `Tiles::each` and `Soft::repair`. `xctrace version` reported `tool 'xctrace' requires Xcode` with CommandLineTools selected. The run held the shared lock and used no interactive surfaces or prompts.

## Gate commands

```text
nice -n 15 cargo test --offline --release -p bough-kernel --test shadow_cache --test exact -j 8
exact: 6 passed, 0 failed
shadow_cache: 1 passed, 0 failed

nice -n 15 cargo test --offline --release -p bough-x --test repair_bounds --test anim -j 8
anim: 2 passed, 0 failed
repair_bounds: 1 passed, 0 failed

nice -n 15 cargo test --offline --release -p bough --test active_transitions -j 8
active_transitions: 1 passed, 0 failed
```

Each named single-test binary failed with its corresponding `mutant_shadow_cache`, `mutant_repair_bounds` or `mutant_skip_transition` feature. `tests/exact.rs` includes the integer shader backend, one and several CPU workers, partial pixel clips and adversarial fractional shapes.
