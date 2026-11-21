# Card: resident palette tables read by an expression; a fade on a broad clock (slice 8, decision D4)

```
lane:            style-motion-media
state:           CARD
owner:           fx session (Claude, claude-fable-5-2), 2026-11-21
predecessor:     4470f378da (slice 7 merged with lane/integration/wave3)
question:        Can a colour be a slot of a resident table, read inside the expression (so a hover or a time
                 word can still compute on the resolved colour), with a theme switch and a timed cross-fade
                 that never visit a record?
change:          (1) `bough-expr` (D4, option 1): `Cx` gains `pal: &[u32]` (empty from `Cx::of`), `Code` gains
                 `Pal(index)`, and `Pal<I>` is the expression that reads word `index` of the table (0 past the
                 end). Every line is listed in the report.
                 (2) `bough-paint`: the scene keeps N tables of 256 slots in its arena (so Metal reads them
                 with no copy); every evaluation context carries them; `emit` renders `Code::Pal` for Metal
                 and WGSL; a leaf `x_palix(table, slot)`. Writing a table's contents re-mounts the lanes (a
                 record's opaque cover may depend on a table word); choosing a table is a block word.
                 (3) A **derived clock word**: `Scene::clock(word, start, dur)` declares that block word
                 `word` is the scene's to compute each frame: the Q15 progress of `Time` from word `start`
                 over word `dur`. It is a broad word like any other, so a fade is "the frame is damaged whole"
                 with no record in an active set; `Scene::animating` holds while a clock runs; executors draw
                 under `Scene::block()`.
what this is not: palettes of geometry (a slot is a colour word; a geometry operand reading a broad word still
                 takes its window pass); inks (R2); per-table damage (a table write re-mounts everything).
implication:     Passing gives R1: a theme is one word, and N themes cost N tables of memory and nothing per
                 record or per frame.
workloads:       tests/fx.rs (a new paletted scene); fxbench effects with `PALETTE=1` (card colours through
                 slots, three tables, a theme cell that switches table and a fade cell).
gates:           P1 a paletted scene == the same scene with every slot replaced by the literal colour (for
                    each of N >= 3 tables, and mid-fade against the literal mix), on the CPU kernel, retained
                    and fresh, and == the specification.
                 P2 Metal (resident plan) == CPU on the paletted scene, every step, including a slot chosen by
                    a hover word (the CPU's derivative under a non-broad word: retained == fresh) and a fade.
                 P3 mutants: `mutant_pal_stale` (the context's table is not the scene's current one after a
                    write) and a source mutant (the Metal read ignores the index) must fail.
                 P4 all earlier gates; `cargo check --workspace` (the core lanes still build); pbench A1
                    <= 1.03x of baseline.
thresholds:      F-a a fade frame's turn (`Scene::update`): within 1.2x of the theme cell's turn (2.4 K), flat
                    in cards 200 / 1,000 / 5,000, 0 records visited.
                 F-b a fade frame's frame-thread instructions on Metal: within 1.1x of the theme cell at the
                    same size (today 216 to 220 K at 8 ranks).
                 F-c a table switch: the same as the theme cell.
measurement:     frame-thread instructions, three repetitions, no lock.
budget:          this session.
stop conditions: a `bough-expr` consumer outside this lane stops building: revert to source-compatible form
                 or report which.
do not revisit:  an `Ink` word type; paint-level lookup (D4).
```
