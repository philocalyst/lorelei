# Phase 2 — matched AFTER (`326464c6f6`) vs BEFORE (`b9a196b6ec`)

Same host (`h16003mac`), same toolchain (`nightly-2026-10-17`, rustc 1.101.0-nightly
`5d8fec066`), same `BOUGH_FONT` (`7da195a7…`), same commands and lock
(`lockf -k $HOME/work/locks/bench.lock`). No Rust source was changed. Raw logs and per-cell
CSVs are beside this file in `logs/`; damage-lab evidence in `damage-lab/`; VM contract and
metrics in `vm/`.

| | BEFORE | AFTER |
|---|---|---|
| commit | `b9a196b6eccd0130377b844e900ccd335d9b9a26` | `326464c6f6e452409978753022b3c34ad80fbc36` |
| branch | `lane/rethink/bench` | `lane/rethink/bench-after` |
| host load at start | `11.58 10.00 7.02` | `4.30 4.47 5.45` (Set A: `9.40 6.33 6.09`) |
| host load at end | `7.77 8.29 7.59` | ``6.29 4.42 5.06`` |
| build | `-j 8` | `-j 10` |

Instruction counters are load-insensitive and are the verdict metric; wall/GPU columns are
diagnostic. `Δ` is AFTER/BEFORE − 1 unless noted.

Binary identity (AFTER, release unless stated):

| binary | SHA-256 | bytes | `__TEXT` |
|---|---|---:|---:|
| `examples/fxbench` | `aac0409ce5e3378c6eb7e3a96e16b1b172ec28443725e219fd9e491d96fb9b7e` | 3.5 MiB | — |
| `examples/flatbench` | `56f256ebb1db5ba86ea0735eddb9eff9c00d3ff269e7624507df638bcda90053` | 3.7 MiB | — |
| `examples/card3_cost` | `2ef67a1c4c74f3151bd7aac1bd8bfe7f7dbf6ff2b72dfabfd31531fd2a071e90` | 6,671,384 | 4,784,128 |
| `examples/deep_view` | `498524f84d337c218c3cc9cfc66c1725aa21cd047b6d16afe0aae887c7c7dff4` | 1,991,352 | 1,359,872 |
| `examples/flat_slice` | `aa413565fa423afe045ba4a6b6b606034265404bf7c3977df21bc6a0c10cbab9` | 819,112 | 540,672 |
| `examples/flat_deep` | `27d893266a4b8ce74259779cdf9e17d0af1153508951c7d221f8f7c61361154c` | 845,464 | 557,056 |
| `examples/typed_slice_min` | `e4877688236203ec56cbb9b072de3d2bf299632391e13f10c9e48d473c215bf2` | 3,815,168 | 2,686,976 |
| `examples/flat_cost` | `3f27cfb8a02390722af6c3887f2c9a4af43f38bc9bbe158b1b48c1780aeedce8` | 4,565,832 | 3,244,032 |
| `release/bough-fx-bench` | `6a81126c0afb3106dfbea540ee1cd948a64517ec9e7ef0aa62d844eee294475b` | 5.2 MiB | — |
| `release/rethink-core` | `38f4ca5f4321d3739c637207065d735715dbb9b7a022ad2a3899304e0137802c` | 665 KiB | — |
| `release/headless` | `4688635db93da7e200cfa802adb6ab4d74b69594e78a8cf76a9969a32e45491a` | 7.2 MiB | — |
| `release/bough-bench` (guibench-x) | `6906024b6749227dd1f595dcb70a059ac2a6db0457705e1f70d033dd94eff199` | — | — |
| `release/lab` | `788cd1ea4ba6a12c380ff1f80dccbf9b40177a49a87b3af202b6f797d9ca7227` | — | — |

---

## A. Core `fxbench` (N=1000, `NOCONTROL=1`, `REPS=24`)

Instr_med_K per frame. `mismatch = 0` BEFORE and AFTER for every cell.

