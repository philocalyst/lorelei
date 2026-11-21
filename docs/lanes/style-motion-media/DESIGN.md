# Effects, motion and media without a paint pass (design note, 2026-11-21)

Card: `cards/2026-11-21-layers-motion-media-slice1.md`. Report: `cards/2026-11-21-layers-motion-media-slice1-report.md`.
Code: `crates/bough-paint` (`layer.rs`, `media.rs`, `motion.rs`, and the additions to `ops.rs`, `paint.rs`,
`tile.rs`, `scene.rs`, `emit.rs`, `metal.rs`). Gates: `crates/bough-paint/tests/fx.rs`. Numbers: `bench/fx/run.sh`.

The renderer's property is that a frame is not painted: a state change is damage, damaged tiles are the
dispatch, and one integer kernel repairs them from retained columns. This note says how three kinds of thing
that do not fit "a pixel is a fold of pure functions of one record each" enter that model without breaking
it: effects that read a neighbourhood of what is beneath them, motion that ends, and content that changes by
itself. The owner asked for a decision on **how offscreen layers are damaged and cached**; section 1 is the
proposal, section 5 the measurement behind it.

## 1. Layers: a retained resource owned by the record that reads it

**The problem.** A backdrop blur at pixel p is a function of the composite of every record beneath it over a
neighbourhood of p. No per-record function computes it, so the usual answers are a second pass (render the
backdrop to a texture, blur it, composite) or a compositor of layers redrawn per frame. Both make cost follow
the scene, not the change.

**The decision.**

1. *A layer belongs to its reader.* A record whose paint has a `Backdrop` primitive owns a layer: the
   **prefix** (the flattened, 8-bit composite of the records beneath it) over its **halo** (the frame pixels
   its reads reach), and a **pyramid** of that prefix. There is no layer tree and no stacking context: the
   paint order is still one list of keys, and a layer is the pair (a key, a pixel window per level).
2. *Storage is toroidal and addressed by frame position.* Level-`l` pixel (x, y) of a layer lives at
   `base + (y mod ch) * cw + (x mod cw)`. When the record moves, the pixels it still needs stay where they
   are and only the pixels that entered its windows are new; nothing is copied. Level 0 is kept by whole
   tiles, so a moving panel recomputes a prefix tile only when it enters one.
3. *The pyramid is anchored to the frame's pixel grid.* Level `l + 1` is level `l` under a separable
   [1 3 3 1]/8 tent decimated by two; level-`l` pixel i covers frame pixels [i 2^l, (i+1) 2^l). A read is
   bilinear at the level the blur asks for, mixed with the next coarser one for radii in between. Because
   the grid does not move with the record, the pyramid of a moving panel is as cacheable as a still one's.
   Everything is integer arithmetic on stored 8-bit words: one definition (`ops.rs` leaves) for the CPU
   kernel, the Metal kernels and the specification.
4. *Damage reaches a layer as events, is dilated per level in tile space, and leaves as output damage.*
   Every damaged window is an event (window, paint-order key). A layer is dirtied by the events with a key
   below its record's, at level 0, in blocks that are frame tiles. A dirty block dirties the blocks of the
   next level whose taps reach it (pixel p is read by pixels ceil((p-2)/2) ..= floor((p+1)/2)); a block at
   level `l` is `32 >> l` pixels a side, so damage keeps the frame's grain at every level. The dirty blocks
   of the levels the record reads become output damage of the record (the frame tiles whose bilinear taps
   reach them), which is itself an event for the layers above. Records above a layer never reach it.
5. *A frame is a staged tile program, not a traversal.* `Scene::update` produces a plan: per layer in paint
   order, prefix jobs (the tile program of a tile up to the record, stored into level 0) and one stage per
   pyramid level (dirty blocks); then the output tiles. On the CPU `Scene::prepare` runs the stages; on
   Metal `Gpu::staged` encodes them as dispatches of two kernels (`k_seg`, `k_down`) in one command buffer.
   The schedule's cost is O(events x layers) in the first cut (a tile index would make it O(reached)).
6. *A stored prefix is also a place to restart.* Inside its window a backdrop record flattens the
   accumulator, so the stored prefix *is* the accumulator there. A tile wholly inside the window of a layer
   record starts from that layer's stored tile instead of from the bottom of its list: everything above a
   glass panel is repaired without shading anything beneath the panel. This is roadmap X14 (tile prefix
   caching) arriving as a consequence, for the tiles a layer happens to cover.
7. *Idle pays nothing.* No event, no geometry change: no schedule, no stage, no tile.

**Semantics chosen along the way** (each is in the specification, `Scene::spec`):

