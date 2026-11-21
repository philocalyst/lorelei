# macOS baseline 71ee3be

The first reproducible baseline recorded by `bench/baseline/run.sh`. Read it with
`bench/RUNBOOK.md` §3; the lanes and thresholds are in `bench/baseline/compare.py`.

* Commit `71ee3be0aa39d37c0fb3bc32997d4aed631e2cc2` of the read-only worktree
  `~/work/lorelei-wt/baseline`; worktree clean at measurement (the cargo run modified
  `comparisons/bough-guibench/Cargo.lock`, which was restored before the recorded run).
* Toolchain `rustc 1.101.0-nightly (5d8fec066 2026-10-16)`, the pinned `nightly-2026-10-17`.
* Host Apple M3 Ultra, 32 cores, macOS 26.7.3. **Load 19.2 → 17.2** (other sessions building), so
  the run is **LOADED**. Instructions are the primary column and their spreads are under 1 %;
  wall times are not usable at this load and the markdown says so. RSS is from `/usr/bin/time -l`.
* Profiles: `dist` for the `bough-x`/`bough-paint` examples, `release` for `bough-guibench-x`,
  `bough-guibench`'s headless driver and the apps. Default features.
* Binaries: `71ee3be.json`'s `binaries` map has sha256, size and `__text` bytes for each.

The cells are `sheet_x` at 1,000 and 10,000 rows, `events_x` at 10,000, `text_x`, `anim_x` 208,
`pbench canvas 1`, and the façade's headless mount/hover/typing at 1,000 rows plus filter at 100 and
filter-nochange at 1,000. Empty slots for the scenes other agents are building are named in the JSON
and the markdown.

Known failure, not a cell: `headless --scenario search --rows 1000 --op filter` panics after about
40 operations with `bough-mem: column glyph_x is full (capacity 524288, need 524304)`. The 100-row
filter and the 1,000-row filter-nochange cells are measured instead.