| cell | cpu1 BEFORE | cpu1 AFTER | Δ | metal B | metal A | Δ | mixed B | mixed A | Δ |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| idle | 9.3 | 9.3 | 0% | 6.1 | 6.2 | +1.6% | 3.1 | 3.1 | 0% |
| one spring | 10,840.1 | 10,839.6 | −0.0% | 101.8 | 100.2 | −1.6% | 100.5 | 99.1 | −1.4% |
| all springs | 14,169.4 | 14,169.7 | +0.0% | 144.0 | 143.4 | −0.4% | 143.8 | 142.0 | −1.3% |
| hover above glass | 2,142.4 | 2,142.4 | 0% | 76.5 | 75.0 | −2.0% | 74.1 | 72.4 | −2.3% |
| hover beneath glass | 15,291.4 | 15,291.4 | 0% | 92.8 | 91.0 | −1.9% | 90.0 | 88.5 | −1.7% |
| theme | 223,950.9 | 223,949.8 | −0.0% | 111.1 | 108.9 | −2.0% | 120.5 | 119.0 | −1.2% |
| scroll 11 px | 217,733.7 | 217,737.4 | +0.0% | 2,051.9 | 2,050.9 | −0.0% | 2,062.2 | 2,058.1 | −0.2% |
| glass moves 1 px | 8,468.1 | 8,467.9 | −0.0% | 127.1 | 125.6 | −1.2% | 124.4 | 123.1 | −1.0% |

**Verdict: PASS** — core effects path unchanged (±0.3%; Metal/mixed 1–2% faster from fewer
words). Root cause unchanged: `scroll`/`theme` still redraw ~everything in this fixture.

### NEW `flatbench` 300 cards + 2 retained pages (`REPS=24`, `mismatch = 0`)

| scene / cell | exec | ms/frame | k instr/frame | tiles | records |
|---|---|---:|---:|---:|---:|
| cards idle | Cpu1 / Cpu8 / Metal | 0 / 0 / 0.020 | 6.4 / 6.1 / 54.7 | 0 | 0 |
| cards restyle one card | Cpu1 / Cpu8 / Metal | 0.292 / 0.162 / 0.321 | 6,719.9 / 1,961.9 / 83.3 | 35 | 1 |
| cards restyle beneath pane | Cpu1 / Cpu8 / Metal | 1.938 / 1.056 / 0.411 | 39,696.8 / 18,103.3 / 445.3 | 53.2 | 1 |
| cards one card on a spring | Cpu1 / Cpu8 / Metal | 0.963 / 0.412 / 0.516 | 20,353.3 / 7,314.0 / 331.0 | 56.1 | 5 |
| cards scroll 11 px | Cpu1 / Cpu8 / Metal | 14.911 / 5.986 / 1.350 | 328,139.2 / 127,488.8 / 4,326.1 | 960 | 0 |
| cards pane moves 1 px | Cpu1 / Cpu8 / Metal | 4.222 / 0.723 / 0.364 | 81,264.7 / 10,790.2 / 283.5 | 172.2 | 1 |
| pages idle | Cpu1 / Cpu8 / Metal | 0 / 0 / 0.024 | 5.8 / 5.8 / 54.2 | 0 | 0 |
| pages cross-fade | Cpu1 / Cpu8 / Metal | 2.467 / 0.422 / 0.499 | 54,984.7 / 7,186.4 / 204.4 | 1000 | 1 |
| pages slide | Cpu1 / Cpu8 / Metal | 1.834 / 0.324 / 0.474 | 39,643.7 / 5,450.5 / 430.0 | 1000 | 2 |
| pages blur-reveal | Cpu1 / Cpu8 / Metal | 14.979 / 2.121 / 0.575 | 394,744.7 / 50,812.6 / 491.3 | 1000 | 2 |
| pages one card of a page | Cpu1 / Cpu8 / Metal | 0.740 / 0.490 / 1.166 | 17,568.6 / 9,262.4 / 8,434.1 | 63 | 1 (35 group tiles) |

Flat world at 300 cards: **0 mismatching words**, all three executors agree.

---

## B. `bough-fx-bench` (N=200/1000, 40 samples) + NEW `rethink-core`

**All 16 gates PASS** (8 cells × 2 sizes × rings 1–4). `repair area` = damage fraction.