* *Flatten.* Inside a backdrop record's window the accumulator is quantised to 8 bits before the record
  composites (`x_flat`). That is what makes the prefix storable in the surface's own format and the restart
  exact. It was first applied over the whole halo; that was wrong (quantisation one LSB outside the window
  moved with the record and was not damaged; the gate caught it) and is now confined to the window.
* *Edges.* Reads beyond the frame clamp to the frame's edge at every level.
* *Sampling outside the box.* A blur reads what is beneath beyond its own box (not a clamp at the box edge).
  That costs a halo, and buys a pyramid that does not depend on where the box is.
* *Blur quality.* A tent pyramid is not a Gaussian: the read at level `l` has a standard deviation of about
  0.645 x 2^l px, intermediate radii mix two levels, and the coarsest level (5) caps it near 40 px. The
  trade is a cost per pixel independent of the radius (4 to 8 taps to read, 16 per pyramid pixel) against a
  halo wider than a Gaussian's 3 sigma (a read at level `l` reaches about 2 x 2^l px). No visual fixture
  against a true Gaussian has been made yet.

**What a layer costs in memory.** Level 0 is the halo's pixels, the pyramid a third more: 4/3 words per
halo pixel per glass panel, allocated from one pool, repacked when moved layers have left more than half of
it unused.

**Alternatives considered.**

| alternative | why not |
|---|---|
| Shared planes by level ("every backdrop at depth k shares plane k"), dense, tile-major | Deep stacks of overlapping panels need a plane per chain depth, and a moving panel forces re-colouring. Per-record ownership has neither problem and the same total memory (each panel needs its own area either way). |
| Separable Gaussian/box passes cached per tile | Cost per pixel grows with the radius on a tile-parallel GPU (no sliding sums), and damage dilates by the radius at level 0; the pyramid's dilation is one block per level. |
| Clamp the blur at the box edge (Chromium's edge mode) | Makes the pyramid a function of the box position: a moving panel would invalidate all of it. |
| Growing every level's window to whole blocks (so nothing enters until a tile is crossed) | Measured: the growth doubles at each level beneath; the effects scene's layers went from 478,634 to 1,762,248 pixels and every cell got dearer. Only level 0 is grown. |
| A compositor that redraws each layer when anything under it changed | This is the control in section 5 (`fx_nocache`). |

**Refused by the fast path, and what happens instead (the fallbacks).**

* A tile not wholly inside a layer record's window recomputes its list from the bottom (or from a lower
  layer that does hold it).
* The GPU-resident frame (`Gpu::frame`: window pass and bins on the GPU) does not schedule layers: it
  panics on a scene with layer columns, and such scenes run `Gpu::staged` (the CPU schedules, the GPU
  executes). A resident schedule needs the event algebra on the GPU; not built.
* A backdrop must be the bottom layer of its record (a `const` assertion in `Over`).
* A tile under a layer record's halo is not cut at an opaque cover above that record (the prefix needs what
  is beneath); the pixel-grain cut still applies inside each job's range.

