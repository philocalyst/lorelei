# Report. The flat paint bank, the animation bank, retained groups and page transitions

```text
lane:        style-motion-media (branch lane/rethink/fx, from lane/rethink/unified b9a196b6ec)
owner:       fx (Fable 5.2)
host:        h16003mac (M3 Ultra, Metal), 2026-11-22; other lanes were building on it (load 6 to 8):
             instruction counts are the frame thread's and stand; wall times are indicative
body:        B (the paint lane, crates/bough-paint) and a new non-generic surface over it in
             crates/bough (fx_api, pages). The façade's View tree (body A) does not draw through it.
```

## What was asked and what stands

| asked | stands as | where |
|---|---|---|
| styles without generic type towers | `FlatScene` (`FlatPaintScene`, `PaintBank`): nodes are data (`DrawCmd`), lowered onto a closed set of 20 archetypes (10 kinds x fixed/panned) that exist once in the binary | `bough-paint/src/flat_scene.rs` |
| N-stop linear, radial (elliptical, focal), conic gradients | one `Ramp` primitive over a 256-entry premultiplied RGBA8 table in the scene's table region, shared by content hash; a pixel is one parameter, two words, one mix | `ops.rs` (`mk_ramp`, `gl_*`, `p_ramp`), `scene.rs` (`bake_ramp`, `lut`) |
| stacked drop shadows | one record per shadow beneath the fill; the bins' occlusion cut hides them under an opaque fill; they read the retained coverage patches | `flat_scene.rs` |
| inset shadows | `Inset`: the box's coverage times the complement of `km::shadow` of the shrunk, offset shape; a core where it is exactly nothing | `ops.rs` (`mk_inset`, `p_inset`), `tile.rs` |
| backdrop blur with radius, tint, filter, rounded clip | the existing layer pyramid under a `Glass` node (backdrop + tint in one record) | `flat_scene.rs` |
| element blur cached by content | `layer::Surface`: a child scene painted into a retained surface with its [1 3 3 1] pyramid; `Mip` reads two levels. The blur radius is a record word | `layer.rs`, `media.rs`, `ops.rs` (`mk_mip`) |
| isolated opacity at tile grain | the same surface: the child is repainted in place at its own damage, the pyramid repaired by dirty blocks with change gating, the parent damaged where it changed (`Scene::damage_part`) | `layer.rs`, `scene.rs` |
| a clock that does not wrap | `ClockNs(u64)`; `tick()` is the projection; `tick_elapsed`/`tick_reached` compare modularly; `FrameClock` keeps 64 bits | `motion.rs` |
| springs, Béziers, keyframes, timelines | `AnimBank`: closed forms over `ClockNs`, analytic velocity across a retarget, repeat/direction/delay, pause/resume/scrub, exact motion hulls, parking | `motion.rs` |
| page transitions | `PageRouter`: pages are retained groups of a stage; cross-fade, slide, scale-fade, blur-reveal, shared element; interruptions retarget | `bough/src/pages.rs` |
| a façade surface | `BoxStyle`, `Gradient`, `ShadowSpec`, `BlurSpec`, `AnimSpec`, `Motion`, `SceneExt` | `bough/src/fx_api.rs`; conversions in `deco.rs`, `transition.rs` |

Exactness is by construction where it can be: every new pixel function is a leaf of `ops.rs` (integers,
the wgsl-rs subset), which rustc compiles into the CPU kernel and naga turns into the MSL the generated
functions call. The glue that reads tables and surfaces is the only hand-written twin, and each has a
mutant.

## Gates (all on the binaries of this branch; `--release`, and `tests/flat.rs`, `tests/motion.rs` also in a debug build)

| suite | tests | what it holds |
|---|---|---|
| `bough-paint` lib, `exact`, `fx`, `review`, `scenes` | 4, 5, 21, 8, 2 | unchanged, all pass |
| `bough-paint/tests/flat.rs` | 11 | every effect: retained CPU frame == specification == Metal, per frame; groups also == a fresh scene's full redraw; counts (idle is nothing; one property is one record; an idle child does no work under its parent's animation; a child's small change repaints 1 to 6 tiles) |
| `bough-paint/tests/motion.rs` | 7 | the clock across the tick's wrap; Bézier against bisection (worst 2.8e-7 over 314 curves); springs against RK4 (worst 1.3e-5 relative); 1,920 mid-flight retargets (value jump 0, velocity jump 1.5e-13 relative); keyframes; hulls (600,000 future samples inside); parking; 0 allocations in 1,332 warm ticks of 3,000 tracks |
| `bough-kernel/tests/simd.rs` | 2 | the new vector rows against the scalar arithmetic, lane by lane |
| `bough/tests/fx_pages.rs` | 6 | styles and motion; each of 7 transitions for 140 frames and 110 interrupted frames exact on the CPU and on Metal against the specification and a fresh router; 0 units of page work in flight; five interruptions each with position jump 0 and velocity jump below 1e-15 relative |
| `bough` (all 40 test binaries) | | pass |
| `cargo check --workspace --all-targets` | | clean (one unused import that was there before) |

