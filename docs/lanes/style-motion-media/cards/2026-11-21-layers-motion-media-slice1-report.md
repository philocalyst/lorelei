# Report: layers, motion and media, slice 1 (card 2026-11-21-layers-motion-media-slice1)

State reached: **MEASURED**, with two thresholds missed and one cell void (below). Not independently replayed.
Code at `6c5fd7d42f` (benchmarks were run from that tree). Host: Apple M3 Ultra (h16003), macOS, Metal headless;
other agents were building throughout (load average 14 to 27 during the runs), so **instructions of the frame
thread are the ranking column** and wall / GPU times are indicative. Every run took `lockf -k` on the shared
bench lock. Raw output: `bench/results/fx/fxbench-6c5fd7d42f.txt`, `pbench-6c5fd7d42f.txt`,
`baseline-pbench-71ee3be.txt`, and the stopped first attempt `fxbench-6c5fd7d42f-stopped-at-3000-dense.txt`.

Reproduce: `cargo test --release -p bough-paint --test fx` (gates); `bench/fx/run.sh` (numbers).

## Gates

| gate | result |
|---|---|
| G1 retained == fresh full redraw, ring ages 1..=4 | 6 scenes x 26 steps, 0 differing words (`retained_equals_full_redraw_and_the_specification`) |
| G2 == whole-frame specification | same test, 0 words (every step of seed 0, every fifth elsewhere) |
| G3 CPU == Metal, bytes | 6 scenes x 26 steps, 0 words; 7,432 prefix jobs, 53,370 pyramid blocks, 6,394 output tiles in 3,019 dispatches (`gpu_staged_frame_equals_the_cpu`) |
| G4 CPU mutants | `mutant_layer_event` 149 failing steps, `mutant_layer_weights` 56 (specification), `mutant_settle` 66, `mutant_video` 99, `mutant_layer_dilate` 48 of 48 block-edge cases. The first version of the gate did **not** catch `mutant_layer_dilate` (the random scenes never isolate a block edge); `a_change_at_a_block_edge_reaches_the_next_block` was added for it. |
| G4 GPU source mutants | pyramid taps shifted 310,126 words; no flatten 4,186; prefix not stored 318,183; no restart 654 (on the aligned pane: the random scenes do not pin the restart either, a fully covering backdrop replaces the prefix); chroma swapped 349,886; coarser level never mixed 67,058 |
| G5 fallbacks | the resident `Gpu::frame` asserts on a layered scene (not exercised by a test); partial tiles recompute (exercised by every scene); cache on/off give the specification's pixels (`the_stored_prefix_never_changes_pixels`) |
| G6 existing suites | `cargo test --release -p bough-paint -p bough-kernel -p bough-expr`: all green (paint: lib 2, exact 5, fx 6, review 8, scenes 2; kernel 3+6+6+4+1; expr 1+1) |
| springs | within 0.477 / 0.061 / 0.093 / 0.222 px of the ODE for (170,26) / (120,8) / (300,40) / (90,30); value and velocity continuous across 40 retargets each; exact at the settle tick |
| output pacing | 49 / 97 / 116 frames for the same motion at 60 / 120 / 144 Hz (60.0 / 120.0 / 144.1 per second), then none |

## Thresholds (dist, one core, median instructions per frame unless said)

