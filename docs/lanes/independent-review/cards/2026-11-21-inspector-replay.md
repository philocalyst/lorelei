lane:            independent-review
state:           CARD
owner:           solinsp
predecessor:     inspector d56ea0f5f3 and integration 7991e7bd4e, nightly-2026-10-17
question:        Does the inspector meet its published gates and compose without an off-feature cost?
change:          Replay the developer-loop card, exercise both sides of the socket bridge, and preserve the original report.
implication:     Bounded headless verification of this app adapter and the core examples.
platforms:       macOS aarch64, headless CPU
workloads:       Rows and Table, inspector hover, sheet_x, events_x, anim_x, release workspace
thresholds:      All six published mutants fail at their intended assertions, workspace adds no baseline failures,
                 off-feature core operation instructions equal the integration baseline, text-byte delta zero.
measurement:     Per-thread instructions, three repeats with load average and matched release builds.
stop conditions: Document a falsified claim or a build/platform boundary with the observed result.
reformulation:   Keep cold diagnostics on the reflection walk. Where per-leaf metadata is absent, prefer a
                 cold source-site table keyed by record position to adding a source location to every live row.
