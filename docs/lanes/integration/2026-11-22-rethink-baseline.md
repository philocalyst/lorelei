# Phase 1 — BEFORE baseline (`b9a196b6ec`)

Raw evidence for the Lorelei/Bough "rethink" lane. Everything below was captured on
`h16003mac` in the isolated worktree `/Users/rmccrar6/work/lorelei-wt/rethink-bench`
(branch `lane/rethink/bench`). No Rust source was changed. The only tree change is this
`bench/results/rethink-baseline/` directory.

## Contract

| | |
|---|---|
| commit | `b9a196b6eccd0130377b844e900ccd335d9b9a26` (`b9a196b6ec`) |
| branch | `lane/rethink/bench` |
| host | Apple M3 Ultra, 32 cores, 96 GiB (103,079,215,104 B), macOS 26.7.3 (25G342) |
| toolchain | `nightly-2026-10-17-aarch64-apple-darwin`; rustc 1.101.0-nightly (5d8fec066 2026-10-16) |
| cargo | 1.101.0-nightly (3960fa69b 2026-10-12) |
| build env | `PATH=$(brew --prefix rustup)/bin:$PATH`, `RUSTC_WRAPPER=""`, `CARGO_TARGET_DIR=$HOME/work/targets/rethink-bench`, `nice -n 15`, `-j 8`, `--offline --locked` where the lock is usable |
| lock | `lockf -k $HOME/work/locks/bench.lock` around every timed batch |
| font | `BOUGH_FONT=/Users/rmccrar6/work/reactivity/ref/zgx/comparisons/fonts/DejaVuSans.ttf`, SHA-256 `7da195a74c55bef988d0d48f9508bd5d849425c1770dba5d7bfc6ce9ed848954` (matches `bench/baselines/native/reference.json` `runtime_inputs.BOUGH_FONT`) |
| mono font (sketch-code) | `/Users/rmccrar6/work/notes/reviews/card3-round2-evidence/test-fonts/DejaVuSansMono.ttf`, SHA-256 `0f5db4f1749979d961019838b160bec74abdf7f9eca69553fe1aa856bbff49a4` |
| load, start | `11.58 10.00 7.02` (before Set A); `5.98 6.96 5.13` before build |
| load, end | `7.77 8.29 7.59` (after the VM runs); `4.24 8.13 7.33` after Sets A–G; guest load `0.27` before / `0.38` after the unthrottled VM run |
| process load | host is shared and stayed at load ≈ 12–15 during Sets A–C. **Instruction counts are the qualified metric; wall time is diagnostic.** |

Binary SHA-256 / size (release profile unless stated):

| binary | SHA-256 | bytes | `__TEXT` |
|---|---|---|---|
| `examples/fxbench` | `3f94237501a088ebd16ae8ec2a182e163bceaf99f5eee6476e1e40eea593c3cf` | 3.4 MiB | — |
| `examples/card3_cost` | `bf406c5290f8f0ea604b67f4c9b54793ad08e7bee797bdee64114812e32952ac` | 6,671,704 | 4,784,128 |
| `examples/deep_view` | `dd720dd22466fa96277c8c3d5e95dc6b2641b66159e9734359268dbbbbf3334b` | 1,988,664 | 1,359,872 |
| `bough-fx-bench` | `dbb86f664386222bb25d2481656e305f61e1e061417ca18ffbbb4538135dfdd0` | 5.2 MiB | — |
| `headless` (Zortax `bough-guibench`) | `3689df70092f8e129bfdb7e5b2ffa315aa69b72c70e2a061f17eab71b00ede34` | 7.2 MiB | — |
| `bough-bench` (`bough-guibench-x`) | `6906024b6749227dd1f595dcb70a059ac2a6db0457705e1f70d033dd94eff199` | 461,480 | 278,528 |
| `lab_headless` | `ffc719514bf8626bb45fbdede51f04e40555289c3b275d0dc8cd28b6c16c8bbf` | 6,238,128 | — |
| `animate_headless` | `197eadc1bfc8bd23a2e3979883b3276576687da8f247a33eaa0ea6ae58562ddb` | 6,181,648 | — |
| `micro_bench` | `7da534fe8ec4adc34e2b32be8333f33337574ecde6a2cccca89531975c06f6c4` | 6,239,064 | — |
| `kernel_bench` | `5912c8b869ea558a7d056678c8eb6e395c709506ef2ccc9fe133f6dd28253712` | 6,262,752 | — |
| `apps_bench` (mail/dash) | `f9b7cb1583675964d0e9cf643482c3d708fd0f73fa508fe1b6df164905aea75f` | 7.0 MiB | — |
| `apps_bench_sketch_code` | `8e040b5970984cd4c25dba4e9c768a551be502ef95feb10c60722899cc7aa124` | 6.4 MiB | — |

### Deviations / honesty notes

