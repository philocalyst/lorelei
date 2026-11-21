# Card 10. Park motion by where it can reach

```text
lane:            style-motion-media
state:           CARD
owner:           fxsol (GPT-6.2 Sol)
predecessor:     2da10f0603, pinned nightly, release gates, dist measurements
question:        Can a clock-only frame with all springs launched cost the motion that can meet the frame?
change:          Expr::time_hull is an optional component-wise bound for fixed non-clock inputs.
                 Fixed-point springs use integer extrema from the actual law through settle. Progress and
                 cubic eases use their closed bounds. Rect arithmetic lifts the bound with checked sums.
                 Paint uses the swept window and halo for active membership. The pan index keeps swept
                 rows for moving geometry and visits unknown bounds on a pan as a fallback. Rewrites
                 and geometry turns reconsider membership. Clock-only visits start from active indices.
implication:     Steady frames stop following world size for spatially bounded motion. A bulk retarget
                 still changes one motion law per record and its cost must appear in the raw measurements.
platforms:       CPU tile kernel and Metal resident, headless macOS
workloads:       effects 200 / 1,000 / 5,000, 600 / 3,000 dense, pane 300 / 3,000, media 4 / 24 / 48,
                 PALETTE=1 effects 200 / 1,000 / 5,000, pbench canvas 4 and anim 600
gates:          Compare exact spring hulls against discrete leaf samples, including interruption,
                 stagger, overshoot and all damping modes. Compare CPU retained and Metal against fresh
                 across motion entering by itself, a pan, and a retarget, at ages 1–4. The mutant
                 mutant_hull_small drops spring overshoot or travel and must fail. Replay existing gates.
thresholds:      All springs Metal median max/min <= 1.2x across 200/1,000/5,000, with <= 50 steady
                 visits per size. No pbench A1 cell >1.03x baseline. Record separately any launch burst
                 and other cell regression over 1.10x predecessor where matched. Kill above 1.2x or on
                 byte mismatch. Any admitted hull that misses an actual sample invalidates the method.
measurement:     Frame-thread instructions, three repetitions, recorded load, no bench lock.
                 Dense six measured frames per repetition, all other workloads at the default 24.
budget:          One lane worktree, no helper agents.
stop conditions: Use the time-active fallback when no safe bound is available.
do not revisit:  Per-record integration at refresh, or any GPU readback to schedule motion.
```

The spring's stored law supplies the spatial bound. Scroll still updates visible bins. The rank schedule and CPU prefix check have separate cards. Report steady and launch costs independently so the work of changing many laws remains visible.