**What the same machinery is for next** (not built): an isolated group is a layer whose prefix starts from
transparent at the group's first key instead of from the frame's bottom; group opacity, masks, blend modes
and subtree filters are readers of such a layer; a subtree transform is a reader that samples its layer
through an inverse matrix (the kernel's `to_local`); drop-shadow of alpha is a pyramid read of a group's
alpha; a retained shadow coverage (the canvas's dominant cost, 89 % of its instructions by the measurement
lane's diagnosis) is a layer whose content is one record's coverage.

## 2. Motion: time is not differentiated, its readers say when they stop

* `Paint::until(cx)` (from `Expr::until`, which `bough-expr` already defines) is the value of `Time` from
  which a record no longer changes with time alone. Each column keeps the indices of the records still
  ahead of it (`Col::active`). A frame in which only `Time` changed visits those records and nothing else;
  a column with none is skipped. Before: any change of the `Time` word ran the window pass over every
  record of every column that reads it.
* A record that has ended is evaluated once more at the frame that passes its end (so it lands exactly),
  then leaves the set. `Scene::animating()` is false when every set is empty: the host owes no frame.
* **Springs are a closed form**, `ops::x_spring(now, t0, dur, to, a0, b0, decay, omega)`: under-damped
  (`env (a cos + b sin)`), critical (`env (a + b t)`) and over-damped (two decays), from integer `exp` and
  `sin` leaves. It is a pure function of the clock and six words, so the raster evaluates it for the frame
  it draws, at whatever instant the output asks for, on any executor, to the same word. A frame that was
  never drawn changes nothing; pacing cannot change the trajectory. Against the differential equation it is
  within 0.5 px over the tested responses (the stated tolerance for "is this the spring you asked for";
  across executors it is exact).
* **Retargeting** (`motion::Spring::retarget`) reads the value and the velocity the spring has now (a
  central difference of the same leaf) and writes the six words of the motion that continues from them.
  A start in the future staggers a list. `dur` is where the envelope falls under an eighth of a pixel; from
  there the leaf returns the target exactly.
* **Transform and opacity motion** are paint operands (`Off`, `Fade`, `MixR` over a spring or a `Prog`):
  no layout runs, no record is rewritten per frame, damage is the union of the old and the new window.
  What is *not* built: rigid tile-space moves (pixels copied instead of re-shaded) and late latching.
* **Output pacing** (`motion::FrameClock`): the `Time` word is latched from the presentation time of the
  frame being drawn, never from a timer. Frames are requested only while `animating()`. The new core's host
  (`bough-host/src/run.rs`) already advances time only on a frame callback; the façade's benchmark app drives
  its spinner from a 16 ms clock (62.5 fps, X22), which is in `crates/bough` / `comparisons/bough-guibench`
  and outside this lane's files: the measurement lane's diagnosis is in its `animation-pacing` note. The
  mechanism the fix needs is this clock plus `animating()`.

## 3. Media: a frame is a leased slot, a new frame is one record word

* `media::Pool` is one page-aligned allocation. A source leases a slot nobody shows, writes NV12 planes
  into it, and publishes it; from then on the slot is immutable until the frame that stopped showing it has
  been drawn (`reclaim`). The Metal executor wraps the pool without a copy, so the bytes a decoder wrote are
  the bytes the kernel samples.
* A `Video` record carries the slot's header offset in one word. Publishing rewrites that word
  (`Scene::set`): the record's visible window is damaged and nothing else is; a video under a glass panel
  dirties that panel's layer through the same event as any other record.
* Sampling is in the kernel: bilinear luma and chroma, BT.709 limited range, integers, clamped to the pool
  (a stale or forged slot word reads pool words, never beyond). Rounded corners and opacity are the
  primitive's own; blur over video is a glass panel above it.
* `media::Synth` stands where a decoder would (deterministic bars and a box): lease, write, publish.
* Not built: I420, IOSurface-backed planes (the kernels read buffers; a texture path would be a second
  sampler), colour management, PNG/JPEG/WebP decode and atlas residency by exposure, SVG as a cached
  element. "Animated images" in the media benchmark are video sources of another size.

## 4. Author shaders

`paint::Shader` is the kernel's user node (`bough_kernel::fx::user_shade`) as a paint: a rounded box whose
colour is a function of the point, the clock operand and two colours. Its clock is an expression (the
frame clock, a constant for a still), so it is damaged where it shows and only when an operand moves; with
the frame clock it is endless and sits in its column's active set (cost per frame: its visible tiles).

What "an application's own shader" needs, designed and not built: the application's `#[wgsl]` module is
compiled by wgsl-rs and naga at *its* build time (as `bough-paint/build.rs` does for the leaves), its MSL is
appended to the generated source, and a paint type names its entry function as the primitive's `p_` leaf.
The CPU executor runs the same Rust function, so the exactness contract is the leaves' (integer arithmetic,
no float in a result). Admission is naga validation at build time plus the existing rule that a name called
from generated code carries no trailing digit.

## 5. The measurement behind the layer decision

See the report for every cell. The decisive ones (dist profile, this host, instructions of the frame
thread, one core): the same script on the same scene with layers cached and with `fx_nocache` (every layer
recomputed every frame and no restart, which is what a compositor that redraws its layers does).

## 6. What was taken from zgui and zgx, and what was not

Read: `zgui-anim` (tiers, `motion/spring.rs`, `frame/`), `zgui-shader`, `zgui/docs/performance.md`; `zgx-fx`,
`zgx-gpu` (`effects.wgsl`, the backdrop walk in `lib.rs`), `zgx-video`; and the orchestrator's study
(`ZGUI-OCHANGES.md`).

| | taken | rejected | measurement |
|---|---|---|---|
| zgui: three animation tiers (repaint / placement / cascade) | the split itself: paint-operand motion never re-layouts | the per-frame `Animator::tick` over every animating element (O(N log N) with allocation by the study) | here a time-only frame visits the active records only (`recs/frame` in the report) |
| zgui: spring integrated per frame (semi-implicit Euler, 1/120 s substeps) | the rest test on distance *and* speed (ours is an envelope bound) | integrating per frame: the trajectory depends on pacing, cannot be evaluated by the raster for a late-latched instant, and a dropped frame is work | closed form within 0.5 px of the ODE, exact across executors; retarget continuity gated |
| zgui: read-extent registry (damage grown over fragments that read outside what they write) | the idea, as the layer's halo and the event/dilation algebra | growing damage and then *repainting* the reader | prefix tiles and pyramid blocks repaired per frame against the layer's size |
| zgui: `shader!` with declared reads | reads as the expression's operands (a shader with no clock is never damaged by time) | a time shader forcing a full frame pipeline entry | a shader's frame cost is its tiles |
| zgx: backdrop filter as a copy of the target at a run boundary, blurred across and down in an atlas | the prefix boundary as *the* place a backdrop is defined | re-copying and re-blurring the filter's bounds whenever damage reaches it | the `fx_nocache` control |
| zgx: isolated layers cached as textures, redrawn on content change | layer as cache | redrawing the cached layer on a transform change | a moving panel's pyramid pixels per frame |
| zgx-video: playback on a virtual clock, idle when unseen | the lease/publish discipline; time as data | a player thread owning presentation | one published frame damages one window |

Neither system's numbers for these effects were run here (the zgx checkout is shallow and its headless
drivers did not build in the measurement lane's run; zgui's effects benchmarks need its own harness): the
comparisons above are of mechanisms, and the column on the right is this implementation measured against
its own control, not against them.

