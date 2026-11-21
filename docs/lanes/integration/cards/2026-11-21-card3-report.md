# Integration Card 3: report (2026-11-21, first slice)

```
lane:     integration
card:     2026-11-18-card3-facade-vertical-slice.md, amendment 2026-11-21-card3-amendment-1.md
state:    GATED for the vertical slice (correctness gates and negative controls pass);
          MEASURED with misses (two of three latency thresholds missed, reported below);
          not INDEPENDENT_REPLAY, not the whole of Card 3 (see "What this is not")
branch:   lane/integration/card3-facade, from 71ee3be0aa
host:     Apple silicon, 32 cores, 96 GB, macOS, headless; other agents building (load average 13 to 27)
```

Everything below belongs to **body A's author surface running on body B**: the same `bough::prelude` view source,
lowered onto `World<V, B>`. Numbers are for the binaries and host named with them.

## What was built

* **`bough-x`**: free rows (`Sink::free`, `sink::free_sync`): a store whose rows come and go, each with its own
  rect, in ordinary columns with ordinary tile damage, compare-and-patch and paint order; the generic record
  `prim::Prim` (ten words: box with radius, border and alpha; shaped line; image); `World::resize` (in place, no
  remount, counted) and `World::free_set` (one record patched with no description); two mutants.
