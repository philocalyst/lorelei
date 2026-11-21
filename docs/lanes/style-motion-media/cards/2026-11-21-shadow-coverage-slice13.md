# Card 13. Retain complete shadow coverage

```text
lane:            style-motion-media
state:           CARD
owner:           fxsol (GPT-6.2 Sol)
predecessor:     e7463cbdbb
question:        Can damaged tiles composite shadow coverage without reintegrating the profile?
change:          Cache Q15 coverage in the scene's retained table arena. The key contains the
                 evaluated radius and blur, phases of all four edges, and the bounded dimensions.
                 Collapse any intervening run where the kernel's support reaches neither end
                 into one sample. Narrow boxes keep both ends in the same patch. Whole-pixel
                 translations change only the patch's origin. Opaque cover masks skip its interior.
                 Refuse a patch when its key changes or its allocation is over budget.
platforms:       CPU tile kernel and Metal. CPU specification remains the exact closed form.
workloads:       The effects matrix, pbench canvas and anim. Record table capacity and launch cost.
gates:           Compare CPU and Metal against the closed form, including all edge phases, narrow
                 boxes, clipping, a whole-pixel move and a fractional move. A stale phase/lookup
                 mutant must fail. All existing scene and ring-age gates apply.
thresholds:      At 600 dense one-spring and theme, CPU frame instructions <= 0.75x predecessor.
                 Kill above 0.95x in both or upon any byte mismatch. Resident coverage budget
                 at most 4 MiB per scene. Every pbench A1 cell <= 1.03x the standing baseline.
measurement:     Three repetitions of frame-thread instructions with load. GPU timings need the lock.
stop conditions: A patch with no exact certificate uses the existing analytic profile.
```
