# M1 report: the matrix, the asm harness, and the first baseline

lane: measurement · state: **INFRASTRUCTURE GATED; FIRST BASELINE MEASURED (LOADED)** · card
`cards/2026-11-21-harness-and-baseline.md` · branch `lane/measurement/harness`

## What was built

1. **`bench/vm/`** — a Lima/Apple-Virtualization Ubuntu aarch64 VM that builds and runs zortax's
   gui-bench runner (`13f3cb61687c`) under fakeland, with lorelei's two apps registered as `bough`
   (façade) and `bough-x` (new core). One command per job: `up.sh`, `build.sh`, `run.sh`. Results
   come back with `CONTRACT.txt` (guest, VM, toolchains, renderer, timing, pinning, load, binary
   sha256). zgui's app is built from the `feat/fakeland-marks` branch of zortax/zgui.
2. **`bench/asm/`** — a manifest of fifteen hot functions of the new core, `cargo asm`-based
   `emit`/`diff`/`graph`/`check`, a harness binary that instantiates the generic bodies from the
   gui-bench `rows` scene and the layered canvas, and a cfg-mutant negative control.
3. **`bench/baseline/`** — a manifest-building runner for the native headless jobs (whole-process
   retired instructions from `/usr/bin/time -l`, wall, RSS, sha256, `__text`), deterministic
   separate rounds under one lock, JSON + markdown, and `compare` with per-cell thresholds.
4. **`bench/RUNBOOK.md`** and `bench/vm/README.md`, `bench/asm/README.md`.

## Verified commands and results

### 1. The VM matrix

```
$ bench/vm/up.sh
up: benchvm ready
benchvm  Running  vz  aarch64  10  16GiB  80GiB

$ FRAMEWORKS=bough,bough-x,zgui bench/vm/build.sh
build: gui-bench runner ... Finished `release` profile in 15.81s
apps/bough/target/release/bough-bench     4220b8c8…ee5a  47,988,160 bytes
apps/bough-x/target/release/bough-bench   8dbfd750…ff720  6,624,296 bytes
target/release/gui-bench                  d29ac90f…4e9   19,487,512 bytes
apps/zgui/target/release/zgui-bench       63d22b5d…ea36  (2m01s build)

$ FRAMEWORKS=bough,bough-x,zgui SCENARIOS=rows,table,search bench/vm/run.sh
[1/16] rows/bough/...   ...   [16/16] search/zgui/default/60hz/1000
results: results/vm-20261121T092701Z (0 failed measurements)
run: results copied to bench/results/vm/20261121T092701Z (runner exit 0)
```

> Artifact note: the raw per-cell trees of the four VM runs are no longer in the tracked tree (they
> remain in git history at 34a1be466f). `bench/results/vm/<stamp>.json` is the compact summary per
> run: every cell is `invalid` (wake lateness), `layout` is listed done but writes no file
> (`missing`), and each cell records its framework and timing scope. See `ARTIFACT-POLICY.md`.

All 16 cells completed; `run.json` lists 16 cells, every `cell.json` has `"errors": []`. zgui's 6
cells failed before `VK_LOADER_DRIVERS_DISABLE=` was emptied (`GpuUnavailable`, no adapter, then
"the surface is not compatible with this adapter"); after it, all pass. The first failed run is not
kept; the 8-cell run `20261121T091446Z` and the full run `20261121T092701Z` are.

Numbers (fixed 60 Hz, lavapipe/Pixman, CPUs 0-5, `first_present_ms`, and hover `total_p50`):

| cell | first_present ms | hover p50 ms | validity |
|---|---:|---:|---|
| rows bough 100 / 3000 | 14.91 / 32.68 | 12.22 / 13.06 | false |
| rows bough-x 100 / 3000 | 16.03 / 16.03 | 10.79 / 11.83 | false |
| rows zgui 100 / 3000 | 766.17 / 249.47 | 17.11 / 29.53 | false |
| table fine bough / bough-x / zgui | 16.13 / 15.40 / 198.82 | – | – |
| table coarse bough / bough-x / zgui | 31.56 / 15.39 / 198.44 | – | – |
| search 1000 bough / bough-x / zgui | 31.96 / 15.51 / 398.25 | – | – |

Every interactive report's `validity.valid` is false: fakeland saw 44-91 of ~200 output wakes more
than 2 ms late inside the VM (max 7.4 ms). The cells are reported, not ranked, exactly as the
contract says. Startup carries no wake-lateness check. The bough-x 16 ms at both 100 and 3000 rows is
flat as the representation predicts; the façade grows (14.9 → 32.7) and zgui's llvmpipe startup is
hundreds of ms. **Contract**: VM, software rendering, fixed 60 Hz, pinned CPUs, fixed-run only (the
`GB_TIMING` patch is not in this tree) — never pooled with macOS numbers.

### 2. The asm harness

