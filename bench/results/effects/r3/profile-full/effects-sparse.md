# Sparse effects phase attribution

Calling-thread instructions per virtual tick at `7991e7bd4e`. The stock effects script advances shader time during spring motion. The profiler uses 12 timed ticks after eight warmup ticks. Each bracket mean subtracts calibrated probe instructions and reconciles across disjoint phases. The uninstrumented median uses the same trace.

## one spring

| Calling-thread instructions per tick | 300 | 600 |
|---|---:|---:|
| Turn and retarget | 34 | 34 |
| Block compare and window pass | 11,742 | 14,963 |
| Layer geometry and allocation | 28 | 28 |
| Build bins | 323,425 | 381,514 |
| Layer dependency schedule and damage | 74,552 | 82,885 |
| Prefix setup, fold and store | 7,397,056 | 7,398,379 |
| Tent downsample | 1,669,538 | 1,669,868 |
| Output setup, fold and store | 58,893,616 | 58,833,436 |
| Harness damage list and residual | 1,093 | 1,093 |
| Output rounding residual | 3 | 3 |
| **Corrected mean** | 68,371,087 | 68,382,203 |
| Uninstrumented median | 68,736,608 | 68,802,383 |

## hover above glass

| Calling-thread instructions per tick | 300 | 600 |
|---|---:|---:|
| Turn and retarget | 28 | 28 |
| Block compare and window pass | 20,464 | 39,789 |
| Layer geometry and allocation | 28 | 28 |
| Build bins | 0 | 0 |
| Layer dependency schedule and damage | 10,127 | 16,894 |
| Prefix setup, fold and store | 10 | 10 |
| Tent downsample | 0 | 0 |
| Output setup, fold and store | 11,200,880 | 11,206,749 |
| Harness damage list and residual | 365 | 365 |
| Output rounding residual | 0 | 1 |
| **Corrected mean** | 11,231,902 | 11,263,864 |
| Uninstrumented median | 11,232,079 | 11,257,471 |

## hover beneath glass

| Calling-thread instructions per tick | 300 | 600 |
|---|---:|---:|
| Turn and retarget | 27 | 27 |
| Block compare and window pass | 24,737 | 44,636 |
| Layer geometry and allocation | 28 | 28 |
| Build bins | 0 | 0 |
| Layer dependency schedule and damage | 154,116 | 161,119 |
| Prefix setup, fold and store | 26,127,993 | 26,134,588 |
| Tent downsample | 4,763,509 | 4,764,667 |
| Output setup, fold and store | 54,488,384 | 54,466,944 |
| Harness damage list and residual | 761 | 761 |
| Output rounding residual | 0 | 0 |
| **Corrected mean** | 85,559,555 | 85,572,770 |
| Uninstrumented median | 85,529,386 | 85,548,277 |

[Raw profiles and reached-work counters](profile-table.tsv)
