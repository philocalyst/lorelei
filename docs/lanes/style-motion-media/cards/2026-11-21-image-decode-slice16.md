# Card 16. Decode image content away from the frame thread

```text
lane:            style-motion-media
state:           CARD
owner:           fxsol (GPT-6.2 Sol)
predecessor:     8d85404c8b
question:        Can compressed content arrive as device-ready premultiplied BGRA without doing decode work on a frame?
change:          A bounded, nonblocking submission queue feeds a single worker. macOS ImageIO
                 admits PNG, JPEG, and WebP by signature, including available animation frames.
                 Frame timing metadata drives selection from an absolute elapsed time.
                 The renderer reads completed frames through the existing BGRA publication seam.
platforms:       Headless macOS ImageIO and the existing CPU/Metal content sampler.
gates:           Decode real fixtures for each format. Check format and size refusal, exact
                 lossless alpha and channels, native JPEG reference colours, and independent
                 animation timings. A mutant that drops later frames must fail the animation gate.
thresholds:      A frame poll never waits for decoding. Bounded input bytes, decoded pixels and
                 frame count. Reject failed decodes before any publication.
```
