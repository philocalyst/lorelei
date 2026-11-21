# Card: a clock reader that shows nothing is not in the active set (slice 9)

```
lane:            style-motion-media
state:           CARD
owner:           fx session (Claude, claude-fable-5-2), 2026-11-21
predecessor:     2871770c0d (slice 8 and the slice 7 follow-ups)
question:        Can a frame in which only the clock moved cost the records in motion that are in view, not
                 those in the world?
change:          A record joins its column's active set only if it still depends on the clock AND (its
                 geometry reads the clock, or its window meets the frame). A reader whose geometry the clock
                 does not move and whose window is outside the frame is **parked**: it is in no set and no
                 list. Nothing has to wake it: every way its window can come into view is a geometry change,
                 and every geometry pass (the whole column, the pan index, a record rewrite) already decides
                 membership for each record it visits. `Scene::animating` therefore means "a frame is owed":
                 motion nobody can see owes none. No ends are kept in time order: a parked one-shot that ends
                 off screen is simply found at rest when it is next visited.
what this is not: parking records whose geometry reads the clock (a sprung dot off screen is still visited:
                 it could move into view by itself). That needs a bound on where a motion can go from its
                 endpoints and law (R3), which no expression exposes today; the report says what it would take.
workloads:       tests/fx.rs; fxbench effects 200 / 1,000 / 5,000 (and PALETTE=1 for the fade cell).
gates:           K1 all gates (the pan-and-hover gate runs endless paints and one-shots through a 7,000 px
                    world; the retained == fresh == specification gates).
                 K2 a new check in the pan gate: after a pan brings a parked endless paint into view it is in
                    the active set and animates (retained == fresh on the following clock-only frames).
                 K3 mutant `mutant_park` (a reader in the lower half of the frame is parked) must fail.
thresholds:      K-a one spring, Metal frame-thread, 200 / 1,000 / 5,000: within 1.2x (slice 7: 214 / 239 /
                    322 K); records visited per frame the same at the three sizes.
                 K-b palette fade with the shader paints present: turn within 1.2x across sizes (slice 8:
                    10.6 / 32.9 / 125 K).
                 K-c all springs: NOT expected to change (275 / 430 / 1,150 K): recorded, not a target.
measurement:     frame-thread instructions, three repetitions, no lock.
budget:          this session.
do not revisit:  ends in time order for `animating` (not needed under this formulation).
```
