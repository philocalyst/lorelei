# Report: instruction chooser; compare-on-write measured, then built; each_in in the span painter (slice 5)

State reached: **MEASURED**. Not independently replayed. Raw: `bench/results/fx/slice5.txt` (three repetitions of
effects 200 / 1,000 / 4,000; 600 dense; 3,000 dense; pane; media 24) and `slice5-chooser.txt` (after the
chooser's estimate was cut short; effects 200 / 4,000 x 3 and 3,000 dense). dist, no bench lock, instructions
of the frame thread; every run "mismatching words in total: 0".

**Amendment to the card, made after its measurement and before building.** The card said (c) was "counters
only". The counters said 99 % of what the dense stack repaired came out unchanged (below), so I built
compare-on-write for the CPU executor in this slice. The card's prediction C2 was wrong and stands as written.

## C2: the measurement that decided it (counters, before any behaviour changed)

600 dense, CPU executor, six frames, repairs that left every word as it was / repairs made:

| cell | prefix tiles | level 1 | level 2 | level 3 | level 4 | level 5 |
|---|---|---|---|---|---|---|
| one spring | 3,936 / 3,972 | 7,578 / 7,650 | 8,670 / 8,742 | 8,224 / 8,298 | 6,905 / 6,936 | 2,974 / 2,976 |
| hover beneath glass | 2,568 / 3,072 | 5,676 / 6,120 | 6,750 / 7,128 | 6,534 / 6,846 | 5,736 / 5,958 | 2,526 / 2,592 |
| glass moves 1 px | 2,579 / 2,595 | 5,993 / 6,010 | 6,870 / 6,888 | 6,552 / 6,570 | 5,733 / 5,748 | 2,590 / 2,592 |
| scroll | 1,331 / 10,981 | 1,716 / 10,981 | 1,663 / 10,981 | 1,248 / 10,177 | 403 / 7,548 | 21 / 2,871 |
| theme | 0 / 13,380 | 0 | 0 | 0 | 0 | 0 |

I predicted "fewer than 30 % unchanged" for one spring. It is 99 %. The cascade through the stack was almost
entirely damage that changed nothing: a reader's output was declared changed over everything its coarse
levels could reach, and the readers above repaired all of it to find the same bytes.

## What was built

* **Compare-on-write on the CPU executor** (`Scene::repair`). `update` now leaves what is due (pending tiles,
  entered strips, fresh layers) and schedules nothing further. `prepare` repairs layer by layer in paint
  order and propagates only from prefix tiles and pyramid blocks whose stored words changed; only the blocks
  a reader's levels really changed become output damage and events for the layers above. Exact: a comparison
  of words. Pixels that entered a window and fresh layers are changed by rule (their old contents are not
  theirs). The output damage is complete after `prepare`, not after `update`.
* **Metal keeps the conservative plan** (`Scene::plan_eager`, `update_eager`): it cannot look at a result
  before the next stage is encoded. Its pixels are the same; its work is not pruned.
* **The chooser in instructions** (`mixed::CPU_INSTR_PER_WEIGHT`, `GPU_FRAME_INSTR`; `calibrate` removed); its
  estimate stops summing once the threshold is passed.
* **`Backend for Spans::fill` through `each_in`** with the pass's cut (clip, frame, the mask's bounding box).
  `num` and `label` were **not** changed: the span painter cuts their text to the space's clip, not to the
  record's rect, so a record outside the cut could still draw inside it.
* The layers' pool reservation is 512 MiB of address space: slice 4's 128 MiB refused the 3,000-dense scene
  (not caught then because slice 4 did not run it).

## Gates

`cargo test --release -p bough-paint -p bough-span -p bough-guibench-x`: all passed (paint lib 2, exact 5, fx 8,
review 8, scenes 2; guibench-x 7 + 11 + 2 + 2 + 1). `mutant_prune` (a block in an even column counts as
unchanged): 123 failing steps of the main gate, 17 of the mixed gate, 24 of 48 block-edge cases.
guibench-x `mutant_straddle`: 4 tests fail; `mutant_age`: 5 fail.

## Thresholds

