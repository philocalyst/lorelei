# Effects and blur: what is tested, and would it catch a bug

Lane `style-motion-media/fx-coverage`, 2026-11-23. This is an audit of the damage engine (`crates/bough-damage`) and the flat core's
blur and effects raster (`crates/bough-flat/src/raster{,/blur}.rs`). It records the tests added to close the gaps, a hand mutation
check of those tests and the native gate cells added for the flat paint bank and the engine.

"Before" names what existed when the lane started. "Now" names what this lane added. "Catches" says whether a plausible bug in the behaviour makes a test fail, judged
by the mutation check below where a mutant exists.

## bough-damage

`bough-damage` had in-crate unit tests in `src/engine.rs` (16) and no `tests/` directory. The flat core's oracles
(`bough-flat/tests/oracle.rs`, `mutants.rs`, `alloc.rs`) exercise it through `FlatWorld`, on one fixed scene.

| Behaviour | Before | Now | Catches |
|---|---|---|---|
| `blur_write` (write footprint, exact) | unit `the_blur_footprint_is_exact_for_the_pyramid`: 1-D, levels 0..=4, interior pixels only, exact ends | `tests/footprint.rs`: 1-D, levels 0..=6, five domains (negative, odd, zero origin, sizes `7<<l + 13`), every pixel incl. clamped edges, exact as a set; ranges are the union of their pixels and a whole-range perturbation | yes: M3 |
| `blur_write` blocks (dirty texels per level) | not tested (only counted in the report test) | footprint: each level's block equals the texels of that level that change, exactly; engine oracle: report blocks equal `blur_write` of block 0 | yes: M4 |
| `blur_read` (read footprint, exact) | unit: only "the pixel is inside the read of its write ends" | footprint: exact against the set of pixels whose writes meet the output range, every level and domain; transpose test over ±300 px | yes: M1, M2 |
| `blur_level` | not tested | bit-length rule, clamp to `MAX_BLUR_LEVEL`, 0 for radius ≤ 0 | yes (by inspection: any change of the rule breaks the bounds) |
| `BlurLayer::expanded_read_rect`, `output_of` (2-D) | unit: one inequality; raster unit `a_blurred_region…` checks `output_of` ⊇ change for random pixels | footprint: 2-D, levels 0..=3, a 75 x 53 window, six rects (inside, across the left/bottom/right edges, a 1-px column, the whole window): every pixel on the read rect's inner edges is read, none just outside is; `output_of` of random changed rects is exactly the changed outputs; empty rects and level 0 | yes: M1–M3, M15 |
| Opaque covers: `cover_push` (cut to window), `covers_commit` (whole tiles; window-edge rule), culling `cover.z > z` | unit: four hand cases | `tests/engine.rs` exact mode: random covers and damage, the mask, `occlusion_skips` and `occluded_tiles_skipped` predicted tile for tile from an independent whole-tile rule; flat oracle mutant `OCCLUSION_ANY_Z` | yes: M5, M6, M12 |
| Damage under a cover still reaches a blur layer between (`CULL_BEFORE_BLUR`) | unit + flat mutant | engine oracle (full mode) | yes: M14 |
| Blur propagation and cascade through stacked layers | unit: one cascade | engine oracle: random stacked, overlapping layers at levels 0..=4 over a pixel reference that repaints everything; gated and ungated | yes: M7, M15 |
| Gate (`resolve` with a gate) halting/narrowing | unit: two cases | engine oracle gated runs: the gate is the real backdrop diff, as a retaining executor computes it; the mask must still cover every changed pixel | yes (M7/M15 also fail gated runs) |
| `damage_move` (hull when touching, two rects apart) | unit: two cases | exact against `tiles(old) ∪ tiles(new)` (∪ the hull when they touch) over 400 random moves; full oracle uses it | yes: M13 |
| `scroll` (blit, strip, virtual backdrop damage, whole-clip fallback) | unit: one scroll each axis | engine oracle: random scrolls of a clipped group (small, sideways, a jump past the clip), the old frame blitted must equal the new outside marked tiles | yes: M8, M9, M18 |
| `seam` | unit + flat mutant | engine oracle: every fixed item over the clip | yes: M16 |
| `ScrollBlit::kept` | raster unit (blit) | `a_scroll_blit_keeps…`: a kept pixel and its source are both in the clip, and every such pixel is kept | yes: M9 |
| `damage_mask`, `damage_virtual` | unit (report) | engine oracle (theme op; scroll) | yes (indirect) |
| `rebase_orders` | unit: one case | engine oracle: inserts and removals renumber mid-frame; exact mode predicts no culling of rebased damage | yes: M17 |
| `TileMask` (`mark_tile`, `mark_rect`, `or`, `clear`, `each_tile`, `each_span`, `count`, `test`, `fraction`, `mark_all`, `resize`) | unit: `mark_rect`/`clear`/iteration vs naive on one size | `a_tile_mask_equals_a_naive_bitmap…`: every mark kind, unions, clears, on 2100 x 900, 300 x 2200, 33 x 33 and 4103 x 300 (more than one word of tiles, several super-tile rows) | yes: M10, M11 |
| Report (`report`, `report_into`, counts, `repair_percent`, blocks, blits, covers, layers) | unit: one frame | engine oracle: every frame, report equals the engine's state, and `report_into` a kept value equals `report()` | yes (indirect) |
| `to_json` | unit: substring checks | unchanged | partly (field order or escaping bugs would pass) |
| "A warm frame allocates nothing" | flat `tests/alloc.rs` through `FlatWorld` | `tests/alloc.rs`: counting allocator over 200 frames of every kind straight on the engine (recommits, stacked layers, scroll + seams, mask, rebase, gated resolve, `report_into`) with a negative control | yes |
| `resize`, `pending`, `layer_push` clamp, one resolve per frame | not tested | engine.rs: two tests | yes |
| `Chooser`, `WorkEstimate::cost`/`signature` | unit: hysteresis per signature | unchanged | partly: the cost weights are only tested through choices |
| `EffectExtents::window`, `shadow_reach` | unit + raster unit (shadow inside its window) + flat mutant `NO_EFFECT_HULL` | unchanged | yes |
| `opaque_core` | unit: bounds for one radius | `bough-flat/tests/blur.rs`: 3000 random boxes and radii, every core pixel has raster coverage 255 | yes (an under-inset core fails) |
| `damage_hull` | not tested (a pair of `and`s, no caller in tree) | unchanged | no |