* **`bough-lay`**: `heap::Tree`, the one flex function over a heap tree with no 96-element or 32-child limit
  (the fixed shape's limits refuse into it), a size memo per element so depth does not multiply questions; the
  freeze loop spills past 32 items instead of asserting.
* **`bough`**: `Element::lower` (the second protocol of every element value; the default refuses by name);
  lowering of `div`/`row`/`column`/`block`, `text`/`para`/`label`, `button`, `image`, `each`, `Vec`, tuples,
  `.bg`/`.rounded`/`.border`/`.opacity`/`.translate`/`.w_op`/`.h_op`, every `Styled` method, `Flex` recipes,
  `on_click`/`on_press`/`on_hover`/`on_key`/`on_drag`/`focusable`/`hover_bg`, `.scroll(sig!(..))`, and
  `list(sig!(..))` with `.viewport()`/`.scroll(..)`/`.estimate(..)`/`.overscan(..)` (rows by exposure, owners
  reached by slot and generation, heights by slot, 64-bit positions); `lower::App` (describe, world passes, a ring
  of surfaces, lexical hit list with bubbling, focus and Tab, wheel, resize, the four oracles, count bands);
  `lower::text` (rustybuzz with face fallback, a coverage rasteriser, colour bitmap strikes, run store with
  generations); `lower::canvas` (integer compositor: antialiased rounded boxes, borders, alpha, glyphs, images);
  `lower::provenance`.

## Gates (card, as amended)

| gate | result | evidence |
|---|---|---|
| retained == full redraw == immediate == fresh mount, ring ages 1..=4, on the slice | **pass** | `lowered_slice::a_random_script_holds_the_four_oracles_at_ring_ages_1_to_4` (4 rings x 80 random turns: clicks on five buttons, wheel, pointer moves, focus and keys, a signal write, resizes; scale 1 and 2), `lowered_counter` (twin of `tests/counter.rs`), `records_that_enter_and_leave_in_place_are_damaged`, deep view (`examples/deep_view.rs` runs `check()`) |
| the same on "migrated tests" | **partial** | one twin (`counter`). The other eighteen legacy test files have no twin yet; fifteen of them assert `Rt` internals (amendment A2) |
| provenance: 0 passes through legacy `Rt` | **pass** | `provenance::counts().legacy() == 0` asserted in `lowered_counter` and `lowered_slice`; the legacy `App::new`, `turn`, `frame_as`, `settle_input` count themselves |
| non-Latin text, emoji, translucent paint through a fallback in the same runtime | **pass** | `non_latin_text_and_emoji_are_drawn_through_the_fallback` (CJK, Greek, Cyrillic, Arabic rows all draw ink; the emoji row has saturated colour pixels; `fallback_segments > 0`), `a_translucent_band_is_composited_over_what_is_under_it` (red channel 177 +- 6 under a 96/255 band). Limits: no bidi reordering across segments, greedy breaks, whole-pixel pens (stated in `lower/text.rs`) |
| resize keeps the world, focus, a held press, the scroll anchor | **pass** | `resize_keeps_the_world_focus_a_held_press_and_the_scroll_anchor`: `lifecycle() == (1 mount, 4 resizes)` |
| negative controls | **pass** | `sh bench/card3/mutants.sh`: `mutant_free_enter`, `mutant_free_leave`, `mutant_lower_alpha`, `mutant_lower_hover` each fail a gate. The card's `mutant_delta`, `mutant_moves`, `sabotage_copy` do not apply to records that read no block word and are not copied by scroll; they are replaced by these four. `mutant_lower_alpha` is killed by the numeric translucency test only: the four oracles share one compositor and cannot see it |
| second world isolation | **pass** | `a_second_world_is_isolated` |

## Thresholds

| threshold | result | number |
|---|---|---|
| 100,000-item `list`: <= 400 records | **pass** | asserted at offsets 0, 40,000 rows and 99,990 rows (`a_hundred_thousand_rows_cost_what_is_exposed`) |
| deep view clean release build of `bough` + slice + deep view <= 35.0 s | **pass under load** | 11.91 s at `-j 8`, load 16.8 (`bench/results/card3/compile-20261121.txt`). Scope: `cargo clean --release -p bough` then build; dependencies stay built |
| deep view incremental leaf edit <= 8.0 s | **pass under load** | 3.61 s |
| virtual 100k mount <= 0.50x legacy | **pass** (mount) / miss (mount + first frame) | 9.61 M against 26.30 M instructions = 0.37x; with the first frame 20.5 M against 36.6 M = 0.56x |
| hover <= 1.00x legacy | **miss** | 17,869 against 14,597 = 1.22x as bracketed. The bracket itself is about 13,000 (the still-frame row), so net of it the lowered hover is about 4,900 against about 450: a pick that scans the hit list, and two records patched |
| single-signal update <= 1.00x legacy | **miss (the risk the amendment named)** | 255,793 against 21,876 = 11.7x. A lowered turn runs the view and lowers it (170 K), lays out the documents whose layout words changed (65 K), compares 96 records (9 K). The legacy runtime patches one wire |

## Instructions per operation (process instructions, medians of 60; `bench/card3/cost.sh`)

Binary `target/release/examples/card3_cost` at `c7bc4ccca3` plus the per-document layout memo; legacy repair
`Kernel`; load average 16.7. "turn" is the input call alone.

| cell | legacy turn | lowered turn | ratio | legacy turn+frame | lowered turn+frame | ratio |
|---|---:|---:|---:|---:|---:|---:|
| mount, 1,000 rows | 26,032,196 | 7,433,388 | 0.29x | 36,391,097 | 18,363,153 | 0.50x |
| mount, 100,000 rows (virtual) | 26,295,112 | 9,607,771 | 0.37x | 36,620,048 | 20,537,771 | 0.56x |
| hover, one row to the next | 14,597 | 17,869 | 1.22x | 29,022 | 906,854 | 31.25x |
| one signal (the title) | 21,876 | 255,793 | 11.69x | 90,447 | 367,358 | 4.06x |
| click Add first (insert at 0) | 131,655 | 698,992 | 5.31x | 5,108,251 | 7,596,445 | 1.49x |
| scroll by one row | 104,433 | 700,412 | 6.71x | 7,316,464 | 7,485,207 | 1.02x |
| theme (opaque state) | 21,094 | 221,184 | 10.49x | 8,024,129 | 11,330,830 | 1.41x |
| resize by 40 pt | 4,341,181 | 1,824,905 | 0.42x | 14,748,543 | 13,032,228 | 0.88x |
| still frame | 14,151 | 12,922 | 0.91x | 28,545 | 25,883 | 0.91x |

Read with these in hand:

* The "+frame" columns compare two rasters, not two runtimes: the lowered frame is an integer compositor on the
  calling thread at 1280 x 960; the legacy `Kernel` repair chooses its executor per frame. The hover row (906 K
  against 29 K) is two rows of 1280 x 56 pixels cleared and refilled on the CPU.
* Against the hand-written core (REPORT section 2: sheet hover 6,826, theme 31,010, mount about 230 K): a different
  scene, so no ratio is given. The order of magnitude is what matters: the lowered path is at 18 K to 700 K per
  turn where typed columns are at 1 K to 31 K. The generic record gives up per-word routing; that is the price
  of decision D1 and the reason the typed fast path is the next card.
* Two earlier runs are kept (`bench/results/card3/cost-*.tsv`): before fonts were mapped once (mount 169 M: a
  190 MB emoji face was read and copied per application) and before the layout memo (title edit 512 K).

## zgui: taken, rejected, and the measurement behind each

| idea | decision | measurement |
|---|---|---|
| Count bands (a count is a property of the design; ceiling with no tolerance) | **taken** | `count_bands`, `hover_is_one_record_patched_and_no_description`, `an_idle_frame_repaints_nothing`, the 400-record ceiling, `lifecycle()`, `layouts()` (documents laid out and kept, asserted exactly per kind of turn) |
| Dirty-path relayout with kept lines | **taken in a coarser form** | layout is kept per document when its layout words are equal, compared word for word (title edit: 316 K to 65 K of layout). Not yet per subtree |
| "Where the frame's cost is" as standing counters | **taken** | `Stats::stage` (view+lower / layout+emit / world compare), printed by `card3_cost` |
| Resize without remount | **taken** | `World::resize`; resize is 0.42x the legacy turn |
| Per-signal fan-out to exactly the affected nodes (subscriptions) | **rejected as a graph, owed as typed columns** | the 11.7x single-signal miss is what this costs today; SETTLED and ROUND8 exclude observers, so the answer is `sig!` operands as Frame-stage columns |
| Keyed structural diffing of rows | **not built** | free rows are positional: an insert at 0 re-stores the exposed tail (698 K, 5.3x legacy). Item identity for handlers and heights is by slot and generation already |
| R-tree per coordinate space for hit testing | **not built** | the hit list is scanned (about 130 entries on the slice). The research note's A4 (a geometry summary per page) is the form that fits |

## Boundaries (probes in `tests/lowered_boundary.rs`; nothing is drawn as something else)

* **Refused by name at description**: `text_input` / `text_field`, `tooltip`, `Entities` / `entity_view`,
  `boundary` / `Reader` / `Request`, `list_with` and `index_list` (legacy row mount functions), `paint_with`,
  `.scrolls(..)`, every `bough::raw` element. Probes exist for `text_input` and `tooltip`; the others take the
  same default path (`Element::lower`, `ActOp::lower_act`, `RowSource::lower_rows`) but have no probe.
* **Lowered with a stated downgrade**: `.transition(curve)` operands show their target at once (probe:
  `boundary_a_transition_jumps_to_its_target`); `Align::Baseline` aligns as `Start`; an `abs` child is placed
  against its parent's box, not the nearest `relative` ancestor; the wheel moves the offset at once (no glide);
  no focus ring is drawn; `View::changed` is called with an empty `Changed` (list delta logs are not produced
  when a list has no wires).
* **`Application::run`** still opens the legacy runtime: there is no windowed host over the lowered runtime (an
  AppKit host over `bough-x` is roadmap area 15). The lowered runtime is headless (`bough::lower::App`).
* **Reflection and accessibility** do not see free rows (`reflect::Learn` keeps its place and skips them).
* The prepared Latin-1 face (`bough-face`) is not wired as the admitted fast path; every run takes the shaper.

## What this is not

Not all of Card 3: the prelude exports above that are refused are not lowered; eighteen legacy test files have
no twin; the runner scenarios and the five heavy scenes are not ported; nothing was measured alone on a quiet
host or by a second party. `bough::raw::App` remains the legacy runtime and every legacy test still runs on it.

## Whole-workspace suite

`cargo test --release --workspace --no-fail-fast -j 8` on this branch: **460 passed, 6 failed, 3 ignored**
(`bench/results/card3/workspace-release-tests-20261121.txt`). The six failures are the six of the baseline run at
`71ee3be0aa` on this host (437 passed, 6 failed): `bough-app-sketch-code` `code.rs` x3 (a missing sample file),
`bough-text` `parley_twin::random_mixed`, `bough-x` `text::the_glyph_memo_is_collected_and_frames_stay_exact`,
`bough-zgui` `drawing::faces_and_display_text_draw_their_own_font`. None is in code this branch changed in
behaviour; none was fixed here. The 23 additional passes are this branch's tests.

## Not verified

* The compile thresholds alone at `-j 3` (run at `-j 8` under load 17).
* Legacy tests in debug: `tests/far.rs` trips a `debug_assert!` in `bough-core/src/nrect.rs:209`, a file this
  branch does not touch; the baseline log is a release run. Not replayed at the baseline in debug.
* Miri, Loom, sanitizers: none run. The three new `unsafe` sites (free-row site access, the two offset
  projections for scroll setters and list rows, the font mapping) carry SAFETY arguments and tests
  (`bough-x/tests/free.rs`, the scroll and list paths of `lowered_slice`, `lower::text::tests`).
* Wall-clock latency: only instruction counts were taken.

## Next card (recommendation)

**Card 4: the typed fast path under the same lowering.** A `sig!` operand lowers to a record that reads its block
word (the element layer's `At<V, X, OFF>` lens) instead of a constant, so a signal write is the world's delta
over the columns that read it and no description. Gate: the same four oracles plus "a turn that wrote only
bound signals describes nothing" as a count band; threshold: single-signal update <= 1.00x legacy on this slice.
Then, in order: `text_input` (editing state, caret, selection) and transitions as `Time` expressions; a pick
that does not scan (page geometry summaries); keyed free rows; twins for `describe`, `boxops`, `scroll_box`,
`list`, `select_all`; the sheet scene through `bough::prelude` on this path.