| | prediction | measured | verdict |
|---|---|---|---|
| P1 idle | 0 records, 0 tiles, 0 layer work | 0 / 0 / 0 in every scene (5.4 K instructions: the block compare and the empty tile scan) | held |
| P2 one spring, 300 vs 3,000 cards (`pane`) | records visited 1 and 1; instructions within 1.15x | 1.0 and 1.0; 690.8 K vs 921.0 K = **1.33x** | **missed** on instructions: a moved window rebuilds the bins (`Bins::build`), which walks every record (about 68 instructions per extra record). Incremental binning is the fix (roadmap area 6). |
| P3 hover above glass | 0 prefix tiles, 0 blocks | 0 / 0 (effects 300, 3,000, pane, 600 dense) | held |
| P4 hover beneath one glass (`pane`) | layer pixels recomputed <= 25 % | 16 prefix tiles + 7,281 pyramid px of a 46,275 px layer = **51 %** | **missed**: the card's window is a third of the panel's halo (16 of its 50 tiles); the repair is exactly the reached part. The threshold was a guess about geometry, not a property; the property (repair == reach) is what G1 and the counts show. |
| P5 glass moving 1 px/frame (`pane`) | pyramid px/frame <= 15 %, prefix tiles <= perimeter | 80 px/frame (0.2 %), 0.2 prefix tiles/frame | held |
| P6 one video among N | tiles == its window; 48 within 1.15x of 4 | 32 tiles (its 8 x 4 window); 17,756.9 K at 24 vs 17,757.7 K at 48 = 1.00005x. **The 4-video cell is void**: the script publishes video 4, which a 4-video scene does not have (0 tiles). | held for 24 vs 48; not measured against 4 |
| P7 no regression (pbench A1) | <= 1.03x per phase | canvas full 1.008, hover 1.007, hover1 1.014, theme 1.007, pan 1.008, drag 1.019, time 1.008; anim full / theme / time 0.989 | held. The window-pass-and-bins sub-row (`A`) is 1.02x to 1.18x (e.g. 322 -> 339 us): the pass now also keeps the active set and the events. |
| P8 cache vs redraw-the-layer control | <= 0.5x on P3 and P5 frames; kill above 1.0x | hover above: 0.023 (effects 300), 0.28 (pane). glass moves: 0.11 (effects 300), **0.52** (pane) | held on three, **missed by 0.02** on the fourth; kill line not approached |

## The cells

Columns: tiles shaded, records visited, prefix tiles, pyramid blocks per frame; median K instructions on one
CPU core cached / control; Metal GPU microseconds (median, load 15 to 27). Mismatches against a full redraw: 0
in every cell of every scene.

**effects 300** (445 records, 25 glass panels; 3,000 gives the same counts in the frame: more world, not more depth)

| cell | tiles | records | prefix | blocks | CPU K instr | control | Metal GPU us |
|---|---:|---:|---:|---:|---:|---:|---:|
| idle | 0 | 0 | 0 | 0 | 5.4 | 494,767 | 0 |
| one spring (+19 endless shader paints) | 59.8 | 20 | 6 | 154 | 67,390 | 526,231 | 388 |
| all springs | 89.7 | 57 | 6 | 154 | 86,301 | 545,621 | 380 |
| scroll 11 px | 1,000 | 445 | 348.5 | 1,342.8 | 1,221,214 | 1,226,774 | 1,939 |
| hover above glass | 6 | 226 | 0 | 0 | 11,221 | 494,994 | 280 |
| hover beneath glass | 36 | 226 | 27 | 337 | 85,440 | 507,770 | 816 |
| theme | 1,000 | 327 | 367 | 1,439 | 1,223,475 | 1,228,686 | 1,777 |
| glass moves 1 px | 29.7 | 0 | 8 | 189.3 | 55,756 | 497,235 | 400 |

effects 3,000 (4,439 records): the same tiles and layer work; CPU 5.8 / 67,793 / 86,978 / 1,224,531 / 11,444 /
85,664 / 1,223,393 / 56,134 K. Records visited by a state-word change grow with the column (hover 2,251):
the known per-column window pass, not new.

**pane** (one glass panel, no endless paint; 300 / 3,000 cards): idle 5.5 / 5.6; one spring 690.8 / 921.0
(1 record, 2.8 tiles; Metal 88 / 92 us); all springs 19,613 / 20,071 (38 / 375 records); hover above 11,208 /
11,359; hover beneath 43,886 / 44,046; glass moves 21,829 / 22,062 (15.7 tiles, 0.2 prefix tiles, 80 pyramid
px per frame).

