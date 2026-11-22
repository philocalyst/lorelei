# Integration Card 5: a description is an update, a list is its own log (frozen 2026-11-21, before its code)

```
lane:            integration
state:           CARD
owner:           core2 (session of 2026-11-21, continuing Card 4 from 091f91578d); reviewed by the
                 independent-review lane and the orchestrator
predecessor:     091f91578d (Card 4 report 2 + the Card 3 review's patches; merged with lane/integration/wave3
                 3b9238cdf1; nightly-2026-10-17, release)
question:        Can a handler's turn on the typed prelude path keep the façade's contract exactly (the owner is
                 described again after its handlers: ruling D3) and still cost what its change costs, by making
                 a description an *update against what the last description said* rather than a second
                 construction that the world then compares?
change:          Four representations, each replacing a per-pass recomputation by a compare of one word or a
                 short run of bytes against what is kept:
                 (a) text: the k-th text of the fixed part keeps its source bytes, its run handle, its width and
                     its baseline (a tape in description order); a description compares the bytes and reuses
                     the handle. Replaces a string-hash lookup through two hash tables per text per pass. A
                     projection of a word-sized signal (`sig!(n).map(f)`) is keyed by the word: `f` (and its
                     `format!`) runs only when the word differs.
                 (b) layout of the fixed part: the layout words of the last description are kept with their
                     rects; a description emits its words into a comparer, and equal words at the same
                     viewport and scale are the kept rects. Replaces building a heap tree and solving it
                     every pass.
                 (c) list rows: the façade `List<T>` already keeps the turn's log (insert, remove, move,
                     permute, edit) for its legacy regions. The typed list reads that log as the world's
                     journal: a pass describes the rows the log names and the rows that enter, never "every
                     exposed row because a description ran". Replaces `cx.all` over a list with no journal.
                 (d) the count-based skip of Card 4 report 2 (a turn whose signal writes equal its changed
                     block words is not described) is REMOVED: it shows a plain-field change late when a
                     handler writes both, which D3 rules out. Every handler turn is described; the saving is
                     (a) to (c). `Config::mount_once` keeps its meaning (the legacy runtime has it too).
                 Then, in the same basis: an insert as one row entering with the rows after it moved by their
                 rects (no row described again); hover through the operators lane's equality index (`sel`
                 over the hover word, a keyed column); a twin test against the legacy runtime for D3.
implication:     Passing makes "re-describe after handlers" affordable as the default, so the typed path needs no
                 unsound skip and no author-visible mode. It does not establish: a general `String` signal at
                 0.25x legacy (a description still runs; see predictions), token palettes for a theme held in a
                 plain field (the author's `if self.dark` is opaque Rust: only a description can see it), the
                 product paint path, the windowed host, the scenes.
platforms:       macOS aarch64 (this host: 32 cores, shared, load 30 to 60), headless CPU.
workloads:       the Card 3 slice (`examples/slice.rs`, unchanged source), the counter, the word-signal view of
                 `typed_words`, one added twin program (a handler writes a bound word and a plain field).
gates:           four oracles (retained == full redraw == immediate == fresh mount) at ring ages 1..=4 on every
                 workload; provenance (0 legacy passes, 0 `lower` descriptions); count bands, exact:
                 a description of an unchanged view describes 0 rows, solves 0 layouts, shapes 0 runs, makes
                 0 hash lookups of text, allocates 0 times; a title write shapes exactly 1 run and solves the
                 fixed layout exactly once; an insert at 0 describes exactly 1 row; an edit of one exposed item
                 describes exactly 1 row; hover and wheel: 0 descriptions, 0 allocations; hover evaluates
                 exactly 2 records. Twin: the typed and the legacy runtime show the same pixels' *state* after
                 every step of the D3 program (compared through each runtime's own reference image of the
                 same view state: typed immediate vs typed retained, and the text/colour the view would
                 describe). Negative controls: `bough-x/mutant_delta` (kept); `bough-x/mutant_journal` (the
                 journal is forgotten: must fail the typed list gates); a new `bough/mutant_typed_tape` (a
                 changed text is taken for the kept one: must fail); a new `bough/mutant_typed_log` (the
                 list's log is not replayed: must fail). An oracle that does not use this lane's compositor:
                 a fresh `bough::lower` mount of the same state compared by pixels where both admit the view.
thresholds:      (the orchestrator's) description of an unchanged view <= 10,000 thread instructions;
                 one-leaf paint <= 2,000 net; selection about 800; hover <= 0.5x legacy (415: see predictions);
                 one `String` signal <= 0.25x legacy; insert, scroll, theme <= 1.0x legacy; 0 allocations on
                 bound-signal, hover and scroll turns; mount 100,000 <= 0.3x.
measurement:     `bench/card3/cost.sh` (thread instructions net of the bracket, medians of 60; no lock: the
                 column is instruction counts), three runs, load average with each; phase counters
                 (`typed::App::phases`) for the split; allocations by a counting global allocator in a test.
budget:          this session; nice -n 15, -j 8.
stop conditions: an unchanged description above 10,000 after (a) to (c): stop, split again, report the
                 residue by phase before touching any cell. A gate that needs an author-visible change: stop.
do not revisit:  docs/SETTLED.md; D3 (no skip the legacy runtime would not also survive).
```

