# Measurement artifact policy

Applies to everything a measurement run writes: `bench/results/**`, `bench/baselines/**`, and any
lane evidence. Older evidence is not rewritten; this governs new additions.

## What may be tracked

* A **compact summary per run**: one JSON, at most a few hundred KB, one file per run
  (e.g. `bench/results/vm/<stamp>.json`). Generate it with
  `python3 -I docs/lanes/measurement/tools/summarize_vm_results.py <raw-run-dir> <out.json>`.
* A baseline directory with the manifest/JSON, a short markdown, and a README.
* Budget: no tracked measurement file over 1 MB, no run adding more than 1 MB or 20 files.

## What stays out of the tree

Per-cell JSON trees, `app.log`, screenshots, traces, `target/` dirs, binaries. Keep raw output
**ignored** (`bench/results/vm/.gitignore` ignores every `<stamp>/` directory) or **external**
(outside the repo; record only the run stamp and the sha256 of any archive in the summary).
Never commit hundreds of raw files; if raw evidence must be kept, ship one archive externally.

## Required summary content

* Run stamp, and the contract fields (toolchain, renderer, timing mode, pinning, load before/after,
  binary hashes).
* **Scope**: for every cell, the scenario, framework and its scope (which implementation, which
  renderer), plus the timing scope (fixed 60 Hz, unthrottled, ...). A number without its scope is not
  evidence.
* **Explicit state** per cell: `valid`, `invalid` (reported, never ranked), `error`, `startup-only`
  (no validity check exists) and a `missing` list of stages the runner reported done but wrote no
  file for. Never drop a cell silently; never turn a missing value into 0.
* VM numbers are never pooled with native macOS numbers.

## No personal or local identifiers

Do not commit user names, machine/host names, tailnet names, absolute local paths (`/Users/...`,
`/home/...`), home-relative checkout layouts, display serials/makes, or e-mail addresses. Use
placeholders (`<baseline-worktree>`, `<ref-checkouts>`, `<target-dir>`). The summarizer drops pids,
commands, hostnames and display identity. Check before committing:

```sh
git diff --cached | grep -n -E '^\+' | grep -E "/Users/|/home/|~/work|[$]HOME/work|[.]ts[.]net|<your-user>|<your-host>"
```
