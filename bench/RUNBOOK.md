# bench/RUNBOOK.md — running the measurement infrastructure

Written for an agent or person with no context. Every job is one command, fails closed, and writes
self-describing results. Do not invent a number: if a job did not run here, it is not measured.

Use a headless Apple Silicon macOS host. Work happens in a git worktree
under `$HOME/work/lorelei-wt/<name>` with its own `CARGO_TARGET_DIR` under `$HOME/work/targets/<name>`.
Put the **rustup proxies first** on `PATH`. The workspace pins `nightly-2026-10-17`.

```sh
export PATH="$(brew --prefix rustup)/bin:$PATH" RUSTC_WRAPPER=
export CARGO_TARGET_DIR="$HOME/work/targets/measurement"
```

## 0. What each number is

| job | host | renderer | timing | comparable with |
|---|---|---|---|---|
| `bench/vm/run.sh` | Linux aarch64 VM (Apple Virtualization, 10 vCPU) | lavapipe (software Vulkan) for Vulkan apps; bough's CPU executor into `wl_shm` | fakeland's fixed 60 Hz display | only itself, same VM and pinned CPUs |
| `bench/baseline/run.sh` | native macOS | CPU typed core, software façade effects, offscreen Metal in `pbench` | retired instructions at each fixture's stated scope | same host and workload parameters |
| `bench/asm/asm.py` | this Mac | none | static instruction counts of named symbols | same commit, same toolchain, same profile |

A VM number is never pooled with a macOS number. A "fixed 60 Hz" interactive cell contains the
compositor's latch wait (about 10 ms); fakeland marks a cell invalid when the compositor woke late.
`validity.valid: false` in a report means the cell is reported, not ranked. Startup cells carry no
wake-lateness check. `settings.environment.renderer: "pixman"` is the *compositor's* renderer; each
app's renderer is its own (zgui opens llvmpipe/Vulkan; bough rasterises on the CPU).

## 1. Zortax's gui-bench + fakeland matrix in a Linux VM

Requires `lima` (installed by the script with Homebrew if missing), network for the guest packages
and for rustup, and the reference checkouts under `$HOME/work/reactivity/ref` plus
`$HOME/work/ref-extra/{fakeland,zgui-fakeland-marks}` (see `bench/vm/README.md` for why).

```sh
bench/vm/up.sh
FRAMEWORKS=bough,bough-x,zgui bench/vm/build.sh
FRAMEWORKS=bough,bough-x,zgui SCENARIOS=rows,table,search,spinner,ticker,typing,virtual,resize bench/vm/run.sh
```

`run.sh` takes the lock around the guest replay. Its raw output defaults to `bench/results/vm/<stamp>/`, which is ignored by Git. `CONTRACT.txt` records the guest, toolchains, renderer, timing, CPUs, load and binary hashes. `run.json` describes each cell. The audited `VALIDITY.txt` and `VALIDITY.json` name the failed or invalid reports.

The wrapper returns the runner's nonzero exit status or the audit status. Exit 2 covers a failed or skipped phase, a missing cell or report, or a failed result transfer. With `STRICT_VALIDITY=1`, exit 4 means the run transferred successfully and has an invalid or unchecked timing report. A fixed-rate result qualifies only when Fakeland's validity check passes. Startup and retained-memory churn have no timing verdict.

The VM stages the framework source directly, including the Vulkan `c_char` fix. Its gui-bench copy applies the registration and timing patches. `GB_PIN` defaults to `0-5`. `GB_TIMING` accepts `fixed`, `unthrottled` and `vrr`. Record a separate report for each timing mode. Set `BENCH_GUEST_BASE` to isolate the guest source and build directories for another checkout. See the [VM setup and timing contract](vm/README.md).

## 2. The assembly and graph harness

```sh
asm_dir="runtime/asm/$(git rev-parse --short HEAD)"
python3 bench/asm/asm.py list
python3 bench/asm/asm.py emit --out "$asm_dir"
python3 bench/asm/asm.py diff "$asm_dir" "$asm_dir"
python3 bench/asm/asm.py graph crates --out "$asm_dir/crates.svg"
python3 bench/asm/asm.py graph expr --out "$asm_dir/expr.svg"
python3 bench/asm/asm.py graph calls --out "$asm_dir/calls.svg" --emitted "$asm_dir"
python3 bench/asm/asm.py check
```

The example uses a self-diff. For a source comparison, pass the two emission directories in base-to-head order. Use the same toolchain, profile, codegen settings and host contract. An entry grows past the default limit only when its increase exceeds both 2% and 16 instructions. Use `--threshold` to override the relative limit.

