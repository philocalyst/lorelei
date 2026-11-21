# Card 15. Retained RGBA content with the media publication seam

```text
lane:            style-motion-media
state:           CARD
owner:           fxsol (GPT-6.2 Sol)
predecessor:     7cf4214f4a
question:        Can decoded images and retained group pixels share an exact content slot?
change:          Add premultiplied BGRA8 slots to the existing media arena. A typed paint
                 samples the same format on CPU and Metal with bilinear integer filtering.
                 Publishing changes one handle. Slot headers validate dimensions and format.
                 An already published slot remains immutable through the frame that retires it.
implication:     Compressed and vector decoders can publish independently of frame traversal.
                 Group integration can retain its child pixels and composite them once.
platforms:       Headless CPU and Metal
workloads:       Exact content fixture and the full standing instruction matrix.
gates:           Exact asymmetric colour/alpha reference, rounded clipping, replacement,
                 ring ages 1–4, malformed handles, and CPU/Metal equivalence. A mutant swaps
                 red and blue on a sampled content slot and must fail.
thresholds:      A new slot visits the record that publishes it and does not re-bin unchanged
                 geometry. Kill on a mismatch or an admitted out-of-range sample.
```
