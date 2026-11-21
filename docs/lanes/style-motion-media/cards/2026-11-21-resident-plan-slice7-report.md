# Report: the resident, self-pruning layer plan; the cover certificate (slice 7)

Card: `2026-11-21-resident-plan-slice7.md` (frozen at aae97df87e before anything was built). Code: 0d24bbdd3c
(CPU certificate and pixel boxes), 03b3b08b53 (resident plan). Raw output: `bench/results/fx/slice7.txt`,
`slice7-cert-600d.txt`, `pbench-slice7.txt`. All numbers: dist profile, frame-thread instructions, no bench
lock, host load 20 to 49 (GPU times are indicative only).

## What was built

1. `Gpu::resident` (design: DESIGN.md section 8). The same dispatches every frame; the GPU decides what is
   due from per-tile event keys, compares before every write, stamps generations, and marks what a change
   reaches. Output damage is a generation per tile, so a surface of any age is a comparison. Nothing is read
   back to schedule. Seven dispatches per rank of layers (the first version had thirteen).
2. Incremental mirrors for it: tile lists in per-tile slabs (`Bins::take_changed`, now deduplicated: it
   grew without bound when no executor took it), lanes per changed key (`Scene::take_lane_dirty`), layer
   table rows per moved layer (`Layers::take_moved`), slots rebuilt only on re-allocation or a broken rank.
3. `Mixed` runs the resident plan on its GPU frames and routes events to layers only when it chooses the
   CPU (`Scene::route_off`, `route_events`, `event_weight`); its GPU constant is now a base plus a cost per
   encoded dispatch.
4. CPU: `Bins::cover_above` (an event stops at the first record above it whose opaque cover holds the
   event's window inside the tile) and pixel-box repair (what changed is the box of the words that differ,
   dilated in pixels; a block is recomputed only inside it).

## Gates

`cargo test --release -p bough-paint -j 8`: lib 2, exact 5, fx 10, review 8, scenes 2, all passed.

| gate | result |
|---|---|
| R1 resident == fresh CPU redraw | 8 scenes x 26 steps, 116 drawn frames at surface ages 1..4, with and without scene-side routing: 0 failing frames |
| R2 source mutants | prefix never stamps 108, pyramid never stamps 108, pixel looks at one block beneath 106, reach does not lower keys 76, reach damages no output 82, any cover certifies 88 failing frames; `mutant_slab_stale` 84, `mutant_forced` 84 |
| R3 CPU certificate | existing gates pass with it; `mutant_cert` fails 3 tests of fx |
| R4 earlier gates | pass; pbench A1 0.946 to 1.015x of baseline, 0 cross-check failures |

Mutants re-run at head (`--test fx --features ..`): layer_dilate, layer_event, layer_weights, settle, video,
halo_damage, bins_stale, broad_layers, prune, scroll_band, key_old, cert, slab_stale, forced all fail.
**`mutant_full_stale` is no longer seen**: it mutates the resident whole-frame job buffer of `Gpu::staged`,
which only the mixed gate exercised, and `Mixed` no longer runs that path.

## Thresholds against the card

| | card | measured | verdict |
|---|---|---|---|
| T-a Metal one spring, 3,000 dense within 1.2x of 600 dense | I predicted a miss: 0.3-0.6 M and 1-2.5 M | 369 K and 1,214 K = 3.3x (ranks 18 and 68) | missed, as predicted; was 6,296 K and about 215,000 K |
| T-a kill line: 3,000 dense above 10 M | | 1.2 M | not hit |
| T-b GPU time, one spring at 3,000 dense, not above 13 ms; predicted under 5 ms | | 13.3 ms (no lock, load 44) | prediction missed; on the line |
| T-c one spring 200 / 1,000 / 5,000 within 1.2x | | 214 / 239 / 322 K = 1.5x | missed (the turn: endless paints off screen are still visited) |
| T-c glass moves within 1.2x | | 236 / 236 / 233 K | held (was 276 / 302 / 417 K) |
| T-c no cell worse than slice 6 by more than 1.1x except by the fixed dispatches | | hover above glass 73 -> 200 K, hover beneath 270 -> 195 K, theme 179 -> 220 K | the fixed dispatches (8 ranks: 59 dispatches) are the whole difference; reported |
| T-d CPU prefix tiles, one spring at 600 dense, <= 150; kill above 325 | | 650 -> 570 | **kill line hit** |

T-d: the certificate with one cover rectangle per record removes 12% of the prefix tiles there (37% on
glass moves: 605 -> 383) and pixel boxes halve the pyramid blocks (121 -> 62 per frame). What is left is
not provable from the cover lane: the event log showed strips of 1 to 3 pixels of the changed window under
the rounded band of the card above, which is opaque there but outside its inset core rectangle, and reader
output boxes that span the gap between two cards. The next fact to try is a cover that is the cross of a
rounded box (two rectangles), a new lane. Not built.

## Every cell (K frame-thread instructions, median)

Effects, three repetitions, 200 / 1,000 / 5,000 cards; 0 mismatching words in all nine runs.

