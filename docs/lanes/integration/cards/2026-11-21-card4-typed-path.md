# Integration Card 4: the prelude on the typed path (frozen 2026-11-21, before any of its code)

```
lane:            integration
state:           CARD
owner:           core (session of 2026-11-21); reviewed by solcore and the orchestrator
predecessor:     dcc83583a1 (Card 3 slice merged with lane/integration/wave3 7991e7bd4e; nightly-2026-10-17, release)
question:        Can a `bough::prelude` view drive the typed element layer (`bough-x/src/el.rs`: rows as types,
                 operands as staged expressions in record words, `World<V, B>` settle and re-description), so that
                 mount describes once, a runtime event describes nothing, and a handler's turn costs a typed
                 compare of what is exposed, with Card 3's erased `lower` tree kept only as the fallback?
change:          A façade element value converts to an element-layer value (`typed::Typed`): boxes to
                 `bough_lay::LBox` with a paint archetype that evaluates its colour expression when shaded, one-line
                 text to a line archetype over the lowered run store, handlers to typed handler components on the
                 box's record (no `dyn Any`), `hover_bg` to a selection on one hidden block word, `sig!` operands of
                 word-sized types to expressions over a block word found from the field's proven offset, `.scroll`
                 and `list(..)` to a space moved by a block word with rows by exposure. A view whose element type
                 has no conversion is driven through `lower` (decided per root view at compile time by
                 specialisation, same world type family, no legacy `Rt`). Layout stays data: the element's layout
                 words into the heap tree (root) and the list memo (rows).
implication:     Passing makes the typed core the runtime of ordinary prelude programs for the constructs
                 converted, at its own costs. It does not establish: text input, tooltips, entities, readers, a
                 windowed host, the scenes (items 3 to 5 of the orchestrator's Card 4 note, which follow).
platforms:       macOS aarch64 (this host: 32 cores, shared, load 13 to 30), headless CPU.
workloads:       the Card 3 slice (`examples/slice.rs`, unchanged source), the counter, the deep view
                 (`examples/deep_view.rs`), one added view with a word-sized bound signal (a colour and an offset).
gates:           retained == full redraw == immediate == fresh mount at ring ages 1..=4 on each workload, on the
                 typed path; provenance: 0 legacy passes AND 0 `lower` descriptions for views the typed path admits;
                 count bands, exact: a hover (enter or leave) is 0 descriptions; a wheel scroll is 0 descriptions
                 of fixed rows; a turn that writes only word-sized bound signals under `Config::mount_once` is 0
                 descriptions; a handler's turn (the façade's contract: re-describe after handlers) is exactly 1
                 typed description and 0 `lower` descriptions. Negative controls: `bough-x`'s `mutant_delta`
                 (the derivative forgets records) and `mutant_moves` must fail the typed gates; the Card 3 mutants
                 keep failing the fallback's.
thresholds:      (orchestrator's, kept as given) one signal <= 0.25x legacy and within 3x of the hand-written core
                 for the same change; hover <= 0.5x legacy turn and <= 1.0x with the frame; insert at 0 <= 1.0x;
                 scroll by one row <= 1.0x; theme <= 1.0x; mount 100,000 virtual <= 0.3x; 0 allocations on signal,
                 hover and scroll turns; deep view clean <= 35 s, incremental <= 8 s (kill at 30 s).
measurement:     `bench/card3/cost.sh` cells, with one change of method fixed here before measuring: turns are
                 bracketed with the calling thread's instruction counter and the bracket's own cost (measured,
                 `tinstr_overhead`) is subtracted, because the process counter's bracket (about 13,000) is larger
                 than several of the thresholds. Both runtimes are measured the same way; the process-counter
                 columns are kept beside them for the frame cells. Allocations by a counting global allocator in
                 a test binary. Instruction counts under load; load average recorded.
budget:          this session; nice -n 15, -j 8, one cargo at a time.
stop conditions: deep-view incremental build > 30 s: stop and keep the conversion out of the hot build path. A typed
                 re-description of the slice above 100,000 instructions: stop and report what in the conversion
                 costs it before optimising cells.
do not revisit:  docs/SETTLED.md (no per-row signals or observers, no per-view static switch that changes
                 semantics: the fallback draws the same pixels; no monomorphised layout solver).
```

## Predictions (written before the code; a miss is reported, not re-fitted)

| cell | prediction | why |
|---|---|---|
| hover turn | pass: under 2,000 net against a legacy net of a few hundred to a few thousand; **the <= 0.5x ratio is at risk** if the legacy hover is itself only a few hundred | one hidden word changes; the columns that read it are differentiated (about 9 per exposed record that reads any word) |
| hover + frame | pass <= 1.0x only if the damage is the two rows; the Card 3 number (906,854) is two 1280 x 56 px rows cleared and refilled by a scalar compositor: to be measured apart as pixels, not runtime |
| one word-sized signal (new cell) | pass: <= 0.25x legacy, within 3x of the hand-written core |
| one signal, the title (a `String`) | **predicted miss of 0.25x**: 1x to 3x legacy | the text's width changes its row's geometry; in this representation a geometry change is a re-description (typed compare of what is exposed), not a delta. The fix is a geometry-carrying operand, which this card does not build |
| insert at 0 | pass <= 1.0x | a typed re-description of the exposed rows; no relation journal yet (`Seq`/`Table` are the next step) |
| scroll one row (wheel or bound offset) | pass <= 1.0x | the space moves by its word; rows by exposure |
| theme (opaque field) | **predicted miss of 1.0x**: 1x to 3x | opaque state is a re-description by contract |
| mount 100,000 | 0.2x to 0.35x: at risk | first shaping and rasterising of the exposed text dominates both paths' floor |
| allocations on hover / wheel scroll / word-signal turns | 0 | settle only |
| deep view | not admitted by the typed path as written (`each` outside a scrolled box, wrapped paragraphs, `.opacity`): driven by the fallback; its compile numbers must not move by more than 1.5x |

## Decisions frozen here

* **Re-describe after handlers, settle after runtime events.** The façade's contract (01-state.md section 2; the
  legacy runtime's default) is that a view is described again after its handlers; plain fields are state. The
  typed path keeps that: a handler or `turn` is a settle plus one typed re-description. Events the runtime
  handles itself (hover paint, wheel over a scrolled box, time) run no author code, so they settle only. Under
  `Config::mount_once` (the harness option the legacy runtime has) a turn settles only.
* **Block words.** A word-sized bound signal takes one of 47 application words, assigned by first description;
  the 48th is the hover selection. A 48th bound word refuses the typed path for that view (fallback), by name.
* **One record per box**, painted or not (an unpainted box is a record of alpha 0): handlers and hover need the
  record, and the row's shape must not depend on a run-time flag.
