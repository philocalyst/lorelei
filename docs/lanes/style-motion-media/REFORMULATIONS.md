# Reformulations: where a change is broad, change the basis so it is one word (2026-11-21)

State: proposal with one piece built (R0). Each entry has a prediction and a kill line; none is accepted
until its card reports. Owner's direction behind this note: the GPU and the mixed rendering model are the
weak point; a blow-up such as a theme switch must be handled on the GPU, at instruction counts of the same
order as a one-record change; fight for the reformulation, not the constant factor
(the model is https://www.benjoffe.com/fast-day-of-week: the cost was in the *representation*, and the fast
answer came from choosing the epoch and the unit so that the hard cases became arithmetic).

The same move applied here: **a cost that follows the scene is a sign the change is expressed in the wrong
basis.** A theme change is one bit of state; it costs 30,755 instructions in the turn and a full repaint in
the frame because colours were resolved *before* they were stored, so one bit became 4,000 tiles of
different bytes. Every entry below finds the basis in which the broad change is again one word, keeps that
word unresolved in what is retained, and lets the executor (which touches every pixel anyway, in parallel)
resolve it.

| change | today | the basis in which it is one word | resolved by |
|---|---|---|---|
| theme / token colour | every reading record evaluated; every tile re-shaded | colours stored as **palette slots**; retained pixels as **inks**, not colours | palette table at shade or at present |
| many things in motion | per-record evaluation each frame | **endpoints + law** (start, target, velocity, t0); `Time` a uniform | closed form in the kernel |
| resize of a flex layout | layout re-solved, every rect rewritten | rects as **affine in the viewport** (a·W + b) between breakpoints | two multiplies per record in the kernel |
| scroll | (already) rigid move | origin is a uniform | — |
| hover / selection | (already, `sel`/`near`) two records | the pointer and the selected key are uniforms | compare in the kernel |
| DPI / scale switch | remount | geometry in points × scale uniform; glyph strikes by scale class | kernel + atlas residency |
| opacity / tint of a subtree | every record of the subtree | a **group word** multiplied in at the layer | layer composite |

## R0. The turn of a broad change is O(spaces) — BUILT
`World::broad` (`docs/lanes/core/cards/…incremental-operators-REPORT.md`, amendment 3): a column reading a
declared-broad word is not differentiated; its space is marked once. Sheet theme turn 30,755 → 2,424
instructions, flat in rows. This is the core's half of everything below: the CPU never asks a record about
a broad word.

## R1. Palette slots: a theme is a table swap
* **Type.** A token colour is an expression whose *value is a slot*, `tok::<S>()` / `rec(Slot(n))`, not a
  `Color`. `Fill`, `Label`, `Num`, `Line`, borders and shadows take `impl Ink`, implemented by `Color`
  (literal) and by slots. A const-generic slot makes the record zero-sized, so the column is uniform
  (differentiated once, already built).
* **Resource.** A palette is 256 × RGBA8 (premultiplied, plus linear copies if needed) in the region the
  kernels already read; several palettes are resident (light, dark, high contrast). The block holds a
  **palette word**: which table, or two tables and a mix factor.
* **Executor.** The kernel's paletted fill reads `pal[slot]` at shade time (CPU spans, Metal, Vulkan: one
  load). A theme switch is then: write one word; damage = the frame, one rect; executor choice = GPU (R5).
  CPU cost: the R0 turn plus one dispatch. No record is rewritten, no command restaged.
