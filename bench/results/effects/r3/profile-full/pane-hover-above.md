# Pane hover above glass across sparse sizes

Calling-thread instructions at `7991e7bd4e` with the profiler patch, 12 timed ticks per size. Corrected means subtract the measured counter cost. The uninstrumented median uses a separately timed trace with the same inputs.

| Calling-thread instructions per tick | 300 | 600 | 3,000 |
|---|---:|---:|---:|
| Turn and retarget | 29 | 29 | 28 |
| Block compare and window pass | 18,048 | 34,848 | 169,240 |
| Layer geometry and allocation | 29 | 29 | 28 |
| Build bins | 0 | 0 | 0 |
| Layer dependency schedule and damage | 650 | 650 | 648 |
| Prefix setup, fold and store | 11 | 11 | 10 |
| Tent downsample | 0 | 0 | 0 |
| Output setup, fold and store | 11,198,209 | 11,198,138 | 11,198,559 |
| Harness damage list and residual | 372 | 372 | 365 |
| Output rounding residual | 0 | 0 | 1 |
| **Corrected mean** | 11,217,348 | 11,234,077 | 11,368,879 |
| Uninstrumented median | 11,218,142 | 11,234,870 | 11,369,198 |

[Raw phase and reached-work counters](profile-table.tsv)