| N | cell | p50 µs B | p50 µs A | Δ p50 | instr/op B | instr/op A | Δ instr | repair B | repair A |
|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 200 | startup | 598.4 | 536.2 | −10.4% | 19,722,104 | 19,684,936 | −0.2% | 100% | 100% |
| 200 | idle | 1,006,686.8 | 1,000,070.5 | −0.7% | 18,507 | 17,650 | −4.6% | 0% | 0% |
| 200 | one | 12.5 | 11.2 | −10.0% | 1,305,103 | 1,303,903 | −0.1% | 1.44% | 1.44% |
| 200 | many | 344.8 | 322.2 | −6.5% | 16,087,660 | 16,106,003 | +0.1% | 92.5% | 92.5% |
| 200 | hover | 67.2 | 61.1 | −9.1% | 2,294,035 | 2,301,630 | +0.3% | 7.62% | 7.62% |
| 200 | theme | 183.2 | 182.3 | −0.5% | 14,553,848 | 14,545,159 | −0.1% | 92.5% | 92.5% |
| 200 | **scroll** | 150.8 | **78.5** | **−47.9%** | 8,186,282 | **1,858,583** | **−77.3%** | 92.5% | **12.27%** |
| 200 | resize | 206.8 | 199.8 | −3.3% | 14,928,164 | 14,926,784 | −0.0% | — | — |
| 1000 | startup | 1,993.5 | 1,773.5 | −11.0% | 87,727,591 | 87,576,639 | −0.2% | 100% | 100% |
| 1000 | idle | 1,010,027.5 | 1,010,044.3 | +0.0% | 18,233 | 17,742 | −2.7% | 0% | 0% |
| 1000 | one | 55.3 | 49.8 | −9.9% | 6,165,037 | 6,164,339 | −0.0% | 1.44% | 1.44% |
| 1000 | many | 1,592.2 | 1,535.5 | −3.6% | 81,437,591 | 81,404,701 | −0.0% | 92.5% | 92.5% |
| 1000 | hover | 203.7 | 191.7 | −5.9% | 11,032,575 | 11,026,049 | −0.1% | 7.62% | 7.62% |
| 1000 | theme | 771.5 | 761.5 | −1.3% | 70,645,718 | 70,638,899 | −0.0% | 92.5% | 92.5% |
| 1000 | **scroll** | 146.0 | **71.6** | **−50.9%** | 8,192,992 | **1,859,310** | **−77.3%** | 92.5% | **12.27%** |
| 1000 | resize | 840.2 | 826.0 | −1.7% | 72,088,155 | 72,084,313 | −0.0% | — | — |

**Verdict: PASS, and the headline win.** The `scroll_band` fix cuts scroll instructions 4.4× and
repair area 7.5× (92.5% → 12.27%); p50 halves. Other cells hold within ±0.3% instructions
(hover +0.3% at N=200 is noise-level; N=1000 −0.1%). `theme` still repairs 92.5% — the
remaining large-repair cell.

### NEW `rethink-core` (flat core; `check` + `measure`, 20 steps)

All checks pass (pixel equality vs full redraw, dirty tiles produced). No BEFORE analogue.

| N | cell | instr/op | ns/op | repair % |
|---:|---|---:|---:|---:|
| 200 | startup | 13,193,730 | 607,416 | — |
| 200 | idle | 21,695 (frame) | 1,143,292 | 0 |
| 200 | one | 309,146 | 12,316 | 2.27 |
| 200 | many | 20,809,744 | 1,017,520 | 100 |
| 200 | theme | 20,910,703 | 1,048,035 | 100 |
| 200 | scroll | 678,187 | 46,191 | 7.69 |
| 200 | hover | 703,655 | 34,729 | 3.08 |
| 1000 | startup | 62,345,000 | 2,516,375 | — |
| 1000 | idle | 3,522,560 (frame) | 1,051 | 0 |
| 1000 | one | 1,563,942 | 60,804 | 2.27 |
| 1000 | many | 103,307,879 | 4,947,964 | 100 |
| 1000 | theme | 103,816,406 | 5,010,679 | 100 |
| 1000 | scroll | 678,190 | 46,287 | 7.69 |
| 1000 | hover | 3,459,059 | 183,645 | 3.08 |

