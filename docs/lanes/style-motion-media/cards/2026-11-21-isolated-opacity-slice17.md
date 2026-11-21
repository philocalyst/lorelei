# Card 17. Isolate child paint for one group opacity

```text
lane:            style-motion-media
state:           CARD
owner:           fxsol (GPT-6.2 Sol)
predecessor:     5847a5c193
question:        Can a scoped child scene retain its own transparent composite and let the parent apply opacity once?
change:          Retain the child's BGRA layer surface and repair only its damaged tiles.
                 Publish a replacement content slot if the child surface changes. The
                 parent samples it as an Image. Opacity changes need no child shade.
implication:     A first isolated layer can use the same parent paint on CPU and Metal.
                 Publishing copies the group rectangle and the parent damages its window.
                 Subsequent cards should move this surface into the scene's shared layer
                 store and propagate child damage at tile grain through the group reader.
platforms:       CPU child scene, CPU and Metal parent, headless macOS
workloads:       A translucent overlapping child pair, parent opacity changes, a child
                 rewrite and a publication refusal. The standing paint matrix remains valid.
gates:           Independent premultiplied reference and exact CPU/Metal parents. A mutant
                 that omits a later child's publication must fail retained versus fresh.
thresholds:      Child idle visits no records and shades no tiles on parent-only opacity.
                 A failed publish keeps the last good image and retries on a later frame.
```
