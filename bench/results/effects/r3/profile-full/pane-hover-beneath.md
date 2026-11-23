# Pane hover beneath glass across sparse sizes

Calling-thread instructions at `7991e7bd4e` with the profiler patch, 12 timed ticks per size. Corrected means subtract the measured counter cost. The uninstrumented median uses a separately timed trace with the same inputs.

| Calling-thread instructions per tick | 300 | 600 | 3,000 |
|---|---:|---:|---:|
| Turn and retarget | 28 | 28 | 27 |
| Block compare and window pass | 20,508 | 37,308 | 171,662 |
| Layer geometry and allocation | 29 | 29 | 28 |
| Build bins | 0 | 0 | 0 |
| Layer dependency schedule and damage | 51,129 | 50,424 | 50,313 |
| Prefix setup, fold and store | 9,639,214 | 9,609,805 | 9,611,250 |
| Tent downsample | 2,070,068 | 2,060,552 | 2,061,872 |
| Output setup, fold and store | 32,271,201 | 32,196,020 | 32,190,149 |
| Harness damage list and residual | 583 | 584 | 577 |
| Output rounding residual | 0 | 1 | 1 |
| **Corrected mean** | 44,052,760 | 43,954,751 | 44,085,879 |
| Uninstrumented median | 43,925,538 | 43,941,565 | 44,079,772 |

[Raw phase and reached-work counters](profile-table.tsv)