**media 24 / 48** (24 / 48 NV12 tiles, 2 / 3 visible glass panels over video, sprung dots)

| cell | tiles | prefix | blocks | CPU K instr 24 | 48 | Metal GPU us 24 / 48 |
|---|---:|---:|---:|---:|---:|---|
| idle | 0 | 0 | 0 | 5.7 | 5.7 | 0 / 0 |
| one video frame | 32 | 0 | 0 | 17,757 | 17,758 | 191 / 194 |
| video under glass | 44 | 20 | 132 | 39,989 | 39,990 | 372 / 381 |
| all videos, every 4th frame (mean) | 170 / 250 | 20.2 / 29.2 | 81 / 117 | 99,949 | 152,657 | medians are idle frames |
| scroll 11 px | 485 / 1,000 | 40.5 / 121.5 | 152.6 / 459.4 | 250,826 | 621,838 | 317 / 706 |
| hover (card behind a video under glass) | 52 | 25 | 132 | 49,942 | 49,945 | 267 / 323 |
| theme | 1,000 | 90 / 126 | 324 / 468 | 413,264 | 613,069 | 471 / 678 |
| all springs | 36.3 / 45.2 | 0 | 0 | 23,142 | 30,609 | 129 / 168 |
| glass over playing video (mean) | 188.2 / 268.2 | 21.2 / 30.2 | 86.2 / 122.2 | 113,201 | 165,918 | 220 / 219 (median) |

media 4: in the raw file; its "one video" cell is void (above) and its scroll cell scrolls the single row out
of the frame.

**effects 600 dense** (every card in the frame; 50 panels about six deep; 6 frames per cell): idle 5.7; hover
above glass 44,661 K (0 layer work); **one spring 3,185,064 K** (662 prefix tiles and 5,767 blocks per frame:
the 38 endless shader paints lie beneath stacks of panels, and every panel above a change recomputes its
reached prefix tiles from the bottom unless the tile is inside a lower panel's window); control 8,694,341 K;
Metal 19.7 ms GPU. **3,000 dense (31 deep) was stopped**: 3.8 s per cached frame, 9.3 s per control frame.
This is the design's weak regime: a change at the bottom of a deep stack of overlapping layers costs a prefix
tile program per layer above it.

## What was not verified

* Vulkan / OpenGL / the façade: nothing here runs there. The façade's benchmark binaries were not touched;
  the measurement lane's runs of `71ee3be` are their "before" and nothing in this branch changes their code.
* The façade's spinner / ticker at 62.5 fps (X22) is **not fixed**: the mechanism (frame clock, `animating`)
  and its gate are here; the façade's app clock is in files this lane does not own.
* Blur quality against a true Gaussian (no visual fixture); wall-clock numbers on a quiet machine; pool
  workers for layer stages (they run on one thread); `Gpu::frame`'s refusal has no test.
* zgui / zgx were read, not run.
* The effects-heavy scene of the measurement lane's spec (paths, masks, group filters, drop-shadow of alpha,
  PNG tiles, resize) is mostly outside this slice: only its glass, spring, shader-node and video parts exist.

## Next card (recommendation)

1. **Incremental bins** (P2's miss): per-tile lists edited by the records that moved; then the staged Metal
   path stops re-uploading lists and windows on every moving frame (its 350 to 900 K CPU instructions).
2. **Deep stacks**: let a prefix job restart from the layer beneath even on partial tiles (store the
   unflattened accumulator's 8-bit prefix for the halo too and flatten by rule, or keep a second, exact
   prefix), and batch independent layers of one rank into one dispatch.
3. **Groups**: isolated subtree layers (opacity, masks, blend modes, filters, transforms, drop-shadow of
   alpha) as readers of the same layer store; a retained shadow coverage as a layer (the canvas's 89 %).
4. A resident GPU schedule for layers (events and dilation as atomic bit passes), and author `#[wgsl]`
   modules as paints.