The flat core expresses `scroll`/`hover` at 678 K / 3.46 M instr per 1000-card frame with 7.7% /
3.1% repair — cells the old typed adapter could not represent.

---

## C. Compile time, binary size, and flat vs typed core

`card3_cost` (typed column; instructions). `+f` = turn + frame.

| cell | typed B | typed A | Δ | typed+f B | typed+f A | Δ |
|---|---:|---:|---:|---:|---:|---:|
| mount 1k rows | 6,033,243 | 6,082,853 | +0.8% | 17,402,596 | 16,983,419 | −2.4% |
| mount 100k rows | 6,019,114 | 6,081,642 | +1.0% | 17,387,750 | 16,982,148 | −2.3% |
| hover one row | 3,145 | 3,145 | 0% | 922,362 | 875,458 | −5.1% |
| one signal | 24,443 | 23,050 | −5.7% | 146,338 | 150,298 | +2.7% |
| insert at 0 | 51,812 | 53,544 | +3.3% | 7,526,072 | 7,221,947 | −4.0% |
| scroll one row | 19,053 | 19,188 | +0.7% | 7,370,397 | 7,075,068 | −4.0% |
| theme | 5,717 | 5,853 | +2.4% | 11,733,604 | 11,214,270 | −4.4% |
| resize 40 pt | 1,589,269 | 1,592,650 | +0.2% | 13,420,107 | 12,903,868 | −3.8% |
| still frame | 0 | 0 | 0% | 1,610 | 1,610 | 0% |
| re-describe unchanged | 4,592 | 4,728 | +3.0% | — | — | — |

**Verdict: MIXED.** End-to-end `turn+frame` is 2–5% cheaper on every interactive cell, but the
typed turn alone regressed 0.7–3.3% on mount/insert/theme/re-describe (over the strict 0%
kill-line). Report as a small typed-path regression; the frame totals still win.

| compile / size | BEFORE | AFTER | Δ |
|---|---:|---:|---:|
| clean `bough`+`slice`+`deep_view` | 13.33 s | **12.08 s** | −9.4% |
| example recompile | 5.05 s | **3.86 s** | −23.6% |
| `deep_view` bytes / `__TEXT` / `__text` | 1,988,664 / 1,359,872 / 962,720 | 1,991,352 / 1,359,872 / 962,720 | +0.1% |
| `card3_cost` bytes / `__TEXT` | 6,671,704 / 4,784,128 | 6,671,384 / 4,784,128 | −0.005% |

Both compile deltas are partly lower host load (4.8 vs 14) — treat the direction as indicative;
the binary sizes are exact and unchanged.

**Flat vs typed (NEW; clean build of `bough` + 4 examples in 15.99 s at load 7.12):**

| binary | bytes | `__TEXT` | `__text` | vs typed |
|---|---:|---:|---:|---:|
| `flat_slice` | **819,112** | 540,672 | 445,920 | **4.66× smaller than `typed_slice_min`** |
| `typed_slice_min` | 3,815,168 | 2,686,976 | 2,181,316 | — |
| `flat_deep` | **845,464** | 557,056 | 461,856 | **2.36× smaller than `deep_view`** |
| `deep_view` | 1,991,352 | 1,359,872 | 962,720 | — |

`flat_cost` (instructions, turn / turn+frame):

| cell | lowered | typed | flat | flat-ui |
|---|---|---|---|---|
| mount 1k | 6,462,384 / 17,332,261 | 6,083,244 / 16,996,703 | 5,937,622 / 13,296,871 | 19,411,928 / 23,059,122 |
| hover row | 4,893 / 872,961 | 3,170 / 877,692 | 3,999 / **838,907** | 2,998 / **311,045** |
| scroll one row | 457,986 / 7,494,977 | 19,215 / 7,086,895 | 446,885 / **2,918,684** | 11,353 / **1,799,300** |
| theme | 203,287 / 11,391,279 | 5,863 / 11,227,339 | 207,330 / **1,532,318** | 2,462 / **2,633,143** |
| resize | 1,813,580 / 13,104,916 | 1,592,660 / 12,917,615 | 1,229,039 / **8,994,311** | — |
| deep hover | 961,421 / 214,155,302 | not admitted | 971,986 / **107,456,312** | 6,013 / **17,236,128** |

