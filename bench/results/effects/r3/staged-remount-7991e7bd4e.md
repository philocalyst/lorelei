# Fixed-size scene remount diagnostic

This CPU1 adapter uses the `bb116733c5` gate on paint `7991e7bd4e`. The gate checks observed changes at each resize step and compares retained pixels with a full redraw. It retains the fresh mount and independent image constraints. The scene dimensions alternate between 640×400 and 704×448. A fixed-size `Scene` requires rebuilding on every transition, so the retained-resize cell remains unavailable.

Successful rows killed both dropped-input and dropped-damage mutants. Each timed sample includes the remount and full surface shading. Counts alternate between 308 and 260 output tiles per tick.

| Cards | Remount p50/p95/p99 (ms) | Output frames / observations |
|---:|---:|---:|
| 200 | 73.483/117.762/144.097 | 240/240 |
| 1,000 | 235.911/489.383/535.408 | 240/240 |
| 5,000 | 685.033/825.706/912.011 | 240/240 |

[Executable and raw receipts](staged-remount)
