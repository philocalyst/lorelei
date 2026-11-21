# Card: depth must not multiply cost; incremental bins; an O(reached) schedule (slice 3, D1's two conditions)

```
lane:            style-motion-media
state:           CARD
owner:           fx session (Claude, claude-fable-5-2), 2026-11-21
predecessor:     a66102aa8d (slice 2 merged with lane/integration/wave3)
question:        Can a change beneath N stacked readers cost each reader only the records between it and the
                 reader beneath, and can one moving record cost the same in a scene ten times larger?
change:          (1) A backdrop record flattens the accumulator over the whole tiles of its halo, not only
                 inside its window. Its stored prefix is then the accumulator on every tile it is listed in, so
                 every job (a prefix job of a reader above, or an output tile) restarts at the topmost layer
                 record of its range: always, not only on tiles inside a window. The price: the halo's tile set
                 is visible state (quantisation), so a change of it damages the tiles it left and entered
                 (once per tile crossed, level 0 being kept by whole tiles). Replaces: restart only on tiles
                 wholly inside a layer record's window.
                 (2) Bins become per-tile lists edited in place: a moved window is removed from the tiles it left
                 and inserted in those it entered; the cut is a start index recomputed for touched tiles.
                 Replaces `Bins::build` on every moved window.
                 (3) Each tile keeps the layer records listed in it; an event marks, through that index, only
                 the layers above it on the tiles it touches. Replaces the events x layers loop.
                 (4) Metal: layers whose halos share no tile with a lower layer scheduled this frame run in one
                 dispatch per stage (ranks). Stacked readers keep one prefix each: sharing one store with
                 per-reader cut-offs was considered and not taken (their prefixes differ by the records
                 between and by the lower reader's own ink; what is shared now is the work, through restart).
implication:     Passing establishes that depth costs O(records between) per reader per reached tile. It does not
                 make a change beneath N readers free: each reader above still repairs its reached prefix tiles
                 and pyramid blocks, and each reader's blur widens the reach of the next (that is what a blur of
                 a blur is).
platforms:       macOS, CPU tile kernel and Metal.
workloads:       tests/fx.rs (all gates, at ring ages 1..=4, CPU == Metal); fxbench effects 600 dense and 3,000
                 dense (one spring, hover beneath, glass moves), pane 300 / 3,000, effects 300 / 3,000.
gates:           all of slice 1's (G1..G6) under the new flatten rule; a new gate that a halo crossing a tile
                 edge keeps retained == fresh (the reason the first halo-wide flatten was withdrawn); mutant
                 `mutant_halo_damage` (a changed halo damages nothing) must fail it; mutant `mutant_bins_stale`
                 (a moved window is not removed from the tiles it left) must fail G1; the four CPU mutants of
                 slice 1 re-run at head.
thresholds:      D1 entries shaded per prefix job in 600 dense "one spring": <= 0.25x of slice 1's (prefix jobs
                    restart from the reader beneath).
                 D2 600 dense one spring, instructions per frame: <= 0.5x of 3,185,064 K. Kill line: > 0.8x.
                 D3 3,000 dense one spring completes; predicted <= 6x the 600 dense figure (five times the
                    readers above each change, each repairing; NOT the 1.2x the orchestrator set as target: I do
                    not expect to reach that with one prefix and one pyramid per reader, and say so now).
                 D4 pane one spring, 300 vs 3,000 cards: instructions within 1.15x (slice 1: 1.33x).
                 D5 no regression: pbench A1 <= 1.03x of the 71ee3be baseline per phase; fxbench effects 300
                    cells <= 1.05x of slice 1's.
measurement:     instructions of the frame thread; counts exact; the bench lock where it can be had within ten
                 minutes, otherwise unlocked with the load recorded (the lock had a queue of ten in slice 2).
budget:          this session.
stop conditions: the halo-wide flatten cannot be made exact at ring ages 1..=4: withdraw it again and record
                 METHOD_BOUNDARY for restart on partial tiles.
do not revisit:  growing every pyramid level to whole blocks (measured in slice 1: 3.7x the layers).
```