| | prediction | measured | verdict |
|---|---|---|---|
| C1 chooser: same executor in every repetition | | Metal in all three repetitions of every non-idle cell at 200, 1,000, 4,000 and 3,000 dense | held |
| C1 chooser within 5 % of min(CPU kernel, Metal) | | 0.97x to 1.004x of Metal in every cell except theme: 1.057x (200), 1.044x (4,000). Before the estimate was cut short: scroll 1.17x, and 9.9x at 3,000 dense. | held but for theme at 200 (**1.057x**) |
| C2 unchanged fraction, one spring, 600 dense | < 30 % | 99 % | **prediction wrong** |
| C3 span fill visits fewer records, pixels unchanged | | pixel gates unchanged; **records visited not measured** | half |

## Cells: CPU kernel, K instructions per frame, before (slice 3 / 4) -> after compare-on-write

| scene | one spring | hover beneath glass | glass moves | all springs |
|---|---|---|---|---|
| effects 200 | 49,330 -> 35,763 | 54,798 -> 44,198 | 35,164 -> 19,358 | 68,240 -> 54,244 |
| effects 4,000 | 49,425 -> 35,790 | 55,057 -> 44,468 | 35,162 -> 19,364 | 68,667 -> 54,670 |
| 600 dense | 895,405 -> 200,946 | 623,846 -> 270,123 | 533,434 -> 113,252 | 1,037,524 -> 343,630 |
| 3,000 dense | 5,948,383 -> 1,049,886 | 5,281,068 -> 1,133,394 | 4,893,859 -> 497,664 | 6,585,265 -> 1,689,592 |

Unchanged by it: scroll (600 dense 4,755,898; 3,000 dense 25,065,551), theme, hover above glass, idle.
600 dense one spring now repairs 650 prefix tiles in six frames against 3,972, and shades 46.7 output tiles a
frame against 164.

**Metal's staged frame, frame-thread K instructions** (conservative plan): 600 dense one spring 6,369;
3,000 dense one spring 215,198, hover beneath 195,799, glass moves 194,684, scroll 585,355, theme 862. The
theme stays one resident dispatch at any depth. Everything else at 3,000 dense is now dominated on the frame
thread by building the conservative plan's jobs (5,003 prefix jobs and 39,062 blocks a frame), which is the
work compare-on-write shows to be 99 % unnecessary. GPU time at 3,000 dense: about 13 ms (load 50 to 65, no
lock: indicative).

## The orchestrator's (a) to (d), each answered

| | answer |
|---|---|
| (c) stop coarse damage that changes nothing | **taken**, CPU executor: 4.5x at 600 dense and 5.7x at 3,000 dense for one spring; 9.8x for a moving panel at 3,000 dense. Not on Metal: it needs the stage's "changed" bits back before the next stage is encoded (a readback per rank), or the schedule on the GPU. That is the next step and the larger prize there (215 M frame-thread instructions at 3,000 dense). |
| (a) a stack of glass as one reader | **not taken, and it would not apply to the measured scene**: every pair of panels there has a dozen shadowed cards between them. Where nothing lies between, it is right and cheap to state (one read at the composed level with the composed colour matrix) but it changes the specification (no flatten between panels) and so every byte of a stacked-glass frame; I did not make that change without a scene that needs it. After (c), the cost that remains at depth is the first-generation check (614 of the 650 prefix tiles still come out unchanged: they must be computed to know), which (a) does not remove either. |
| (b) strata | **not taken.** Exact only where the stratum above is opaque or of uniform alpha over the blur's support; a glass panel's rounded edge and tint make neither true on its edge tiles. A per-tile certificate with the sequential form as fallback is possible; not designed further here. |
| (d) shadows as retained coverage | **not started.** Still the cost of every "records between" tile and of the canvas. |

3,000 dense against 600 dense for one spring on the CPU is now 5.2x (it was 6.64x). **The 1.2x target is not
met.**

## Not done / not run

* `comparisons/bough-fx-bench` against this seam at 200 / 1,000 / 5,000: **not run**. In the tree I merged
  (wave3 at the merge) it has no dependency on `bough-paint` and no adapter to this seam; the measurement
  lane's binding was not there to reconcile with. 5,000 cards remains refused by this scene's 16-bit window
  lane (4,000 measured).
* R2 (ink-space surface), palette tables, scroll as a uniform, hover through an id index, a per-column
  sweep, groups, shader modules, image decode: not started.
* fx-seam.md was not changed in this slice: `update` no longer schedules the layers, so a driver that uses
  Metal must call `update_eager` (or `plan_eager` before reading the damage), and a CPU driver must read the
  damage after `prepare`. That note is owed to the seam document.
