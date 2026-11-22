# Integration Card 3, amendment 1 (2026-11-21, before any build or measurement of the lowering)

The card of 2026-11-18 stays as frozen. This amendment records what the executing session (alias `core`, branch
`lane/integration/card3-facade`, predecessor `71ee3be0aa`) fixed before writing code. Nothing here was fitted to a
result: no lowering code existed when this file was committed.

## A1. Host

The card names an M3 Pro. This run is on another machine: Apple silicon, 32 cores, 96 GB, macOS (Darwin 25.6),
headless, with other agents building at the same time (cargo at `nice -n 15 -j 8`, a private target directory).
Consequences:

* Compile-time thresholds (35.0 s clean, 8.0 s incremental, kill at 30.0 s) are kept as numbers but are measured at
  `-j 8` under load, not `-j 3` alone. A pass under load stands; a miss under load is reported with the load average
  and is `UNKNOWN`, not `REJECTED`, until replayed alone.
* Latency thresholds are ratios against the legacy façade on the same host in the same process family, so they
  carry over. They are measured as **instruction counts** (stable under load; `bough::sys` thread counter) and
  reported with the load average; wall-clock numbers, if any, are taken under `lockf -k ~/work/locks/bench.lock`.
* "Executed by DeepSeek under bench.lock" is replaced by: commands under `bench/card3/`, replayed by the measurement
  agent.

## A2. A contradiction in the card, and how it is resolved

Workload (2) asks that `crates/bough/tests/*` run **unchanged** against the lowered façade, and the gates ask for
**0 passes through legacy `Rt`**. Fifteen of the nineteen test files reach into the legacy runtime by name
(`app.rt.lay.eng.rect`, `app.rt.bough.records`, `app.rt.lists.regs`, `record_view(..).kind & 0x80`): they are tests
*of* `Rt`. They cannot both stay unchanged and not run `Rt`. Resolution:

* `bough::raw::App` remains the legacy runtime (the matrix's "separate compatibility surface"); every existing test
  keeps running against it unchanged and must stay green.
* The same element values gain a second protocol, `Element::lower`, and the lowered runtime (`bough::lower::App`)
  drives `World<V, B>`. The **same view source** (`impl View for X`, `bough::prelude` only) runs under both; the
  lowered runtime never constructs an `Rt`. This is one switch per entry point, not per view.
* Each existing test whose assertion is about public behaviour (pixels, counts, handler effects, geometry in
  points) gets a twin under `crates/bough/tests/lowered_*.rs` that includes the same example or view source and
  asserts the same thing through the lowered runtime. Tests whose assertion *is* an `Rt` internal have no twin; they
  are listed in the report as legacy-only.
* A provenance counter counts `Rt` constructions, mounts, settles and frames per thread; lowered tests assert it is
  0 before and after.

## A3. Construction decisions (frozen before building)

* **D1, lowering as data.** A façade element lowers into a flat description (layout words, paint records, erased
  handlers, a parent column), not into monomorphised row types. Reason: the card's stop condition and REPORT §5
  (3 h 32 min of code generation on typed layout). The typed fast path (`sig!` operands as Frame-stage columns) is a
  later slice on top; this slice is the complete-capability path every element can take.
* **D2, free rows in the same world.** `bough-x` gains one sink primitive: a store whose rows come and go with
  their own rects (a fixed shape cannot change between passes, and a list's rows share a layout and a pitch).
  Lowered records live there, in ordinary columns, with ordinary tile damage, patch-by-compare and paint order.
* **D3, layout.** `bough-lay`'s one flex function over a heap tree (its `Lay`/`Kids` traits), without the 96-node
  and 32-child limits of the fixed-shape `Flat`.
* **D4, text.** One shaped-run store for the lowered runtime: a full shaper (rustybuzz) with font fallback and a
  coverage rasteriser, colour bitmaps where a face has them. The prepared Latin-1 face is an admitted fast path
  only for the face and size it was prepared for; anything else takes the shaper, never an empty run.
* **D5, paint.** Rounded boxes, borders and alpha are composited in integers in the lowered backend, premultiplied;
  nothing is forced opaque. The `fx` agent's kernels can replace the backend under the same primitive contract.
* **D6, redescribe then compare** is the turn of this slice (the legacy façade's default too). Hover paint
  (`hover_bg`) is a one-record patch without re-description.

## A4. Predictions (written before any measurement; a miss is reported, not re-fitted)

| cell | prediction | kill / miss line |
|---|---|---|
| provenance | 0 `Rt` constructions, mounts, settles, frames in every lowered test | any non-zero |
| oracles | retained == full redraw == immediate == fresh mount at ring ages 1..=4 on the slice, with each mutant failing | a mutant that passes |
| 100,000-item `list` records | ≤ 400 | > 400 |
| virtual 100k mount, instructions | ≤ 0.50x legacy façade | > 0.50x |
| hover (`hover_bg`) turn, instructions | ≤ 1.00x legacy | > 1.00x |
| single-signal update turn, instructions | ≤ 1.00x legacy on the slice. **Risk stated in advance:** D6 re-describes the whole exposed view, the legacy runtime patches through one wire; this cell is the most likely miss, and the fix is the typed fast path, not a tuned benchmark | > 1.00x |
| deep-view (depth 16) clean build of `bough` + slice | ≤ 35.0 s | > 35.0 s |
| deep-view incremental leaf edit | ≤ 8.0 s (D1 exists for this) | > 8.0 s; > 30 s kills D1 |
| resize | same `World`, focus, held press and scroll anchor kept; 0 remounts counted | any remount |

## A5. From zgui (read before designing: `docs/performance.md`, the budget and ledger gates)

Taken now: **count bands**. A count (records patched by a hover, records for a 100,000-row list, frames while idle,
remounts on resize) is a property of the design and is asserted exactly, with no tolerance, in the gate itself.
Considered for later slices and not built here: per-signal fan-out to exactly the affected nodes (the typed fast
path is lorelei's form of it), keyed structural diffing of free rows (this slice is positional), a scroll space
that is translated instead of re-described.