- `comparisons/bough-fx-bench/Cargo.toml` and `comparisons/bough-guibench/Cargo.toml` are **separate workspaces** whose committed `Cargo.lock`s are stale for this revision; `--locked` refuses them. They were built with `--offline` (no network), the lock diff was captured, and both locks were restored with `git checkout --` afterwards. fx-bench's diff was only `+bough-lay,+bough-x` in `bough`'s dep list (no version changes); guibench's was a 56+/423− stale-graph refresh. `git status` is clean apart from this results directory.
- Set A builds `fxbench` in **release**, as instructed (the checked-in reference receipt used `dist`, so hashes differ by profile; that is expected).
- Set A ran with `NOCONTROL=1`, the fixture's own supported switch that skips the `cpu1-nocache` redraw-every-layer control. The three requested executors (`cpu1`, `metal`, `mixed`) all ran.
- Set C ran `BOUGH_FX_SAMPLES=40` (the runner default is 240) so the full 8-cell × 2-size matrix finishes cleanly.
- `bench/card3/compile.sh`'s `sed` pattern (`row((text("leaf"),`) does not match this revision, where the expression spans lines. The "incremental" measurement therefore recompiles an **unchanged** example (deep_view SHA stays `dd720d…`); 5.05 s is a real example-recompile cost, not a leaf-literal edit.
- Complex scenes use reduced repeat counts where noted (`FRAMES=1200`, `REPS=100`, `REPS=5`) to fit the Phase-1 window.
- `bench/baseline/run.py` was run on a representative cell subset (see §A); the omitted cells are covered by the dedicated sets below.

Exact command shapes:

```sh
export PATH="$(brew --prefix rustup)/bin:$PATH" RUSTC_WRAPPER=""
export CARGO_TARGET_DIR="$HOME/work/targets/rethink-bench"
export BOUGH_FONT=/Users/rmccrar6/work/reactivity/ref/zgx/comparisons/fonts/DejaVuSans.ttf

nice -n 15 cargo build --offline --locked --release -j 8 -p bough-paint --example fxbench
nice -n 15 cargo build --offline --locked --release -j 8 -p bough --example card3_cost --example deep_view --example slice

lockf -k "$HOME/work/locks/bench.lock" env NOCONTROL=1 REPS=24 \
  "$CARGO_TARGET_DIR/release/examples/fxbench" effects {200,1000}

sh bench/card3/cost.sh bench/results/rethink-baseline/logs/card3-cost.tsv
sh bench/card3/compile.sh

# fx-bench (separate workspace, lockfile refreshed offline then restored)
nice -n 15 cargo build --offline --release --features full-effects -j 8 \
  --manifest-path comparisons/bough-fx-bench/Cargo.toml
lockf -k "$HOME/work/locks/bench.lock" sh -c '... check/measure/cold ...'

# Zortax native headless (separate workspace, lockfile refreshed offline then restored)
nice -n 15 cargo build --offline --release -j 8 \
  --manifest-path comparisons/bough-guibench/Cargo.toml --bin headless
```

Raw logs: `logs/setA-fxbench.txt`, `logs/setB-card3-cost.txt`, `logs/setB-compile.txt`,
`logs/setC-fx-bench.txt` (+ `logs/c-<n>-<cell>.{gate,csv}`), `logs/setD-*.txt`,
`logs/setZortax-native.txt`, `logs/setComplex.txt`, `logs/card3-cost.tsv`.

---

## A. Canonical baseline runner (`bench/baseline/run.py`)

```sh
python3 bench/baseline/run.py --name rethink-baseline \
  --worktree "$PWD" --target-dir "$HOME/work/targets/rethink-bench" \
  --cells sheet_x/rows1000,sheet_x/rows10000,events_x/rows10000,text_x,anim_x/208 \
  --reps 3 --out runtime/baseline/rethink-baseline
```

Exit 0, **0 errors**, all five cells `valid: true`, 3 reps each. Receipt:
`baseline-core/rethink-baseline.json` + `.md` (raw build logs in `baseline-core/raw/`).
Load `before 9.24 8.67 7.69`, `after 8.74 8.58 7.67`; above the runner's 2.0 ceiling, so
instruction counters are qualified and wall/RSS are not. (The runner fingerprints untracked
files, so this directory was moved aside during the capture and restored afterwards.)

