# bench/baseline — the reproducible macOS baseline and its regression gate

`run.sh` builds an exact manifest and measures the native headless jobs; `compare.sh` fails when a
later run regresses beyond the stated thresholds. `README` in each result directory states that
run's contract. The full procedure is `bench/RUNBOOK.md` §3.

```sh
bench/baseline/run.sh --name 71ee3be --worktree "$HOME/work/lorelei-wt/baseline"
bench/baseline/compare.sh bench/baselines/macos-71ee3be/71ee3be.json NEW.json
```

## What is measured

Every cell is one whole process under `/usr/bin/time -l`: retired instructions (process wide, the
primary column), wall time measured around the process, and maximum RSS. The example binaries also
print their own counters, kept verbatim in `runs[].stdout`/`fields`.

| cell | what |
|---|---|
| `sheet_x/rows1000`, `sheet_x/rows10000` | the new core's sheet: handler + settle, per step, PMU counts |
| `events_x/rows10000` | pointer/click/key/text entry points, warm and cold, allocations |
| `text_x` | staged text shaping and edits |
| `anim_x/208` | animation frames |
| `pbench/canvas1` | the per-archetype CPU raster and the generic kernel on the same pixels |
| `guibench/rows-1000-{mount,hover,typing}` | the façade's headless driver at 1,000 rows |
| `guibench/search-100-filter`, `search-1000-filter-nochange` | the filter path at working sizes |

Scenes that do not exist yet (chat, ide, dashboard, virtual, spinner, ticker, resize on the new core,
effects, media) are `slots` in the JSON, each with the reason. `known_failures` names the cells that
fail in the framework (the 1,000-row filter's `glyph_x` capacity panic) so their absence is not read
as a zero.

Not in this baseline, deliberately:

* `bench/results/fusion/run_slopes.sh` and `bench/integrate/micro.py` compare bough with **zgx** and
  need zgx's dist and headless drivers in the same target layout. They are competitor comparisons,
  not a bough baseline; `bench/zgx/build.sh` builds the drivers and `BOUGH_DIST`/`ZGX_HEADLESS` point
  the scripts at the two trees. Run them as a separate job, never inside this JSON.
* `bench/r8/lay_compile.sh` and `compile_scaling.sh` measure **compile time and `__text` per
  component**. They rebuild examples with `touch`, so a wall-clock column taken on a loaded host is
  not a number; sizes are still valid. Run them alone on a quiet host and commit their TSVs under
  `bench/results/`.
* `bench/memory`, `bench/floor`, `bench/layout`, `bench/windowed` are GPU/window jobs or
  macOS-only windowed adapters; they need a desktop and are out of this headless baseline. The
  `bench/baseline` runner accepts GPU-free additions through `cells()`.

## Adding a cell

Add `(id, argv, env, kind)` to `cells()` in `run.py`; `kind` only selects a stdout parser. Then fill
the matching slot in `SLOTS` (or add one). A cell that cannot run must raise/write an error: the run
exits 1 and the JSON records the error, never a zero.

## Thresholds

Per cell, medians: instructions ≤ 1.02×, wall ≤ 1.10×, RSS ≤ 1.05×. A wall comparison is skipped
when the baseline's own spread already exceeds the threshold (`max/min - 1`). `--thresholds
limits.json` overrides per cell:

```json
{"sheet_x/rows10000": {"instructions": 1.05, "wall": 1.25}}
```

A cell present in the base but missing in the head fails unless `--allow-missing`. A head run above
load 2.0 is marked `LOADED`; `--require-quiet` makes that a failure. Never tighten a threshold after
seeing a result.
