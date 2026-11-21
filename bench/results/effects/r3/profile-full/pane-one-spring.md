| Calling-thread instructions per tick | 300 | 600 |
|---|---:|---:|
| Turn and retarget | 35 | 35 |
| Block compare and window pass | 2,345 | 2,291 |
| Layer geometry and allocation | 29 | 29 |
| Build bins | 285,451 | 328,946 |
| Layer dependency schedule and damage | 690 | 690 |
| Prefix setup, fold and store | 11 | 11 |
| Tent downsample | 0 | 0 |
| Output setup, fold and store | 1,277,738 | 1,279,632 |
| Harness damage list and residual | 335 | 335 |
| Output rounding residual | 1 | 0 |
| **Corrected mean** | 1,566,635 | 1,611,969 |
| Uninstrumented median | 2,000,588 | 2,044,087 |
