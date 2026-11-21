# baseline 71ee3be

commit `71ee3be0aa39d37c0fb3bc32997d4aed631e2cc2` · rustc 1.101.0-nightly (5d8fec066 2026-10-16)
host Apple M3 Ultra (32 cores), macOS 26.7.3
load 19.20 → 17.17 (ceiling 2.0: LOADED)

| cell | instructions (median [min–max]) | wall ms | RSS MiB |
|---|---:|---:|---:|
| sheet_x/rows1000 | 21,414,658 [21,354,466–22,882,871] | 14.1 [8.2–27.1] | 2.3 |
| sheet_x/rows10000 | 25,026,131 [24,930,362–25,033,239] | 8.5 [8.4–9.7] | 2.7 |
| events_x/rows10000 | 2,322,717,289 [2,322,692,693–2,323,889,480] | 188.5 [186.6–208.0] | 66.0 |
| text_x | 231,553,282 [231,440,419–232,972,117] | 23.7 [23.4–39.2] | 6.1 |
| anim_x/208 | 17,923,255 [17,852,666–18,963,367] | 9.3 [8.9–33.1] | 2.0 |
| pbench/canvas1 | 101,888,267,149 [101,884,349,233–101,894,160,238] | 4216.5 [4202.6–4276.8] | 259.3 |
| guibench/rows-1000-mount | 85,450,645 [85,327,608–92,239,906] | 35.1 [33.2–58.3] | 19.2 |
| guibench/rows-1000-hover | 246,313,512 [245,624,161–248,606,830] | 36.7 [35.6–36.8] | 19.3 |
| guibench/rows-1000-typing | 177,056,885 [177,015,894–177,096,041] | 31.0 [30.6–31.1] | 19.9 |
| guibench/search-100-filter | 133,014,444 [132,930,708–133,014,771] | 27.2 [26.4–28.5] | 18.1 |
| guibench/search-1000-filter-nochange | 136,573,577 [136,497,114–137,097,902] | 27.0 [25.9–28.7] | 20.0 |

## Slots (scenes other agents are building)

* `rows`: bough-guibench headless, rows (measured below as guibench/rows-*)
* `virtual`: not yet available: bough's virtual list cells are gui-bench's `virtual` scenario on Linux
* `table`: bough-guibench headless table cells (measured below as guibench/table-*)
* `typing`: bough-guibench headless typing (measured below)
* `search`: bough-guibench headless filter (measured below)
* `spinner`: not yet available: needs the output-paced animation driver (REPORT 4.8)
* `ticker`: not yet available: same clock as spinner
* `resize`: not yet available on the new core; the façade's resize is the gui-bench VM cell
* `sheet`: bough-x sheet_x example (measured below)
* `chat`: not yet available: the element-layer chat scene (REPORT 4.1)
* `ide`: not yet available: the element-layer ide scene (REPORT 4.1)
* `dashboard`: not yet available: the element-layer dashboard scene (REPORT 4.1)
* `canvas`: bough-paint pbench canvas (measured below)
* `effects`: not yet available: the effects/motion lane's scene (slot reserved)
* `media`: not yet available: the video/image scene (slot reserved)

## Known failures (not cells)

* guibench search --rows 1000 --op filter: panics after ~40 filter steps in bough-mem's region layout: column `glyph_x` is full (capacity 524288, need 524304). The 100-row filter and the 1000-row filter-nochange cells are measured instead.
