# Card: a pan and a hover visit what they can reach, not the scene; 5,000 cards admitted (slice 6)

```
lane:            style-motion-media
state:           CARD
owner:           fx session (Claude, claude-fable-5-2), 2026-11-21
predecessor:     edf614cf37 (slice 5 merged with lane/integration/wave3)
note:            Item 1 of the orchestrator's list (the instruction-denominated chooser, R5a re-measured on every
                 cell) was done in slice 5 and is not repeated.
question:        Can a pan cost the frame thread the records near the viewport, and a hover the two records whose
                 id is the old and the new value, whatever the size of the world; and can a 5,000-card world
                 (40,000 px tall) be admitted?
change:          (1) **Scroll.** A column may declare that two block words are its pan (`Scene::scroll`): its
                 records' dependence on them is a translation. The scene keeps, per such column, its records
                 bucketed by where they are in the scrolled space (an ink box evaluated once under a pan that
                 brings the record to the origin, so it does not suffer the window lane's 16-bit clamp). A
                 change of the pan visits only the records whose buckets meet the old or the new viewport;
                 their windows are recomputed by the existing pass and their bins edited in place. Records
                 elsewhere are not asked: their frame windows are outside the frame before and after.
                 (2) **Hover.** A column may declare that a block word is compared with a per-record key
                 (`Scene::keyed`, `Scene::set_key`): a change of that word visits the records keyed by its old
                 and its new value.
                 (3) **Admission.** A rect lane may lie up to +-2^20 px from the origin (it was +-32,767); a
                 record's own extent stays under 32,768 px. The kernels' arithmetic is on differences from the
                 pixel being shaded and on windows clamped to 16 bits, so only a record near the frame is ever
                 shaded.
what this is not: windows and bins stored *in* the scrolled space with the origin applied in the kernel (so a pan
                 rewrites nothing at all, and pixels could be moved rigidly). That needs space-relative
                 coordinates through the CPU tile, the specification and the Metal kernels (per-entry origin,
                 clip at shade time, bins on a 32 px lattice of overlapping 64 px cells so that one cell's list
                 serves any frame tile, anchors to keep 16-bit lanes). Designed in the report; not built in
                 this card. Here a pan still rewrites the windows and bins of the records in view, and the GPU
                 still redraws the viewport.
                 Also not here: a per-column sweep rect for moving records (R3 as the rule), palette tables
                 through an `Ink` word (item 4), depth (a), (b), (d), the effects benchmark harness.
implication:     Passing establishes that the turn of a pan and of a hover is flat in the size of the world on
                 this scene, and that the measurement lane's 5,000-card world is admitted.
workloads:       tests/fx.rs; fxbench effects 200 / 1,000 / 5,000 (three repetitions).
gates:           S1 a scrolled, hovered scene with the declarations equals the same scene without them, a fresh
                    redraw, and the specification, over random pans (small, large, back), hovers, springs and
                    record rewrites, at a world tall enough that most records are never near the frame.
                 S2 mutants: `mutant_scroll_band` (a pan visits only the new viewport's buckets, not the old
                    one's) and `mutant_key_old` (a hover visits only the new id's records) must fail S1.
                 S3 all earlier gates; pbench A1 <= 1.03x of the 71ee3be baseline.
thresholds:      S-a scroll turn (`Scene::update` alone), 5,000 against 200 cards: within 1.2x (slice 4: 1.87 M ->
                    6.72 M from 200 to 4,000). Kill line: grows with cards.
                 S-b hover-above-glass turn, 5,000 against 200: within 1.2x (slice 4: 16.7 K -> 276 K), and under
                    10 K instructions.
                 S-c scroll and hover frame-thread instructions on Metal under the chooser: within 1.2x across
                    sizes.
                 S-d effects 5,000 runs every cell with 0 mismatching words.
measurement:     frame-thread instructions, three repetitions, no lock (instruction columns).
budget:          this session.
stop conditions: the declared-translation contract cannot be kept exact for a column whose windows also move by
                 other words (springs): fall back to the full pass for that column and record which.
do not revisit:  a wall-time chooser; growing pyramid levels to blocks.
```
