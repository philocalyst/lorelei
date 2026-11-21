# Card: the layer plan is resident and prunes itself on the GPU; a cover certifies a prefix tile unchanged (slice 7)

```
lane:            style-motion-media
state:           CARD
owner:           fx session (Claude, claude-fable-5-2), 2026-11-21
predecessor:     b047a361a7 (slice 6)
question:        (1) Can a Metal frame at depth cost the frame thread a turn and one command buffer, with the
                 schedule decided on the GPU and nothing read back? (2) Can the CPU know a prefix tile is
                 unchanged without computing it?
change:          (1) **Resident plan.** Every (layer, level, block) has a slot, in stage order (rank, level);
                 the slot table depends on the layers' storage and ranks only, not on the bins or on where a
                 layer is inside its storage. A frame is the same command sequence whatever changed: per rank,
                 for level 0 a compaction kernel (one thread per slot) decides which prefix tiles are due and
                 writes their jobs and an indirect dispatch size; the tile program runs them indirectly and
                 each thread compares before it writes, stamping its block "changed" with the frame's
                 generation; per level above the same pair (due = a changed block among the 3 x 3 beneath, or
                 forced); then one pass per rank turns the changed blocks of the levels the record reads into
                 per-tile keys and output damage for what is above. The output tiles are compacted the same
                 way. The CPU uploads, per frame, the damage only: per tile the least and greatest event key
                 (and a generation stamp for its own output damage), and "forced" stamps for blocks that
                 entered a moved window. The job's range and starting prefix (`Scene::seg`) are derived in the
                 compaction kernel from the tile's list. Output damage is a generation per tile, so a surface
                 of any age is "tiles stamped after the frame it last showed": no bit is read back.
                 A broad change is one flag: every slot due.
                 (2) **Incremental upload.** The tile lists live in per-tile slabs on the GPU side and the
                 window lane is written per changed record, from what the bins already report.
                 (3) **Certificate.** Per tile the keys of the opaque covers that hold the whole tile. An event
                 with key z reaches, on a tile, only the layers between z and the next such cover above z:
                 above that cover the stored prefix on that tile cannot depend on z. Exact (no tolerance);
                 the cover's own change is its own event. In the compaction kernel the same fact is applied
                 through the least/greatest key (exact when one source is beneath; conservative otherwise).
what this is not: indirect command buffers, or ranks decided on the GPU: the number of dispatches is
                 13 x ranks + 2 and is encoded every frame. Not a pixel-grain certificate.
implication:     Passing (1) makes "the damage is the dispatch" hold for layers: frame-thread cost independent
                 of how far a change propagates. Passing (3) removes most first checks at depth on the CPU.
workloads:       tests/fx.rs; fxbench effects 600 dense, 3000 dense, 200 / 1,000 / 5,000.
gates:           R1 resident Metal frames == a fresh CPU redraw of the same scene, every frame, over springs,
                    hovers, pans, glass moves, halo crossings, a broad change, and for surfaces of ages 1..4.
                 R2 source mutants that must fail R1: the tile program never stamps "changed"; the level
                    compaction looks at 1 x 1 instead of 3 x 3; the reach pass does not lower the tile keys;
                    forced stamps ignored; stale tile slab (incremental upload skipped for a changed tile).
                 R3 CPU: with the certificate == without it == fresh == spec (existing gates run with it);
                    mutant `mutant_cert` (a cover that does not hold the whole tile certifies) must fail.
                 R4 all earlier gates; pbench A1 <= 1.03x of baseline.
thresholds:      T-a (orchestrator's) Metal frame-thread instructions, one spring, 3,000 dense within 1.2x of
                    600 dense. My prediction: MISSED on ratio, because ranks are about 6 at 600 dense and 31 at
                    3,000 dense and each rank is 13 encoded dispatches; predicted 0.3-0.6 M at 600 dense and
                    1-2.5 M at 3,000 dense, against 195-215 M today (>= 80x less). Kill line for the design:
                    3,000 dense above 10 M.
                 T-b GPU time, one spring at 3,000 dense: not above today's 13 ms; predicted under 5 ms.
                    Kill line: above 13 ms the resident path is not the chooser's Metal path at depth.
                 T-c effects 200 / 1,000 / 5,000 Metal frame-thread: one spring and glass-moves cells within
                    1.2x across sizes (today 255 -> 410 K and 276 -> 417 K); no cell worse than slice 6 by
                    more than 1.1x except by the fixed dispatch count, reported.
                 T-d CPU prefix tiles computed, one spring at 600 dense: 650 today; predicted <= 150 with the
                    certificate. Kill line: above 325 the tile is the wrong grain; record and stop.
measurement:     frame-thread instructions, three repetitions, no lock; GPU time indicative without the lock
                 and once under it if the host is quiet.
budget:          this session.
stop conditions: per-dispatch fixed GPU cost makes T-b fail at 31 ranks: report, keep the eager path as the
                 chooser's, and record what a GPU-decided rank would need.
do not revisit:  reading damage bits back; a wall-time chooser.
```
