# Card 14. Keep a rigid scroll space in world coordinates

```text
lane:            style-motion-media
state:           CARD
owner:           fxsol (GPT-6.2 Sol)
predecessor:     ada22d9d83
question:        Can records in one rigid scroll space retain their lists while only the origin changes?
change:          Store overlapping 64-by-64 world cells on a 32-pixel lattice and fixed lists
                 in frame space. A frame tile selects a world cell after subtracting the origin
                 and merges the two key-ordered lists. Staging evaluates the declared translation
                 in the kernel. CPU and Metal use the same lists. A pure pan damages the viewport
                 without rewriting windows or lists. Initially admit one rigid space with static
                 geometry, no clock readers and no backdrop layers. Re-enter the ordinary scene
                 for mixed changes and rebuild the index when geometry changes.
implication:     A pure admitted pan visits zero records on the frame thread. The outputs still
                 require repair. Backdrop level 0 and pyramid anchoring are a separate dependency
                 because a fractional pan changes which frame-grid pixels its filter reads.
platforms:       CPU tiles and Metal headless
workloads:       Existing full effects matrix plus a direct fixture for one admitted scroll space.
gates:           Compare pure and fractional pans with fresh rendering at each step, including
                 entering a world cell, fixed paints between scrolled paints, and fallback. A
                 stale-origin mutant must fail. Admit no default scene change outside this path.
thresholds:      For admitted pure pans, recs_visited is zero and no rebins or world uploads after
                 mount. Kill on byte mismatch. Report the effects cells and their admission.
measurement:     Three repetitions of frame-thread instructions with load, no timing claims.
```
