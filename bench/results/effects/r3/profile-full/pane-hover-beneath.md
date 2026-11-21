| Calling-thread instructions per tick | 300 | 600 |
|---|---:|---:|
| Turn and retarget | 28 | 28 |
| Block compare and window pass | 20,508 | 37,308 |
| Layer geometry and allocation | 29 | 29 |
| Build bins | 0 | 0 |
| Layer dependency schedule and damage | 51,129 | 50,424 |
| Prefix setup, fold and store | 9,639,214 | 9,609,805 |
| Tent downsample | 2,070,068 | 2,060,552 |
| Output setup, fold and store | 32,271,201 | 32,196,020 |
| Harness damage list and residual | 583 | 584 |
| Output rounding residual | 0 | 1 |
| **Corrected mean** | 44,052,760 | 43,954,751 |
| Uninstrumented median | 43,925,538 | 43,941,565 |
