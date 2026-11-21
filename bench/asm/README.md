# bench/asm — the assembly and graph harness

The owner's "harness for examining assembly and graphs" is two existing families of tools plus this
harness, which makes the first usable on lorelei's own hot functions:

* **zgx `scripts/perf/` (upstream, the closest thing to a full harness)**: `suite.py` (retired
  instructions per headless scene, A/B builds), `cgdiff.py` (cachegrind instructions by function,
  base vs head), `inline_scopes.py` (release MIR inlined-callee counts, the inlining gate),
  `sizes.py` (`llvm-size`/`llvm-nm` per section and symbol), `samples.py`/`lines.py`/`frames.py`
  (`xctrace` exports, source lines and stack sizes), `record.sh` (the Time Profiler wrapper),
  `pgo.py`. Its README is `zgx/scripts/perf/README.md`.
* **zgui `cargo xtask perf` + `zgui-bench` + `zgui-profile`**: every number in `docs/performance.md`
  is written by a run and compared with a per-measurement band; `zgui-profile` provides frame-stage
  and work counters. `zgx`'s `ZGX_PROFILE=1` is the same idea per phase.
* **This harness** applies them to lorelei: `cargo asm` (installed, 0.2.65) for the exact body of a
  named function, `dot` for graphs, and a manifest of the new core's hot functions. lorelei's own
  `bench/results/r8/*` TSVs and `docs/lanes/*/JOURNAL.md` are the earlier hand-run measurements of
  the same functions; this harness reproduces them from one command.
* **fakeland's traces and reports** (`crates/fakeland-trace`, `fakeland-bench`): every session
  records a typed event trace and turns it into per-frame reports; this is the *behaviour* graph
  (input → commit → latch → present, with analyzers), not a call graph. Commands:
  `fakeland run --duration 5s --trace run.jsonl --perfetto run.json -- <app>` then
  `fakeland bench hover-latency --report hover.json -- <app>`; `fakeland ctl shot` for a pixel.
  The GUI runner in `bench/vm/` uses these presets.
* **gui-bench's `plots/`**: `python3 plots/all.py results/latest` writes `<run>/plots/<scenario>/`
  PNGs and `<run>/summary.csv` (matplotlib; Libertinus font vendored). These plot the matrix, not
  a program's structure; the runner's JSON is the authoritative data.
* **lorelei's own recipes**: `docs/lanes/threads/MEASURE.md` has `hot.sh`, `lines.sh` and `alloc_x`
  (perf counters, cachegrind lines, allocation counts), `bench/results/r8/*` are the TSVs the lanes
  published, and the journals name the disassembly each finding came from. `bench/r8/*.sh` measure
  compile time and `__text` per component. Use those where they exist; use this harness for what
  they do not emit: per-symbol assembly counts and the two graphs.

## Files

| file | what |
|---|---|
| `manifest.tsv` | the hot functions: name, package, artifact, demangled-symbol substring (`^` = suffix match) |
| `asm.py` | `list`, `emit`, `diff`, `graph {expr,crates,calls}`, `check` |
| `harness/` | its own workspace; drives the gui-bench `rows` scene through the app's headless ring and the layered canvas, so the generic bodies exist as symbols |

The manifest's entries: `turn_settle`, `turn_after`, `turn_resize`, `turn_paint`, `state_compare`
(`World::changed`), `block_delta`, `mount_retable`, `handler_call`, `column_delta`, `column_paint`,
`span_executor` (`Store::paint::<Spans>`), `span_text`, `kernel_shade`, `kernel_pick`,
`layout_row` (`bough-lay`'s `Memo::row` on the façade's sheet row). The wrappers named
`asm_harness::w_*` exist because an inlined function has no symbol; each is `#[inline(never)]`.

## Commands

```sh
export PATH=/opt/homebrew/opt/rustup/bin:$PATH RUSTC_WRAPPER= CARGO_TARGET_DIR=$HOME/work/targets/<you>/asm
python3 bench/asm/asm.py emit --out bench/asm/out/$(git rev-parse --short HEAD)
python3 bench/asm/asm.py diff bench/asm/out/BASE bench/asm/out/HEAD
python3 bench/asm/asm.py graph expr --out /tmp/expr.svg     # learned dependency graph of the rows scene
python3 bench/asm/asm.py graph crates --out /tmp/crates.svg # workspace crate graph
python3 bench/asm/asm.py graph calls --out /tmp/calls.svg --emitted bench/asm/out/HEAD
python3 bench/asm/asm.py check                              # the negative control
```

`emit` runs `cargo asm --everything` once per artifact and slices each entry's symbols; `stats.tsv`
has one row per matched symbol with instructions, branches, calls, panic-call references and
bounds-check/`brk` sites. `diff` sums an entry's symbols and fails above `--threshold` (2 %) and 16
instructions. It exits 2 (not comparable) unless both directories'
`MANIFEST.txt` agree on rustc, profile, codegen, rustflags, host, features and mutant, and unless both
contain every manifest entry; missing, empty or malformed `stats.tsv`, a zero-instruction base entry and
a negative threshold also fail. Exit codes: 0 pass, 1 regressed, 2 invalid comparison/input. `graph expr` uses only the public reflection API (`bough_x::reflect`): it prints one
line per learned column (store, column, kind, stage, the 64-bit word mask it reads, operands, record
type) and renders word → column edges; that is the staged-expression dependency graph and the reader
lists, as the core itself knows them.

## The negative control

`python3 bench/asm/asm.py check` builds the harness twice: normally, and with `--features asm_mutant`,
where the three control wrappers (`turn_settle`, `turn_paint`, `kernel_shade_wrap`) call an
`#[inline(never)]` body of 64 black-boxed multiply-adds. The check fails unless every control entry
grew by more than 8 instructions. The dev-profile comparison it prints is information only: a
different inlining regime changes which symbols exist, so per-symbol counts are not comparable
across profiles. That is the documented boundary of the method — the gate compares same-profile
builds of the same manifest.

## What it does not do

* No flame graph is produced here. macOS `sample <pid>` and `xctrace` work headless for a process
  you started; use zgx's `scripts/perf/record.sh` + `samples.py` on a bough binary built with
  `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only`. Nothing in this harness fakes one.
* No GPU assembly: Metal/Vulkan shader compilation is out of scope; `bough-paint`'s generated
  functions are inspected as CPU code through `kernel_shade`.
* The expression dump is the *rows* scene; add a scene by adding a binary next to
  `harness/src/bin/reflect.rs`.

## Frozen compiler flags

Every cargo child runs with `RUSTFLAGS`, `CARGO_ENCODED_RUSTFLAGS`, `CARGO_BUILD_RUSTFLAGS` and
`CARGO_TARGET_*_RUSTFLAGS` removed; only `--rustflags` reaches the build. Removed variables are named
with a sha256 prefix of their value in `MANIFEST.txt` (`ambient-flags-ignored`) and on stderr. Flags set in
a `.cargo/config.toml` are not removed (the repository's own sets some); they are part of the recorded commit, so `MANIFEST.txt`'s `commit` and `dirty` fields identify them.

## Fast tests

`python3 bench/asm/test_asm.py` exercises the fail-closed paths (mismatched context, empty or
malformed stats, partial emits, empty graphs, bad manifests) without cargo.