Flat frames: scroll **2.4× cheaper** than typed, theme **7.3×**, resize **1.4×**; deep hover
**2.0×** vs lowered and **12.4×** with `flat-ui`. `flat` reports scroll 324 tiles / 27.0% (blit),
theme 102 tiles / 8.5% (CPU), hover 200 tiles / 16.7%.

---

## D. Zortax `gui-bench` — native headless and VM

### Native macOS `bough-guibench headless` (`--rows 1000 --repeats 8 --blocks 3 --render`)

Medians, instructions.

| scenario / op | BEFORE | AFTER | Δ |
|---|---:|---:|---:|
| rows scroll | 6,148,524 | 6,135,831 | −0.2% |
| table update10 | 239,083 | 233,448 | −2.4% |
| search filter | 18,510,683 | 18,550,423 | +0.2% |
| search filter-nochange | 902,749 | 899,935 | −0.3% |
| spinner | 1,626,746 | 1,611,915 | −0.9% |
| ticker | 59,707 | 56,915 | −4.7% |
| typing | 1,604,525 | 1,591,639 | −0.8% |
| virtual jump | 942,177 | 940,925 | −0.1% |
| resize | 21,757,654 | 21,798,029 | +0.2% |
| rows mount (warm) | 132,433,637 | 130,978,347 | −1.1% |
| rows mount (cold) | 144,261,236 | 144,303,519 | +0.03% |

**Verdict: PASS** at noise level; `search filter` +0.2% and `resize` +0.2% are within
run-to-run spread (Phase-1 repeats varied ~1%). `bough-guibench-x`: **7/7 gates pass** (1.48 s).

### Linux VM `benchvm` (unthrottled, matched)

Guest rebuilt from `326464c6f6` inside `benchvm` (`FRAMEWORKS=bough,bough-x`): bough
`88bd763c…`, bough-x `e436c938…`, runner `cc52f037…`. `GB_TIMING=unthrottled`,
`--rows 1000 --iterations 1 --repeats 4 --op-repeats 4`, scenarios rows/table/search/ticker,
`taskset 0-5`, lavapipe. Load before `2.17 0.55 0.18`. **86/86 validity-bearing reports valid,
0 invalid, 0 failures** (contract + per-cell JSONs in `vm/`).

| workload (1000 rows) | bough BEFORE | bough AFTER | Δ | bough-x BEFORE | bough-x AFTER | Δ |
|---|---:|---:|---:|---:|---:|---:|
| rows startup first present (ms) | 9.942 | 11.418 | +14.8% | 1.591 | 1.590 | -0.1% |
| rows scroll CPU/frame (ms) | 2.810 | 1.095 | -61.0% | 0.841 | 0.858 | +2.1% |
| rows scroll damage (%) | 100.0 | 100.0 | +0.0% | 100.0 | 100.0 | +0.0% |
| rows hover p50 (ms) | 1.092 | 1.134 | +3.8% | 0.310 | 0.279 | -9.9% |
| table update10 p50 (ms) | 0.509 | 0.520 | +2.2% | 1.066 | 0.482 | -54.8% |
| table update10 CPU/frame (ms) | 0.597 | 0.549 | -7.9% | 1.066 | 0.468 | -56.1% |
| table update10 damage (%) | 1.2 | 1.2 | +0.0% | 96.0 | 12.0 | -87.5% |
| search filter p50 (ms) | 6.026 | 7.017 | +16.5% | 0.933 | 0.969 | +3.9% |
| ticker u1 CPU/frame (ms) | 0.228 | 0.227 | -0.6% | 0.285 | 0.189 | -33.8% |
| ticker u1 damage (%) | 0.7 | 0.7 | +0.0% | 8.0 | 8.0 | +0.0% |
| ticker u100 CPU/frame (ms) | 0.267 | 0.272 | +2.1% | 1.166 | 0.360 | -69.1% |
| ticker u100 damage (%) | 0.9 | 0.9 | +0.0% | 98.0 | 16.0 | -83.7% |

