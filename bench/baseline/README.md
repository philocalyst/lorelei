# Native baseline and regression gate

Run from the harness checkout on macOS with the workspace's Rust toolchain. Use a separate worktree and target for each source revision. The runner takes the shared benchmark lock for the build and timed batch.

```sh
bench/baseline/run.sh --name baseline --worktree "$BASE_WORKTREE" --reps 3
bench/baseline/run.sh --name candidate --worktree "$HEAD_WORKTREE" --reps 3
bench/baseline/compare.sh runtime/baseline/baseline/baseline.json runtime/baseline/candidate/candidate.json
```

`--cells` selects comma-separated fixture IDs for a shorter replay. The comparator also accepts exact `cell/operation` IDs. A full comparison reports every cell, including added cells and missing baseline cells. `--allow-missing` permits missing baseline cells and requires at least one comparable counter. `--require-quiet` requires both runs to meet the load ceiling.

The runner writes one structured receipt, a readable table, and raw stdout, stderr, build and correctness logs under `runtime/baseline/<name>/`. Use `--out` to choose a different location. The receipt captures source and dependency identities, Rust toolchain, CPU model, operating system, fixture parameters, executable hashes and sizes, repetitions, load and invalid cells. Retain raw diagnostics in ignored storage. The format-1 receipts still compare to each other using their process totals.

## Measurement scope

| Fixture | Primary counter |
|---|---|
| `sheet_x/rows1000`, `sheet_x/rows10000` | Calling-thread instructions per phase |
| `events_x/rows10000` | Calling-thread instructions for dispatch alone and dispatch plus settle, with dispatch allocation counts |
| `text_x` | Calling-thread instructions per scenario and phase |
| `anim_x/208` | Calling-thread instructions per phase |
| `anim_x/208/{near,swept,near-swept}`, `ops_x/{select,lane,ops,search,corpus}` | Calling-thread instructions from the incremental-operators fixtures |
| `pbench/canvas1` | Whole-process instructions for the offscreen CPU and Metal fixture with pixel cross-check |
| `guibench/…` | Façade calling-thread instructions for mount and named batches |
| `effects/…` | Façade mean process instructions per trace step, or exec to first frame for cold start |
| `effects-core/…` | Typed core median process instructions per trace step, or setup to first frame for startup |

Set `BOUGH_FONT` to a known TrueType face for the façade and effects comparisons. Use the same font in both runs. The receipt records font digests when an override is set. The prepared-face typed core uses Latin-1 DejaVu Sans 14/28 px, opaque colours, arithmetic row placement, remount on resize, group-1 keymap and CPU spans. The typed effects adapter covers opaque, integer-position rectangles with colour transitions. The staged video adapter uses a separately pinned paint seam. The receipt records unavailable adapters and old-tree prerequisites.

`--no-build` reuses binaries listed in a previous build receipt in the chosen target. The source, toolchain and binary fingerprints must still match. A failed compile, pixel gate, process launch, counter parse or incomplete repetition set makes the command exit nonzero. Build and counter diagnostics are in `raw/`.

## Thresholds

The gate allows a 2% increase in the primary instruction counters by default. Process wall time and RSS are secondary. Their default limits are 10% and 5%. They are unqualified above the load ceiling and wall is unqualified when either run already spreads more than its tolerance. A positive allocation count where the baseline is zero fails.

Use `--thresholds limits.json` for preregistered per-fixture and per-operation overrides. For example, an override on `hover` leaves every other sheet operation under the fixture default.

```json
{"sheet_x/rows10000/hover": {"instructions": 1.03}}
```
