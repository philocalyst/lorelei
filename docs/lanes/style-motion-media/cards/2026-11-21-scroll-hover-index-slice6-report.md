# Report: a pan and a hover by index; 5,000 cards admitted (card 2026-11-21-scroll-hover-index-slice6)

State reached: **MEASURED**. Not independently replayed. Raw: `bench/results/fx/slice6.txt` (three repetitions
of `NOCONTROL=1 fxbench effects 200 | 1000 | 5000`, then `pbench canvas 4`, `pbench anim 600`; dist; no bench
lock: instruction columns; load recorded per run). All nine fxbench runs: "mismatching words in total: 0".

Of the orchestrator's six items: 1 (chooser) was done in slice 5; **2 is done in a weaker form than asked**
(below); 3 is half done (hover yes, per-column sweep no); **4 (palette tables through an `Ink` word) was not
started**; 5 is done; 6 was not started.

## What was built

* `Scene::scroll(col, wx, wy)`: the column's records are indexed by row (256 px) of the scrolled space. A
  record's reach is evaluated under a pan that puts its box at the origin, so the window lane's 16-bit clamp
  does not touch it. A change of the pan visits the records in the rows the frame showed and shows, through
  the existing window pass; their bins are edited in place.
* `Scene::keyed(col, word)` / `set_key`: a change of the word visits the records keyed by its old and new value.
* Admission: a rect lane may lie up to +-2^20 px from the origin when the record itself is under 32,768 px
  wide and tall; the old rule (everything within +-32,767 px) still admits what it did (the existing gate
  with a 60,000 px box across the surface is unchanged).

**What item 2 asked for and this is not.** "Windows and bins kept in the scrolled space so a pan rewrites
nothing." Here a pan still rewrites the windows and bins of the records in view: the turn is flat in the size
of the world but it is not small (1.1 M to 1.2 M instructions for about 200 visited records: a card with two
24 px shadows sits in some thirty tiles, and each is edited). The GPU still redraws the viewport. The
formulation that rewrites nothing is designed below and not built.

## Gates

`cargo test --release -p bough-paint`: lib 2, exact 5, fx 9, review 8, scenes 2, all passed.
`a_pan_and_a_hover_visit_what_they_can_reach` (a 7,000 px world through a 320 x 250 frame, 90 steps of pans
from 1 px to 3,100 px, hovers, springs, rewrites, and turns that do several at once): 0 failing steps against
the same scene without the declarations drawn fresh, and against the specification; a pan visited 19.7 of 220
records on average, a hover 8.9. `mutant_scroll_band`: 80 failing steps. `mutant_key_old`: 10.
While building, my first admission rule (extent under 32,768 px for every record) broke the existing gate
`a_huge_box_across_the_surface_is_exact`; the rule is now the old one or the new one.

## Thresholds

| | prediction | measured (200 / 1,000 / 5,000 cards) | verdict |
|---|---|---|---|
| S-a scroll turn flat | <= 1.2x | 1,141,368 / 1,222,281 / 1,219,659 instructions = 1.07x (slice 4: 1.87 M -> 6.72 M at 4,000) | held |
| S-b hover turn flat and under 10 K | <= 1.2x | 4,490 / 4,490 / 4,368 (slice 4: 16.7 K -> 276 K) | held |
| S-c scroll and hover frame-thread on Metal flat | <= 1.2x | scroll 2,311 K / 2,407 K / 2,537 K = 1.10x; hover above glass 72.9 K / 72.6 K / 72.5 K | held |
| S-d 5,000 cards, every cell, 0 mismatches | | all eight cells, three executors, three repetitions | held |
| pbench A1 | <= 1.03x of 71ee3be | 0.947 to 1.015; 0 cross-check failures | held |

## Every cell (K instructions of the frame thread, median of three; 200 / 1,000 / 5,000 cards)

