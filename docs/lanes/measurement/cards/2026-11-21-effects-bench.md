# Effects benchmark and standing losses

```text
lane:            measurement
state:           CARD
owner:           solfx
predecessor:     71ee3be0aa (nightly-2026-10-17, release/dist)
question:        What do layered effects and concurrent motion cost end-to-end, and which standing losses reproduce headless?
change:          Introduce a deterministic offscreen effects trace and pixel gates, with a distinct forensic replay of the historical cells.
implication:     Measures headless client work on witnessed features and identifies measured causes where controls are available.
platforms:       Apple Silicon macOS CPU/Metal; Linux compositor cells require a separate Linux replay.
workloads:       EFFECTS-BENCH.md matrix at 200/1000/5000 and all listed historical losses.
gates:           Full and retained byte equality across ages 1–4, fresh mount at rest, change witness, dropped-input mutant fails.
thresholds:      Per-cell targets in EFFECTS-BENCH.md. Loss patches require an unchanged oracle and measured improvement.
measurement:     Shared lock, cell load, process instructions and CPU, frame percentiles, GPU timestamps, RSS/footprint and damage area.
budget:          One lane target, nice 15, cargo -j 8, no window activation or permission prompts.
stop conditions: Record unavailable features and missing dependencies at each affected cell. Report controls independently of the valid matrix.
do not revisit:  App-side benchmark caches, per-row hover signals, generic kernel as default, v7 bin scan, refuted corner-table GPU targets.
```
