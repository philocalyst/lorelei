# baseline 71ee3be

commit `71ee3be0aa39d37c0fb3bc32997d4aed631e2cc2` · rustc 1.101.0-nightly (5d8fec066 2026-10-16)
host Apple M3 Ultra (32 cores), macOS 26.7.3
load 17.79 → 18.19 (ceiling 2.0: LOADED)

| cell | instructions (median [min–max]) | wall ms | RSS MiB |
|---|---:|---:|---:|
| sheet_x/rows1000 | 21,493,610 [21,451,083–21,676,829] | 9.6 [8.2–10.4] | 2.4 |
| sheet_x/rows10000 | 24,995,740 [24,943,127–25,014,250] | 9.1 [8.7–9.5] | 2.7 |
| events_x/rows10000 | 2,322,982,727 [2,322,738,204–2,323,117,669] | 193.3 [192.4–195.0] | 66.0 |
| text_x | 231,528,535 [231,472,290–231,773,191] | 23.2 [22.3–24.3] | 6.1 |
| anim_x/208 | 17,943,022 [17,870,053–18,115,259] | 9.9 [9.6–11.0] | 2.0 |
| pbench/canvas1 | 101,897,391,608 [101,897,192,594–101,903,041,433] | 4382.1 [4325.2–4871.0] | 260.9 |
| guibench/rows-1000-mount | 85,740,499 [85,733,173–86,910,154] | 29.5 [25.9–31.3] | 19.2 |
| guibench/rows-1000-hover | 246,073,991 [245,679,930–247,533,451] | 30.6 [30.5–35.1] | 19.3 |
| guibench/rows-1000-typing | 176,995,353 [176,333,802–177,385,685] | 26.6 [26.6–30.7] | 19.9 |
| guibench/search-100-filter | 132,757,753 [132,199,571–133,042,317] | 25.0 [24.2–29.4] | 18.2 |
| guibench/search-1000-filter-nochange | 137,184,464 [136,938,318–137,808,575] | 27.9 [23.8–30.1] | 20.0 |

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