| cell | operation | median instructions |
|---|---|---:|
| sheet_x/rows1000 | mount | 357,339 |
| sheet_x/rows1000 | idle | 63 |
| sheet_x/rows1000 | hover | 5,192 |
| sheet_x/rows1000 | hover_move | 6,359 |
| sheet_x/rows1000 | theme | 27,283 |
| sheet_x/rows1000 | edit_exposed / edit_unexposed | 1,928 / 1,344 |
| sheet_x/rows1000 | part_model_edit / total_word / delta_total / diff_only / journal_only / mark_cell / pointer_same | 58 / 714 / 634 / 162 / 724 / 89 / 439 |
| sheet_x/rows1000 | select | 766 |
| sheet_x/rows1000 | frame_in_motion / frame_at_rest | 168 / 10 |
| sheet_x/rows1000 | scroll_3_rows / scroll_6px / scroll_page | 5,116 / 980 / 33,196 |
| sheet_x/rows1000 | insert_row / remove_row | 20,140 / 19,840 |
| sheet_x/rows1000 | redescribe_idle / redescribe_edit | 13,988 / 14,048 |
| sheet_x/rows10000 | mount | 355,733 |
| sheet_x/rows10000 | idle / hover / hover_move / theme | 63 / 5,192 / 6,359 / 27,283 |
| sheet_x/rows10000 | edit_exposed / edit_unexposed | 1,928 / 1,344 |
| sheet_x/rows10000 | frame_in_motion / frame_at_rest | 168 / 10 |
| sheet_x/rows10000 | scroll_3_rows / scroll_6px / scroll_page | 5,116 / 980 / 33,196 |
| sheet_x/rows10000 | insert_row / remove_row | 87,640 / 87,340 |
| sheet_x/rows10000 | redescribe_idle / redescribe_edit | 13,988 / 14,048 |
| events_x/rows10000 | pointer_onto_row dispatch / step | 724 / 1,578 |
| events_x/rows10000 | pointer_same_row dispatch / step | 462 / 527 |
| events_x/rows10000 | click_row dispatch / step | 2,221 / 3,137 |
| events_x/rows10000 | key_unfocused dispatch / step | 115 / 331 |
| events_x/rows10000 | text_focused_field dispatch / step | 233 / 1,421 |
| text_x | rows/mount | 120,273 |
| text_x | rows/idle / rows/hover | 53 / 2,444 |
| text_x | rows/tint | 1,528 |
| text_x | rows/scroll 3 / scroll page | 3,813 / 18,585 |
| text_x | rows/label edit / value edit | 1,886 / 1,184 |
| text_x | rows/remove+insert | 32,171 |
| text_x | rows/collect glyph memo | 40,300 |
| text_x | typing/type a character / caret move | 3,014 / 1,370 |
| text_x | search/type a character | 832,961 |
| text_x | search/narrow+widen | 199,468 |
| anim_x/208 | frame_endless / +fade | 161 / 265 |
| anim_x/208 | fade_start_turn | 676 |
| anim_x/208 | pointer_words | 4,182 |
| anim_x/208 | scroll_3_rows | 3,413 |

The `guibench/*` cells could not be included in this runner: they require the
`comparisons/bough-guibench` sub-workspace, whose committed lockfile is stale, and the runner's
"source changed during build" guard treats the resulting lock refresh as a dirty source. The same
façade numbers are captured more richly in §F (native headless, `--repeats 8 --blocks 3`).
`ops_x/*`, `pbench`, the `dist` `fxbench/*` rows and the `effects/*` cells were likewise omitted
from the runner to keep the window; they are covered in §B/§D and by the dedicated sets.

---

## B. Set A — `bough-paint` `fxbench effects` (release, `REPS=24`, `NOCONTROL=1`)

N=200 scene: 298 records, 8 columns, 17 layers (17 glass / 50 dots). N=1000: 1480 records,
83 layers (83 glass / 250 dots). 1280×800 frame. `mismatch` is the retained-surface vs fresh
full-redraw word check; **0 everywhere**.

`instr_med_K` = frame-thread instructions per frame (median, thousands). `wall_med_us` /
`gpu_med_us` = median wall / Metal GPU completion, microseconds (diagnostic under load).

