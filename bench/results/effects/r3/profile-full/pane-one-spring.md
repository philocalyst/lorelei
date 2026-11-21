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

[Raw phase and reached-work counters](profile-table.tsv)
