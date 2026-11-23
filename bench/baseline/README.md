# Native baseline and regression gate

Run from the harness checkout on macOS with the workspace's Rust toolchain. Use a separate worktree and target for each source revision. The runner records three repetitions unlocked for instructions. Use `--timing` to take the shared benchmark lock around each measured process when collecting wall time and RSS. Builds are unlocked.

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
| `pbench/{canvas1,canvas4,anim600}` | Calling-thread instructions per serial CPU phase, rounded to 100 instructions. Requires all executor rows and a passing pixel cross-check. |
| `guibench/…` | Façade calling-thread instructions for mount and named batches |
| `effects/…` | Façade mean process instructions per trace step, or exec to first frame for cold start |
| `effects-core/…` | Typed core median process instructions per trace step, or setup to first frame for startup |
| `fxbench/…` | Calling-thread instructions per frame, rounded to 100 instructions. Keeps CPU cached, CPU uncached, Metal and chooser (`mixed`) rows separate, gates all four and requires the retained-pixel verdict. A `mixed` row also records the chooser's idle, CPU and GPU frame counts. The composite diagnostic columns are checked for shape and left in the raw log. |
| `flatbench/cards300` | Calling-thread instructions per frame of the flat paint bank, rounded to 100 instructions. Gates the single-core CPU and Metal rows. The eight-worker rows must be present and pixel-exact, but their frame-thread share depends on scheduling, so their counters stay in the raw log. |
| `damage_x/{window,stack,4k}` | Calling-thread instructions per frame of the damage engine alone (`crates/bough-damage/examples/damage_x.rs`): damage, moves, scrolls, masks, cover recommits and stacked blur layers |

The `pbench` gate uses `A1`, `A`, `K`, and `K1`. Pooled and Metal executors have diagnostic counters in the raw logs. In repeat runs of one `fbf6a2637b` binary, the `canvas1` pan phase varied from 20,922,200 to 31,426,800 frame-thread instructions on `AP`. The frame thread's share of the parallel work depends on scheduling. The `AG` drag phase varied from 71,600 to 85,500 instructions. The gate still requires both executors to emit complete rows and pass the fixture's correctness check.

Set `BOUGH_FONT` to a known TrueType face for the façade and effects comparisons. Use the same font in both runs. The receipt records font digests when an override is set. The prepared-face typed core uses Latin-1 DejaVu Sans 14/28 px, opaque colours, arithmetic row placement, remount on resize, group-1 keymap and CPU spans. The typed effects adapter covers opaque, integer-position rectangles with colour transitions. The staged paint `fxbench` video cells use the source under test. The older standalone effects adapter also offers a video probe from a separately pinned paint seam. The receipt records unavailable adapters and old-tree prerequisites.

`--no-build` reuses binaries listed in a previous build receipt in the chosen target. The source, toolchain and binary fingerprints must still match. A failed compile, pixel gate, process launch, counter parse or incomplete repetition set makes the command exit nonzero. Build and counter diagnostics are in `raw/`.

The façade no-change search fixture takes 15 blocks. Interleaved replays on the same input gave
842,925, 842,931 and 843,069 instructions at `71ee3be`, `fbf6a2637b` and `bba2ca5447`. The older
three-block fixture samples the initial cost as it decays, and its median can differ by several
percent across launches of one executable. Replay both revisions with the same block count.

## Thresholds

The gate allows a 2% increase in the primary instruction counters by default. Process wall time and RSS are secondary. Their default limits are 10% and 5%. They are unqualified for unlocked captures or above the load ceiling and wall is unqualified when either run already spreads more than its tolerance. A positive allocation count where the baseline is zero fails.

The comparison reports an over-limit instruction count as `VARIED (same binary)` when the executable SHA-256, runtime inputs and workload contract match. Those are repeat measurements of one executable. This rule still checks fixture coverage, correctness, dispatch allocations and qualified time and RSS. A changed executable uses the operation limit. Repeated `fbf6a2637b` captures saw instruction variation in the corpus index and Metal paths, including more than 2× on one small corpus probe. Inspect the samples when a changed executable exceeds the limit on these workloads.

Use `--thresholds limits.json` for preregistered per-fixture and per-operation overrides. For example, an override on `hover` leaves every other sheet operation under the fixture default.

```json
{"sheet_x/rows10000/hover": {"instructions": 1.03}}
```

`../baselines/native/r3-historical-limits.json` applies when comparing typed-core R3 against `71ee3be`. `anim_x/208/pointer_words` permits 1.025× for the remaining 96 instructions in the checked page-delta path. The cap still rejects the pre-recovery 4,345. Dropping the dense partial-page path recovered the pointer count and raised sheet hover from about 5.3k to 12.9k in the control.

The historical cold mounts permit 1.10× at 1,000 rows and 1.05× at 10,000 rows. The recovered 1,000-row instruction median ranged from 366,552 to 384,092 across two three-process replays, compared with a 356,808 historical median. At 10,000 rows the recovered medians were 365,094 and 366,044, compared with 359,070 historically. A cold probe at 1,000, 3,000 and 10,000 rows found a similar fixed increase of about 15k in the nine-process minima. Reusing capacity reduced the measured delta to roughly 0.4–1.8k at 1,000 and 3,000 rows. macOS includes kernel work in this count. The evidence identifies a cold-start boundary and leaves the exact kernel cause open. The default integration caps apply against the current reference.