| N | cell | executor | instr_med_K | wall_med_us | gpu_med_us | mismatch |
|---:|---|---|---:|---:|---:|---:|
| 200 | idle | cpu1 | 9.3 | 1 | 0 | 0 |
| 200 | idle | metal | 6.1 | 0 | 0 | 0 |
| 200 | idle | mixed | 3.1 | 0 | 0 | 0 |
| 200 | one spring | cpu1 | 10,843.5 | 558 | — | 0 |
| 200 | one spring | metal | 102.0 | 599 | 349 | 0 |
| 200 | one spring | mixed | 100.1 | 607 | 351 | 0 |
| 200 | all springs | cpu1 | 14,169.4 | 716 | — | 0 |
| 200 | all springs | metal | 143.7 | 634 | 368 | 0 |
| 200 | all springs | mixed | 143.5 | 643 | 368 | 0 |
| 200 | scroll 11 px | cpu1 | 217,474.4 | 10,928 | — | 0 |
| 200 | scroll 11 px | metal | 1,917.8 | 1,819 | 1,153 | 0 |
| 200 | scroll 11 px | mixed | 1,927.2 | 1,661 | 1,137 | 0 |
| 200 | hover above glass | cpu1 | 2,142.3 | 112 | — | 0 |
| 200 | hover above glass | metal | 76.5 | 306 | 66 | 0 |
| 200 | hover above glass | mixed | 74.0 | 332 | 75 | 0 |
| 200 | hover beneath glass | cpu1 | 15,291.3 | 792 | — | 0 |
| 200 | hover beneath glass | metal | 92.6 | 1,083 | 781 | 0 |
| 200 | hover beneath glass | mixed | 90.1 | 1,184 | 924 | 0 |
| 200 | theme | cpu1 | 223,946.2 | 10,993 | — | 0 |
| 200 | theme | metal | 109.9 | 1,682 | 1,385 | 0 |
| 200 | theme | mixed | 120.0 | 1,892 | 1,621 | 0 |
| 200 | glass moves 1 px | cpu1 | 8,468.3 | 472 | — | 0 |
| 200 | glass moves 1 px | metal | 127.1 | 935 | 649 | 0 |
| 200 | glass moves 1 px | mixed | 125.4 | 1,048 | 769 | 0 |
| 1000 | idle | cpu1 | 9.3 | 1 | 0 | 0 |
| 1000 | idle | metal | 6.1 | 0 | 0 | 0 |
| 1000 | idle | mixed | 3.1 | 0 | 0 | 0 |
| 1000 | one spring | cpu1 | 10,840.1 | 561 | — | 0 |
| 1000 | one spring | metal | 101.8 | 845 | 551 | 0 |
| 1000 | one spring | mixed | 100.5 | 585 | 342 | 0 |
| 1000 | all springs | cpu1 | 14,169.4 | 715 | — | 0 |
| 1000 | all springs | metal | 144.0 | 641 | 360 | 0 |
| 1000 | all springs | mixed | 143.8 | 614 | 358 | 0 |
| 1000 | scroll 11 px | cpu1 | 217,733.7 | 11,023 | — | 0 |
| 1000 | scroll 11 px | metal | 2,051.9 | 1,813 | 1,139 | 0 |
| 1000 | scroll 11 px | mixed | 2,062.2 | 1,692 | 1,143 | 0 |
| 1000 | hover above glass | cpu1 | 2,142.4 | 115 | — | 0 |
| 1000 | hover above glass | metal | 76.5 | 290 | 67 | 0 |
| 1000 | hover above glass | mixed | 74.1 | 364 | 76 | 0 |
| 1000 | hover beneath glass | cpu1 | 15,291.4 | 791 | — | 0 |
| 1000 | hover beneath glass | metal | 92.8 | 1,108 | 808 | 0 |
| 1000 | hover beneath glass | mixed | 90.0 | 816 | 572 | 0 |
| 1000 | theme | cpu1 | 223,950.9 | 11,055 | — | 0 |
| 1000 | theme | metal | 111.1 | 1,550 | 1,265 | 0 |
| 1000 | theme | mixed | 120.5 | 1,538 | 1,265 | 0 |
| 1000 | glass moves 1 px | cpu1 | 8,468.1 | 456 | — | 0 |
| 1000 | glass moves 1 px | metal | 127.1 | 906 | 653 | 0 |
| 1000 | glass moves 1 px | mixed | 124.4 | 964 | 669 | 0 |

Scene descriptors / prefix books kept in the raw log (`# scene:` lines, `prefix_same/prefix`,
`blocks_same/blocks by level 1..5`).

---

## C. Set B — `card3_cost` and `deep_view` compile cost

`BOUGH_REPAIR=kernel`, release. TSV (`logs/card3-cost.tsv`), instructions per operation:

| cell | legacy | lowered | typed | typed/legacy | legacy+frame | lowered+frame | typed+frame | typed/legacy+frame |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| mount, 1,000 rows | 26,325,720 | 6,176,400 | 6,033,243 | 0.229 | 37,164,348 | 17,520,329 | 17,402,596 | 0.468 |
| mount, 100,000 rows | 26,201,841 | 7,778,851 | 6,019,114 | 0.230 | 37,034,935 | 19,111,361 | 17,387,750 | 0.469 |
| hover, one row to the next | 415 | 4,999 | 3,145 | 7.578 | 2,249 | 919,539 | 922,362 | 410.121 |
| one signal (the title) | 7,977 | 199,996 | 24,443 | 3.064 | 43,810 | 293,534 | 146,338 | 3.340 |
| insert at 0 (click) | 126,914 | 614,353 | 51,812 | 0.408 | 5,920,003 | 8,004,029 | 7,526,072 | 1.271 |
| scroll by one row | 86,794 | 446,504 | 19,053 | 0.220 | 7,932,579 | 7,771,812 | 7,370,397 | 0.929 |
| theme (opaque state) | 7,050 | 200,223 | 5,717 | 0.811 | 8,648,303 | 11,898,242 | 11,733,604 | 1.357 |
| resize by 40 pt | 3,403,515 | 1,800,992 | 1,589,269 | 0.467 | 15,140,670 | 13,601,865 | 13,420,107 | 0.886 |
| still frame | 0 | 0 | 0 | — | 1,784 | 1,685 | 1,610 | 0.902 |

Additional typed-core counters: `view()` alone 501 instructions; context read + spec convert/pack
= (8,041, 108,017) units×1000; view+conversion of the fixed part 6,898; re-description of an
unchanged view 4,592; lowered title edit per turn = view+lower 127,613 / layout+emit 63,977 /
world compare 9,264 (96 records).

