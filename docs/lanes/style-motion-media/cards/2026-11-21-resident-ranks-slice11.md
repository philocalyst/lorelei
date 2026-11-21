# Card 11. Resident Metal ranks without per-frame encoding

```text
lane:            style-motion-media
state:           CARD
owner:           fxsol (GPT-6.2 Sol)
predecessor:     47902dcba1, pinned nightly, release gates, dist measurements
question:        Can the Metal frame thread submit reached layer work at a cost independent of depth?
change:          First select the earliest reachable rank while uploading tile events and forced
                 stamps. A tile's sorted layer keys identify the first reader above its event.
                 Repaired readers emit only within their own halo, where every later reader
                 has a greater rank. Submit the suffix from the first seed, or no layer stages
                 if there is no seed. Next store the layer sequence in a reusable indirect
                 compute command buffer with explicit dispatch barriers. A bounded direct
                 prefix dispatch guards workgroups by the compacted count. Clear layer counters
                 on the GPU. Keep output compaction and indirect output shading unchanged.
implication:     A frame that cannot affect glass owes zero layer dispatches. Replaying an
                 existing schedule costs one submission on the frame thread even at depth.
platforms:       Headless Metal on macOS, exact CPU layer kernel oracle.
workloads:       Same full instruction matrix and pbench workloads as slice 10.
gates:           Retained Metal at ages 1–4 equals CPU fresh on events, moving halos, broad
                 changes, resized layer storage and array identity changes. Hover above
                 every panel launches zero layer stages. A mutant omitting event seeds
                 must produce stale pixels. A mutant reusing layer commands after a slot
                 layout change must fail a bounded fixture.
thresholds:      One spring on Metal at 3,000 dense <= 1.2x 600 dense. Hover above all glass
                 at 3,000 dense <= 120 K, zero layer dispatches. No pbench A1 cell >1.03x
                 baseline. Record any scene-dependent topology rebuild or GPU dispatch cost.
measurement:     Frame-thread instructions, three repetitions with load, no bench lock.
                 Record GPU cost only if measured under the timing lock.
budget:          One lane worktree, no helper agents.
stop conditions: If ordered indirect compute is unsupported or byte-inexact, use the rank
                 suffix on the serial encoder and record the device or method boundary.
do not revisit:  CPU readback between dependent ranks or unordered indirect compute.
```

The slot schedule already represents the dependency graph. The frame supplies tile keys and forced stamps. Encoding moves to the lifetime of the schedule. A later experiment can join neighboring stages on the device if dispatch boundaries remain expensive there.
