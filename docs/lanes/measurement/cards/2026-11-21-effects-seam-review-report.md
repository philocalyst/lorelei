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

## Native PNG replay

The two reference adapters classified all 54 rows. zgx passed the dynamic gates at every size and supplied 240 observations per changing cell. Its glass-over-video p50/p95/p99 was **0.676/0.838/1.011**, **0.671/0.839/1.119**, and **0.927/1.137/1.221 ms** in increasing card count, with 240/240 renders. zgui's 1,000- and 5,000-card `many` gates failed in `Buffer::map_async` (`zgui.readback` invalid). The same-binary retry reproduced the error in both retained and forced-full draws at both sizes. Both cells were withheld from timing. [Retry receipts](../../../../bench/results/effects/r3/ref-retry)

Native PNG zgui `one` and 200-card `many` recorded 131 redraw requests over the 239 observable transitions between sampled ticks. The separate 480-tick run tests whether those cells reach 200 distinct redraw steps. Startup for each reference has one observation in the stock matrix. The separate fresh-process 240-repeat capture is queued. Assets share the same decoded RGBA samples. Geometry, motion semantics and GPU completion boundaries still differ across adapters. [27 comparison rows](../../../../bench/results/effects/r3/matrix-native-png.md)

The legacy staged video replay at `64101ac59d` passed all three retained/full gates and recorded 240 ticks per size. See the separate CPU1 synthetic-decode-inclusive column. [Legacy staged video](../../../../bench/results/effects/r3/staged-video-64101ac59d.md)

## Review candidates

1. **Layered driver routing.** `Driver::frame` selects the resident renderer when a GPU is present, while `Gpu::run` rejects a layered scene. The driver's CPU branch calls `shade` without `prepare`. The patch uses the CPU schedule on both branches. Each chosen executor maintains its own layer contents over successive frames. The direct resident refusal remains a separate probe. See [the driver candidate](../../../../bench/results/effects/r3/paint-driver-layered.patch).
2. **Same-tick spring retarget.** `Spring::velocity` returns zero at `now == t0`, even for a spring launched with nonzero velocity. A second retarget at that timestamp loses the first retarget's carried velocity. The patch derives launch velocity from the quantized coefficients for each damping regime. See [the spring candidate](../../../../bench/results/effects/r3/spring-launch-momentum.patch).
3. **P6 four-video fixture.** The stock script asks for zero-based index 4 in the four-source run. The candidate selects the same uncovered video at index 0 at all three sizes and asserts a slot publication, nonempty damage and no backdrop prefix work. See [the fixture candidate](../../../../bench/results/effects/r3/fxbench-media4.patch).

The driver and spring candidates apply to paint-lane head `106e21c922`. Its P6 script selects the fourth source, which exists at all three sizes. The test-only patch adds the four-source publication and damage regression to that revision. [Patch applicability receipt](../../../../bench/results/effects/r3/fx-patch-apply.txt) · [P6 test-only patch](../../../../bench/results/effects/r3/fxbench-media4-test.patch)

The pinned P6 correction passed its focused regression. Restoring the stock fifth source fails the same test at publication. [Stock P6 control](../../../../bench/results/effects/r3/media4-stock-test.txt) The spring candidate preserves the measured same-tick launch velocity for all seven responses in the focused retarget probe. Both patched Driver probes passed the 14-frame full-redraw comparison. The stricter repeat checks the selected executor on each frame. [Driver candidate receipt](../../../../bench/results/effects/r3/driver-fixed-retry.txt) [Spring candidate receipt](../../../../bench/results/effects/r3/spring-launch-gate.txt) [P6 correction receipt](../../../../bench/results/effects/r3/fxbench-media4-gate.txt) The motion and media fixture also covers a combined backdrop, video and spring. All 96 ticks passed retained/full comparison and the neutral BT.709 endpoint pixel check. The full media-lifetime suite remains queued. [Combined media gate](../../../../bench/results/effects/r3/media-layer-gate.txt) The media-slot design in the shared notes specifies release after scene references and in-flight frame fences expire.

