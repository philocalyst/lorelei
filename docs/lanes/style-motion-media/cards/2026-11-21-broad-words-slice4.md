# Card: a broad change is one word (theme on the GPU at one-record CPU cost); the chooser (slice 4: R1, R5)

```
lane:            style-motion-media
state:           CARD
owner:           fx session (Claude, claude-fable-5-2), 2026-11-21
predecessor:     1293a9fbd1
question:        Can a theme switch on the paint scene cost the frame thread what a one-record change costs,
                 at any scene size, with the GPU doing one full-surface staged dispatch, and can the executor
                 be chosen per change from the change alone?
observation:     In this scene a themed colour is ALREADY unresolved in what is retained: a record holds its
                 token's two colours and the kernel mixes them by a block word at shade time
                 (`MixV<Pmo<Rec>, Pmo<Rec>, Un<Theme>>`); nothing is restaged on a theme change. What makes a
                 theme cost follow the scene is elsewhere: (a) the window pass asks every record of every
                 reading column whether its colour changed (O(records)); (b) the layer schedule enumerates
                 every layer's tiles and blocks; (c) the staged Metal path builds a job per tile and per block
                 on the CPU each frame; (d) the CPU and Metal executors keep separate layer pools, so the
                 executor cannot change between frames at all. The basis is right; the bookkeeping resolves it.
change:          (1) **Broad words.** A block word the application declares broad (`Scene::broad`) is never
                 differentiated: a change of it damages the frame as one fact, marks every layer for repair as
                 one fact, visits no record, rebuilds no bins. (A column whose geometry reads the word is
                 refused the shortcut and takes its window pass.)
                 (2) **The full frame as a cached dispatch.** The staged frame for "everything" (every layer's
                 prefix tiles and pyramid blocks by rank, then every output tile) is a job buffer that depends
                 only on the bins and the layer table; Metal keeps it resident and re-submits it while they
                 stand. A theme frame on the CPU side is then: write the block, encode a fixed list of
                 dispatches.
                 (3) **One layer pool.** The pool becomes page-aligned memory that never moves, wrapped by Metal
                 without a copy like the columns: both executors repair the same words (they are byte-equal by
                 gate), so any frame may run on either.
                 (4) **The chooser** (`mixed::Mixed`): a broad change goes to the GPU without looking at
                 anything; otherwise the frame's cost on the CPU is predicted from its damage (tile weights and
                 layer work, both O(damage)) with one device constant, against the GPU's measured round trip
                 (the second constant). No scene constant.
                 A theme cross-fade is the mix word written once per output refresh by the host (a broad
                 word): O(1) on the CPU per frame.
not in this card: palette *tables* (a slot indexing one of N resident palettes). An expression's evaluation
                 context (`bough_expr::Cx`: the block and the record's rect) has no resource to read a table
                 from, on the CPU or in the generated GPU functions; that seam is the core's crate. Two
                 colours per token in the record plus a mix word is the two-table case of R1 with the table
                 entries stored per record; more than two themes needs the table. Stated as a limit, not hidden.
                 R2 (ink-space surface), R3 as a rule, R4: after this card's measurement.
implication:     Passing establishes that a broad paint word costs the frame thread O(1) + the command buffer,
                 and that executors can alternate frame by frame on shared retained state. It does not make
                 the GPU's own work smaller than a full-surface staged dispatch (that is R2), nor cover broad
                 words that move geometry (resize: R4).
platforms:       macOS, CPU tile kernel and Metal.
workloads:       tests/fx.rs; fxbench effects at 200 / 1,000 / 5,000 cards (the world grows, the frame holds
                 about 100 cards: the measurement lane's shape) with executor "mixed", cells theme, one spring,
                 all springs, hover, glass moves; pbench canvas for no-regression.
gates:           B1 a frame sequence that alternates executors at random on the shared pool and surface equals a
                    fresh full redraw at every step (CPU == Metal already gated).
                 B2 a broad change: 0 records visited, bins not rebuilt, and the frame equals a fresh redraw
                    (with layers: glass over themed cards).
                 B3 mutants: `mutant_broad_layers` (a broad change does not mark the layers) fails B2;
                    `mutant_full_stale` (the cached full dispatch is reused after the bins changed) fails B1.
                 B4 all earlier gates.
thresholds:      R1a theme, frame-thread instructions under the chooser: within 1.2x across 200 / 1,000 / 5,000
                    cards. Kill line: grows with cards.
                 R1b theme frame-thread instructions <= 3x the one-spring frame on Metal at the same size
                    ("of the order of a one-card frame"; both pay the command buffer).
                 R1c theme turn (`Scene::update` alone): <= 3,000 instructions at every size.
                 R5a no cell under the chooser worse than min(CPU-only, Metal-only) + 5 % in frame-thread
                    instructions + GPU time converted at the measured device constants; reported per cell.
                 R5b the chooser's thresholds are two device constants measured at start-up.
                 D5 again: pbench A1 <= 1.03x of the 71ee3be baseline.
measurement:     frame-thread instructions (no lock, three repetitions); GPU time under the lock where it can be
                 had, flagged otherwise.
budget:          this session.
stop conditions: executors cannot share the pool exactly (a gate fails): keep separate pools and record that the
                 chooser can then only switch at full redraws.
do not revisit:  growing pyramid levels to blocks; per-record palette resolution on the CPU for a theme.
```

## Reformulations considered for this lane's worst cells (taken / rejected, to be filled with measurements in the report)

| cell | the word | status in this card |
|---|---|---|
| theme | the mix word, declared broad | taken (above) |
| resize | the viewport size, with rects affine in it | not taken here: this scene's rects come from the caller's layout; the kernel side would be `Lane` as `a * W + b` (two more lanes); needs the layout lane's affine answers |
| subtree opacity / tint | a group word multiplied at the layer composite | not taken: needs isolated groups (not built) |
| depth (stacked readers) | one retained thing N readers share | partly taken in slice 3 (restart); a shared pyramid for readers with nothing between is the open step |
| shadows | the coverage function of (radius, blur) as a table, not pixels per record | exists for corners (`Tab`, grid-aligned boxes); off-grid boxes still integrate per pixel: the table over fractional offsets is the open step |
| many moving | endpoints and a law | taken for springs and `Prog` in slice 1; the active-set pass is per moving record (O(moving), not O(1)): a column-level sweep would make it O(columns) |
