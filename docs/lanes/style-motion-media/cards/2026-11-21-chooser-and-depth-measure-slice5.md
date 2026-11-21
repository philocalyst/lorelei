# Card: an instruction-denominated chooser; measure before building for depth (c); each_in in the span painter (slice 5)

```
lane:            style-motion-media
state:           CARD
owner:           fx session (Claude, claude-fable-5-2), 2026-11-21
predecessor:     536653b878 (slice 4 merged with lane/integration/wave3)
question:        (1) Does a chooser whose two constants are frame-thread instructions (not wall time) pick the
                 cheaper executor in every cell and the same one in every run? (2) In the dense stack, what
                 fraction of repaired pyramid blocks and prefix tiles come out byte-identical to what was
                 stored, i.e. how much propagated damage would compare-on-write stop (orchestrator's (c))?
                 (3) Does `ColRef::each_in` in the span painter's fill keep its pixels?
change:          (1) `mixed::Mixed` compares the damage's weight times a fixed instructions-per-weight with the
                 staged frame's fixed frame-thread cost; both constants are instructions, so the choice does not
                 depend on load. `calibrate` is removed. (2) Counters only: the CPU executor counts blocks and
                 prefix tiles whose recomputed words equal the stored ones. No behaviour changes. (3)
                 `Backend for Spans::fill` visits records through `each_in` with the pass's cut (the mask's
                 bounding box, the frame and the space's clip, in the store's space).
implication:     (2) decides whether compare-on-write is worth restructuring the schedule for (the plan is built
                 before anything is executed today; stopping damage at an unchanged block makes the schedule
                 depend on results, which the Metal path cannot follow without a GPU-side schedule).
workloads:       fxbench effects 200 / 1,000 / 4,000 (chooser); effects 600 dense, 3,000 dense (counters);
                 comparisons/bough-guibench-x tests (span painter).
gates:           all of bough-paint's; bough-guibench-x's pixel gates and its two span mutants.
thresholds:      C1 the chooser runs the same executor in all three repetitions of every cell, and its
                    frame-thread instructions are within 5 % of min(CPU kernel, Metal) in every cell.
                 C2 prediction for (c): in "one spring" at 600 dense fewer than 30 % of repaired blocks are
                    unchanged (the changes there are 38 animating shader fills: real changes), so compare-on-
                    write would NOT rescue that cell; in "hover beneath glass" more than 50 % are unchanged at
                    levels >= 3. Either way the number decides; no kill line (a measurement).
                 C3 span fill: records visited per frame fall; pixel gates unchanged.
measurement:     frame-thread instructions, three repetitions, no lock.
budget:          this session.
stop conditions: none.
do not revisit:  a wall-time chooser calibrated at start-up (slice 4: unstable under load).
```

Not in this card, and why: (a) a stack of glass as one reader and (b) strata. Both change what a stacked
backdrop *means* (the specification), and (b) is exact only where the stratum above is opaque or of uniform
alpha. They are assessed in the report against the numbers of (c); neither is built here.
