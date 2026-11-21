# M1: runner accessibility, an asm/graph harness, and a reproducible macOS baseline with a regression gate

lane:            measurement
state:           CARD
owner:           measurement session (bench)
predecessor:     71ee3be0aa (nightly-2026-10-17; workspace profiles release/dist; gui-bench 13f3cb6; fakeland b06eace)
question:        Can zortax's gui-bench/fakeland matrix be run from this macOS host, can the hot functions of the new
                 core be inspected as assembly and as graphs from one command, and can a named commit's headless
                 measurements be recorded and compared so that an integration regresses beyond a stated threshold?
change:          Infrastructure only. No framework crate changes.
                 (1) A Lima/VZ Linux aarch64 VM (`bench/vm/`): Ubuntu, a pinned Rust toolchain, fakeland + the
                 gui-bench runner, and gui-bench registrations for lorelei's two apps (façade row `bough`, new-core
                 row `bough-x`), with one command per job. (2) `bench/asm/`: a manifest of hot functions and scripts
                 that emit their assembly (`cargo asm`), count instructions/branches/calls/panics/bounds checks,
                 diff two commits, and render call/dependency and crate graphs with Graphviz. (3) `bench/baseline/`:
                 a runner that freezes an exact binary manifest and measures every existing headless benchmark in
                 randomised paired blocks under a single lock, and a `compare` command that fails on regression.
implication:     Passing establishes that the Linux matrix is runnable and reproducible from this host with its
                 contract (VM, lavapipe, core pinning, timing mode) recorded per run; that the asm harness reproduces
                 a known compiler behaviour (a negative control that must fail); and that the recorded baseline is a
                 stable reference whose compare command catches an injected regression. It does not establish
                 hardware-GPU performance, scanout latency, or that any number is a framework ranking. Numbers from
                 the VM are Linux/software-rendered and are never pooled with macOS numbers.
platforms:       Host: Apple M3 Ultra, macOS, headless. VM: Linux aarch64 under Apple Virtualization (lima vz),
                 llvmpipe/lavapipe software Vulkan, no hardware GPU. Native path: macOS aarch64, CPU work only.
workloads:       VM: gui-bench `--quick` cells for `bough`, `bough-x`, `zgui` (egui/iced if the build completes).
                 Native: bench/integrate/micro.py (micro_bench), bench/integrate/slope.py (heavy/lab/animate),
                 bench/r8/lay_compile.sh, bough-guibench headless (mount/hover/typing/filter rows 100/1000),
                 bough-guibench-x tests, kernel/paint scene benches, compile time and binary size.
gates:           (a) VM: a retired `--quick` run writes `run.json` with a results line and no cell errors for each
                 registered framework, exit status propagated. (b) Asm harness: negative control — a function
                 compiled with `-C opt-level=0` must show more instructions than the release build of the same
                 function, and the harness must report the difference; a second mutant must fail the panic/bounds
                 counter. (c) Baseline: `compare` returns 0 on identical runs and non-zero when a deliberately
                 different build (release vs release+`-C target-cpu=native` measured cell or a forced sleep) is
                 compared beyond threshold. Every gate's failing case is run and recorded.
thresholds:      Per-cell (each JSON cell): instructions ≤ 1.02x baseline, wall p50/p95 ≤ 1.10x, RSS/footprint ≤
                 1.05x. Cells with a baseline spread above 5 % are compared against that spread (max(1.02, 1+3σ));
                 never tightened after seeing a result. Compare fails on any cell over its threshold, or on a
                 missing cell unless `--allow-missing`.
measurement:     Instructions primary (`/usr/bin/time -l` on macOS reports retired instructions; `perf stat` in the
                 VM), wall p50/p95/p99 from repeated cells, RSS (`time -l` maximum resident size) and footprint
                 where the binary reports it. Randomised paired blocks (ABBA order per repetition) and one lock
                 (`lockf -k ~/work/locks/bench.lock`) for every timed batch. Load average is recorded with each run;
                 a run above load ceiling 2.0 on the timed host is marked `loaded` and compare warns.
budget:          Local only. VM: 10 vCPU, 16 GiB, 80 GiB disk. Builds niced, at most 8 jobs. No remote hosts.
stop conditions: A VM route that cannot build fakeland or the runner after the documented fixes: record the exact
                 errors and fall back to the native path, stating that the Linux matrix is not runnable here.
                 A baseline cell that cannot be measured at all is recorded as `unavailable` with the error, not
                 silently dropped.
do not revisit:  Nothing in docs/SETTLED.md is touched. No benchmark-specific code path, no cache the framework does
                 not provide, no estimate ranked with measurements.

## Amendments (after measurement started; the original stays)

1. **The asm negative control.** As written it was `-C opt-level=0` against release. It was run and
   is not usable as a gate: a different inlining regime changes which symbols exist at all, so
   `column_paint` (1,806 → 340), `state_compare` (756 → 337), `turn_after` (910 → 14) shrank and the
   control reported "MUTANT NOT DETECTED". Replaced before any gate was claimed with a same-profile
   source mutant: `--features asm_mutant` makes three control wrappers (`turn_settle`, `turn_paint`,
   `kernel_shade_wrap`) call an `#[inline(never)]` 64-iteration black-boxed body, and the check
   requires each to grow. It passes (`10→12`, `35→37`, `52→54`, each plus the extra body). The dev
   comparison is now printed as information only. Reason recorded here, not fitted to a result.
2. **The baseline host was loaded.** The first baseline at `71ee3be` was taken at load 11.0-18.2
   (other sessions building). Instruction counts are the primary column and their spreads are under
   1 %; wall and RSS are recorded but the run is marked `loaded` and wall comparisons suppress
   themselves when the baseline spread exceeds the threshold. A quiet re-run is the next card.
3. **The compare gate's non-zero case** was exercised with a synthetic mutation of the recorded JSON
   (one cell ×1.5) because no second, genuinely different build could be measured on this host
   today. The gate returns 1 with `REGRESSED: instructions 1.500x > 1.02`; the positive case returns
   0. A real different-build control remains owed.