## 7. Audio (slice 2): a callback, a clock, and audio as the master of video

Crate `bough-audio` (no dependencies; `bough-paint` does not depend on it, only its benchmark does). Card and
report: `cards/2026-11-21-audio-seam-slice2*.md`.

**The interface.** A stream has a `Spec` (rate, channels), a `Flow`, and one callback (`Render`, any
`FnMut(&mut Block) + Send`). A `Block` is the interleaved samples for this call only (zeroed on entry), the
stream position of its first frame, and the latency to the ear in frames. That is the whole playback
surface. A decoder on another thread reaches the callback through `ring` (single producer, single consumer,
samples as atomics: safe code; the producer leases slots the callback cannot see and commits them; the
callback never waits, and what is missing is silence).

**The clock.** The sink stamps a `Clock` after every block: frames delivered, the host time of the stamp,
rate, latency, and the block's length. `Clock::audible_frame(host_ns)` is the delivered count less the
latency, carried forward at the stream's rate by at most one block: a stream whose callback stopped coming
is not playing, so its position holds. A sequence lock over atomics, sequentially consistent (the weaker
orderings tore a stamp on arm64 under the gate).

**A/V sync through the output-paced clock.** At each refresh the host has one instant: when the frame it is
drawing will be shown. `motion::FrameClock::present(instant)` makes it the `Time` word; `sync::Pacer::at(clock,
instant)` makes it "which video frame belongs to the sound heard at that instant". A changed index is a frame
to publish (one record word; the video's window is damaged); an unchanged one is nothing: no record visited,
no tile. The video never reads a wall clock, so a device 0.5 % fast drags the video with it, a stall holds the
frame, and a seek is a change of the pacer's origin. Many videos are many clocks and many pacers; each costs a
few dozen instructions per refresh and nothing in the raster unless its frame changed.

**Sinks.** `NullSink` has no device and no thread: the test pumps it at host times of its choosing, which is
what makes every gate deterministic (rate skew, jitter, stalls are scripted). `coreaudio::Output` (feature
`coreaudio`, macOS) is the default output unit with the same callback and the same clock; output only, so no
permission prompt; it was run here rendering silence.

**What was taken from Boilerpipe** (`src/root.zig`, `src/Callback.zig`, `src/linux/jack.zig`), and what was not:

| Boilerpipe | here |
|---|---|
| one callback started on the session; the buffer is acquired inside it and must not be cached across calls (`Connection.getBuffer`) | `Render` over a `Block` that borrows the samples for the call: the borrow checker enforces what Boilerpipe documents |
| `Callback.init(pointer, function)`: a typed pointer and a function, to avoid globals | a closure: its captures are the typed state; `Send` is the thread contract |
| `Flow { receives, sends }` from the port's side; one session-wide sample rate; a small fixed channel limit | `Flow` with the same words; one `Spec` per stream |
| "boilerplate-less": default port, connect, start | `Output::start(spec, callback)`: one call to sound |
| ports, port iteration, connections between other applications' ports, loopback and application capture | **not taken yet**: only the default output exists. `Flow::Sends` is declared so capture, loopback and application capture have their place; device enumeration is the next step |
| JACK's process callback as the model of a low-latency backend | CoreAudio's render callback is the same shape |
| (absent: the application keeps time) | the explicit `Clock`, the latency in every block, and the pacer: what a UI needs to put pictures on sound |