Compile (`logs/setB-compile.txt`, load ≈ 14 while shared):

| metric | value |
|---|---:|
| clean release build of `bough` + `slice` + `deep_view` | **13.33 s** (load `{13.99 11.15 7.71}`) |
| incremental rebuild of the `deep_view` example | **5.05 s** (load `{14.64 11.33 7.80}`) |
| `deep_view` release binary | **1,988,664 B** (1.9 MiB), `__TEXT` **1,359,872 B** |
| `deep_view` behaviour | 132 records, 1 describe, 0 legacy passes |
| `card3_cost` release binary | **6,671,704 B** (6.4 MiB), `__TEXT` **4,784,128 B** |

---

## D. Set C — `bough-fx-bench` (release, `--features full-effects`, `BOUGH_FX_SAMPLES=40`)

All 16 gates (8 cells × 2 sizes, rings 1–4) **PASS**; oracle checks clean.
`summary,n,cell,samples,draws,p50,p95,p99,instr/op,cpu_ns/op,rss_max,footprint_max,damage_frac`.
`damage_frac` is the repaired/damaged fraction of the frame (the "repair area"); `idle` sleeps 1 s.

| N | cell | draws/40 | p50 µs | p95 µs | p99 µs | instr/op | cpu ns/op | RSS | repair area |
|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 200 | startup | 40 | 598.4 | 768.5 | 1,259.2 | 19,722,104 | 1,345,837 | 28,934,144 | 100% |
| 200 | idle | 0 | 1,006,686.8 | — | — | 18,507 | 17,541 | 17,825,792 | 0% |
| 200 | one | 40 | 12.5 | 232.5 | 297.4 | 1,305,103 | 80,032 | 20,971,520 | 1.44% |
| 200 | many | 40 | 344.8 | 498.5 | 532.0 | 16,087,660 | 996,798 | 29,474,816 | 92.5% |
| 200 | hover | 39 | 67.3 | 207.5 | 270.7 | 2,294,035 | 134,816 | 20,201,472 | 7.62% |
| 200 | theme | 40 | 183.2 | 254.0 | 275.3 | 14,553,848 | 841,171 | 21,069,824 | 92.5% |
| 200 | scroll | 40 | 150.8 | 171.6 | 184.3 | 8,186,282 | 537,556 | 21,004,288 | 92.5% |
| 200 | resize | 40 | 206.8 | 232.1 | 296.6 | 14,928,164 | 866,115 | 19,546,112 | — |
| 1000 | startup | 40 | 1,993.5 | 2,187.3 | 2,636.7 | 87,727,591 | 4,997,916 | 38,469,632 | 100% |
| 1000 | idle | 0 | 1,010,027.5 | — | — | 18,233 | 21,833 | 21,069,824 | 0% |
| 1000 | one | 40 | 55.3 | 815.6 | 840.6 | 6,165,037 | 324,347 | 24,231,936 | 1.44% |
| 1000 | many | 40 | 1,592.3 | 1,917.3 | 2,537.4 | 81,437,591 | 4,583,609 | 64,618,496 | 92.5% |
| 1000 | hover | 39 | 203.7 | 835.0 | 846.0 | 11,032,575 | 635,107 | 24,297,472 | 7.62% |
| 1000 | theme | 40 | 771.5 | 836.4 | 906.8 | 70,645,718 | 3,664,264 | 24,281,088 | 92.5% |
| 1000 | scroll | 40 | 146.0 | 182.1 | 199.8 | 8,192,992 | 535,691 | 24,969,216 | 92.5% |
| 1000 | resize | 40 | 840.2 | 883.1 | 993.4 | 72,088,155 | 3,706,582 | 22,806,528 | — |

Cold exec-to-first-frame (`spawn_to_first,n,ns,instr,cpu_ns,rss,footprint`), 5 runs each:

| N | ns (runs) | instr | footprint |
|---:|---|---:|---:|
| 200 | 20.96M, 19.53M, 15.17M, 14.68M, 63.31M | ≈63.0M | ≈8.92 MB |
| 1000 | 24.68M, 23.38M, 20.13M, 19.47M, 16.54M | ≈132.5M | ≈12.1 MB |

The 63 ms / 24.7 ms outliers are cold page-cache / dynamic-loader events; the instruction totals
are tight (63.0M and 132.5M).

---

## E. Set D — `bough-guibench-x`

- Window host is **Linux/Wayland only**; the macOS binary exits 2 with:
  `bough-bench: the window host of this build is Linux/Wayland only; the scenarios and their gates run headless (cargo test -p bough-guibench-x)`.
- Binary `bough-bench` 461,480 B (`__TEXT` 278,528 B).
- Headless pixel-gate suite (`cargo test -p bough-guibench-x --release`):
  **7 passed / 0 failed in 1.34 s** — `keys_do_what_the_probes_expect`, `filter_equals_the_definition`,
  `typing_and_search`, `spinner_scene`, `table_fine_and_coarse`, `ticker`, `rows_and_virtual`.
- No startup/frame timing is produced by this crate on macOS; it needs the VM/Wayland host for that.

