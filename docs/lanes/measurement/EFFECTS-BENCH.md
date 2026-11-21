# Heavy effects benchmark

## Contract

Count logical cards at 200, 1,000 and 5,000. In the canvas use a 640 × 400 RGBA8 target at scale 1 and a ten-by-ten grid with 64 × 40 spacing. Draw all cards in insertion order; card `i` uses cell `i % 100` with a quarter-pixel stagger by layer. Each card has a translucent rounded 60 × 36 body, deterministic palette and asymmetric two-stop gradient, plus two outer shadows with distinct spreads, offsets and blur radii. Add a shared procedural PNG tile every 16 cards. Reserve the last card as the foreground motion subject. The scroll scene puts the same cards in a ten-column vertical strip with a fixed viewport. Limit mounted rows only through the framework's normal API. Footprint includes resources and all retained buffers.

The full-effects extension adds independently observable feature layers to the same trace. Draw a stroked cubic and filled even-odd path, then rounded clips and an alpha mask. Composite a filtered group in order (blur sigma 3 px, saturate 1.4, brightness 0.8, alpha drop-shadow with sigma 4 px and offset (3, 5)). Give three visible fills respectively a hash-noise plasma, a circle-minus-circle SDF and a moving sine-ramp uniform. Composite four 112 × 76 frosted panels with 12 px corner radius and Gaussian backdrop sigma 6 px over moving content. One panel overlaps a 256 × 144 video tile (deterministic synthetic 120-frame 30 fps motion pattern, 4:2:0 limited-range BT.709, one new decoded frame each four 120 Hz ticks). Submit frames at fixed virtual timestamps. Report decode-inclusive and predecoded upload/composite separately. Use the same coefficients, sampling edges, crop, ordering and assets across all adapters.

| Cell (all three sizes) | Input | Acceptance target after parity |
|---|---|---|
| Start-up | Fresh process, instantiate resources and mount first available offscreen frame | p95 ≤ 50 ms warm / 250 ms cold, footprint ≤ best matched rival |
| Idle | Sit after mount with no active timer, input or damage for 1 second | Zero framework frames, tile repair, or scheduled animation wakes |
| One moving | Interrupt foreground card's translation by 10 px at 100 ms, then sample each 120 Hz tick | 5,000/200 median instructions ≤ 1.10, p99 ≤ 2 ms |
| Many moving | Start all cards with staggered translation, opacity, color and width, interrupt at 100 ms. Full tier adds spring `(m=1,k=260,c=23)` to every other card | p95 ≤ 8.33 ms and p99 ≤ 16.67 ms on hardware GPU, CPU instructions ≤ best matched rival |
| Hover | Pointer walks 20 distinct grid cells then reverses | p99 ≤ 4.17 ms, no full-frame damage for an isolated change |
| Theme | Replace the shared palette every frame, preserving ordering and alpha | p95 ≤ 8.33 ms GPU, p99 ≤ 16.67 ms |
| Scroll | Advance 11 px per frame through the strip, then reverse | p95 ≤ 8.33 ms GPU, p99 ≤ 16.67 ms, footprint growth follows exposure |
| Resize | Alternate 640 × 400 with 704 × 448, keeping the current live state | p95 ≤ 16.67 ms GPU, no remount in the retained adapter |
| Glass over video | Video advances every fourth tick while the frosted pane translates each tick | p95 ≤ 8.33 ms GPU, p99 ≤ 16.67 ms |

These targets leave a 120 Hz frame budget for GPU-heavy cells and half a frame for isolated input. Startup budgets cover resource preparation and first render. Maintain separate matched CPU and hardware GPU comparisons.

## Tiers and adapters

1. Façade author scene built with `bough::prelude`. Use an offscreen harness to drive the existing app boundary. Include available modifiers and report any unsupported primitive or workload dimension explicitly. The typed-core B adapter must identify its own narrower contract and only rank visually matched cells.
2. `full-effects` adapter consumes the fx lane seam. Until the seam exists, express desired operations as backdrop, ordered group filters, authored paint with uniforms, path/mask, spring and sampled video on the benchmark scene. Unsupported cells return an explicit non-success. A static image cannot stand in for video or a translucent tint for backdrop blur.
3. Build local zgui `80baa3c7cb0fbbe1c058f10059e076e41c0a1b9a` and zgx `6dba24de4c1b63d9c84d6acd0e40b1ec7c3f8305` via isolated driver packages/snapshots, following `bench/zgx/build.sh` and `STATUS.md` §1. Use zgui-shader/zgui-anim and zgx-fx/zgx-video when admitted. Use the same trace in each driver. Validate semantics before comparing; preexisting unrelated driver numbers are historical context.

## Evidence

Use `lockf -k ~/work/locks/bench.lock` and record load before and after each cell. Run a correctness pass separately from warmup and measurement. Exact retained versus full redraw at every age 1–4 and fresh mount at rest. Require an independent expected pixel constraint and a changing checksum (except idle). Check that dropping an input/damage update fails. Cross-framework edge differences require explicit envelope and independent fixtures before ranking. Record per-step traces and percentiles via nearest rank. Initial bring-up can use 20 samples with p99 labelled as the observed maximum. Ranking requires ≥ 200 active samples and three paired blocks, with ABBA for GPU times.

Collect process-scope retired instructions as primary, all-thread CPU time, end-to-end offscreen wall p50/p95/p99, GPU timestamp interval, peak RSS, physical footprint, and fraction of framebuffer in damaged pixel tiles. Record actual submitted/presented counts and respect the backend's own idle scheduling. Missing metrics remain unavailable. Offscreen completion does not establish display pacing. Measure app mount to first frame separately from process spawn to first frame when the host can observe both.

## Historical loss triage

Re-run the original workload driver where it builds. Probe canvas shadow work at fixed geometry by comparing the same renderer with its shadow work removed as a diagnostic control, then retain the unchanged pixel contract for candidate patches. Re-run the sheet at increasing logical sizes and count materialized records and resident bytes. Separate direct in-process operations from Wayland transport/timeout outcomes. Replay `sheet_x` and compare `part_journal_only` to the reported baseline and to a scoped journal control, along with sheet oracles. The spinner/ticker source clock and output-paced host require independent evidence. Keep one index entry for each historical cell. Drop candidate patches without an attributable gain and passing gates.
