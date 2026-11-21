# Integration Card 5: report (2026-11-21)

```
card:    2026-11-21-card5-description-as-update.md
state:   GATED for what it changed: a description is an update, a list is its own log, hover is two records,
         a press and a hover are items. Thresholds: 5 of 8 met; 3 missed and stated below with their floors
         (hover, the `String` title, hover + frame). Items 4 and 5 of the continuation brief (the product paint
         path, the windowed host, the rest of the prelude, the scenes) are NOT started.
branch:  lane/integration/card3-facade at the commit that adds this file (predecessor of the card: 091f91578d)
host:    macOS aarch64, 32 cores, shared; load average 28 to 34 during the three cost runs (instruction counts,
         thread counter net of its bracket; no lock taken, by the amended rule)
```

## What was built

| | before (Card 4 report 2) | now |
|---|---|---|
| a handler's turn | described unless "only bound words were written" (a count of signal borrows against changed words) | always described (ruling D3), as the legacy runtime does. The skip is gone; `mutant_typed_skip` reintroduces it and the twin test fails |
| a description of the fixed part | the view's element converted to an element-layer value, a heap layout tree built and solved, every record compared, every exposed row described and compared: 116,750 instructions for an unchanged view | **the update walk**: the element value is read where it is against tapes of what the last description said (layout specs by their bytes, paint operands as words, texts by their bytes or, for a capture-free projection of a word, by the word). Nothing is converted, laid out or compared record by record unless a layout word differs: **4,633** for an unchanged view |
| one paint operand differs | the whole row compared | its one record is brought up to date (`Sink::leaf`, checked against the column's archetype) |
| list rows | every exposed row described on every description (no journal) | the `List<T>`'s own log is the world's journal: a row is described when it enters or its item is edited |
| an insert or a remove | every exposed row from the edit on described again | the rows that stay are the same records (`Store::bias`): an edit before the window moves no record and describes the one row that entered; an edit inside it writes the smaller side again. Only for rows declared to be described by item (`RowsHow::by_item`); a list whose row function reads the index keeps the old behaviour, and `events.rs::captures_follow_the_ring` holds that |
| hover | every column reading the hover word scanned (one per button: each button's handler type is its own column) | `hover_bg` over a constant base is a keyed selection; when every reader of a word is keyed by it the change is two searches in one index across columns: 2 records evaluated, 0 columns visited |
| the pick in a list | every record of every handler column tested | the row under the point by arithmetic, then that row's records (uniform rows whose records stay in their band: `Store::spill`) |
| a hovered or pressed row after an edit | the place (a slot) was taken for the row; a slot given to another row lost the leave or got the click | slots are stamped by the write that filled them; the hovered set keeps what it entered (record and captures) and delivers the leave to it; a held press follows its item through the journal after every pass; place tokens carry the stamp |
| `View::changed` | never called on the typed path | called with the lists' logs, in waves |
| scroll past the content after a resize or a shrink | kept (blank port) | brought back within the content (`World::clamp_scroll`) |
| shaping in `lower::text` | a shape plan per call, SipHash maps, a per-character face search | plans kept per face, FxHash, an ASCII text of the primary face is one segment: a fresh 8-character title 47,973 to 16,659 |

## Cells (`sh bench/card3/cost.sh`, three runs, `bench/results/card3/cost-card5-run{1,2,3}.txt`; they agree to 0.3 %)

Thread instructions, net of the counter's bracket, medians of 60; typed and legacy from the same binary.

| cell | legacy | typed (Card 4 r2) | typed (now) | now / legacy | threshold | |
|---|---:|---:|---:|---:|---|---|
| mount, 1,000 rows | 26,411,807 | 7,042,320 | 6,107,040 | 0.23x | | |
| mount, 100,000 rows | 26,742,530 | 7,227,648 | 6,101,859 | 0.23x | <= 0.3x | met |
| description of an unchanged view | (legacy has no such cell) | 116,750 | 4,633 | | <= 10,000 | **met** |
| theme (a plain field) | 7,171 | 116,967 | 5,907 | 0.82x | <= 1.0x | **met** |
| insert at 0 (a click) | 120,546 | 216,942 | 54,487 | 0.45x | <= 1.0x | **met** |
| scroll by one row (a `turn`) | 89,494 | 23,642 | 19,676 | 0.22x | <= 1.0x | met (and now with the description D3 requires) |
| resize by 40 pt | 3,480,254 | 1,600,095 | 1,593,976 | 0.46x | | not worked on |
| one signal, the title (a fresh `String` each turn) | 8,098 | 165,658 | 22,971 | 2.84x | <= 0.25x | **missed** |
| hover, one row to the next | 415 | 4,018 | 3,623 | 8.73x | <= 0.5x | **missed** |
| hover + frame (process) | 2,224 | 897,261 | 868,389 | 390x | <= 1.0x | **missed** (not worked on: the reference compositor; item 4) |
| title + frame (process) | 64,472 | 301,496 | 149,844 | 2.32x | | |
| still frame (process) | 1,809 | | 1,609 | 0.89x | | |

Count bands, exact, asserted in tests: a handler's turn is 1 description; a description that changed nothing
describes 0 rows, solves 0 layouts, looks up 0 texts and allocates 0 times; hover and the wheel are 0
descriptions and 0 allocations; a row-to-row hover evaluates 2 records and visits 0 columns; a front insert
describes 1 row, a front remove 1, an insert inside an 8-row window 4 (three rows above and the new one) or 2,
an exposed edit 1, an unexposed edit 0, a permutation 8. Not a band: a title write allocates (a run is stored).

### Predictions against results

| cell | predicted | result | |
|---|---|---|---|
| unchanged description | 3,000 to 8,000 | 4,633 | held |
| theme | 0.5x to 1.2x, at risk | 0.82x | held |
| title | miss of 0.25x; 0.8x to 2x | 2.84x | **missed the prediction too**: I put shaping at "thousands"; it is 16,659 after the three fixes above, of which rustybuzz alone is 11,691 |
| scroll (a turn) | 0.15x to 0.4x | 0.22x | held |
| insert at 0 | <= 0.5x | 0.45x | held, narrowly: two fresh strings are shaped in that turn (the edit count and the new row's number), about 30,000 of the 54,487 |
| hover | 1,000 to 2,000; miss of 0.5x | 3,623 | **missed**: the delta fell as predicted (2 records) but the turn around it did not |
| allocations (unchanged description, hover, wheel, bound-word turn) | 0 | 0 | held |

## The three misses, by phase (`typed::App::phases*`, feature `phase_probe`; marks cost nothing without it)

**Hover, 3,623 against 415.** Pointer words 75; the hit chain 972; the hovered set and two handler calls 697;
the turn's settle about 1,550 (the view's block gathered and compared twice, the relation epoch, the keyed
delta and two tile marks). Each phase was reduced once (the chain by the arithmetic row, the delta by the
cross-column key index: 2,324 to 1,548), and the marks themselves blur numbers this small (about 30
instructions each). What remains is the shape of the dispatch, not a loop over the scene: a chain is built,
diffed against the hovered set, two trampolines run, a block is compared. The legacy runtime's 415 is one
retained hit and an inline paint write. I do not see a way to 207 (0.5x) in this design; the reformulation
that would do it is the one REFORMULATIONS lists for hover: the hovered id as a uniform the executor compares
(`Pick` read by the paint, no handler, no settle), which needs the raster's pick word to name items, not
places. Not built.

**The title, 22,971 against 8,098.** 16,659 of it is shaping "Slice 17" (rustybuzz 11,691; my wrapper around
it about 5,000 after the fixes), about 3,300 the description, the rest the handler, the layout memo's miss
check and the settle. The run handle is the word (the tape), and the row is not laid out again when the width
is unchanged; a width change solves the fixed part once (the flat form, not the heap tree). The floor is the
shaper: the legacy runtime shapes with its own Latin path and its whole turn is less than rustybuzz's call.
Reusing the unchanged prefix of the old run would make this one glyph, but HarfBuzz's safe-to-break flags
are a statement about one text, not about an edited one: I did not find an exact rule and did not build an
inexact one. The honest routes are the staged shaper of `bough-face` (the text lane's) under this seam, or
prepared digit runs for numeric projections (the review lane's staged prototype did that for digits only).

**Hover + frame.** Untouched: the typed path still repairs through this lane's scalar reference compositor.
The seam to `bough_paint::Scene` (item 4) is the next card.

## Gates run (exact commands; release; `CARGO_TARGET_DIR` this session's own)

| command | result |
|---|---|
| `cargo test --release --workspace --no-fail-fast -j 8` | **556 passed, 4 failed, 13 ignored** in 244 test binaries. The 4 failures are the baseline's font-path ones, unchanged from `lane/integration/wave3` (`bough-app-sketch-code --test code` x3, `bough-zgui --test drawing` x1) |
| `cargo test --release -p bough-x` | 136 passed, 0 failed, 3 ignored (TR1-03, TR1-09, `el_scroll_binding`) |
| `cargo test --release -p bough --test typed_slice --test typed_words --test typed_list --test typed_review --test typed_twin` | 3 + 1 + 3 + 6 (1 ignored: `clip_overflow`) + 1 passed |
| `cargo test --release -p bough --test lowered_slice --test lowered_counter --test lowered_boundary --test lowered_review` | 9 (6 ignored) + 4 + 3 + 8 passed: the fallback is as it was |
| `sh bench/card3/mutants.sh` | see the next table |

Oracles: retained == full redraw == immediate == fresh mount at ring ages 1..=4 after every step of every typed
test (the immediate and fresh passes consult none of the tapes, memos or kept rows: `Shared::plain`); a
**model oracle** that shares nothing with the runtime (`typed_list::model`, `typed_twin`): the application's
own state says what colour each row or block is, and the presented pixels are sampled there; the **twin**
(`typed_twin`): the legacy runtime and the typed path run the same program and both must show its state.

### Negative controls

| mutant | what it breaks | typed tests it fails |
|---|---|---|
| `bough-x/mutant_delta` | the derivative forgets records | `typed_slice` |
| `bough-x/mutant_journal` | a list ignores its journal | `typed_list` (3), `typed_slice` |
| `bough-x/mutant_keyed` | the key index forgets the old key's records | `typed_slice` (2), `typed_words` |
| `bough-x/mutant_rigid` (new) | kept rows keep their old rects | `typed_list` (3), `typed_slice`; and bough-x's own `review_round1_lists` |
| `mutant_typed_tape` (new) | a changed text is taken for the kept one | `typed_slice` (3), `typed_words` |
| `mutant_typed_log` (new) | a list's log is not read | `typed_list` (3), `typed_slice` |
| `mutant_typed_skip` (new) | Card 4's skip (a bound-word turn is not described) | `typed_twin` |
| `bough-x/mutant_moves` | a moved record damages only where it is now | **survives**: nothing on the typed path moves by its paint yet (`.translate` is not converted) |

`sh bench/card3/mutants.sh` (the typed mutants against the five typed test files, then Card 3's four against
the lowered gates), as printed:

```
killed: bough-x/mutant_delta (1 typed tests failed)
killed: bough-x/mutant_journal (7 typed tests failed)
killed: bough-x/mutant_keyed (3 typed tests failed)
killed: mutant_rigid (6 typed tests failed)
killed: mutant_typed_tape (4 typed tests failed)
killed: mutant_typed_log (7 typed tests failed)
killed: mutant_typed_skip (3 typed tests failed)
killed: mutant_free_enter (1 tests failed)
killed: mutant_free_leave (1 tests failed)
killed: mutant_lower_alpha (1 tests failed)
killed: mutant_lower_hover (2 tests failed)
exit 0
```

Three controls bite the typed lists (journal, rigid, log); the brief asked for two more than the one of report 2.

## The review's findings (`reviews/card3-round1.md`), on the typed path

`tests/typed_review.rs` is the review's probes with the same views and assertions, the runtime under them
changed to `bough::typed::App`. On the lowered fallback they stay ignored: it keeps those defects.

| finding | typed path | how |
|---|---|---|
| a front insert delivers the next key to the wrong item | passes | the focused row follows its item through the journal (`refocus`, already in the core), now fed by the list's log |
| a recycled slot receives a stale release | passes | a held press follows its item (`rehold`) and is dropped with it; the handler's reach is by slot and generation |
| lost hover leave / enter under a stationary pointer | passes | the hovered set keeps what it entered and its slot's stamp; the pick is taken again after a pass that moved something |
| blank pixels after enlarging the viewport at the end of 100,000 rows | passes | `World::clamp_scroll` |
| `Changed::list` empty | passes | `View::changed` with the lists' logs |
| the typed scroll-only binding | passes for the prelude (`typed_words`: a signal used only as a scroll offset is a block word); bough-x's own `el_scroll_binding` (its `Sig` lens) is still ignored: not this path |
| `clip_overflow` leak | **open** (ignored probe): a clip is a space, and a box whose clip is a run-time flag cannot become one without changing its record type |
| S13 in `free_sync` | fixed before this card (091f91578d, the review's patch) |
| TR1-02 (a stale place after ring reuse) | **closed**: place tokens carry the slot's stamp; the probe is un-ignored unchanged |
| TR1-10 (a front insert describes every visible row) | **closed** with one line of the probe changed (`Rel::by_item`), reason in the file: rows that read their index must be described again, and another gate of the same suite requires it |
| TR1-03 (a replaced relation is not subscribed) | open in bough-x (`settle` cannot see a relation it never read). On the typed path a `List` put in the field's place is detected by its head tag and every row is another row (`typed_list`: "replaced") |

Two things found while porting, both in the lowered runtime and not fixed here: `bough::lower::App::wheel`
takes the opposite sign from `bough::raw::App::wheel` (a positive turn scrolls up there, down on the legacy
runtime and on the typed path); and it recycles a list slot inside the turn that freed it, because it never
marks the list displayed.

## Reformulations: what was taken, what was not

| cell | basis | taken? measurement |
|---|---|---|
| any handler turn | the description as a compare of operands against a tape, per element | taken: 116,750 to 4,633 |
| any handler turn | "did what the view reads change" as one compare of the owner's bytes (an auto trait proving no indirection in plain state) to skip the description entirely | **not taken**: reading padding bytes is not defined, and the update walk was needed anyway. It would turn a signal-only turn into about 1,500 instructions; recorded for the owner |
| layout of the fixed part | words compared, rects kept | taken; a spec that differs solves once (flat form). Rects affine in the viewport (resize as two words): not taken, the layout lane's |
| insert | the row keeps its record; the index is `slot - bias` | taken. The rects of the kept rows are still written (one add per record: about 900 instructions for 18 rows of 4); a row origin as a word the kernel adds would remove that and is not taken (it changes the page's rect lanes for every backend) |
| scroll | the list reached without the view | partly: a pass after a turn converts nothing and only reaches the lists, but the author's `view()` still runs (501 instructions) to reach them |
| hover | two records by key, across columns | taken. The hovered id as a uniform the raster compares: not taken (above) |
| theme in a plain field | none below the author's `if`; made cheap by the walk | 0.82x. A bound theme as a palette slot under `World::broad`: not built (the slice does not bind its theme) |
| title | the run handle is the word | taken (tape). One glyph reshaped instead of the run: no exact rule found |

## Compile time and size (added after the first commit of this report)

`sh bench/card3/compile.sh`, load average 23 to 25, **no bench lock** (the measurement is a build, and the
amended rule forbids holding the lock across one): wall seconds, indicative only.

```
clean build of bough + slice + deep view: 19.33 s   load: { 23.07 22.29 26.96 }
incremental leaf edit (deep view example): 5.96 s   load: { 25.23 22.75 27.09 }
deep_view binary bytes: 1982472
deep view: 132 records, 1 describes, legacy passes 0
```

Against the thresholds (clean <= 35 s, incremental <= 8 s, kill at 30 s): inside both. The deep view is still
driven by the fallback (it uses `each`, wrapped paragraphs and `.opacity`), so this is what `typed.rs` costs a
program that does not use it, not what a deep typed view costs; that number does not exist yet.

## Not done, not verified

* Items 4 and 5 of the brief: `bough_paint::Scene` through the paint lane's seam, executor chosen per change,
  the windowed host with output pacing (X22), `each`/`Vec` children, `para`, `image`, box operands, transitions,
  `eq().select()`, the refused list, the runner scenarios and the five heavy scenes.
* A view with a plain scrolled box (`ScrollBox`) or a handler that captures values takes the conversion on every
  description (`BY_REF` false): correct and slow. Pinned after this report was first written by
  `tests/typed_byvalue.rs` (clicks, a captured value that changes, the wheel; four oracles; 1 passed).
* A list whose rows spill out of their band falls back to the column search for the pick: no test.
* The run store drops a run nobody asked for in 256 descriptions; rows are no longer asked on every
  description, so every 128th description describes and compares every exposed row. Wheel-only sessions never
  collect (as before). A proper liveness (runs held by records) is owed.
* Miri, Loom, sanitizers: not run on any of this. New `unsafe`: `Sink::leaf` (type-checked site), the kept
  hover copies (`World::keep`, the leave from a copy), `spec_same`, the list log readers by offset
  (`list_ops`, `list_end`, `list_pubs`), each with its argument; their tests are the suites above, not
  dedicated ones.
* `bough-x/mutant_moves` does not bite the typed path.
