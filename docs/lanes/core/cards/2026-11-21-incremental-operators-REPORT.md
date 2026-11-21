# Report: the incremental-operators card (2026-11-21)

State reached: **MEASURED** for every cell below on one host (not INDEPENDENT_REPLAY). Card:
`2026-11-21-incremental-operators.md`. Use: `../OPERATORS.md`. Raw output: `bench/results/incr/`
(`before-71ee3be.txt`, `after.txt`, `gates.txt`).

Host: Apple M-series, 32 cores, macOS, other agents building and measuring (load average 11 to 21 during the
runs, recorded in the files). All numbers are retired instructions of the calling thread (`thread_selfcounts`),
bracket subtracted, median of 32 to 64 steps unless "one step" is said, profile `dist`, taken under
`lockf -k ~/work/locks/bench.lock`. No wall-clock number is claimed. Body: B (`bough-x`), on fixtures written
against the core; nothing here went through `bough::prelude` or the gui-bench runner.

## What was built

| mechanism | files | gate |
|---|---|---|
| order lane `Lane`, relation `Seq` | `src/rel/lane.rs`, `src/rel/mod.rs` | lane unit tests, `tests/ops.rs` |
| `Table` + `Log`; `Filter`, `Sort`, `Group`, `Total`, `Join` | `src/rel/mod.rs`, `src/ops.rs` | `tests/ops.rs` |
| `Trigrams`, `Substring`; the search fixture's twin | `src/index.rs`, `scenes/search_ops.rs` | `tests/search_ops.rs` |
| `sel` (declared key), `near` (declared reach) | `bough-expr/src/lib.rs` | `tests/select.rs` |
| key index, page boxes, their delta kernels | `src/store/fast.rs`, `World::delta_fast` | `tests/select.rs` |
| sweeps as rects | `Tiles::{sweep_rects, swept, tick_rects, settle_swept}` | `tests/select.rs` |

Edits to shared files, all additive: `store.rs` (+`pub mod fast`, `Arch::KEYED`/`NEAR`, `Desc.fast`,
`Tiles.swept`/`sweep_rects`/`tick_rects`/`settle_swept`, `any` and `clear` read `swept`), `world.rs`
(`FastStats`, `Fast`, `fast_of`/`fast`/`wrote`/`no_fast`, `delta_fast`, three `wrote += 1`, one branch in
`delta_one`, one in `tick`, the fast table in `retable`), `kinds.rs` (`Fill` forwards `KEYED`/`NEAR`: 3 lines),
`lib.rs` (3 modules), `Cargo.toml` (6 mutant features), `scenes/anim.rs` (a `near` variant, default off),
`scenes/search.rs` (`relabel`), `examples/anim_x.rs` (`NEAR`, `SWEPT`). `sink.rs`, `el.rs`, `event.rs` untouched.

## Gates (all in `bench/results/incr/gates.txt`)