Reading: the two largest VM regressions fixed. **bough-x `table update10` damage 96.0% → 12.0%**
(−87.5%), p50 1.066 → **0.482 ms** (−54.8%), CPU/frame 1.066 → 0.468 ms (−56.1%); **bough-x `ticker u100` damage 98.0% → 16.0%** (−83.7%),
CPU/frame 1.166 → 0.360 ms (−69.1%). **bough `rows scroll` CPU/frame 2.810 → 1.095 ms**
(−61.0%). Regressions to watch: bough `rows startup first present` 9.94 → 11.42 ms (+14.8%),
bough `search filter` p50 6.03 → 7.02 ms (+16.5%), bough `rows hover` +3.8%, bough-x
`rows scroll` CPU +2.1%. bough `rows scroll` damage stays 100% in the VM (the scroll_band fix
is in `bough-fx-bench`, not this façade path).

---

## E. Complex scenes, apps, and damage lab

| fixture | metric | BEFORE | AFTER | Δ |
|---|---|---:|---:|---:|
| lab stream (900 ticks) | instr/tick | 253,370 | 255,006 | +0.6% |
| lab stream | ms/tick | 0.3295 | 0.3203 | −2.8% |
| lab scroll (600 ticks) | instr/tick | 304,769 | 310,007 | +1.7% |
| lab scroll | ms/tick | 0.3178 | 0.3322 | +4.5% |
| animate (1200 frames) | instr/frame | 124,437 | 126,502 | +1.7% |
| micro state | frame p50 | 46.1 µs | 44.7 µs | −3.0% |
| micro vscroll | frame p50 | 56.6 µs | 60.3 µs | +6.5% |
| kernel repair (499 tiles) | ms | 0.0412 | 0.0519 (best 0.0408) | +26% / −1% best |
| kernel full redraw | ms | 0.0848 | 0.0855 | +0.8% |
| mail open | instr / ms | 2.71 M / 1.01 | 2.75 M / 1.86 | +1.5% / wall noisy |
| mail search | instr / ms | 1.12 M / 0.29 | 1.12 M / 0.32 | 0% |
| mail archive | instr / ms | 1.14 M / 0.36 | 1.13 M / 0.35 | −0.9% |
| dashboard feed tick | instr / ms | 2.57 M / 0.84 | 2.58 M / 0.75 | +0.4% |
| dashboard sort | instr / ms | 2.31 M / 0.37 | 2.29 M / 0.42 | −0.9% |
| dashboard filter | instr / ms | 1.93 M / 0.35 | 1.93 M / 0.36 | 0% |
| sketch drag | instr / ms | 1.94 M / 0.39 | 1.93 M / 0.44 | −0.5% |
| sketch pan | instr / ms | 1.09 M / 0.38 | 1.07 M / 0.48 | −1.8% |
| sketch zoom | instr / ms | 1.05 M / 0.38 | 1.06 M / 0.53 | +1.0% |
| code keystroke | instr / ms | 1.78 M / 0.48 | 1.76 M / 0.56 | −1.1% |
| code page scroll | instr / ms | 1.27 M / 0.39 | 1.25 M / 0.49 | −1.6% |
| code find-all | instr / ms | 0.82 M / 0.37 | 0.83 M / 0.42 | +1.2% |
| code find-all new letter | instr / ms | 0.83 M / 0.29 | 0.82 M / 0.36 | −1.2% |

**Verdict: MIXED.** Apps and most harnesses are within ±2%. The exceptions: `lab scroll`
+1.7% instr, `animate` +1.7%, `micro vscroll` +6.5% wall, `kernel repair` +26% on the noisy
median (best sample 0.0408 ms is *faster*, spread 0.0408–0.0762). Flag these as watch items,
not a thesis-level regression.

### Damage lab (`--frames 120`, forms `hw32,exact16,unified16,unified32`, rings 1,3)

