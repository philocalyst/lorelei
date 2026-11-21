# Card: the typed core made O(changes) where it is O(exposed) or O(dataset) today

```
lane:            core (branch lane/core/incremental-operators)
state:           CARD
owner:           the `zgui` session (Claude, claude-fable-5-2), one session
predecessor:     71ee3be0aa (nightly-2026-10-17, profile `dist`, no features)
question:        Can the new core (bough-x) cost what changed, not what is exposed or what the dataset holds, for
                 selection fan-out, pointer readers, relation edits and derived collections, without a write
                 barrier, without anything unexposed existing, and with idle and still frames unchanged?
change:          Five mechanisms, all below the author API, all new modules plus small additive edits:
                 (1) `rel::Lane<T>` / `rel::Seq<T>`: an order lane (gapped blocks under a Fenwick tree of block
                     lengths) with the journal of `Rel`. Replaces nothing: `Rel` (a `Vec`) stays; a list reads either.
                 (2) `rel::Table<T>` with a bounded change log, and `ops::{Filter, Sort, Group, Total, Join}`:
                     operators that read a log and maintain an output (ids in an order lane) and emit a positional
                     journal a list reads. More edits pending than a log keeps = recompute (the fallback).
                 (3) `index::Trigrams` and `ops::Substring`: a trigram posting index (a superset index: a query is
                     verified against the text, stale postings are bounded by rebuilds) plus narrowing from the
                     previous result. Replaces the scan per keystroke in the search fixture.
                 (4) Keyed selection: a typed expression `sel::<L, _, _, _>(key, on, off)` (`word == key ? on :
                     off`) declares its key word; a column of such records gets a sorted (key, record) index built
                     lazily by the world and dropped by any description pass; a change of only that word marks the
                     records keyed by the old and the new value. Fallback: the page scan, as today.
                 (5) Pointer influence: a typed expression `near::<R, _, _, _>(centre, inside, outside)` declares
                     that only records whose centre is within R of the pointer read it; the pointer delta tests the
                     centre words (and cached per-page boxes) before evaluating. Fallback: the page scan.
                 (6) Sweeps as rects: `Tiles` can hand the boxes of records in motion to a raster as rects
                     (`Tiles::swept`) instead of marking tile bits each frame; off by default.
implication:     Passing establishes the cost model of each mechanism on the fixtures below, with exact oracles.
                 It does not establish anything about the façade (`bough::prelude`): the lowering of `each`,
                 `sig!(..).eq(..)` and search onto these is the integration lane's. It does not establish wall
                 time, cold-cache behaviour or anything on Linux.
platforms:       macOS (Apple M-series, 32 cores), headless, CPU only. Counters: `thread_selfcounts` (bough-mem pmu).
workloads:       anim fixture (H=10288: 2,048 pulse + 2,048 pointer readers); sheet fixture (10,000 rows);
                 search fixture (100,000 rows) and its operator-backed twin; new fixtures: `select` (N exposed
                 rows x 8 cells keyed by row, N x 8 = 2,048 and 16,384 readers), `ops_x` (relations of 10^4, 10^5,
                 10^6 rows; a repetitive corpus (384 distinct labels) and a unique corpus).
gates:           G1 operator output == recompute from scratch after every step of random edit sequences (insert,
                    remove, update, predicate change), and the emitted positional journal replayed onto a mirror
                    equals the output (so the deltas are right, not only the state); 
                 G2 Lane == Vec model under random insert/remove/get, at block boundaries;
                 G3 Substring hits == naive scan after every step of: append, backspace, middle edit, replacement,
                    dataset edits, queries under three bytes, an empty query;
                 G4 keyed delta and near delta: damage bitmap == the page scan's bitmap (same world, fast path
                    off) at every step, and retained == full redraw == fresh mount in the soft raster;
                 G5 sweeps as rects: bits | rects-as-tiles == bits with the mode off, every frame;
                 G6 growth: live lengths after 10 and after 1,000 rounds of churn are within a stated band;
                 G7 counts: a selection change evaluates at most 4 records; a still frame visits 0.
                 Negative controls (features, each must fail its gate): `mutant_keyed` (index forgets the old
                 key), `mutant_near` (reach test uses R/2), `mutant_lane` (a split drops the Fenwick rebuild),
                 `mutant_ops` (Filter ignores Update), `mutant_tri` (a query skips verification),
                 `mutant_swept` (a swept rect is dropped).
thresholds:      (before: this host, this commit, bench/results/incr/before-71ee3be.txt; instructions, p50)
                 T1 selection change at 2,048 keyed readers: opaque `map` form measured as the before; after
                    <= 600 and within 10 % of the 16,384-reader value (flat). Kill: > 1/3 of before, or not flat.
                 T2 pointer_words, 2,048 readers: before 65,339; after <= 21,700 (3x). Kill: under 2x.
                 T3 Seq insert in the middle of 10^6 rows of 40 bytes: <= 3,000; Rel is the memmove. Kill: Seq
                    sequential `get` over a window dearer than 2x `Rel::get`.
                 T4 search, 100,000 rows: a selective keystroke (third and later characters, narrow by a key /
                    widen by a backspace) before 6,465,205 for the pair; after <= 1/5. A first character (low
                    selectivity) not dearer than 1.1x the scan (before 27,645,951 includes the first keys).
                    Index memory stated against the text bytes for both corpora. Kill (X21): low-selectivity
                    slower than the scan beyond 1.1x; index bytes above the dataset's bytes is reported as a loss
                    per corpus, not hidden.
                 T5 frame with endless motion, sweeps as rects: before 2,059 (H=10288); after <= 100.
                 T6 every other cell of anim_x, sheet_x, text_x within 1.02x of before (no warm cell dearer by 2 %).
measurement:     instructions of the calling thread, bracket subtracted, median of 32 to 64 steps, under
                 `lockf -k ~/work/locks/bench.lock`, load average recorded with each run; before and after from the
                 same binary profile. Raw output under bench/results/incr/.
budget:          one session; no remote hosts.
stop conditions: a mechanism whose gate cannot be made exact is removed and recorded REJECTED; a threshold missed
                 is reported as missed with its number; active-set maintenance for bounded transitions is not in
                 this card (ENDLESS columns already short-circuit; the remaining case needs page summaries with
                 hooks on the record write path, which the integration lane is editing).
do not revisit:  per-row hover signals; a separate hit index; bounded rect lists for damage; hooks, a `cx`, or
                 subscriptions in application code (docs/SETTLED.md). The app-level filter cache stays deleted:
                 what is built here is the framework's operator.
```

Notes fixed before building:

* The key index and the page boxes are caches with exact invalidation, not maintained structures: the world
  drops them whenever a description pass runs (mount, a list pass, a re-description) or the tree changes shape,
  and builds them on the second consecutive use. So the record write path gains no instruction; the adversarial
  case (a pass between every selection change) takes the scan every time and is a gate cell.
* A list still re-describes the exposed rows from an insert on (they did move on screen). The order lane makes
  the relation's own cost one block; a slot remap that turns the exposed part into a rigid move is not in this card.
* Predictions are bids; a miss is reported, not refitted.