`cargo test --release -p bough-x -p bough-expr`: every target passes except
`tests/text.rs::the_glyph_memo_is_collected_and_frames_stay_exact`, which fails identically on the predecessor
(`71ee3be`, the orchestrator's baseline log) and is not touched by this card.

| gate | test | result |
|---|---|---|
| G1 operators == recompute at every step; journals replayed onto mirrors | `ops.rs::operators_equal_a_recompute_after_every_step` (12 seeds x 1,500 steps, small and large logs, batches, predicate changes, a filter feeding a sort and a total) | pass |
| G2 lane == `Vec` | `rel::lane::tests` (random edits at ends, middle and one place; `lower_bound`; `from`) and `ops.rs::a_seq_and_a_rel_tell_a_list_the_same` | pass |
| G3 hits == naive scan | `search_ops.rs`: two corpora x 4 seeds x 2,500 steps (append, backspace, middle edit, replacement, empty and short needles, dataset edits; every route asserted taken); the operator fixture paints what the scanning fixture paints over 500 steps, retained == full redraw == fresh mount | pass |
| G4 bounded deltas leave the scan's damage, bit for bit; retained == full redraw == immediate == fresh mount == the opaque form | `select.rs::bounded_deltas_leave_the_scans_damage_and_the_same_pixels` (3,000 steps), `the_anim_fixture_with_declared_reach_paints_the_same` | pass |
| G5 swept rects == the other mode's tiles | `select.rs::swept_rects_are_the_tiles_of_the_other_mode` (600 frames) | pass |
| G6 growth | `ops.rs::churn_leaves_nothing_behind` (10 vs 1,000 rounds: slots, sorted ids, groups equal; logs <= cap; lane bytes <= 2x; join index keys bounded), `search_ops.rs::the_index_is_a_superset_and_stays_in_its_band` (postings <= 1.4x exact under 20,000 edits) | pass |
| G7 counts | `select.rs::a_selection_change_evaluates_two_records_whatever_is_exposed` at 328, 2,056 and 8,200 px (exactly 2 records per change, 1 build, 1 scan; with a pass between every change: 0 hits, 0 builds, 32 scans) | pass |

Negative controls, each failing its gate as required: `mutant_lane` (2 lane tests), `mutant_ops` (G1),
`mutant_tri` (4 tests of G3), `mutant_keyed` (G4 and G7), `mutant_near` (G4, both tests), `mutant_swept` (G5).

`cargo check --workspace --all-targets`: no errors (warnings are the predecessor's plus none new in the lib).

## Thresholds, every cell

| # | cell | before | after | ratio | card said | verdict |
|---|---|---:|---:|---:|---|---|
| T1 | selection change, 2,048 keyed readers (`ops_x select`, opaque form is the before) | 33,768 | 1,294 | 26x | <= 600, flat | **missed the 600** (a turn that changes one word one record reads costs about 1,000 on this build: sheet `select` is 1,005); flat holds |
| T1 | the same at 16,384 readers | 264,040 | 1,348 | 196x | within 10 % of the 2,048 value | met (+4 %) |
| T2 | `anim_x pointer_words`, 2,048 pointer readers (`NEAR=1`) | 65,339 | 21,585 | 3.03x | <= 21,700; kill under 2x | met, by 115 instructions |
| T2' | pointer event on the grid fixture (8 px rows: about 180 records in reach) at 2,048 / 16,384 readers | 63,628 / 465,932 | 40,105 / 52,201 | 1.6x / 8.9x | not in the card | reported: the cost is the records in reach (each its own mark), not the readers |
| T2' | pointer event far from every reader, 2,048 / 16,384 | 57,703 / 460,007 | 2,030 / 14,126 | 28x / 33x | not in the card | reported; grows with pages (one box test each), not records |
| T3 | `Seq` insert + remove in the middle of 10^6 rows of 40 bytes | 7,499,912 (`Rel`) | 1,423 | 5,270x | <= 3,000 | met |
| T3 | read a window of 50 in order | 410 (`Rel`) | 1,112 | 0.37x | kill: dearer than 2x `Rel::get` | **kill line crossed (2.7x)**: 22 per `get` against 8. First version was 4.2x; the cursor fast path brought it to 2.7x. Recorded as a loss; `Lane::from(lo)` iterates a window without `get` |
| T4 | search 100,000 rows: type / backspace at "quick river" (text_x's phase) | 27,645,951 | 4,634 | 5,966x | <= 1/5 | met |
| T4 | narrow by "z" to nothing, widen by a backspace (text_x's phase) | 6,465,205 | 317,951 | 20x | <= 1/5 | met |
| T4 | third and later characters of "amber river", one step each | 23.1 M to 46.0 M | 1.5 M to 1.7 M, then 6 K to 8 K once nothing matches | 14x to 6,000x | <= 1/5 | met |
| T4 | first character "a" / "i" (low selectivity), one step | 11,161,607 / 11,671,722 | 12,533,192 / 13,126,481 | 0.89x / 0.89x | not dearer than 1.1x the scan | **missed: 1.12x both** (an earlier run of the same binary read the scan's "a" at 11,712,207, which is 1.07x: the scan's own count moves by 5 % between runs here) |
| T4 | backspace to the empty needle (100,000 hits) | 1,014,694 | 2,298,963 | 0.44x | not dearer than 1.1x | **missed (2.3x)**: the output is the whole table and is rebuilt as a lane |
| T4 | dataset edit under a selective needle | 31,066,917 | 22,575 | 1,376x | not in the card | reported |
| T4 | index bytes / text bytes | - | 3.60 to 3.85 (repetitive), 3.69 to 6.27 (unique) | - | kill (X21): above the dataset's | **loss on both corpora**: `u32` postings, about one per text byte |
| T5 | frame with endless motion, 10,272 tiles (`SWEPT=1`) | 2,059 | 96 | 21x | <= 100 | met |
| T6 | every other cell of `anim_x`, `sheet_x`, `text_x` within 1.02x | | | | | met: worst are `fade_start_turn` 820 -> 835 (1.018), `part_total_word` 891 -> 906 (1.017), `select` 990 -> 1,005 (1.015): the one branch in `delta_one`. `sheet_x mount` is one sample per run and read 356,768, 385,164 before and 387,239, 346,008, 346,008 after: not attributable |

### Operators, one edit against a recompute (`ops_x ops`)

| operator | n = 10^4 | 10^5 | 10^6 | recompute at 10^6 |
|---|---:|---:|---:|---:|
| `Filter`, one row edited | 566 | 664 | 785 | 25,076,645 |
| `Filter`, a row inserted and one removed | 644 | 738 | 831 | |
| `Sort`, one row edited (it moves) | 1,528 | 1,817 | 2,053 | 465,689,186 |
| `Group`, one row edited (another group) | 572 | 628 | 628 | 425,665,614 |
| `Join` (n/10 x 200), one left key edited | 2,751 | 2,618 | 2,792 | 24,564,191 |

Growth with n is the logarithm (and cache-independent here: these are instructions).

### Substring alone, unique corpus (`ops_x corpus`), one step each

| step | n = 10^5 | n = 10^6 | scan with `String::contains` at 10^6 |
|---|---:|---:|---:|
| needle of 3 bytes (index, 225 / 2,273 verified) | 127,194 | 1,112,009 | 273,264,531 |
| 4th byte (index, 208 / 2,231 verified) | 88,180 | 554,421 | |
| 5th, 6th byte (narrow) | 7,818; 1,806 | 53,311; 5,398 | |
| backspace to a kept result | 2,337 to 140,286 | 4,767 to 1,327,616 | |
| 1st byte (narrow over everything) | 14,147,494 | 140,263,064 | |
| replacement by an unrelated needle (index) | 50,594 | 497,719 | |
| dataset edit under the needle | 6,936 | 8,245 | |
| build the index (once) | 260,459,978 | 2,285,451,920 | |

A selective step costs its candidates (which grow with n at a fixed selectivity), not the dataset: X21's "flat in
dataset size for selective queries" holds only once a needle is selective enough that candidates stop growing.

## Amendments to the card (made while building, with the reason)

1. G7 said "at most 4 records". The fixture keys each cell by its own id, so a change evaluates exactly 2; the
   gate asserts 2.
2. `Tiles::tick` is unchanged; rect mode is `Tiles::tick_rects`, chosen by `World::tick`. The first version
   branched inside `tick` and cost a still-motion frame 12 instructions (169 -> 181); that version was not kept.
3. `Substring` keeps results oldest-first up to 16 needles and the table's size in ids (the card did not say).
   Results over half the table are not kept, which is why a backspace to a one-byte needle is a scan.
4. `Filter::replace` lists no individual changes when the result's size moves by more than half a log: a reader
   of its log recomputes.

## What is not verified

* Nothing was run on Linux, through the runner, or through `bough::prelude`.
* Cold-cache cost, cycles under a quiet machine, and code size / I-cache of the new kernels (the card's
  measurement was instructions only; `fast.rs` adds four monomorphised functions per keyed or near archetype).
* Miri on `Lane::get`'s `get_unchecked` (its invariant, "every edit empties the cursor", is exercised by the
  random-edit tests, not proved).
* `Group`, `Total` and `Join` have no world-level fixture: their journals are checked against mirrors, not pixels.
* The independent replay.

## Open problems and the next card

A note on the scan's own counts: `String::contains` over the same rows read 11.16 M and 11.71 M in two runs of
one binary (allocation of the result vector, most likely). Every ratio above against a scan carries that 5 %.

1. **Index memory** (3.6x to 6.3x the text). Delta-coded posting blocks (one byte per posting on dense lists)
   and a dictionary of distinct values are the two known answers; measure both against the same cells.
2. **Low-selectivity steps** (a first character 1.12x, the empty needle 2.3x): the result is rebuilt as a lane of ids. A result
   that is "all rows" or "all rows but these" as a representation, not a list, removes both.
3. **`Seq` window read** (2.7x `Rel`): give `list_sync` the lane's window iterator instead of `get(r)`.
4. **Records in reach each mark their own tiles** (T2' on the grid: 40,105 for about 180 records). One mark of
   the reach box when most of it changed is the same trick `mark_rects` uses per page, across pages.
5. **Exposed inserts**: with `Seq` the relation's insert is 1,423 but `sheet_x insert_row` is still 88,992
   (the fixture uses `Rel`, and the exposed suffix is re-described). The slot remap belongs with item identity.
6. Forward `KEYED`/`NEAR` from `Num`, `Label`, `Line`, `Outline`; lower `sig!(..).eq(..).select(..)` and
   `each(..filter..)` onto these in the façade (`../OPERATORS.md` has the table).
7. zgui's gates worth porting next, in order: named-gate lists (a gate fails if a test it names is gone), the
   ledger self-test (every mutant feature is run by one script and must fail), a same-run slope gate for the
   "flat" claims above (the 2,048 / 16,384 pair is the first instance).

## Amendment 1 (2026-11-21, after the lane changed hands)

The lane was taken over by the orchestrating session at `6313f25e9a`. `lane/integration/wave3` (the independent
review's recoveries and fixes, the effects benchmark) was merged in with no conflict; after the merge
`cargo check --workspace --all-targets` is clean, `cargo test --release -p bough-x -p bough-expr` is 110 passed,
0 failed, 5 ignored, and all six mutant features still fail their gates. Counts on the merged tree: `sheet_x`
idle 63, hover 5,354, theme 30,789, edit_exposed 2,615, select 797, insert_row 88,721.

**Constant-free columns are differentiated once** (the first piece of "partial evaluation by dependency", built
before this amendment was written; recorded here rather than as a prediction). An archetype of size zero has one
value, so `delta_col` asks `changed(old, new)` of one live record and either returns without opening a page or
marks every live record without asking again (`store.rs::uniform_changed`). Admission: `size_of::<A>() == 0` and
not `Arch::STATE`; everything else takes the page scan as before. Gate: `tests/uniform.rs` (evaluations per
change are 2 at 8, 64 and 512 exposed rows, changed or not; retained == full redraw == fresh mount over 300
steps), and the review's probe `pointer_change_without_damage_has_bounded_record_work` (TR1-09's evaluation
half) is un-ignored and passes. Negative control: `--features mutant_uniform` fails both tests of
`tests/uniform.rs`. After: 113 passed, 0 failed, 4 ignored; `sheet_x` and `anim_x` cells unchanged within noise
(no column of those fixtures is constant-free). Not done: a record whose expression has a block-only
*sub*expression beside record constants still evaluates it per record.