Negative controls. Features (each makes the named test fail): `mutant_ramp_stop` (gradients),
`mutant_inset` (inset shadows), `mutant_blur_stale` (element blur), `mutant_group_tile` (tile-grain
opacity), `mutant_spring_velocity` (`motion.rs` and, as `--features bough-paint/mutant_spring_velocity`,
the router's interruption test). In the suites themselves: eight mutated Metal sources (a ramp that never
mixes, a ramp one entry off, an inset shadow uncomplemented, an inset shadow uncut, a surface read
without its coarser level, half a texel off, twice under a page transition), each seen; a retarget that
forgets its velocity, seen by the measure that passes the real one; an eased curve's restart, seen.

## Measurements

`fxbench` (the lane's standing benchmark; `effects`, `pane`, `media`; 96 cells), this branch against
b9a196b6ec, same host, alternated: **0 mismatching words in every run**; instructions per frame changed
between -3.2 % and +2.3 %, median 0.00 %; over the 45 cells above a million instructions, between
-0.29 % and +0.18 %. No regression.

`flatbench` (new; 1280 x 800; 300 cards of five records each, a backdrop pane; two pages as retained
groups). Milliseconds per frame, median of 24; **0 mismatching words** (retained against whole redraw,
Metal against CPU):

| cell | CPU, 1 core | CPU, 8 workers | Metal | tiles |
|---|---|---|---|---|
| idle (either scene) | 0.000 (6 k instr) | 0.000 | 0.02 | 0 |
| restyle one card | 0.29 | 0.16 | 0.34 | 35 |
| restyle a card beneath the pane | 1.94 | 1.05 | 0.40 | 53 |
| one card on a spring (5 records a frame) | 0.96 | 0.41 | 0.60 | 56 |
| scroll 11 px (every tile) | 15.5 | 6.0 | 1.36 | 960 |
| pane moves 1 px | 4.23 | 0.72 | 0.37 | 172 |
| page cross-fade | 2.63 | 0.41 | 0.50 | 1000 |
| page slide (whole pixels) | 1.92 | 0.32 | 0.49 | 1000 |
| page blur-reveal (blur, scale, opacity) | 14.9 | 2.11 | 0.57 | 1000 |
| one card of a page changes (35 tiles, 330 pyramid blocks repaired) | 0.79 | 0.47 | 1.18 | 63 |

Losses and costs, stated: a full-frame scroll of the flat cards scene costs 328 M instructions on one
core against 217 M for `fxbench effects` (another scene: one composed record per card there, five
here; the gradient is 6 ns a pixel). A blurred, scaled full-page composite is 15 ms on one core (two
bilinear reads and a mix per pixel per page). A page's content is rasterised by the CPU tile kernel
on both executors (into memory Metal binds without a copy); only the composite runs on the GPU.

## Limits, as they are

* The façade's `View`/element tree does not lower onto this. `fx_api` and `pages` are a surface of
  their own over flat scenes; wiring `ui::div().bg(..)` to it is `app.rs`/`elem.rs`/`lower/` work.
* A node keeps its kind and its place in paint order. Mounting, inserting beneath, and compacting are
  full frames (`Scene::push` always was); content that comes and goes is mounted hidden and shown.
* A drop shadow is drawn under its whole box, as `style.rs` does (CSS clips it to the outside): it
  shows through a translucent fill.
* The typed path's clock words (`x_prog`, `x_spring`) are still a wrapping tick: a motion that ended
  more than 2^31 ticks ago reads as not started. `Spring::settle` removes the hazard for a spring;
  nothing here rewrites a `Prog`. The flat bank has no clock word at all.
* An eased curve interrupted restarts from rest (C0); only springs keep velocity.
* A shared element in flight lands at once if a transition of another kind to another page interrupts it.
* `Isolated` (slice 17's publishing form) is unchanged; `Surface` is the tile-grain form beside it.
* The shadow coverage store, when full, is started over and the bank rewrites the shadows it shows
  (one redraw of their windows); a typed scene keeps the old behaviour (no patch when full).
* A conic gradient's seam is not antialiased; gradients are 8-bit tables (premultiplied).

## For the merge

* `crates/bough/Cargo.toml` gains `bough-paint` (and `Cargo.lock` one line under `bough`).
* `crates/bough/src/lib.rs` gains `pub mod fx_api;` and `pub mod pages;` (no prelude re-export).
* A latent debug-build overflow in `Tile::store_layer` (`then_some` evaluating an empty box's
  sentinels) is fixed.
