# Effects seam replay and review

The replay pin is wave3 `7991e7bd4e`, the merge of the Round 2 measurement branch into `fbf6a2637b`. Builds and runs use local archive copies and dedicated targets. The freeze record is [the review card](2026-11-21-effects-seam-review.md).

## Comparison scope

| Adapter | Scene and timing boundary |
|---|---|
| Lorelei façade | Author prelude, CPU offscreen surface, from input through repair and offscreen completion. `startup` from the original matrix repeats mount within one process. The separate process replay measures from exec through the first completed frame. |
| Lorelei typed `Soft` | Opaque integer rectangles, arithmetic placement and color transitions. The counts report repairs with nonempty damage. Theme and motion are diagnostics for the typed runtime. No hover, scroll, resize or video adapter for this fixture. |
| Lorelei staged `bough-paint` | Direct paint seam. State transition and raster stages use the fixed virtual clock. Match CPU and Metal bytes where supported. The full tier needs isolated subtree groups, paths, alpha masks and a general PNG input. |
| zgui `80baa3c7` | Headless GPU, native PNG decode when the asset path is set, 120 Hz virtual display. The timer includes event delivery, harness advancement and pumping. The count is redraw requests, excluding the first recorded cumulative value for non-startup cells. Pixel hash checks use a GPU fence outside the timed replay. |
| zgx `6dba24de` | Headless GPU, byte-identical decoded tile samples and explicit curve sampling. Timing ends when the renderer call returns. It performs readback in the gate. Synthetic NV12 input is converted to RGBA for the image leaf. |

The historical Round 2 wave3 matrix contains all 27 classifications at `62d53d3abe`. Its hover and scroll gates passed at all three sizes. Façade video was unavailable at all three sizes. The reference columns in that capture predate the native PNG replay. See [the complete older capture](../../../../bench/results/effects/r3/matrix-round2-completed-wave3.md).

## Review candidates

1. **Layered driver routing.** `Driver::frame` selects the resident renderer when a GPU is present, while `Gpu::run` rejects a layered scene. The driver's CPU branch calls `shade` without `prepare`. The patch uses the CPU schedule on both branches. Each chosen executor maintains its own layer contents over successive frames. The direct resident refusal remains a separate probe. See [the driver candidate](../../../../bench/results/effects/r3/paint-driver-layered.patch).
2. **Same-tick spring retarget.** `Spring::velocity` returns zero at `now == t0`, even for a spring launched with nonzero velocity. A second retarget at that timestamp loses the first retarget's carried velocity. The patch derives launch velocity from the quantized coefficients for each damping regime. See [the spring candidate](../../../../bench/results/effects/r3/spring-launch-momentum.patch).
3. **P6 four-video fixture.** The stock script asks for zero-based index 4 in the four-source run. The candidate selects the same uncovered video at index 0 at all three sizes and asserts a slot publication, nonempty damage and no backdrop prefix work. See [the fixture candidate](../../../../bench/results/effects/r3/fxbench-media4.patch).

The driver and spring candidates apply to paint-lane head `106e21c922`. Its P6 script selects the fourth source, which exists at all three sizes. The test-only patch adds the four-source publication and damage regression to that revision. [Patch applicability receipt](../../../../bench/results/effects/r3/fx-patch-apply.txt) · [P6 test-only patch](../../../../bench/results/effects/r3/fxbench-media4-test.patch)

The candidates await the focused run receipts. The motion and media fixture also covers a combined backdrop, video and spring. Each of its 96 ticks checks retained repair against full drawing and validates a neutral BT.709 endpoint pixel. The media-slot design in the shared notes specifies release after scene references and in-flight frame fences expire.

## Dense layer work

The published `effects 600 dense` script advances 38 shader records along with the one spring. The published 3.19 G instructions belong to that combined workload. The independent profile repeats it with the same script and separately disables endless shader motion. The profile distinguishes the rebin, event reach, prefix selection and shading, pyramid downsample, and final output phases. The patch applies to the pinned source and the instrumented executable has built. Its grouped phase table preserves the rounding residual. The corrected instruction means and stock per-frame medians have different aggregation. The timing runner records early exits for the larger dense reproductions. [Profiler patch and commands](../../../../comparisons/bough-fx-bench/scripts/paint-profile)

The design under review is in the shared `fixes/tile-prefix-checkpoints.md`. It keeps exact 4×Q15 restart state before each chosen paint key alongside the existing BGRA8 layer samples. Incremental ordered memberships and a halo index give a tile the relevant cuts in draw order. The first restored prefix starts the chain. Higher cuts replay the interval from the preceding cut after the preceding reader's halo stages complete. The design includes invalidation, budget fallback, and required mutants.
