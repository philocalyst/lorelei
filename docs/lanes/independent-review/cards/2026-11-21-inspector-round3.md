lane:            independent-review
state:           CARD
predecessors:    c7fef9b474 and pre-recovery inspector r2 at 933a74a5d3
question:        Do the inspector golden, replay and IDs survive internal mount revision changes?
change:          Document volatile wire fields and use one description projection for goldens and new replay logs.
workloads:       Rows at 20 rows, 200 by 160, scale 1, plus the existing replay and workspace suites.
gates:           Compare real pre- and post-recovery mounts. Require equal node, kind and operand identities and
                 equal contract bytes, with a changed structure revision. Keep replay sensitive to a changed
                 description, pixels or refusal. Preserve an explicit compatibility path for earlier logs.
controls:        Retain the old failing golden receipt. Change a contract field and require a replay mismatch.
                 Compare the complete live snapshots after masking only the documented volatile fields.
thresholds:      One deliberate re-bless. No workspace failures beyond the four existing missing-font cases.
measurement:     Headless macOS aarch64 with pinned nightly. Compare serialized fields and release test totals.
