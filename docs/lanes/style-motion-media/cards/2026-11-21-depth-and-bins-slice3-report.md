# Report: depth, incremental bins, an O(reached) schedule (card 2026-11-21-depth-and-bins-slice3)

State reached: **MEASURED** for D1's two conditions, with one prediction missed and the orchestrator's 1.2x
target not met. Not independently replayed. Items 3 to 7 of the orchestrator's list were **not started**
(below). Raw output: `bench/results/fx/slice3.txt`, `pbench-slice3.txt` (dist, M3 Ultra).

**Measurement conditions.** None of these runs held the bench lock: it had 16 jobs waiting and the load
average was 49 to 69. Instructions of the frame thread are the only column used; they repeated to four digits
between the two runs I made of the unchanged cells. Wall and GPU times in the raw files are not reliable.
`NOCONTROL=1` skipped the redraw-every-layer control (it is seconds per frame in the dense scenes).

## What changed

1. A backdrop record flattens over the whole tiles of its halo; its stored prefix is the accumulator on every
   tile it is listed in; every prefix job and every output tile restarts at the topmost layer record of its
   range (`Scene::seg`). A changed halo damages the tiles it left and entered (`Pass::halo_to`).
2. `Bins`: per-tile lists edited for the records that moved (`Bins::update`, cuts recomputed once per touched
   tile); a full rebuild when more than a quarter of the records moved.
3. Each tile keeps its layer records; an event marks only the layers above it on the tiles it touches; layers
   with work are taken in paint order from a set (`Scene::update`).
4. Metal: one dispatch per stage for layers that share no tile with a lower layer scheduled that frame.