## Independent motion and media findings

| Priority | Baseline evidence | Fix or design |
|---|---|---|
| High | `a_slot_a_record_still_names_is_never_reused_after_a_publish_storm` reused slot 0 while the record still named it. A full redraw exposed 2,291 changed pixels with no scene record write. | Pin the scene's installed slot separately from the producer's most recent publication. Reclaim only when the last installed reference and last submitted frame fence expire. |
| High | `a_second_retarget_in_the_same_tick_keeps_the_momentum` measured −12.5 Q8/tick after a second same-tick retarget versus 346 after a single retarget. | `spring-launch-momentum.patch` evaluates the launch derivative from the stored coefficients. The candidate gate matched all seven response cases. |
| Medium | `a_long_critically_damped_move_starts_at_rest_and_follows_the_ode` found a 31.50 px error on a 700 px move and 325.59 px on a 1,500 px move. Both saturated `b0` at −2,097,151. | Specify a representable range for the critical coefficient or encode its exponent explicitly. Use the same precision in CPU and GPU evaluation and preserve the exact initial displacement, derivative and final value. Reject or fall back before saturation. Add gates at the coefficient limit and across both sides of the near-critical regime. |
| Medium | `a_storm_between_draws_holds_a_bounded_number_of_slots` reserved 51 slots during 50 producer publishes between draws. `an_abandoned_lease_is_not_lost_for_good` exhausted the pool after 3 cancellations. | Keep a single pending slot per stream and release superseded, uninstalled slots before the next fence. Add cancellation for decode leases. |
| Medium | The `(20,000, 200)` response missed the velocity envelope by 1,996.7 px/s at 4 ms. Five retargets in the same regime missed the ODE trajectory envelope, with 5.44 px maximum error. | Derive velocity from the same response equation and quantized coefficients at every tick, with explicit start and settle behavior. Test peak and early acceleration separately to distinguish derivative approximation from coefficient range limits. |
| Fixture | The original format-change probe ran out of allocations with only three slots and stopped at its setup assertion. | The corrected capacity ensures the request fits in two adjacent free ranges while it exceeds the unallocated tail. Its allocator verdict is queued. |

The baseline suite passed staggered launch, retarget at rest, and the 480-frame retained spring and settle script. Its 8 failures include the format-change setup failure above. [Baseline receipt](../../../../bench/results/effects/r3/motion-media-baseline.txt) · [Media ownership design](../../../../bench/results/effects/r3/paint-video-slot-lifetime.md)

## Blur error and layer storage

The rendered fixture measured **12.30 maximum and 4.62 RMS byte error** for the contract's σ=6 step under the seven tested sub-grid positions, within the review envelope of 16 maximum and 5 RMS. At σ=24 it measured **22.18 maximum and 6.78 RMS**. At σ=40 it measured **51.88 maximum and 21.35 RMS**. The coarsest level is selected when σ reaches 20.625, and all larger radii use the same reconstruction. This accounts for the growing error against the Gaussian at large radii. The rendered errors match the scalar model at all seven tested radii. σ=0.5, 24 and 40 exceed the review envelope. The output records the quality verdicts separately from the passing fixture checks. To admit broader Gaussian widths, first calibrate each reconstructed level across all sub-grid phases and specify an admitted sigma interval. Interpolate using the two calibrated variances, then rerun the independent step envelope at level boundaries and at the interval endpoints. A range above the coarsest level requires an extra level or a wide Gaussian fallback. Expand the corresponding halo and add an edge mutation for that support. An implementation that keeps the current filter can document and enforce the capped range at admission. [Renderer receipt](../../../../bench/results/effects/r3/gaussian-renderer.txt) · [Scalar model](../../../../bench/results/effects/r3/gaussian-step-model.tsv)

At 640×400 the four 112×76 σ=6 panels require **502,960 bytes** for their live BGRA8 level pixels. Toroidal padding and inactive level sentinels bring the first-update pool reservation to **698,688 bytes**. Individual panels reserve **135,040–208,160 bytes**. The allocator reported the same capacity as its 698,688-byte live pool. The counts exclude metadata and any separate Metal copy. [Source-derived allocation ledger](../../../../bench/results/effects/r3/four-pane-source-ledger.tsv)

