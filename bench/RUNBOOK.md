# bench/RUNBOOK.md — running the measurement infrastructure

Written for an agent or person with no context. Every job is one command, fails closed, and writes
self-describing results. Do not invent a number: if a job did not run here, it is not measured.

Machine: the owner's Apple M3 Ultra (`h16003`), macOS, headless. Work happens in a git worktree
under `$HOME/work/lorelei-wt/<name>` with its own `CARGO_TARGET_DIR` under `$HOME/work/targets/<name>`.
Put **rustup's proxy first** on `PATH` (`/opt/homebrew/opt/rustup/bin`); Homebrew's `cargo` is a
different, stable toolchain and the workspace pins `nightly-2026-10-17`.

```sh
export PATH=/opt/homebrew/opt/rustup/bin:$PATH RUSTC_WRAPPER= CARGO_TARGET_DIR=$HOME/work/targets/<name>
```

## 0. What each number is

| job | host | renderer | timing | comparable with |
|---|---|---|---|---|
| `bench/vm/run.sh` | Linux aarch64 VM (Apple Virtualization, 10 vCPU) | lavapipe (software Vulkan) for Vulkan apps; bough's CPU executor into `wl_shm` | fakeland's fixed 60 Hz display | only itself, same VM and pinned CPUs |
| `bench/baseline/run.sh` | this Mac, native | CPU work only; the bough-x examples count their own PMU | instructions retired per whole process | only itself on this Mac |
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
bench/vm/up.sh                                  # first time: ~10 min (image + apt + toolchains)
FRAMEWORKS=bough,bough-x,zgui bench/vm/build.sh  # first time: ~10 min (zgui dominates)
FRAMEWORKS=bough,bough-x,zgui SCENARIOS=rows,table,search bench/vm/run.sh
```

`run.sh` takes the lock (`lockf -k $HOME/work/locks/bench.lock`) around the whole build and run;
its timestamped output lands in `bench/results/vm/<stamp>/` with `CONTRACT.txt` (guest, VM, toolchains,
renderer, timing, CPUs, load, binary sha256) beside `run.json` and one directory per cell. Read a
cell's numbers in `startup.json` (`first_present_ms`), `hover.json` / `scroll.json` (the `latency`
section, and `validity`), `idle.json`, `op-*.json`, `churn.json`, `filter.json`, `typing.json`.
Every cell must be present in `run.json`; a cell error is printed at the end and is a failure.

Exit status: the runner's. The runner currently returns 0 even when a cell had errors (its own
behaviour); `run.sh` prints the error count and its own exit follows the runner. Treat a non-empty
`"errors"` in any `cell.json` as a failed cell whatever the exit status.

The VM needs these workarounds, recorded in the contract: `VK_LOADER_DRIVERS_DISABLE=` is emptied
(otherwise Mesa's device-select layer hides lavapipe and no Vulkan adapter exists), `taskset -c 0-5`
pins the runner and every app, and `dbus-daemon` must be installed (fakeland starts a private bus).
`bench/vm/patches/` carries the two source-level registrations the VM applies to its *copies* of
lorelei and gui-bench; the repository's framework source is untouched.

## 2. The assembly and graph harness

```sh
python3 bench/asm/asm.py list
python3 bench/asm/asm.py emit --out bench/asm/out/$(git rev-parse --short HEAD)
python3 bench/asm/asm.py diff OLD_DIR NEW_DIR [--threshold 0.02]
python3 bench/asm/asm.py graph crates --out /tmp/crates.svg
python3 bench/asm/asm.py graph expr   --out /tmp/expr.svg
python3 bench/asm/asm.py graph calls  --out /tmp/calls.svg --emitted bench/asm/out/HEAD
python3 bench/asm/asm.py check        # negative control: a cfg mutant must move the count
```

`emit` builds the harness's symbols (`bench/asm/harness`: the gui-bench `rows` scene and the layered
canvas, plus the layout memo) and writes `fn/<entry>.s`, `raw/`, `stats.tsv` and `MANIFEST.txt`
(commit, rustc, profile, host, load). Each entry counts instructions, branches, calls, panic paths
and bounds-check/`brk` sites. `diff` sums an entry's symbols and exits 1 when it grows past the
threshold (default 2 %, above 16 instructions). `check` is the gate on the gate: with
`--features asm_mutant` the control wrappers do real extra work and the count must grow.

Run it alone (no build on another core), because `cargo asm` invokes cargo. Read
`bench/asm/README.md` for the symbol names and the two documented boundaries (inlining regimes and
the reflection export).

## 3. Reproducible macOS baselines and the regression gate

```sh
bench/baseline/run.sh --name 71ee3be --worktree "$HOME/work/lorelei-wt/baseline" --reps 3
bench/baseline/compare.sh bench/baselines/macos-71ee3be/71ee3be.json NEW.json
```

`run.sh` takes the lock and calls `bench/baseline/run.py`. It refuses to time anything it could not
build, records the commit (dirty flag), rustc, host, load before/after, binary sha256 and `__text`
bytes, and runs every cell in three deterministically shuffled rounds. Cells: `sheet_x` at 1,000 and
10,000 rows, `events_x`, `text_x`, `anim_x`, `pbench canvas`, and the façade's headless cells
(mount, hover, typing, filter at 1,000 rows). Output: `bench/baselines/macos-<name>/<name>.json` and
`.md`, with `slots` naming the scenes other agents are still building.

Thresholds per cell (median): instructions ≤ 1.02×, wall ≤ 1.10×, RSS ≤ 1.05×. A wall comparison is
suppressed when the baseline itself spread more than the threshold; `--thresholds limits.json`
overrides per cell. A cell missing from the head run fails unless `--allow-missing`. A head run above
load 2.0 is marked `loaded`; `--require-quiet` makes that a failure.

To add a cell for a new scene, add it to `cells()` in `bench/baseline/run.py` with an argv, env and
parser, and fill its slot in `SLOTS`. Never add a cache or shortcut the framework does not have.

## 4. The repo's older headless jobs (still valid, macOS)

```sh
cargo build --profile dist -p bough-heavy -p bough-scenes -p bough-vlist
bench/results/fusion/run_slopes.sh         # per-frame instructions, bough vs zgx, RENDER 0/1
python3 bench/integrate/micro.py 3         # micro_bench ns/op (needs zgx headless drivers)
bench/r8/lay_compile.sh                    # per-row-type compile time and __text bytes
bench/gate.sh                              # the code gate (check, tests, sabotage), not a benchmark
```

`bench/results/fusion/run_slopes.sh` and `micro.py` expect `target/fusion/dist`; point
`BOUGH_DIST` at wherever the dist binaries were built. `zgx` drivers need `bench/zgx/build.sh` and
the zgx checkout (`BOUGH_REF`/`ZGX`). A cell of these jobs that needs a competitor's binaries is not
a bough baseline; `bench/baseline/` deliberately measures bough alone and labels the competitors'
cells unavailable.

## 5. Validity rules (fail closed)

* Freeze the card before measuring; report every preregistered cell, losses included.
* One lock for every timed batch; load average recorded with every run (baseline) or every contract
  (VM). Above the ceiling the run is marked, never silently ranked.
* `validity.valid` from fakeland is the VM cell's authority. Missing validity is not valid.
* Instructions are the primary column; wall time is secondary and is compared only inside its own
  spread. Retained state is compared with the output's own counters, not inferred.
* A job that cannot build a binary writes the error and exits non-zero; it does not write a cell.
* A gate whose mutant passes is a broken gate: run `bench/asm/asm.py check` and the baseline's
  `--cells` on a different build before trusting either comparison.
