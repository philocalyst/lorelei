# Card 19. Cache SVG raster output by source and device size

```text
lane:            style-motion-media
state:           CARD
owner:           fxsol (GPT-6.2 Sol)
predecessor:     2e956b3c48
question:        Can SVG author assets reuse device-ready pixels on the same image paint seam?
change:          Send an SVG byte string and target pixel dimensions to the existing bounded
                 worker. ImageIO rasterizes the source and a capped exact-key cache retains
                 the resulting BGRA. The typed Image paint reads a published result.
platforms:       macOS headless ImageIO, existing CPU and Metal image paints
workloads:       Two-size asymmetric SVG fixture, repeated requests, over-budget input.
gates:           Independently expected solid and transparent pixels, queue replay
                 reuses the shared allocation, distinct sizes have distinct pixels.
                 A mutant ignoring raster dimensions in the cache key must fail.
thresholds:      SVG decode and cache are worker operations. Cache stays within 4 MiB.
```
