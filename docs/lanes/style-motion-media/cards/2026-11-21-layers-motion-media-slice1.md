# Card: effects, motion and media as residents of the damage-is-dispatch pipeline (slice 1)

```
lane:            style-motion-media
state:           CARD
owner:           fx session (Claude, claude-fable-5-2), 2026-11-21
predecessor:     71ee3be0aa (nightly-2026-10-17; release for gates, dist for numbers; default features)
question:        Can effects that are not pointwise functions of one record (backdrop blur and filters), motion
                 that ends (springs), and media that changes by itself (video) live in bough-paint's fused
                 per-archetype kernel so that cost follows change: idle costs nothing, an element above a blurred
                 layer costs nothing extra, one moving element costs the same in a scene ten times larger?
change:          (1) A *layer* is a retained resource owned by the record that reads it (a backdrop record): the
                 8-bit prefix of everything beneath it over its halo, and a pyramid of it ([1 3 3 1] tent
                 downsamples anchored to the global pixel grid), each level a toroidal pixel window with dirty
                 32 px blocks. Damage reaches a layer as (window, key) events, is dilated level by level in block
                 space, and comes out as output damage of the reader. A frame is a staged tile program
                 (prefix jobs, pyramid jobs per layer in paint order, then output tiles); no traversal, no
                 per-frame redraw of a layer. Replaces: nothing (the kernel had no such effect).
                 (2) `Paint::until`: records say when they stop depending on Time; the scene keeps an active set
                 per column and a time-only frame visits only those records. Replaces the whole-column window
                 pass on every change of the Time word. Springs are a closed-form integer leaf of Time
                 (`x_spring`), retargeted with value and velocity continuity; `Scene::next_wake` tells the host
                 whether any frame is owed (output pacing; none when everything rests).
                 (3) `Video`: an NV12 frame is a leased, immutable slot of a media pool the kernel samples
                 directly (BT.709 limited range, integer); publishing a frame rewrites one record word, which
                 damages the video's visible window and nothing else.
                 (4) `Shader`: the kernel's user-shader node as a paint (time and record words in, damaged only
                 where exposed and only when an input moves).
implication:     Passing establishes the layer representation and its damage algebra on the per-archetype scene
                 (CPU tile kernel and Metal, byte-equal), with one reader kind (backdrop blur + brightness /
                 saturate / contrast). It does not establish subtree groups, masks, blend modes, 3-D, real
                 decoders, IOSurface zero copy, the GPU-resident (k_win/k_bin) path for layer scenes, or the
                 element-layer author surface (a seam document is published instead).
platforms:       macOS, Apple silicon (host h16003), CPU tile kernel (one core and pool) and Metal compute.
workloads:       tests/fx.rs adversarial scenes (random layered scenes with glass over cards, moving glass,
                 springs, videos, shaders; 320 x 250, odd stride); bench `fxbench effects` (cards with shadows and
                 gradients, a quarter of them glass, shader paints, springs; 1280 x 800; 300 and 3,000 cards) and
                 `fxbench media` (4 / 24 / 48 NV12 tiles, animated images as video sources of other rates,
                 scrolling); baselines `pbench canvas 4`, `pbench anim 600` and the crate test suites.
gates:           G1 retained == full redraw (fresh scene, every tile) at ring ages 1..=4, every step, 0 words.
                 G2 retained == an independent whole-frame specification (no tiles, no layers store, no damage:
                    records folded in paint order over the frame, a plain mip chain per backdrop record).
                 G3 CPU == Metal, bytes, no tolerance, for every new archetype (Backdrop, Spring operands, Video,
                    Shader), retained across steps.
                 G4 negative controls, each must fail G1 or G3: mutant_layer_dilate (block dilation dropped),
                    mutant_layer_event (an event beneath a layer does not dirty it), mutant_settle (a record leaves
                    the active set before its last frame), mutant_video (a published frame damages nothing),
                    and a GPU source mutation of the pyramid weights.
                 G5 fallback: a scene with layers on the GPU-resident path is refused and drawn by the staged
                    path; a tile not wholly inside a layer's halo recomputes its prefix instead of restarting.
                 G6 existing gates unchanged: bough-paint, bough-kernel, bough-expr suites green.
thresholds:      P1 idle frame: 0 records visited, 0 tiles, 0 layer jobs.
                 P2 one spring moving: records visited == 1 at 300 and at 3,000 cards; instructions per frame at
                    3,000 within 1.15x of 300.
                 P3 hover on a card above glass: 0 prefix tiles, 0 pyramid blocks.
                 P4 hover on a card beneath one glass card: layer pixels recomputed <= 25 % of that layer.
                 P5 a glass card moving 1 px per frame over a still scene: pyramid pixels recomputed per frame
                    <= 15 % of its pyramid; prefix tiles recomputed <= its halo's perimeter tiles.
                 P6 one video publishing among N: tiles shaded == that video's visible tiles; instructions per
                    frame at 48 videos within 1.15x of 4.
                 P7 no regression: pbench canvas and anim, executor A1, median instructions per phase <= 1.03x
                    the baseline taken at 71ee3be on the same host.
                 P8 the cache earns its memory: frames of P3 and P5 cost <= 0.5x the instructions of the same
                    frames with every layer invalidated each frame (the redraw-the-layer compositor).
                 Kill line: P8 > 1.0x on either cell rejects the cached layer representation.
measurement:     Instructions of the frame thread (bough_mem::pmu) as the ranking column; wall and GPU seconds
                 reported with the load average; one core for CPU cells; `lockf -k` on the shared bench lock
                 around every timed run; 24 repetitions, median and minimum; counts (records, tiles, blocks)
                 are exact and need no repetition.
budget:          this session; no remote hosts.
stop conditions: G1 or G3 not reachable byte-exact for a new archetype: record the tolerance needed and stop
                 that archetype (METHOD_BOUNDARY). Kill line above: REJECTED.
do not revisit:  interval lists / summed-area tables for damage (tile bits stay); per-column bar strips; present
                 paths with large commit costs; the per-tile sequential bin scan.
```

Amendments are appended below with their reason; the text above stays.