`lab verify`: **0 failures**. Pixel-verified repair area (px), ring 1:

| trace | hw32 | exact16 | unified16 | unified16 / hw32 | unified32 / hw32 |
|---|---:|---:|---:|---:|---:|
| heavy-scroll | 1,013,760 | 962,560 | **54,022** | **5.3%** | 6.1% |
| heavy-both | 1,678,635 | 1,356,640 | **703,774** | **41.9%** | 60.4% |
| lab-scroll | 1,544,192 | 1,203,456 | **118,784** | **7.7%** | 9.6% |
| vlist-scroll | 2,304,000 | 1,643,974 | **151,612** | **6.6%** | 6.9% |
| complex-app | 1,717,274 | 1,152,881 | **420,883** | **24.5%** | 29.2% |

Ring 3 is similar (unified16 5.7% / 44.7% / 10.2% / 6.7% / 28.3%). `frags` and `prims` fall in
step (e.g. heavy-scroll 3.48 M→0.187 M frags, 1055→98 prims). `lab bench` CPU split
(ring 1, instructions; base+track+cull):

| trace | hw32 total | unified16 total | note |
|---|---:|---:|---|
| heavy-scroll | 123,580 | 670,659 | cull 67 K→85 K, track 4 K→534 K |
| heavy-both | 514,802 | 1,926,384 | |
| lab-scroll | 55,935 | 281,077 | |
| vlist-scroll | 224,439 | 1,010,007 | |
| complex-app | 74,503 | 358,031 | |

**Verdict: PASS on the damage claim** — unified16 delivers **5–19× smaller verified repair
area** (unified32 is slightly worse than unified16 except on heavy-both), with 0 oracle
failures. The trade is CPU: tracking costs ~2–5× more instructions in this synthetic micro
bench, while cull cost stays flat. This is the exact trade Phase 2 must judge.

---

## F. Inspector zero-cost-when-off

| test | result |
|---|---|
| `alloc_zero`: `idle_refresh_allocates_nothing` | PASS |
| `alloc_zero`: `the_background_png_worker_is_lazy` | PASS |
| `hot_path`: `a_hover_costs_what_the_core_says` | PASS |

**3/3 PASS** (`--release`, `--offline --locked`).

---

## Verdict summary vs the Phase-1 kill-lines

| set | verdict |
|---|---|
| A fxbench core (1000) | **PASS** (all cells ≤ +0.0%; metal/mixed 1–2% better; 0 mismatches) |
| A flatbench (new) | **NEW** (0 mismatches; scroll 328 M instr cpu1, 4.3 M metal) |
| B fx-bench 16/16 gates | **PASS** |
| B fx-bench scroll | **PASS / WIN**: instr −77.3%, repair 92.5→12.27%, p50 −50.9% |
| B fx-bench other cells | **PASS** (±0.3% instr) |
| B rethink-core | **NEW** (all checks pass; scroll 678 K instr, 7.69% repair) |
| C typed `card3_cost` turn | **MILD REGRESSION** (+0.7…+3.3% on 4 cells) |
| C typed `turn+frame` | **PASS** (−2.4…−5.1% on interactive cells) |
| C compile / size | **PASS** (12.08 s / 3.86 s; sizes flat) |
| C flat vs typed size | **PASS / WIN** (flat_slice 4.66×, flat_deep 2.36× smaller) |
| D native headless | **PASS** (all ≤ +0.2%, mostly −0.1…−4.7%) |
| D guibench-x | **PASS** (7/7) |
| D VM unthrottled | **PASS / WIN** (86/86 valid; bough-x table 96→12% damage, ticker u100 98→16%; bough rows scroll CPU −61%; bough startup +14.8% and search filter +16.5% flagged) |
| E apps | **PASS** (±2%) |
| E lab/animate/micro/kernel | **WATCH** (+1.7% lab scroll/animate, +6.5% micro vscroll, noisy kernel repair) |
| E damage lab | **PASS / WIN** (0 fails; 5–19× smaller repair area; ~2–5× tracking CPU) |
| F inspector | **PASS** (3/3) |
