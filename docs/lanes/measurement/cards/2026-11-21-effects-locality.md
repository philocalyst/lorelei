# Effects locality and exact shadow reuse

```text
lane:            measurement
state:           CARD
owner:           solfx
predecessor:     1b025a8efd, 71ee3be0aa and wave3 76e38f54f1 (nightly-2026-10-17, optimized)
question:        Which end-to-end phases scale with unrelated records, and can they be repaired without changing pixels?
change:          Attribute the exact one-card trace by phase. Test an exact axis-aligned shadow representation and a clipped typed-core renderer improvement.
implication:     Identifies the scaling phase on each tested offscreen executor. Comparison scores require the same scene and verified frame counts.
platforms:       M3 Ultra headless, software and Metal paths kept distinct
workloads:       Round 1 inputs at 200/1000/5000. Profile the original 20-tick trace, then measure 240 repeating ticks. Canvas 1000 and 5000, hover/theme/resize/full, CPU1/CPUN and Metal where affected.
gates:           Retained equals full and fresh, ring ages 1–4. Keep a missed-update mutant and a bad coverage sample. Use a separate overlapping/fractional control when changing coverage.
thresholds:      <= 1.20x at 5000/200 for single-record turns plus repair. Exact shadow bytes. Reject a cache that cannot beat its setup plus retained memory on repeat frames.
measurement:     Phase counters in the same sampled operation with explicit residual. Cross-check single-thread phase sums against process counts, and observe sampled hot loops. Time whole frames under the lock.
budget:          Lane-private targets and snapshots, nice -n 15 cargo -j 8, no windows or permission prompts.
stop conditions: Unsupported reference capability and failed pixel checks stay explicit. A bounded implementation can be shipped as partial only with the unbounded remainder measured.
do not revisit:  No app-side caches. No reuse of rounded or interpolated coverage where the integer reference disagrees.
```
