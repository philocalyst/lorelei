# Native PNG zgui motion on redraw-request ticks

Only virtual ticks with an observed increase in the cumulative redraw counter enter these distributions. The first cumulative value is excluded. These measurements use the same binary as the main reference captures.

| Cell | Observations | Redraw steps | Requests | Active p50/p95/p99 ms |
|---|---:|---:|---:|---:|
| zgui-1000-one | 480 | 263 | 263 | 0.144/11.777/14.200 |
| zgui-200-many | 480 | 263 | 263 | 33.062/84.023/142.364 |
| zgui-200-one | 480 | 263 | 263 | 0.085/2.243/3.882 |
| zgui-5000-one | 480 | 263 | 263 | 0.432/104.178/165.745 |