| cell | CPU kernel | Metal resident | chooser |
|---|---|---|---|
| idle | 8.9 / 8.9 / 9.0 | 5.9 / 5.9 / 5.9 | 3.0 / 3.0 / 3.0 |
| one spring | 35,051 / 35,080 / 35,173 | 214 / 239 / 322 | 216 / 238 / 320 |
| all springs | 53,545 / 53,705 / 54,416 | 275 / 430 / 1,150 | 275 / 433 / 1,143 |
| scroll 11 px | 992,807 / 992,720 / 992,876 | 1,498 / 1,626 / 1,882 | 1,505 / 1,639 / 1,882 |
| hover above glass | 6,082 / 6,082 / 6,082 | 200 / 201 / 202 | 197 / 198 / 200 |
| hover beneath glass | 41,964 / 41,964 / 41,965 | 193 / 199 / 203 | 191 / 196 / 201 |
| theme | 986,098 / 986,259 / 986,275 | 220 / 216 / 220 | 222 / 227 / 232 |
| glass moves 1 px | 17,781 / 17,786 / 17,844 | 236 / 236 / 233 | 237 / 237 / 235 |

Scroll on Metal was 2,311 / 2,407 / 2,537 K in slice 6. Ranks 8 (9 while scrolling); slots 3,299 / 4,619 /
11,295. GPU time: 0.4 to 1.0 ms for the small cells, about 1.7 ms theme, 1.8 ms scroll.

Dense (one run of 6 frames each; 0 mismatching words).

| cell | 600 dense: CPU / Metal / chooser | 3,000 dense: CPU / Metal / chooser | Metal GPU ms (600 / 3,000) |
|---|---|---|---|
| one spring | 199,257 / 369 / 379 | 1,034,382 / 1,214 / 1,194 | 2.6 / 13.3 |
| all springs | 341,974 / 557 / 555 | 1,707,260 / 2,052 / 2,012 | 2.8 / 13.3 |
| scroll 11 px | 4,774,154 / 4,440 / 4,624 | 24,277,296 / 24,039 / 24,075 | 5.6 / 26.9 |
| hover above glass | 7,314 / 347 / 342 | 12,176 / 1,026 / 1,028 | 0.9 / 2.8 |
| hover beneath glass | 240,849 / 341 / 347 | 1,049,819 / 1,037 / 1,029 | 3.0 / 15.4 |
| theme | 5,453,571 / 355 / 368 | 26,812,590 / 1,051 / 1,055 | 5.9 / 28.9 |
| glass moves 1 px | 85,495 / 377 / 374 | 366,867 / 1,076 / 1,095 | 2.1 / 10.8 |

Ranks 18 and 68; slots 18,596 and 91,963. The Metal column before this slice (eager plan): one spring 6,296 K
at 600 dense and 195,000 to 215,000 K at 3,000 dense. A frame that changes nothing a layer can see still
pays the plan: hover above glass at 3,000 dense is 1,026 K and 2.8 ms of GPU for 68 ranks of empty stages.
Scroll at 3,000 dense is the turn (12.2 M in `update`: windows and bins of every record in view) plus
uploads; the CPU kernel's scroll turn there is 414 M, of which routing with the certificate scan is a part
I did not separate.

Pane 300 / 3,000 (0 mismatches): one spring 422 / 422 CPU, 93 / 93 Metal; all springs 19,364 / 19,788 CPU,
176 / 606 Metal; hover above 9,448 / 9,448 CPU, 90 / 91 Metal; hover beneath 32,947 / 32,946 CPU, 91 / 90
Metal; glass moves 16,975 / 16,973 CPU, 123 / 123 Metal. Chooser within 3 K of Metal in every cell.

Media 24 (0 mismatches; CPU / Metal / chooser): one video frame 14,367 / 90 / 89; video under glass 25,461 /
91 / 89; all videos (every 4th) 9.2 / 5.8 / 2.8 median (idle between frames); a/v playback 188,168 / 126 /
129; scroll 240,597 / 848 / 833; hover above glass 35,031 / 93 / 92; theme 388,660 / 119 / 118; all springs
22,988 / 138 / 139; glass over playing video 10,548 / 141 / 145.

## What still follows the scene

* **Ranks.** Frame-thread cost is 70 K + about 2.5 K per dispatch, seven dispatches per rank, every frame
  that is not idle. 8 ranks in the scrolling world, 18 and 68 in the dense ones. Empty stages also cost GPU
  time (2.8 ms at 68 ranks).
* **All springs** (275 -> 1,150 K) and **one spring** (214 -> 322 K): the turn. Moving records are visited
  one by one, and endless paints are visited off screen.
* **Scroll**: the turn rewrites the windows and bins of the records in view (0.65 to 0.75 M of the 1.5 to
  1.9 M).
* **CPU at depth**: 570 of 650 first checks remain (above).

## Not verified

GPU time under the bench lock or on a quiet host; Vulkan; a wrap of the 32-bit generation; the measurement
lane's harness (no adapter in the merged tree at the time of this slice).