## bough-flat blur and effects raster

| Behaviour | Before | Now | Catches |
|---|---|---|---|
| `paint_backdrop_blur` is the documented pyramid | no direct test: the oracles compare the raster with itself | `tests/blur.rs`: bit for bit against the pyramid written out from `bough-damage`'s doc (per channel, `[1 3 3 1]`, `[1 3]`/`[3 1]`, each level rounded), levels 0..=6, three windows (97 x 75, 64 x 64, 130 x 41), five rects (inside, off the top-left, off the right, the whole window, a 1-px column), radii 0..12 px and opacity 255/128/1/0 | yes: R1, R2 |
| A backdrop cut to the read rect gives the same pixels (the executor's assumption) | not tested | `tests/blur.rs`, every case above | yes: R3 |
| A tile repaired alone equals that tile of a full repaint (blurred content) | raster unit `a_blurred_region_equals…` (random sub-rects, levels 1..=4); `oracle.rs` random walks with two glass panes (`retained == full redraw` after every operation) | `tests/blur.rs`: every 32 x 32 tile of a 150 x 110 window, levels 1..=6, a rect partly off the window | yes |
| Blur footprint as the raster sees it | raster unit: changed outputs ⊆ `output_of` | unchanged (exactness is shown against the oracle pyramid, which the raster equals) | yes |
| `paint_shadow`, `soft`, `sdist`, `Shadow::window` | raster unit: zero outside the window, window not slack | unchanged | yes for the window, no for the falloff shape |
| `paint_box` (runs vs per pixel, borders, radius, gradient) | raster unit vs a reference per-pixel path | unchanged | yes |
| Element blur (`elem_blur`) | `oracle.rs` (self-consistency only) | unchanged | no reference for its shape |
| `Surface::blit` | raster unit, every direction | unchanged | yes |
| Flat driver's use of the engine | `oracle.rs`, `mutants.rs` (8 negative controls), `alloc.rs` | unchanged | yes, on its one scene |

`bough-paint` (`ops.rs` backdrop/shadow/inset/mip, `fx.rs`, `flat_scene`) has its own pyramid and does not use `bough-damage` or
`bough-flat`. Its tests (`fx.rs`, `exact.rs`, `flat.rs`, `scenes.rs`, `review.rs`, `motion.rs`) compare executors and retained
frames with full redraws. They are outside this map except for the gate cells below.

## Mutation check

Each mutant was applied by hand to the source, the tests were run, and the source was restored
(`git status` clean under `src/` afterwards). "Old" is the in-crate unit tests that existed before
(`bough-damage --lib`, `bough-flat --lib`). "New" names the new test binaries that fail. In every row at
least one new test fails.

| # | Mutant | Old | New |
|---|---|---|---|
| M1 | `blur_read`: on the way up `lo = lo >> 1` (for `(lo - 1) >> 1`) | caught | footprint (3 tests), engine oracle |
| M2 | `blur_read`: on the way down `hi = 2hi + 1` (for `2hi + 2`) | caught | footprint (4 tests), engine oracle |
| M3 | `blur_write`: on the way down `hi = hi >> 1` (for `(hi + 1) >> 1`) | caught | footprint (3 tests), engine oracle |
| M4 | `blur_write`: level blocks recorded with `lo + 1` | missed | footprint (blocks per level) |
| M5 | cover culls whatever its order (`cz != 0`) | caught | engine exact mode and oracle |
| M6 | cover culls its own order (`>=` for `>`) | caught | engine exact mode and oracle |
| M7 | layers read only direct damage (no cascade from a layer's output) | caught | engine oracle |
| M8 | scroll strip on the wrong side for `dy` | caught | engine oracle |
| M9 | `ScrollBlit::kept` shifted by `-d` | caught | `a_scroll_blit_keeps…`, engine oracle |
| M10 | `mark_tile` sets the super-tile bit of `tx` instead of `sx` | caught | tile-mask test, engine exact and oracle, alloc |
| M11 | `TileMask::clear` stops one tile short of a super-tile | caught | tile-mask test, move test, engine exact and oracle |
| M12 | cover whole-tile rule rounds its left edge down | caught | engine exact mode and oracle |
| M13 | `damage_move` damages only the new window when old and new touch | missed | move test, engine oracle |
| M14 | damage under a cover is kept from the glass between them | caught | engine oracle (after adding opaque caps over glass edges to its scenes: the first run missed it) |
| M15 | a layer's output is the change cut to its rect (no pyramid dilation) | caught | engine oracle |
| M16 | `seam` damages where the record is, not where the blit carried it | caught | engine oracle |
| M17 | `rebase_orders` keeps the orders, so covers still cull | caught | engine exact mode and oracle |
| M18 | a scroll's virtual damage to the layers above is dropped | caught | engine oracle |
| R1 | raster up-sample weights swapped (`3,1` on even texels) | missed | flat `tests/blur.rs` (bit-exact pyramid) |
| R2 | raster down-sample rounds down (`pack(acc, 0, 6)`) | missed | flat `tests/blur.rs` |
| R3 | raster level domain's far edge counted from the backdrop's origin instead of the window's | missed | flat `tests/blur.rs` (backdrop cut to the read rect) |

The script is in the lane's scratch space and is not committed. Each row is a single text substitution, which
can be reapplied from this table.

### Known blind spots

- The engine oracle's driver tells the engine what changed the way `FlatWorld` does: scroll first in the frame,
  seams for every fixed item over the clip, removals at the order below. A driver that reports differently
  is not covered.
- No blur layer moves with scrolled content in the oracle's scenes. `damage_virtual(z_lo - 1, …)` is
  claimed to cover that case and is exercised only for layers above the group.
- Over-marking is not a failure, apart from exact mode and the overall marked-tile bound (under 80% of
  tiles). A mutant that only adds tiles where layers or scrolls are involved passes.

## Native gate cells

The new cells are `flatbench/cards300` and `damage/{window,stack,4k}` in `bench/baseline/run.py`, parsed by
`operations.py` (`flatbench`, and the `damage` table in `sheet`'s format). `damage` is
`crates/bough-damage/examples/damage.rs`. It counts calling-thread instructions per frame of the engine
alone, bracket cost subtracted, as the median of 64 frames after 16 warm-up frames. The scenes are
`window` (1280 x 800, 26 covers, a glass pane and a glass header), `stack` (66 covers, three stacked panes
at levels 5, 4 and 3, plus the header) and `4k` (3840 x 2160, 1026 covers, four layers).

The same change fixes the fxbench parser for fxbench's current output (the `mixed` executor, `turn_med`, the
composite diagnostics and the media `a/v playback` phase). Every executor is gated. Spreads were measured
across three replays of one binary: `metal` and `mixed` up to 5% on rows near 100k instructions, `cpu1` up
to 8% on rows under 10k, `cpu1-nocache` 0.1%. flatbench gates `Cpu1` and `Metal`. `Cpu8`'s frame-thread
share moved 1.7% between replays (the pbench pooled case), so it is required and pixel-checked but not
gated.

### Reference receipt

Measured at `fee87b8e09` (clean tree, three repetitions, unlocked, `BOUGH_FONT` = `fonts/dejavu/DejaVuSans.ttf`,
SHA-256 matching the reference) with

```sh
BOUGH_FONT=$PWD/fonts/dejavu/DejaVuSans.ttf bench/baseline/run.sh --name fx-cover-fee87b8e09 --reps 3 \
    --expected-inputs-from bench/baselines/native/reference.json --cells <the eight fxbench cells>,flatbench/cards300,damage/window,damage/stack,damage/4k
```

This added four cells and 75 fxbench operations (`mixed/*` and the media `a/v playback` rows) to
`bench/baselines/native/reference.json`. Every value measured earlier was kept, and the merge refused
any change to an existing cell's parameters or counter columns. The receipt's identity (commit, source digest, date, binaries,
load) is recorded under the reference's new `supplements` field.

Median calling-thread instructions per frame:

| Operation | `damage/window` | `damage/stack` | `damage/4k` |
|---|---:|---:|---:|
| idle | 433 | 755 | 795 |
| hover (two rows) | 3,195 | 3,587 | 8,350 |
| under_glass (cascade) | 3,344 | 10,712 | 6,708 |
| under_glass_gated | 3,343 | 10,631 | 6,527 |
| move (across a pane's edge) | 6,761 | 8,225 | 8,520 |
| scroll (blit, strip, seams) | 4,677 | 8,391 | 43,264 |
| theme (mask) | 12,421 | 35,713 | 122,613 |
| recommit (every cover and layer) | 18,534 | 24,531 | 272,958 |
| report (`report_into`) | 4,749 | 13,220 | 9,630 |

| `flatbench/cards300` | Cpu1 | Metal |
|---|---:|---:|
| idle | 6,000 | 54,500 |
| restyle one card | 6,396,700 | 83,300 |
| restyle a card beneath the pane | 39,636,800 | 449,200 |
| one card on a spring | 18,722,400 | 330,700 |
| scroll 11 px | 320,631,300 | 4,453,100 |
| pane moves 1 px | 81,902,400 | 282,600 |
| pages idle | 5,700 | 54,100 |
| page cross-fade | 57,340,400 | 210,200 |
| page slide | 41,245,500 | 434,900 |
| page blur-reveal | 405,627,100 | 496,900 |
| one card of a page | 17,500,400 | 8,509,200 |

### fxbench against the kept reference (761d227)

Of the 189 existing fxbench counters, 170 improved, 1 is within the limit and 18 exceed it. These 18 are measured
regressions of fxbench at this tree against the reference and are reported, not hidden: no limit was changed.
`media/4 cpu1/one video frame` is a workload change. At `761d227` that frame repainted no tile (the reference
records 0 tiles/frame); it now repaints 28.

| Operation | Reference | `fee87b8e09` | Ratio |
|---|---:|---:|---:|
| `fxbench/effects/300` cpu1/idle | 5,400 | 9,200 | 1.704 |
| `fxbench/effects/3000` cpu1/idle | 5,400 | 9,200 | 1.704 |
| `fxbench/effects/600-dense` cpu1/idle | 5,500 | 9,600 | 1.745 |
| `fxbench/media/24` cpu1/all videos (every 4th) | 8,100 | 9,000 | 1.111 |
| `fxbench/media/24` cpu1/idle | 5,400 | 9,100 | 1.685 |
| `fxbench/media/24` metal/scroll 11 px | 543,300 | 845,000 | 1.555 |
| `fxbench/media/4` cpu1-nocache/one video frame | 39,529,300 | 41,935,300 | 1.061 |
| `fxbench/media/4` cpu1-nocache/scroll 11 px | 88,200 | 135,100 | 1.532 |
| `fxbench/media/4` cpu1/all videos (every 4th) | 6,900 | 9,000 | 1.304 |
| `fxbench/media/4` cpu1/idle | 5,400 | 9,400 | 1.741 |
| `fxbench/media/4` cpu1/one video frame | 5,300 | 8,282,700 | 1562.774 |
| `fxbench/media/4` cpu1/scroll 11 px | 88,100 | 135,000 | 1.532 |
| `fxbench/media/4` metal/one video frame | 56,000 | 76,200 | 1.361 |
| `fxbench/media/4` metal/scroll 11 px | 166,800 | 589,400 | 3.534 |
| `fxbench/media/48` cpu1/idle | 5,400 | 9,400 | 1.741 |
| `fxbench/media/48` metal/scroll 11 px | 1,075,400 | 1,189,300 | 1.106 |
| `fxbench/pane/300` cpu1/idle | 5,500 | 9,500 | 1.727 |
| `fxbench/pane/3000` cpu1/idle | 5,400 | 9,100 | 1.685 |


### Focused gate at `16a4c1cb39`

`BOUGH_FONT=$PWD/fonts/dejavu/DejaVuSans.ttf bench/baseline/gate.sh --cells <the eight fxbench cells>,flatbench/cards300,damage/window,damage/stack,damage/4k`
built all three binaries again, replayed three repetitions with no errors and compared 316 primary counters.
The result was 170 improved, 128 within the limit and the same 18 kept fxbench counters over it as above (the ratios agree with the
receipt within 0.07), so the gate exits 1. All 52 counters of the four new cells pass on their own
(`compare.sh … --require-same-cells --cells flatbench/cards300,damage/window,damage/stack,damage/4k`: PASS),
and so do the 75 new fxbench operations. The largest move on any new counter between the receipt and the gate run is +1.9%
(`fxbench/media/4 mixed/glass over playing video`). The `mixed` rows sit close to the 2% limit.

## Performance

The counters listed above as over the reference were not introduced by `lane/rethink/fx`: every lane
commit (and the merge's first parent `8c48450874`) measures the same as the integration head. Bisected with
each commit's own `fxbench` (instructions per frame, `REPS=24`) from `761d227fd4` (which reproduces the
reference) they come from the effects-motion lane's merges into wave3:

| class | culprit | cause |
|---|---|---|
| cpu1/idle (+3.7 K), all videos and a/v playback | `5e545db945` (fxbench `turn_med`) | two PMU reads inside the measured span, 1,614 instructions each (a system call, the same every time) |
| cpu1/idle (rest) | not one commit | `Scene::shade` with no tiles still made a worker's scratch, a tile set and a lock: 3.1 K |
| scroll, cpu1 and Metal | `1293a9fbd1` (rebuild when a quarter of the records moved) | the media scroll moves 13 of 14 records over almost no tiles; the rule rebuilt all 1,000 tiles' lists, and Metal then re-mirrored every tile |
| Metal scroll (3.5x at media/4) | `03b3b08b53` (resident plan) | mirroring a rebuilt bin set asked each buffer for `contents` and `length` twice per tile (Metal messages): 373 K of a 1,000-tile frame |
| one video frame | before `a66102aa8d` (a correctness fix) | the reference damaged 0 tiles for a published frame; the head damages 28 |

Fixes (`c0ba171246`, `8aae02002d`, `711b7f2640`, `29df8a394f`): fxbench takes the measured read cost off the span; `shade` returns
at once for no tiles; `Bins::rebuild_cheaper` weighs the edits (tiles each moved record leaves and enters)
against a build (tiles plus entries), measured at about one instruction-unit each over the benches' large
moves; `Bins::build` makes the cuts while it fills the lists; `Gpu::res_lists` takes its three buffers once.

| counter (instructions/frame) | reference | head before | after |
|---|---:|---:|---:|
| media/4 cpu1/idle | 5,400 | 9,400 | 3,000 |
| effects/300 cpu1/idle | 5,400 | 9,500 | 3,100 |
| media/4 cpu1/scroll 11 px | 88,100 | 135,500 | 16,900 |
| media/4 metal/scroll 11 px | 166,800 | 589,000 | 93,100 |
| media/24 metal/scroll 11 px | 543,300 | 846,200 | 496,800 |
| media/48 metal/scroll 11 px | 1,075,400 | 1,189,300 | 791,500 |
| media/4 cpu1/all videos (every 4th) | 6,900 | 9,400 | 3,000 |
| pbench canvas4 canvas.pan/A | 3,426,600 | 5,200,700 | 2,940,600 |
| pbench canvas4 canvas.full/A | 4,285,500 | 4,118,800 | 3,759,600 |
| pbench canvas1 canvas.full/A | 2,546,400 | 2,553,700 | 2,368,200 |
| pbench canvas1 canvas.drag/A1 | 7,200 | 7,900 | 5,000 |
| pbench canvas1 canvas.drag/A | 3,800 | 4,400 | 4,500 |
| pbench canvas1 canvas.hover1/A | 50,600 | 51,600 | 51,700 |

The one video frame stays above the reference for a reason: the reference drew nothing when a video
published a frame (0 tiles: the picture on screen was stale). The head damages the video's window and no
more: video 3 is the record at (648, 12)-(848, 124.5) px, which is tiles x 20..26 by y 0..3, 28 tiles, and
the frame reports 28 tiles, 0 records visited, 0 prefix and 0 blocks. What the 28 tiles cost:

| counter | reference (0 tiles) | after (28 tiles) |
|---|---:|---:|
| media/4 cpu1/one video frame | 5,300 | 8,279,600 |
| media/4 cpu1-nocache/one video frame | 39,529,300 (27 tiles) | 41,932,700 (55 tiles) |
| media/4 metal/one video frame | 56,000 | 72,800 to 75,000 |

(media/24 and media/48's reference already drew 32 tiles for this frame; they are under it.)

Not recovered: pbench canvas1 `canvas.drag/A` (window pass + bins only) and `canvas.hover1/A`. Neither frame
builds bins; the drag frame's update is about 2.3 K of window pass over five key-indexed columns, and it
grew by about 600 instructions across the wave3 merges of the effects-motion lane (3.8 K at `761d227fd4`,
4.0 K at `a3ddfa09a7`, 4.1 K at `03b3b08b53`, 4.5 K at `9c987df26c`), not in one place. The cost-weighed
bins decision adds up to 100 to each (4.4 K and 51.6 K with the old quarter rule, at the TSV's 0.1 K
rounding). The `mixed` rows that cross the limit in one gate run and not the next (effects/600-dense
hover above glass 72.4 K to 75.9 K between repetitions of one binary) are the Metal command path's jitter.