## Staged paint capability at the pin

The adapter drives the same nine cell scripts at 200, 1,000 and 5,000 cards. These are source admissions. The pinned build and pixel gates determine whether each row receives timings.

| Cell | Coverage | Boundary |
|---|---|---|
| Startup | Mount and first offscreen frame. Fresh-process repeats measure the in-child interval and process wall time. | CPU1. |
| Idle | Fixed shader clock, zero changed records, tile repairs, layer repairs, and active paint records. | The headless harness drives virtual ticks. |
| One | One translating spring among dense rounded, translucent gradient cards and shadows. The trace retargets at tick 12. | The clock-only test isolates the foreground spring. |
| Many | Staggered width, translation, color and opacity, with springs on alternating records. | Staged paint expressions apply geometry and color directly. |
| Hover | Uniform selects the top card under 20 pointer positions and their reverse. | Manual hit target selection. |
| Theme | Alternate shared palette words with preserved alpha and order. | Direct paint uniform update. |
| Scroll | Pan the ten-column strip by 11 pixels per tick and reverse. | No admitted virtualization or mounted-row limit. |
| Resize | Surface replacement preserves the replay inputs used for a remount diagnostic. | Retained resize is not expressible. `Scene` has a fixed surface. |
| Glass over video | Four rounded backdrop panels, staged shader paints and synthetic 256×144 NV12, advanced every fourth tick. A panel moves each tick. | Synthetic generation and repair have separate counters. The path has no codec and no group effect. |

The seam exposes solid and gradient paints, shadow with geometry-derived spread, rectangle clips, and two built-in shader paints. The frozen PNG, cubic and even-odd path, rounded clip, alpha mask, isolated group filters and drop-shadow of a subtree's alpha require additional primitives. Author registration of all three frozen shaders is unavailable. The headless adapter uses CPU1. The stock paint replay and independent layered test cover Metal separately.

## Dense layer work

The published `effects 600 dense` script advances 38 shader records along with the one spring. The published 3.19 G instructions belong to that combined workload. The independent profile repeats it with the same script and separately disables endless shader motion. The profile distinguishes the rebin, event reach, prefix selection and shading, pyramid downsample, and final output phases. The patch applies to the pinned source and the instrumented executable has built. Its grouped phase table preserves the rounding residual. The corrected instruction means and stock per-frame medians have different aggregation. The 300-card smoke test records a corrected mean of **1,999,554 instructions** and a median of **2,001,679** with instrumentation disabled. Bins account for **285,477** instructions in the bracket. The six-frame trace covers four dirty tiles each tick. The full size sweep uses longer runs to cover more of the spring trajectory. [Smoke receipt](../../../../bench/results/effects/r3/profile-smoke) The timing runner records early exits for the larger dense reproductions. [Profiler patch and commands](../../../../comparisons/bough-fx-bench/scripts/paint-profile)

The 300-card `pane` profile reproduced the hover reach result independently. Above glass it scheduled zero prefix tiles and zero tent pixels. Beneath glass it repaired 16 prefix tiles and 7,281 tent pixels out of 46,275 logical level pixels, or **51.14%**. The layer stored 248,208 allocated bytes. The bracket is 44,052,760 calling-thread instructions for the beneath hover. The phase table separates the prefix fold and final tile rendering. [Hover phase table](../../../../bench/results/effects/r3/profile-full/pane-hover-beneath.md)

The design under review is in the shared `fixes/tile-prefix-checkpoints.md`. It keeps exact 4×Q15 restart state before each chosen paint key alongside the existing BGRA8 layer samples. Incremental ordered memberships and a halo index give a tile the relevant cuts in draw order. The first restored prefix starts the chain. Higher cuts replay the interval from the preceding cut after the preceding reader's halo stages complete. The design includes invalidation, budget fallback, and required mutants.