---

## F. Zortax `gui-bench` — native `bough-guibench` `headless`

`--render` offscreen GPU, `--repeats 8 --blocks 3` (2 warm-up pairs + 24 timed op/undo pairs),
`--rows 1000`. `mount` is the same-process mount printed before the measured operation; the first
`rows` mount is a cold process (note its 490 ms wall).

| scenario / op | mount instructions | mount ns | perop median instr | lo | hi | frames |
|---|---:|---:|---:|---:|---:|---:|
| rows / mount (cold, `--op mount`) | 144,261,236 | 490,343,500 | — | — | — | — |
| rows / scroll | 132,433,637 | 57,589,333 | **6,148,524** | 6,147,668 | 6,190,492 | 53 |
| table / update10 | 99,085,538 | 63,766,417 | **239,083** | 236,708 | 272,420 | 53 |
| search / filter | 140,511,062 | 54,183,833 | **18,510,683** | 18,498,083 | 19,111,453 | 53 |
| spinner / spinner | 139,888,522 | 81,140,000 | **1,626,746** | 1,601,529 | 1,828,027 | 53 |
| ticker / ticker | 97,732,206 | 78,316,000 | **59,707** | 57,152 | 94,177 | 53 |
| typing / typing | 140,177,784 | 57,392,417 | **1,604,525** | 1,595,053 | 1,873,405 | 53 |
| virtual / jump | 95,727,715 | 66,024,542 | **942,177** | 896,626 | 1,039,838 | 53 |
| resize / resize | 130,138,495 | 53,403,125 | **21,757,654** | 21,741,925 | 21,803,888 | 53 |
| search / filter-nochange (6×15) | 137,637,228 | 58,759,875 | **902,749** | 861,359 | 1,131,010 | 186 |

`resize` is the runner's legacy alias for the `rows` scenario; the operation is a real
640×480↔1280×800 resize.

---

## G. Complex scenes and full apps

All under the bench lock, `BOUGH_FONT`/`BOUGH_MONO_FONT` fixed.

| fixture | setting | result |
|---|---|---|
| `lab_headless` | `ZGX_MODE=stream TICKS=900 RENDER=1` | **0.3295 ms/tick**, `253,370 instr/tick`, 900/900 frames, 157.3 repaired tiles/frame, 425.8 MB footprint |
| `lab_headless` | `ZGX_MODE=scroll TICKS=600 RENDER=1` | **0.3178 ms/tick**, `304,769 instr/tick`, 600/600 frames, 64.2 repaired tiles/frame, 316 rows mounted |
| `animate_headless` | `FRAMES=1200 RENDER=1` | **124,437 instr/frame** after 120 warm-up, 1080/1080 drawn, 457.4 MB |
| `micro_bench` | `SCENE=state OPS=2000 RENDER=1` | write p90 20 ns; **frame median 46.1 µs** (p10 34.7, p90 59.1) |
| `micro_bench` | `SCENE=vscroll-fixed OPS=2000 RENDER=1` | write p90 20 ns; **frame median 56.6 µs** (p10 43.6, p90 83.0) |
| `kernel_bench` | `ZGX_MODE=stream REPS=100` | **repair 0.0412 ms** (499 tiles) `[0.0410..0.0742]`; full redraw 0.0848 ms; copy 1920×1412 0.0136 ms |
| `apps_bench` (mail) | `REPS=5` | open message 2.71 M instr / 1.01 ms; search keystroke 1.12 M / 0.29 ms; archive 1.14 M / 0.36 ms |
| `apps_bench` (dashboard, 100k rows) | `REPS=5` | feed tick 2.57 M / 0.84 ms; sort 2.31 M / 0.37 ms; sort-new-column 2.02 M / 0.37 ms; filter keystroke 1.93 M / 0.35 ms |
| `apps_bench_sketch_code` | `REPS=5` | sketch drag 1.94 M / 0.39 ms; pan 1.09 M / 0.38 ms; zoom 1.05 M / 0.38 ms; code keystroke 1.78 M / 0.48 ms; page scroll 1.27 M / 0.39 ms; find-all keystroke 0.82 M / 0.37 ms; find-all new letter 0.83 M / 0.29 ms |

---

## H. Linux VM (`bench/vm`, `benchvm`) — captured

Guest `benchvm`: Linux `7.3.0-9-generic` aarch64, 10 vCPU, 16 GiB, vz. Renderer **lavapipe**
(Mesa software Vulkan) for the compositor; bough drives its CPU executor into `wl_shm`.
`taskset -c 0-5` pins the runner and every app. Contract and per-cell JSONs:
`vm-unthrottled/` and `vm-fixed/`.

Guest build (no local source change; `sync.sh` copies the worktree):

```sh
FRAMEWORKS=bough,bough-x JOBS=8 bench/vm/build.sh     # exit 0, 50.4 s
# apps: bough 12a92ab5…49 MB, bough-x ed1a3cd9…7.5 MB,
#       gui-bench runner cc52f037…19.5 MB
```