| cell | CPU kernel | Metal staged | chooser (Metal in every run) |
|---|---|---|---|
| idle | 8.9 / 8.9 / 9.0 | 56.6 / 56.4 / 56.0 | 2.7 / 2.7 / 2.7 |
| one spring | 35,761 / 35,786 / 35,879 | 255.5 / 285.4 / 409.6 | 253.2 / 282.7 / 408.3 |
| all springs | 54,257 / 54,421 / 55,105 | 306.1 / 458.0 / 1,189.6 | 302.5 / 454.0 / 1,185.4 |
| scroll 11 px | 991,086 / 991,082 / 991,166 | 2,311 / 2,407 / 2,537 | 2,321 / 2,436 / 2,533 |
| hover above glass | 6,082 / 6,082 / 6,082 | 72.9 / 72.6 / 72.5 | 70.0 / 73.6 / 71.5 |
| hover beneath glass | 44,182 / 44,182 / 44,181 | 269.5 / 270.2 / 268.7 | 266.1 / 270.5 / 266.5 |
| theme | 977,059 / 977,055 / 977,109 | 178.7 / 181.9 / 178.9 | 189.0 / 192.3 / 189.9 |
| glass moves 1 px | 19,358 / 19,358 / 19,363 | 275.9 / 302.1 / 417.4 | 271.6 / 299.3 / 416.2 |

Metal GPU time (indicative: no lock): theme about 1.3 ms, scroll about 1.4 ms, hover above 0.11 ms.

## What still follows the scene, and why

* **one spring: 255 K -> 410 K.** The cell's 13 / 63 / 313 visited records are the world's endless shader
  paints: a record that reads the clock forever is in its column's active set wherever it is, and is visited
  every frame though it is off screen. With the scroll index a paint-only clock reader could be visited only
  when in view; what stops that being a two-line change is `Scene::animating()`, which would then never learn
  that an off-screen finite animation ended. It needs ends kept in time order. Not built.
* **all springs: 306 K -> 1,190 K.** 37 / 187 / 937 records are moving (the cell retargets every dot in the
  world): O(moving), as designed. A per-column sweep rect (the core's `Sweep`) is the O(columns) form; not built.
* **glass moves: 276 K -> 417 K** on Metal while its turn is flat (92 K): when the bins change, `Gpu::staged`
  re-uploads every record's window (a loop over the world) and the whole list array. The bins now know which
  tiles changed (`Bins::take_changed`); the upload does not use it yet.
* **scroll turn 1.1 M** (flat): see above; and the staged frame's job building.

## The formulation in which a pan rewrites nothing (designed, not built)

1. A scrolled column's windows, covers and halos are kept in the scrolled space, relative to a per-space
   anchor (a multiple of 32 px) so that 16-bit lanes hold; the anchor moves rarely (a full pass for the space).
2. Its bins are cells of 64 x 64 px on a 32 px lattice in that space. Any frame tile, wherever the pan puts it,
   lies inside exactly one cell, whose list is a superset of what touches the tile (exact by the kernel's
   superset identity). A pan therefore changes which cell a tile reads, and no list.
3. The kernel applies the origin: an entry of a scrolled column is shaded at (pixel - origin) with the space's
   clip; on Metal that is two words per column in the uniform block and a subtraction per entry; on the CPU
   tile it is the tile's origin for that column's run. The specification does the same.
4. A frame tile's program is the merge by key of the fixed columns' list and the cell's list.
5. Then the surface can move too: tiles whose program is entirely of the scrolled space are copied by the pan
   and only the uncovered band is shaded; layers of that space are addressed in the space, not the frame.
What it touches: `tile.rs` (per-run origin, cover masks), `scene.rs` (lanes, bins, damage in two coordinate
systems, `seg`), `layer.rs` (addressing), `emit.rs` / `metal.rs` (clip as a parameter, per-entry origin, two
list sources per job), the specification and the resident path. It is the next card for scroll.

## Not done

Palette tables through an `Ink` word type (item 4: no edit was made to `bough-expr`); the per-column sweep;
depth (a), (b), (d); Metal-side pruning; incremental upload; `comparisons/bough-fx-bench` against this seam
(still no adapter to it in the merged tree: not run); groups, shader modules, image decode.