Sharing one prefix store between stacked readers was **not** built: their prefixes differ (the records
between, and the lower reader's own ink). What stacked readers share now is the work beneath the lower one.

## Gates (release, at the head of this slice)

`cargo test --release -p bough-paint`: lib 2, exact 5, fx 7, review 8, scenes 2, all passed; CPU == Metal 0
words over 6 scenes x 26 steps (7,526 prefix jobs, 53,418 blocks, 6,530 output tiles, 2,887 dispatches against
3,019 before ranks). New gate `a_halo_crossing_tile_edges_stays_exact` (60 drags of three stacked panes).

Mutants, each run against `tests/fx.rs` at this head (the four of slice 1 re-run as asked):

| mutant | result |
|---|---|
| `mutant_layer_event` | 149 failing steps |
| `mutant_layer_weights` | 56 failing steps |
| `mutant_settle` | 66 failing steps |
| `mutant_video` | 95 failing steps (99 in slice 1) |
| `mutant_layer_dilate` | 48 of 48 block-edge cases |
| `mutant_halo_damage` (new) | 51 failing steps of the main gate. The gate written for it (halo crossing) does **not** catch it: its three panes' halos overlap almost everywhere, so little is ever left or entered. |
| `mutant_bins_stale` (new) | 114 failing steps. Its first form (a moved window not *removed* from the tiles it left) was not caught and cannot be: a record shades nothing outside its window. It now skips the insertion. |

The GPU source mutant "no flatten under a backdrop" was removed: with every job starting from a stored 8-bit
prefix the flatten in the shade path is a no-op, so nothing can see it dropped. Five GPU mutants remain, all seen.

## Thresholds

| | prediction | measured | verdict |
|---|---|---|---|
| D1 entries per prefix job, 600 dense one spring | <= 0.25x slice 1 | not instrumented per job; see D2 | **not measured** |
| D2 600 dense one spring | <= 0.5x of 3,185,064 K | 895,405 K = 0.281x (same 662 prefix tiles and 5,767 blocks) | held |
| D3 3,000 dense one spring completes, <= 6x of 600 dense | | completes, 0 mismatches; 5,948,383 K = **6.64x** (slice 1: stopped at about 69.8 G) | completes; **missed** the 6x by a tenth; the orchestrator's 1.2x target is **not met** |
| D4 pane one spring 300 vs 3,000 | within 1.15x | 417.3 K vs 417.4 K = 1.000x (slice 1: 690.8 vs 921.0) | held |
| D5 no regression | pbench A1 <= 1.03x; effects 300 <= 1.05x slice 1 | pbench A1 0.945 to 1.017 (pan 1.010 after two corrections, below); effects 300 every cell faster | held |

Two things found while measuring D5, both fixed before the numbers above: the canvas pan was 12.2x dearer in
its bins step (1.063x on the whole frame) because each moved record recomputed the cut of each of its tiles
(now once per touched tile), and a pan that moves most records is cheaper as one rebuild (now chosen when
more than a quarter moved). The pan's bins step is still 1.67x the baseline's rebuild.

## Cells (CPU one core, median K instructions per frame; slice 1 in brackets where it existed)

| scene | idle | one spring | all springs | scroll | hover above glass | hover beneath | theme | glass moves |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| pane 300 | 5.6 | 417 [691] | 19,345 [19,613] | - | 9,461 [11,208] | 34,858 [43,886] | - | 16,961 [21,829] |
| pane 3,000 | 5.5 | 417 [921] | 19,612 [20,071] | - | 9,625 [11,359] | 35,022 [44,046] | - | 16,960 [22,062] |
| effects 300 | 5.7 | 49,398 [67,390] | 68,270 [86,301] | 990,646 [1,221,214] | 6,097 [11,221] | 54,785 [85,440] | 977,130 [1,223,475] | 35,150 [55,756] |
| effects 3,000 | 6.0 | 49,408 [67,793] | 68,532 [86,978] | 994,133 [1,224,531] | 6,282 [11,444] | 54,968 [85,664] | 977,705 [1,223,393] | 35,153 [56,134] |
| effects 600 dense (6 frames) | 5.8 | 895,405 [3,185,064] | 1,037,524 [3,328,134] | 4,752,862 [10,426,671] | 7,349 [44,661] | 623,846 [2,631,098] | 5,407,668 [12,547,104] | 533,434 [2,076,649] |
| effects 3,000 dense (3 frames) | 5.4 | 5,948,383 | 6,585,265 | 25,055,729 | 12,365 | 5,281,068 | 26,847,496 | 4,893,859 |

media 24 / 48: one video frame 14,338 / 14,338; video under glass 30,841 / 30,840 [39,989]; hover 39,483
[49,942]; theme 386,689 / 576,337; glass over playing video 10,539 / 10,566 (median). Every cell of every
scene: 0 mismatching words against a full redraw, CPU and Metal.

Metal, 3,000 dense one spring: 13.6 ms GPU median in the raw file (unreliable at this load), and 219,611 K
instructions on the CPU side to build 5,003 + 39,062 jobs and re-upload the lists: the staged path's CPU side
is now its own cost there.

## Why 3,000 dense is still 5.9 G, and what would change it

The cell is 187 endless shader paints beneath stacks of about thirty glass panels. Each changed tile is
repaired by every reader above it (5,003 prefix jobs a frame), each job shading the dozen shadowed cards
between two readers, and each reader's blur widens the reach handed to the next. Restart removed the
"everything beneath" factor; what is left is (readers above) x (records between) x (reach), which is one full
tile program per reached tile plus one pyramid repair per reader. Getting from here to "depth-independent"
needs one of: readers that share a pyramid when nothing lies between them; a cap on how far a coarse level's
tiny weights propagate damage (a stated tolerance instead of exactness); or cheaper records between (the
shadows: item 3).

## Not done from the orchestrator's list

* **3 Shadows.** Sol's patch was applied in slice 2 (commit `0ec2df9c05`; its gates pass here, its speed was
  not re-measured). No retained shadow coverage, no nine-patch: not started.
* **4 Groups, author shader modules, image decode, SVG.** Not started.
* **5 The effects benchmark as judge.** `lane/integration/wave3` is merged into this branch (no conflicts).
  `comparisons/bough-fx-bench` was not run against this seam; none of its cells is reported here.
* **6 X22 and the façade seam.** Read `lane/integration/card3-facade` at `ef4c1c38cb`: nothing in it depends
  on `bough-paint`'s `Scene` (the lowered runtime draws through `bough-x` sinks and `bough-span`), so there is
  no call site to patch and **no `fx-facade-seam.patch` was exported**. `~/work/notes/reports/fx-seam.md` says
  what a driver must call; that is a description, not a patch.
* **7 `ColRef::each_in` in bough-span.** Waiting for it to reach wave3, as instructed.
* The void 4-video cell was fixed in slice 2 (its report has the re-measured P6: 1.29x against 4 videos).