Two captures, same binaries:

| attempt | timing | result | wall |
|---|---|---|---|
| `vm-20261122T080123Z` | `GB_TIMING=fixed` (60 Hz) | 26 cells / 86 reports: **26 valid (idle only), 60 INVALID `wake_lateness`**, 0 failures | 10 m 02 s |
| `vm-20261122T081143Z` | `GB_TIMING=unthrottled` | 26 cells / 86 reports: **86 valid, 0 invalid, 0 failures** | 6 m 46 s |

Runner args (both): `--rows 1000 --frameworks bough,bough-x --scenarios rows,table,search,ticker`;
unthrottled used `--iterations 1 --repeats 4 --op-repeats 4`; host load before `0.27 0.29 0.19`.
The fixed replay is reported, not ranked: the guest could not wake at 60 Hz under the shared host.

Qualified unthrottled numbers (`vm-unthrottled/metrics.tsv`; latency in ms, CPU in ms/frame):

| workload (1000 rows) | bough | bough-x |
|---|---:|---:|
| rows startup, first present / toplevel | **9.94 / 9.97 ms** | **1.59 / 1.61 ms** |
| rows scroll: fps / interval p99 / CPU per frame | 240.6 / 4.85 ms / **2.810 ms** | 240.2 / 4.70 ms / **0.841 ms** |
| rows scroll damage fraction | 100% | 100% |
| rows hover: latency p50 / p95 | **1.09 / 1.16 ms** | **0.310 / 0.336 ms** |
| rows hover CPU per frame / damage | 1.120 ms / 8% | 0.219 ms / 8% |
| table op-update10: p50 / p95 / CPU per frame | **0.509 / 0.584 / 0.597 ms** | 1.066 / 1.301 / 1.066 ms |
| table op-update10 damage | 1.2% | 96.0% |
| table op-insert: p50 / p95 / CPU per frame | 1.223 / 1.251 / 3.793 ms | 1.316 / 1.410 / 1.355 ms |
| table op-insert damage | 88.1% | 92.0% |
| table op-clear: p50 / p95 / CPU per frame | 1.927 / 1.984 / 3.270 ms | 1.110 / 1.724 / 0.992 ms |
| table op-clear damage | 96.3% | 98.0% |
| search filter: p50 / p95 / CPU per frame | 6.026 / 9.676 / 5.731 ms | **0.933 / 1.339 / 0.958 ms** |
| search filter damage | 96.1% | 97.3% |
| search typing CPU per frame / damage | 0.638 ms / 0.1% | 0.357 ms / 1.3% |
| ticker u1: fps / interval p99 / CPU per frame | 62.41 / 21.91 ms / 0.228 ms | 62.59 / 20.88 ms / 0.285 ms |
| ticker u1 damage | 0.7% | 8.0% |
| ticker u100: fps / interval p99 / CPU per frame | 62.40 / 21.34 ms / 0.267 ms | 62.50 / 21.50 ms / 1.166 ms |
| ticker u100 damage | 0.9% | 98.0% |
| RSS (rows / table / search / ticker) | 30–45 MB | 11–15 MB |
| CPU ms/s (rows scroll) | 471.2 | 140.4 |

Reading: the VM is a 60 Hz-class software-rendered reference, not a throughput ceiling. The
`bough-x` core is markedly cheaper on hover/filter and RSS, while the façade `bough` has lower
startup and much lower `table op-update10` / `ticker u100` damage (1–1.2% vs 96–98% for
`bough-x`) — the two adapters damage very differently and must be compared per column, never
pooled.

## Phase-2 kill-lines (must hold or beat)

Instruction counts are the hard targets (load-insensitive). Wall targets assume a comparable host
load; qualify wall only against a run with similar `uptime`. A regression is **any** increase above
the stated number; equality is a pass. Correctness gates are non-negotiable: fxbench `mismatch = 0`,
fx-bench every gate PASS, card3/`deep_view` pixel verdicts, guibench-x 7/7 tests.

**Core `fxbench effects` (release, 1000 cards, per frame):**

| executor | idle | one spring | all springs | hover above | hover beneath | theme | scroll 11 px | glass 1 px |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| cpu1 | 9.3 K | 10,840.1 K | 14,169.4 K | 2,142.4 K | 15,291.4 K | 223,950.9 K | 217,733.7 K | 8,468.1 K |
| metal | 6.1 K | 101.8 K | 144.0 K | 76.5 K | 92.8 K | 111.1 K | 2,051.9 K | 127.1 K |
| mixed | 3.1 K | 100.5 K | 143.8 K | 74.1 K | 90.0 K | 120.5 K | 2,062.2 K | 124.4 K |

**`bough-fx-bench` (release, 40 samples):**

