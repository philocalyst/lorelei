# Card 12. Each prefix receives changes from its immediate predecessor

```text
lane:            style-motion-media
state:           CARD
owner:           fxsol (GPT-6.2 Sol)
predecessor:     1cd7d9ef22 (including the slice 11 implementation)
observation:     600 dense, six one-spring frames, 534 of 570 prefix repairs unchanged.
                 3,000 dense, 3,150 of 3,348 unchanged. Counts repeated three times.
question:        Can an unchanged checkpoint stop an original event before every prefix above it is repaired?
change:          Route each event tile only to the first later layer holding that tile's prefix.
                 Propagate changed level-0 pixel boxes to the next checkpoint and damage the output.
                 Also propagate existing events from a layer's sampled pyramid to the tiles it affects.
                 The eager plan retains conservative routing before any repair result exists.
implication:     Above a checkpoint, a later prefix depends on its stored level 0, its effect output
                 and the intervening paints. A first check above an unchanged checkpoint is unnecessary.
                 A cross-shaped cover catches additional individual opaque records, while a checkpoint
                 can certify all earlier paints at once. Evaluate the checkpoint representation first.
platforms:       CPU layer kernel and resident Metal, headless macOS
workloads:       Full instruction matrix at the same sizes as slices 10–11.
gates:           Existing retained/full/Metal/specification. Add two overlapping readers where the
                 lower's output depends on its level-0 accumulator even with only a coarse read.
                 `mutant_prefix_lost` removes direct level-0 forwarding and must fail that fixture.
thresholds:      At 600 dense, reduce 570 one-spring prefix checks to at most 250 per six frames.
                 Kill above 400 or on a pixel mismatch. No pbench A1 cell > 1.03x baseline.
                 Report the changed and unchanged check counts and the dense CPU instructions.
measurement:     Frame-thread instructions, three repetitions with load. Report all cells.
stop conditions: An unproven checkpoint or different compositing semantics keeps the conservative route.
do not revisit:  Whole-cover per-tile key and tolerated pixel differences.
```