`emit` builds the GUI rows and canvas fixtures and the layout memo and writes symbol assembly, a `stats.tsv` and `MANIFEST.txt`. `check` uses an `asm_mutant` build to confirm that a change in the control wrappers moves the counts. Build and count instructions directly. See [the assembly harness](asm/README.md) for symbol coverage and the inlining and reflection boundaries.

## 3. Reproducible macOS baselines and the regression gate

For an integration checkout, set `BOUGH_FONT` to the matching DejaVu Sans font. The reference receipt records its SHA-256 under `runtime_inputs.BOUGH_FONT`. The capture used the font from `zgx/comparisons/fonts/DejaVuSans.ttf`.

Run the gate from the integration checkout.

```sh
bench/baseline/gate.sh
```

The command checks the font digest before building, runs the complete native suite and compares its receipt with `bench/baselines/native/reference.json` using the checked-in limits. An unexplained regression, an invalid fixture, or a changed set of operation names makes it exit nonzero. Treat an added fixture as a reference update requiring a measured receipt. The gate writes a fresh output directory under `runtime/baseline/` for each invocation. The native performance gate runs alongside the existing `bench/gate.sh` code gate.

Use the separate commands for historical source comparisons:

```sh
bench/baseline/run.sh --name base --worktree "$BASE_WORKTREE" --reps 3
bench/baseline/run.sh --name head --worktree "$HEAD_WORKTREE" --reps 3
bench/baseline/compare.sh runtime/baseline/base/base.json runtime/baseline/head/head.json
```

Set `BASE_WORKTREE` and `HEAD_WORKTREE` to the checkouts being compared. Omit `--worktree` to use the harness checkout. Use `--cells sheet_x/rows10000` to run the sheet alone or `--cells sheet_x/rows10000/hover` with `compare.sh` to isolate the hover counter. Use the same face override `BOUGH_FONT` in both runs when measuring façade or effects workloads.

`run.sh` builds and records instruction counts without the lock. Use three repetitions. `--timing` takes the shared lock around each timed child. Unlocked wall and RSS columns are diagnostic. The native gate compares named per-operation counters emitted by `sheet_x`, `events_x`, `text_x`, `anim_x`, `ops_x`, and the effects adapters. The current matrix includes near/swept animation variants, retained-pixel correctness checks for both effects adapters, and the staged paint, panes and video rows from `bench/fx/run.sh`. The JSON also keeps whole-process instructions, wall time and RSS as separate metrics. Results default to `runtime/baseline/<name>/`, with raw logs in `raw/`. `--no-build` requires matching source, toolchain and binary fingerprints.

Default limits are 2% for instructions, 10% for wall time and 5% for process RSS. Wall and RSS are unqualified above the load ceiling. Wall is also unqualified if either input spreads more than the tolerance. See [native measurement scope and threshold overrides](baseline/README.md).

Run `bench/fx/run.sh` for the standalone staged paint replay. It writes the eight `fxbench` invocations and two `pbench` invocations under `runtime/fx/`. The native gate includes the same workloads and verifies the pixel verdicts. Set `FX_BENCH_TIMING=1` for a timing replay with the lock held per invocation.

## 4. The older headless jobs on macOS

```sh
cargo build --profile dist -p bough-heavy -p bough-scenes -p bough-vlist
export BOUGH_DIST="$CARGO_TARGET_DIR/dist"
bench/results/fusion/run_slopes.sh
python3 bench/integrate/micro.py 3
bench/r8/lay_compile.sh
bench/gate.sh
```

The slope and micro jobs also require the corresponding `zgx` binaries. Point `BOUGH_REF` or `ZGX` at the reference sources and `ZGX_HEADLESS` at the staged headless drivers. `bench/zgx/build.sh` builds the workload, animation and vlist drivers. The heavy and micro example binaries come from the separate `zgx` release or dist build. A missing driver exits nonzero. Run the compile probe on a quiet host when comparing compile times. The code gate runs checks and tests, including controls that must fail.

## 5. Validity rules (fail closed)

* Freeze the card before measuring; report every preregistered cell, losses included.
* Take the lock only for commands measuring time, GPU time or RSS. Build outside it. Record load for
  each native repetition and every VM contract. Above the ceiling, time and RSS are unqualified.
* `validity.valid` from fakeland is the VM cell's authority. Missing validity is not valid.
* Instructions are the primary column; wall time is secondary and is compared only inside its own
  spread. Retained state is compared with the output's own counters, not inferred.
* A job that cannot build a binary records the error and exits nonzero. Any affected cell has no measurement.
* A gate whose mutant passes is a broken gate: run `bench/asm/asm.py check` and the baseline's
  `--cells` on a different build before trusting either comparison.
