# Independent replay of the typed core

```
lane:            independent-review
state:           CARD
owner:           review
predecessor:     71ee3be0aa (nightly-2026-10-17, default features and one mutant per build, release; dist for counters)
question:        Do the composed typed core gates and reported performance survive independent replay and boundary inputs?
change:          Replay existing gates and review the safe API. Fix isolated correctness defects with regression tests.
implication:     Establish which gates hold for this source and host and identify blockers for the next integration.
platforms:       Apple Silicon macOS, headless CPU and existing headless executor parity tests
workloads:       Cards 1 and 2 targets and mutants, original review inputs where available, sheet/events/layout microfixtures, Miri on pure crates
gates:           Existing default tests pass, each named mutant compiles and fails its own oracle. Compare retained against full/fresh where applicable. Run new regressions on base and fixes.
thresholds:      No newly reproduced safe-output defect for acceptance. Sparse and retained limits follow Card 1. Counter replay target within 5% of the published cell at matching inputs, with both historical revision and current source recorded when available.
measurement:     Exact operation counts and the repository instruction counter harness, three runs per cell where available, exclusive bench.lock and host load. No latency or display claim.
budget:          Dedicated target, nice 15, eight cargo jobs, serial cargo within this worktree.
stop conditions: Record boundaries for absent original bytes, unavailable toolchain components or denied counters. Report all actual cells and keep independent acceptance contingent on the five report gates.
do not revisit:  docs/SETTLED.md
```
