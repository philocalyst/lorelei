# Report: a broad change is one word; the chooser (card 2026-11-21-broad-words-slice4)

State reached: **MEASURED** for R1's scene side and R5; R5's chooser misses its own criterion (below). R2, R3
as a rule and R4 were **not started**. Not independently replayed.
Raw: `bench/results/fx/slice4.txt` (three repetitions of `NOCONTROL=1 fxbench effects 200|1000|4000`, dist),
`pbench-slice4.txt`. No bench lock (instruction columns; load average 55 to 67 throughout, recorded per run):
GPU microseconds below are indicative only.

**5,000 cards was not run.** The scene refuses it at admission ("a rect lane beyond +-32767 px": that world is
40,000 px tall; the 16-bit window lane is a known limit of this scene). The largest size this scene shape
admits, 4,000, stands in for it. The measurement lane's `comparisons/bough-fx-bench` harness was **not** run
against this seam; these are `fxbench` cells.

## What was built

* `Scene::broad`: a declared-broad block word is never differentiated. Its change damages the frame as one
  fact and sets `Plan::full` (every layer whole, nothing enumerated): no record visited, no bins rebuilt.
* The layers' pool is one page-aligned reservation (`layer::PoolMem`, 128 MiB of address space, touched where
  layers are) that Metal wraps without a copy: one retained store for both executors.
* `Gpu::staged` keeps the whole-frame job buffer resident, keyed by the bins and layer-table epochs; a broad
  frame re-submits it.
* `mixed::Mixed`: broad goes to the GPU unseen; otherwise predicted CPU time of the damage (tile weights and
  layer work, O(damage)) against the GPU's round trip; two device constants from `calibrate()`.
* Not built: palette **tables** (slot -> one of N resident palettes). The scene's themed colour is two colours
  per record mixed by a block word in the kernel; `bough_expr::Cx` has no resource an expression could read a
  table from. More than two themes needs that seam.

## Gates

`cargo test --release -p bough-paint`: lib 2, exact 5, fx 8, review 8, scenes 2, all passed.
`executors_alternate_and_a_broad_word_costs_nothing_to_find`: 6 scenes, 204 frames (13 idle, 93 CPU, 98 Metal,
executor forced at random or chosen), 41 broad changes each with 0 records visited, bins untouched and no
layer enumerated; 0 failing steps against a fresh redraw. `mutant_broad_layers`: 127 failing steps;
`mutant_full_stale`: 95.

A gate was found to have gone soft and was repaired: with one shared pool, the CPU == Metal test's CPU
`prepare` repaired whatever a broken GPU stage left ("prefix not stored" went unseen). The retained scene is
now drawn by Metal alone and compared with a fresh CPU redraw; the five GPU source mutants are seen again.

## Thresholds

| | prediction | measured (median of three; [min-max]) | verdict |
|---|---|---|---|
| R1a theme frame-thread instructions flat across sizes | <= 1.2x | chooser: 194.5 K [191.1-206.2] at 200, 189.8 K [179.7-193.6] at 1,000, 191.4 K [186.9-201.2] at 4,000 = 1.02x | held |
| R1b theme <= 3x a one-spring frame on Metal | | 191.5 K against 277.7 K (200), 191.6 against 286.0 (1,000), 189.2 against 330.0 (4,000): 0.57x to 0.69x | held |
| R1c theme turn (`Scene::update`) | <= 3,000 instructions | 2,294 at every size (an idle turn is 2,061). It was 12,263 until `Damage::all` stopped setting a bit per tile. | held |
| R5a chooser never worse than min(CPU, Metal) + 5 % | | **missed**: see the table; and it is not stable | missed |
| R5b two device constants measured at start-up | | built; measured under load they came out at 392 to 904 ns per weight unit and 1.2 to 5.0 ms for the GPU floor across runs | built, **unreliable under load** |
| D5 pbench A1 | <= 1.03x of 71ee3be | 0.946 to 1.015 (canvas full 1.008, hover 1.007, hover1 1.015, theme 1.007, pan 1.010, drag 0.946, time 1.002; anim 0.989 / 0.989 / 0.990); 0 cross-check failures | held |

Before this slice the theme frame on Metal cost the frame thread 1,472 K (300 cards) and 1,994 K (3,000);
on the CPU kernel it is 976.8 M at every size, which is why the chooser never sends it there.

## Every cell (K instructions of the frame thread per frame, median of three runs; 200 / 1,000 / 4,000 cards)