## Predictions (written before the code; a miss is reported, not re-fitted)

| cell | today (typed / legacy) | prediction | why |
|---|---:|---|---|
| description of an unchanged view | 116,000 | **3,000 to 8,000** | view + conversion is 8,729 today, most of it text lookups and `format!`; with the tape the fixed part is struct building, ~25 byte compares and ~22 record compares; the world's pass around it (gather, rederive, relations, refocus) is unsplit and may be the residue |
| theme (plain field) | 116,967 / 7,049 | **0.5x to 1.2x: at risk** | an unchanged description plus three colour writes and their damage; legacy's 7,049 is itself a tape update |
| one signal, the title (`String`) | 165,658 / 7,976 | **miss 0.25x; 0.8x to 2x** | a description, one fresh shaping (rustybuzz on 8 characters: unmeasured, probably thousands), and the fixed layout solved once because the title's width is a layout word. The basis where it is one word (rects affine in the width word) is not in this card |
| scroll by one row (a `turn`) | 23,642 / 92,363 | 0.15x to 0.4x | a description (now run again on every turn: D3) plus one row entering |
| insert at 0 | 216,942 / 123,467 | **<= 0.5x** | a description, one row described, 20 rows' rects shifted |
| hover | 4,018 / 415 | **1,000 to 2,000; miss of 0.5x stands** | two records through the key index plus the pointer dispatch (pick, chain, two handler calls, a settle); legacy's 415 is a retained hit-test with an inline paint write. Stated as a floor question, measured by phase |
| mount 100,000 | 0.27x | unchanged | not touched |
| allocations: unchanged description, hover, wheel, bound-word turn | — | 0 | |
| allocations: title write | — | > 0 (a run is stored) | stated, not a band |

## Reformulations considered (the standing direction), per worst cell

| cell | the basis where the change is one word | taken here? |
|---|---|---|
| any handler turn | "did what the view reads change" as one compare: the owner's bytes against a snapshot, sound only for a view whose plain state has no indirection (an auto trait could prove it; padding bytes make the compare formally undefined). | **considered, not taken in this card**: it would give 0 descriptions on signal-only turns for such views; it needs a decision on reading padding. Recorded as the follow-up. The update form is needed either way (a view with a `Vec` field is described). |
| title | the run handle is the word; width a second word; rects affine in it | handle: yes (tape). Affine rects: no (layout lane's parametric answers) |
| theme in a plain field | none below the author's `if`: a description is the only reader of opaque Rust. A token palette + `World::broad` applies to a theme the author binds (`sig!`), which this slice does not do | description made cheap instead; the bound-theme form is measured on the word-signal view as "one-leaf paint" |
| insert | the journal says one row entered; the rows after are the same records at another y | yes: rects shifted, no row described. The further step (row origin as a word so no rect is written) changes the store's rect lanes and is not taken |
| hover | the hover id is a key: two records | yes (`sel`) |
| hover + frame | executor chosen by the change (R5) through the paint lane's seam | no: item 4 of the brief, next card |
