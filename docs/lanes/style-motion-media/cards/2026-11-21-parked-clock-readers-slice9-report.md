# Report: a clock reader that shows nothing is not in the active set (slice 9); slice 7 follow-ups

Card: `2026-11-21-parked-clock-readers-slice9.md` (frozen before the change). Raw output:
`bench/results/fx/slice9.txt`, `pbench-slice9.txt`. Dist, frame-thread instructions, three repetitions, no
bench lock, 0 mismatching words in all 18 runs.

## What was built

A record joins its column's active set only if it still depends on the clock and either its geometry reads
the clock or its window meets the frame (`Pass::shows`, decided in `window_pass` and `rewin_one` after the
window is known). Nothing wakes a parked record: the geometry passes that can bring it into view (whole
column, pan index, record rewrite) decide membership for every record they visit. `Scene::animating` now
means "a frame is owed".

Follow-ups to slice 7 done before this card (commit 2871770c0d):
* the CPU scroll turn at 600 dense, separated by measurement: 13.5 M at slice 6, 18.0 M at slice 7, 13.85 M
  with the certificate off. The certificate is now skipped on frames where more than a quarter of the
  records moved (the line the bins already draw between editing and rebuilding): 13.84 M, with the one-spring
  and glass-moves prefix counts unchanged (570 and 383). The remaining 2.5% over slice 6 is the lane and tile
  change lists the resident plan reads.
* `Gpu::staged`'s whole-frame job cache is deleted (the resident plan replaces it) and `mutant_full_stale`
  with it. No mutant is unseen and that path no longer exists.

## Gates

`cargo test --release -p bough-paint -j 8`: lib 2, exact 5, fx 11, review 8, scenes 2, all passed. The pan
gate now carries 40 endless paints through its 7,000 px world:

```
parked clock readers: 40 endless paints in the world, at most 18 records in an active set
scroll and hover by index: 260 records in the scene; a pan visited 22.5 records on average, a hover 8.9; 0 failing steps
```

Mutants: `mutant_park` (a reader in the lower half of the frame is parked) fails 5 tests of fx;
`mutant_park_wake` (an indexed pass does not put what it brought into view in the active set) fails the pan
gate. pbench A1: see `pbench-slice9.txt` (ratios in the commit message), 0 cross-check failures.

## Thresholds (200 / 1,000 / 5,000 cards)

| | card | measured | verdict |
|---|---|---|---|
| K-a one spring, Metal, within 1.2x; same records visited | slice 7: 214 / 239 / 322 K; 13 / 63 / 313 records | 205 / 202 / 202 K; 7 / 7 / 7 records; turn 10.8 K at every size | held |
| K-b palette fade with the shader paints present, turn within 1.2x | slice 8: 10.6 / 32.9 / 125 K | 8.2 / 8.1 / 8.1 K (6 records); Metal 215 / 217 / 217 K against theme 208 / 212 / 213 K | held |
| K-c all springs (not a target) | 275 / 430 / 1,150 K | 266 / 402 / 1,105 K; 31 / 131 / 631 records | unchanged, as predicted |

## Every cell (K, median of three; CPU kernel / Metal resident / chooser; 200, 1,000, 5,000)

| cell | 200 | 1,000 | 5,000 |
|---|---|---|---|
| idle | 9.2 / 6.0 / 3.1 | 9.2 / 6.0 / 3.1 | 9.1 / 6.0 / 3.1 |
| one spring | 35,051 / 205 / 200 | 35,051 / 202 / 204 | 35,051 / 202 / 199 |
| all springs | 53,550 / 266 / 269 | 53,681 / 402 / 403 | 54,383 / 1,105 / 1,106 |
| scroll 11 px | 992,424 / 1,479 / 1,494 | 992,631 / 1,613 / 1,614 | 992,716 / 1,859 / 1,865 |
| hover above glass | 6,083 / 190 / 190 | 6,083 / 191 / 188 | 6,083 / 192 / 188 |
| hover beneath glass | 41,966 / 191 / 189 | 41,965 / 192 / 189 | 41,965 / 195 / 189 |
| theme | 986,052 / 208 / 219 | 986,069 / 208 / 219 | 986,106 / 212 / 219 |
| glass moves 1 px | 17,778 / 227 / 221 | 17,778 / 224 / 223 | 17,784 / 227 / 222 |

With `PALETTE=1`: idle 9.1 / 6.0 / 3.1 at every size; theme 987,224-987,249 / 208-213 / 221-223; palette
switch 981,428-981,463 / 209-212 / 219-222; palette fade 981,499-981,538 / 215-217 / 229-230; hover above
glass 6,083 / 192-193 / 189-194; scroll 994,678-995,018 / 1,637-2,022 / 1,648-2,033.

## What still follows the scene

* **All springs**: every sprung dot in the world is visited (631 at 5,000 cards, about 40 of them in view),
  because its geometry reads the clock and it could move into view by itself. Parking it needs a bound on
  where the motion can go from its endpoints and law. No expression exposes one: it would be an additive
  `Expr` method (the hull of the value over all later time, defaulting to "unknown"), exact for `Spr` from
  its start, target and velocity. Not built.
* **Scroll**: the turn (0.65 to 0.72 M) and the uploads; scroll as a kernel uniform is not built.
* **Ranks** on Metal and the CPU's first check at depth: as in the slice 7 report; not touched here.

## Not verified

Dense and pane and media cells were not re-run for this slice (the change is in the active sets; the gates
cover them, the numbers are slice 7's). Vulkan. GPU time under the lock.