| cell | CPU kernel | Metal staged | chooser | chooser ran |
|---|---|---|---|---|
| idle | 8.8 / 8.7 / 8.7 | 57.9 / 58.2 / 58.8 | 2.6 / 2.6 / 2.6 | nothing |
| one spring (+ endless shader paints) | 49,330 / 49,490 / 49,425 | 277.7 / 286.0 / 330.0 | 285.9 / 298.8 / 342.0, but 49.4 M in 3 of 9 runs | GPU in 6 runs, CPU in 3 |
| all springs | 68,240 / 68,578 / 68,667 | 311.1 / 404.4 / 752.8 | 315.4 / 421.8 / 758.2, 68.3 M in 1 of 9 | GPU in 8, CPU in 1 |
| scroll 11 px | 990,811 / 992,128 / 995,997 | 2,400 / 3,463 / 7,384 | 2,544 / 3,609 / 7,539 | GPU |
| hover above glass | 6,094 / 6,148 / 6,353 | 94.9 / 152.8 / 365.5 | 6,092 / 6,146 / 6,351 | CPU, always |
| hover beneath glass | 54,798 / 54,851 / 55,057 | 303.6 / 371.3 / 566.3 | 312.1 / 367.2 / 580.2 | GPU |
| theme | 976,849 / 976,839 / 976,919 | 191.5 / 191.6 / 189.2 | 194.5 / 189.8 / 191.4 | GPU, by class |
| glass moves 1 px | 35,164 / 35,163 / 35,162 | 294.9 / 319.9 / 424.3 | 35,161 / 323.7 / 35,160 | mixed: 22 CPU + 2 GPU frames in most runs |

Metal GPU time (median, microseconds, load 55 to 67, no lock): theme 1,339 / 1,668 / 1,363; scroll 1,661 /
2,393 / 1,507; one spring 337 / 421 / 433; hover above 147 / 149 / 148. Every run: 0 mismatching words.

## What the table says, plainly

1. **The theme is one word now**: its turn is an idle turn, its frame-thread cost is the command buffer, and
   it is flat in scene size. The GPU does one full staged frame (every layer, then every tile); making *that*
   smaller is R2.
2. **The chooser is wrong for this project's bar.** It minimises predicted wall time, and by frame-thread
   instructions Metal wins every non-idle cell by 20x to 5,000x. It sent "hover above glass" to the CPU every
   time (6.1 M instructions against Metal's 95 K to 366 K) and flipped "one spring" and "glass moves" between
   executors from run to run because its two constants are wall-clock measurements taken under a load of 60.
   A chooser denominated in frame-thread instructions (damage weight times a fixed instructions-per-weight,
   against the command buffer's ~190 K) would choose Metal for everything above a few tiles and would not
   depend on load. I did not make that change after seeing the numbers; it is the first item of the next card.
3. **Three cells still follow the scene on the frame thread, and each is a basis problem, not a constant:**
   * *hover* (95 K -> 366 K on Metal; turn 16.7 K -> 276 K): the window pass asks every record of the hovered
     column whether its colour changed. The word is the hovered id; the basis is "the two records whose id
     equals the old and the new value", which needs an id -> record index per column (or the pick's own
     answer): O(1). Not built.
   * *scroll* (turn 1.87 M -> 6.72 M): a pan moves every record's window, so every window, bin and layer is
     rewritten. The word is the origin; the basis is windows and bins kept in the scrolled space with the
     origin a uniform, damage = the rigid shift's uncovered band. This scene has no spaces (the core's stores
     do). Not built; it is the largest remaining frame-thread cost in the table.
   * *all springs* (311 K -> 753 K): per moving record per frame (the active set). Endpoints and a law are
     already what is stored; what is per-record is the window lane and the damage. A column-level sweep rect
     (the core's `Sweep`) makes it O(columns). Not built.

## Reformulations considered (the card's table, with what was measured)

| cell | the word | outcome |
|---|---|---|
| theme | the mix word, declared broad | taken: turn 2,294, frame 190 K, flat |
| scroll | the origin, with windows in the scrolled space | considered, not built (above) |
| hover | the hovered id, with an id -> record index | considered, not built |
| many moving | a sweep rect per column | considered, not built |
| resize | the viewport, rects affine in it | not built: needs the layout lane's affine answers |
| subtree opacity / tint | a group word at the layer | not built: needs isolated groups |
| depth | one retained thing shared by stacked readers | slice 3 took restart; a shared pyramid is open |
| shadows | the coverage function's table | corners only, grid-aligned (existing `Tab`); off-grid open |

## Not verified

GPU time on a quiet machine or under the lock; the effects benchmark harness of the measurement lane; 5,000
cards; Vulkan; the chooser on any other device; a theme cross-fade driven by a host (the word is broad, so a
per-refresh write costs the theme row's turn, but no test animates it).