| N=1000 cell | p50 µs | p95 µs | p99 µs | instr/op | repair area |
|---|---:|---:|---:|---:|---:|
| startup | 1,993.5 | 2,187.3 | 2,636.7 | 87,727,591 | 100% |
| idle | (1 s sleep) | — | — | 18,233 | 0% |
| one | 55.3 | 815.6 | 840.6 | 6,165,037 | 1.44% |
| many | 1,592.3 | 1,917.3 | 2,537.4 | 81,437,591 | 92.5% |
| hover | 203.7 | 835.0 | 846.0 | 11,032,575 | 7.62% |
| theme | 771.5 | 836.4 | 906.8 | 70,645,718 | 92.5% |
| scroll | 146.0 | 182.1 | 199.8 | 8,192,992 | 92.5% |
| resize | 840.2 | 883.1 | 993.4 | 72,088,155 | — |

N=200 equivalents are in §D (e.g. startup 598.4 µs / 19.72 M; scroll 150.8 µs / 8.19 M;
many 344.8 µs / 16.09 M; theme 183.2 µs / 14.55 M; one 12.5 µs / 1.31 M; hover 67.3 µs / 2.29 M;
resize 206.8 µs / 14.93 M; idle 18.5 K).

**Typed core (`card3_cost`, instructions):** mount 1k ≤ 6,033,243; mount 100k ≤ 6,019,114;
scroll ≤ 19,053; theme ≤ 5,717; insert ≤ 51,812; hover ≤ 3,145; resize ≤ 1,589,269;
still frame (incl. frame) ≤ 1,610; re-description of unchanged view ≤ 4,592.

**Compile / size:** clean `bough`+`slice`+`deep_view` ≤ **13.33 s**; example recompile ≤ **5.05 s**;
`deep_view` ≤ **1,988,664 B** (`__TEXT` ≤ 1,359,872); `card3_cost` ≤ **6,671,704 B**
(`__TEXT` ≤ 4,784,128). Compare size and instruction columns rather than requiring a bit-identical
binary across rebuilds.

**Canonical baseline core (`dist`, instructions):** sheet rows1000 mount ≤ 357,339;
rows10000 mount ≤ 355,733; idle ≤ 63; hover ≤ 5,192; hover_move ≤ 6,359; theme ≤ 27,283;
edit_exposed ≤ 1,928; frame_in_motion ≤ 168; frame_at_rest ≤ 10; scroll_3_rows ≤ 5,116;
scroll_6px ≤ 980; scroll_page ≤ 33,196; insert_row ≤ 87,640; remove_row ≤ 87,340;
redescribe_idle ≤ 13,988. events_x pointer_onto_row ≤ 724 dispatch / 1,578 step;
click_row ≤ 2,221 / 3,137; key_unfocused ≤ 115 / 331. text_x mount ≤ 120,273; hover ≤ 2,444;
scroll page ≤ 18,585; collect glyph memo ≤ 40,300; search/type ≤ 832,961.
anim_x/208 frame_endless ≤ 161; +fade ≤ 265; pointer_words ≤ 4,182; scroll_3_rows ≤ 3,413.

**Zortax native headless (rows 1000, medians):** rows mount ≤ 132.4 M (warm; cold ≤ 144.3 M),
rows scroll ≤ 6,148,524, table update10 ≤ 239,083, search filter ≤ 18,510,683,
search filter-nochange ≤ 902,749, spinner ≤ 1,626,746, ticker ≤ 59,707, typing ≤ 1,604,525,
virtual jump ≤ 942,177, resize ≤ 21,757,654.

**Complex scenes:** lab stream ≤ 253,370 instr/tick and ≤ 0.3295 ms/tick; lab scroll ≤ 304,769
instr/tick and ≤ 0.3178 ms/tick; animate ≤ 124,437 instr/frame; micro state frame ≤ 46.1 µs,
vscroll frame ≤ 56.6 µs; kernel repair ≤ 0.0412 ms for 499 tiles and full redraw ≤ 0.0848 ms;
mail open ≤ 2.71 M / 1.01 ms; dashboard feed tick ≤ 2.57 M / 0.84 ms; sketch drag ≤ 1.94 M /
0.39 ms; code keystroke ≤ 1.78 M / 0.48 ms.

**VM unthrottled (1000 rows, qualified):** startup first-present bough ≤ **9.94 ms**,
bough-x ≤ **1.59 ms**; rows scroll CPU/frame ≤ **2.810 ms** (bough) / **0.841 ms** (bough-x);
rows hover p50 ≤ **1.09 ms** / **0.310 ms**; table op-update10 p50 ≤ **0.509 ms** /
**1.066 ms**; search filter p50 ≤ **6.03 ms** / **0.933 ms**; ticker u1 CPU/frame ≤
**0.228 ms** / **0.285 ms**; ticker u100 damage must stay ≤ **0.9%** (bough) and the
`bough-x` 98% baseline must not grow. Fixed-60 Hz VM cells remain unqualified unless the guest
can hold the latch; do not use them for Phase 2.

**Inspector / full-app gates to add:** the Phase-2 inspector must not raise any of the above, must
keep `deep_view` 132 records / 1 describe / 0 legacy passes, and must keep the guibench-x 7/7
headless pixel gates and the fx-bench 16/16 gates green.