* **Prediction.** Theme on the 5,000-card effects scene: turn ≤ 3,000 instructions at every size; frame-thread
  instructions ≤ 1.2x a one-card frame; GPU time ≈ one full-surface dispatch (≈ 2 ms at 2560×1600 on the M3
  class from the paint lane's numbers). Kill: frame-thread instructions grow with cards.
* **Oracle.** Paletted == literal frame with the palette substituted, byte for byte, CPU and GPU; mutant:
  stale palette word.

## R2. Inks: retained pixels that survive a theme change
The step past R1. Most UI pixels are a mix of at most two token colours: glyph coverage over a background,
an anti-aliased edge between two fills. Keep the retained surface in that basis.
* **Surface format.** Per pixel `(ink_a: u8, ink_b: u8, cov: u8, flags: u8)`: colour = mix(pal[a], pal[b], cov).
  `flags` says *literal* (the other three bytes are RGB: images, gradients, video, blurred backdrops, three or
  more inks). Same 32 bits as today's surface.
* **Compositing in ink space** is exact for the common cases and decides itself when it is not: opaque over
  anything → (a, a, 255); coverage c of ink f over a pure pixel (a, a, ·) → (a, f, c); over a two-ink pixel
  with a third ink → resolve to literal and set the flag. A tile that holds a literal pixel which read the
  palette is flagged in the tile plane ("palette-dependent literal").
* **Present.** One resolve kernel over the tiles being presented: a table load and a lerp per pixel. It runs
  only when a surface is presented; idle presents nothing.
* **A theme switch** is then: palette word; resolve every tile (no shading, no records, no text); re-shade
  only palette-dependent literal tiles. A theme **cross-fade** is the mix factor as a function of `Time`: zero
  CPU per frame, zero re-raster, for the whole animation.
* **Prediction.** Theme frame GPU time ≤ 0.3x R1's (a resolve against a re-shade); CPU instructions equal to an
  idle present plus O(palette-dependent literal tiles). Kill: more than 15 % of tiles of the sheet, chat, ide
  and dashboard scenes are literal, or the resolve costs more than 0.5 ms per full surface.
* **Risk.** A second surface format through every executor; the three-ink rule must be exact, with an oracle
  per composite op and the mutant "treat three inks as two". Build R1 first; R2 only after its measurement.

## R3. Motion is endpoints and a law, not a value per frame
The paint lane's closed-form spring is already this for its own records. Make it the rule: any Frame-stage
value driven by `Time` is stored as (start, target, v0, t0, law) and evaluated in the kernel; the core's part
is the sweep rect and `until`. N concurrent springs cost the CPU a sweep per column per frame (96
instructions for an endless column today), whatever N is.
* **Prediction.** `many` cell of the effects benchmark: frame-thread instructions within 1.2x of `one` at
  every size (today 10.3 M against 138 K at 5,000 on the typed core's software painter). Kill: any per-record
  CPU work per frame survives.

## R4. Resize as a uniform
A flex row with fixed and fractional tracks has rects affine in the container's width between breakpoints
(REPORT 4.4, "parametric answers"). Store `(a, b)` per rect lane with the breakpoint interval; the viewport
size is a block word; the kernel computes the rect. Inside an interval a resize is a broad word change (R0)
and a full-frame dispatch; crossing a breakpoint re-solves only the rows whose topology changed.
* **Prediction.** Resize by 40 pt on the sheet: turn ≤ 5,000 instructions (legacy 4.3 M, lowered 1.8 M).
  Kill: fewer than 80 % of the resize steps of the chat and sheet traces stay inside one interval.

## R5. The mixed model chooses per change, from the change
Small damage on the CPU (no command-buffer floor, ≈200 K instructions on Metal); broad damage on the GPU.
The chooser must not look at the scene: its inputs are the damage (rect area, tile count) and the class of
the change (a broad word is declared, so the turn already knows). Two thresholds per device, measured once.
A broad change never touches the CPU painter at all.
* **Prediction.** No cell of the effects benchmark is worse than min(CPU, GPU) + 5 %. Kill: the chooser needs
  a per-scene tuning constant.

## R6. Order of work
1. R1 on the paint scene (palette resource, paletted fill and text ink, palette word, the theme cell of the
   effects benchmark on CPU and Metal), with R5's chooser for the full-surface case. One card.
2. R3 as a rule for every timed archetype.
3. R2 behind a feature, measured on the four content scenes before any executor other than Metal gets it.
4. R4 with the layout lane's parametric answers.
Each lands with its oracle, its mutant, and the effects-benchmark cells at 200 / 1,000 / 5,000.
