# Pane one spring across sparse sizes

Calling-thread instructions at `7991e7bd4e` with the profiler patch, 12 timed ticks per size. Corrected means subtract the measured counter cost. The uninstrumented median uses a separately timed trace with the same inputs.

| Calling-thread instructions per tick | 300 | 600 | 3,000 |
|---|---:|---:|---:|
| Turn and retarget | 35 | 35 | 34 |
| Block compare and window pass | 2,345 | 2,291 | 2,312 |
| Layer geometry and allocation | 29 | 29 | 28 |
| Build bins | 285,451 | 328,946 | 566,002 |
| Layer dependency schedule and damage | 690 | 690 | 688 |
| Prefix setup, fold and store | 11 | 11 | 10 |
| Tent downsample | 0 | 0 | 0 |
| Output setup, fold and store | 1,277,738 | 1,279,632 | 1,277,630 |
| Harness damage list and residual | 335 | 335 | 328 |
| Output rounding residual | 1 | 0 | 1 |
| **Corrected mean** | 1,566,635 | 1,611,969 | 1,847,033 |
| Uninstrumented median | 2,000,588 | 2,044,087 | 2,281,677 |
| Uninstrumented mean (rounded) | 1,568,200 | 1,613,600 | 1,851,300 |

`Bins::build` runs once each tick. The count pass grows from 123,429 to 286,643 instructions and the fill pass from 141,820 to 259,164. Together they contribute 280,558 additional instructions. Clear and offsets save 7. Each pass visits the same mean 4,325.17 tile memberships and the output still stores 4,126.17 list entries. The ordered record scans grow from 379 to 3,754 entries, with 2,464 in-frame records at 3,000 cards. This isolates the slope to walking the record arrays on each rebuild.

The 12-tick trace spans both four-tile and smaller repairs. Its mean is lower than its median. Compare the phase sum with the matching uninstrumented mean. The differences at 300, 600 and 3,000 are −0.10%, −0.10% and −0.23%. The outer measurement also brackets the wall-clock calls and one counter read. Those costs matter in idle ticks of only a few thousand instructions. Use the stock idle counts to check that records, tiles and layer jobs stay at zero.

[Raw phase and reached-work counters](profile-table.tsv)