```
$ python3 bench/asm/asm.py emit --out /tmp/asm-out
column_paint  1806 instructions (5 symbols)   span_text 1631   turn_resize 1555
layout_row 1331   turn_after 910   state_compare 756   column_delta 606
mount_retable 490   block_delta 244   kernel_shade 212   kernel_pick 159
handler_call 78   span_executor 67   kernel_shade_wrap 52   turn_paint 32   turn_settle 9
asm: emitted 25 symbols, 9937 instructions

$ python3 bench/asm/asm.py diff /tmp/asm-out /tmp/asm-out
asm: no entry grew beyond 2.0% (and 16 instructions)     exit 0

$ python3 bench/asm/asm.py check
turn_settle  10 -> 12 (+ asm_mutant::extra)   ok
turn_paint   35 -> 37                          ok
kernel_shade_wrap 52 -> 54                     ok
asm check: PASS (the cfg mutant moves every control entry)   exit 0
```

`graph crates` (45 KB SVG), `graph calls` (52 KB), and `graph expr` (5 columns, 1 word read for the
rows scene; the learned dependency graph comes from the public `bough_x::reflect` API) all render.
The graph of the *sheet* scene is not covered: the reflect binary drives `rows`, as stated in
`bench/asm/README.md`.

### 3. The first baseline and the compare gate

```
$ bench/baseline/run.sh --name 71ee3be --worktree "<baseline-worktree>" --reps 3 --no-build
...
baseline: wrote .../bench/baselines/macos-71ee3be/71ee3be.json and .md
```

Commit `71ee3be0aa`, clean worktree, `rustc 1.101.0-nightly (5d8fec066 2026-10-16)`, M3 Ultra,
macOS 26.7.3, **load 19.2→17.2 (LOADED)** because other sessions were building. Instructions are the
primary column and are stable (spread under 1 %): `sheet_x` 1000/10000 = 21,414,658 / 25,026,131;
`events_x` 10k = 2,322,717,289; `text_x` = 231,553,282; `anim_x` 208 = 17,923,255; `pbench canvas 1`
= 101,888,267,149; headless mount/hover/typing at 1,000 rows = 85,450,645 / 246,313,512 /
177,056,885; filter at 100 = 133,014,444; filter-nochange at 1,000 = 136,573,577. RSS and wall are in
the JSON; wall is **not usable** under this load (the markdown labels the run LOADED).

```
$ bench/baseline/compare.sh base.json base.json      -> compare: PASS, exit 0
$ (a copy of the recorded JSON with sheet_x/rows10000's median x1.5)
  bench/baseline/compare.sh base.json mutant.json     -> REGRESSED: instructions 1.500x > 1.02, exit 1
```

The second input is a synthetic mutation of a real recorded baseline (the gate-logic control). A
genuinely different build was **not** run: the host never went quiet enough, and instruction counts
do not move under load. This is the honest limit of the day.

**A framework defect found by the baseline**: `headless --scenario search --rows 1000 --op filter`
panics after ~40 filter steps with `bough-mem: column 'glyph_x' is full (capacity 524288, need
524304); raise it in the region layout` (exit 101). It is recorded in every baseline's
`known_failures`; the working 100-row filter and 1,000-row filter-nochange cells stand in.

## What was not verified / boundaries

* No hardware GPU, no scanout latency, no unthrottled or VRR timing. The VM is software-rendered and
  fixed 60 Hz; its interactive validity is false.
* The VM's `bough-vk` build needed a three-line type patch (`*const i8` → `*const c_char` on
  aarch64 Linux). It is applied to the VM copy only; the framework source is untouched and the bug
  should be fixed in `crates/bough-vk`.
* `<ref-checkouts>/fakeland` is missing `crates/fakeland-scenario/src/target/` in its working
  tree; the harness uses a fresh clone in `<extra-checkouts>/fakeland` (and `origin/main`, because
  `b06eace` does not compile). `gui-bench/apps/zgui` needs the `zgui-fakeland-marks` branch.
* The asm harness compares same-profile builds only. Cross-profile counts are not comparable
  (inlining changes which symbols exist); `check` therefore uses a cfg mutant for the negative
  control and prints the dev comparison as information.
* The eframe/egui and iced rows were not built: the brief allowed them "if they build in reasonable
  time" and zgui already establishes the third framework. The runner registers them; building them
  is a later job.
* Wall-time cells under load: reported, marked, never ranked. A quiet re-measure is the next card.

## Next card

1. Wait for load < 1.0 and re-run `bench/baseline/run.sh --no-build` for a quiet baseline; compare
   it with this one to bound the loaded run's instruction noise (expected under 1 %).
2. Add the sparse `bough-vk` c_char fix in the framework's area, remove the VM patch, and re-verify
   the VM build.
3. Fill the empty slots as the element-layer scenes land (chat, ide, dashboard, virtual, spinner,
   ticker, resize, effects, media); add one cell per slot, with a failing-case expectation.
4. Fix or file the `glyph_x` capacity defect with a failing test at 1,000 rows.
5. Add the unthrottled/Vrr timing patch (`GB_TIMING`) to the VM so the 60 Hz latch is separable,
   and report validity separately per timing mode.
