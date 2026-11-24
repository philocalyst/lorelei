# The author surface, realigned under `designs/reactivity/final`

Status: plan and frozen card, written before any of it was built or measured. Results are appended at the
end as they exist; nothing above the "Results" heading is edited after a measurement.

## What the design says and what the code does

Read against `final/00`–`04`, `10` and the examples. The author writes plain structs; `view` returns one
concrete type; a property takes a constant, a borrow of a cell, or `source.map(f)`; a route is a method.

| Design (`final`) | Today (`bough::prelude` at `010a7b1af4`) | Gap |
| --- | --- | --- |
| `fn view(&self) -> El<'_>` (02 §1); helpers return `El<'_>` / `impl Element` | `fn view(&self) -> impl Element<Self> + '_`; every builder is a nested generic (`On<LBox<Color, (Para<S>,)>, HoverBg>`) | The owner type is in every signature; a helper that returns one of two shapes does not compile (gate 1); error messages print the nested type (gate 5). |
| `.bg(&self.tint)`, `text(&self.name)` are direct ports (03 §2) | `&Signal<T>` is a constant of the description; a port needs `sig!(self.tint)`, which returns `Wired<'_, T, V>` | `sig!` and `Wired` are in the surface (gate R1 forbids `Wired` in the corpus). |
| `self.fade.map(f)` is a computed feed | `sig!(self.fade).map(f)` | same |
| `div().child(x)`, `row((a, b))`, `Option<El>`, iterators as children (02 §4) | `div().child(x)` exists on the typed box; iterators need `each(..)`; `Option<E>` exists | `ChildSeq` is not one family; conditional setters change the type. |
| `.on_click(Counter::inc)`; the receiver is resolved against the owner chain at mount (04 §2, L4) | `on_click<V, F: Fn(&mut V)>`; routes run on the root owner only (`root_owner_only` asserts) | Receiver is fixed to the root by an assert at mount, not resolved; no `.with(..)`, no key table, no propagation. |
| `.gap(8)`, `.p(16)` | `f32` only | integer literals do not compile |
| `Flex::row().align(..).gap(8)` as `const` | present (`ui::Flex`), `gap(f32)` | small |
| identity = parent x ordinal x kind (L3); a kind change remounts that position | the tape assumes the same shape at every fixed position; a different shape is only legal inside `Option`/`each` | a type-erased tree needs a shape check the typed tree got from the type system |
| `Reader`, `Entity`, `Motion`, tokens as plug forms; `boundary()`; `Win::demo` | kernel has `Reader`, `Entities`, `boundary`, transitions under their own builders | not unified into the plug family |

Three more author surfaces exist beside the prelude (`bough_dataflow::el`, `bough::flat::ui`, `bough::raw`);
`010a7b1af4` hid the first two from the documentation. `raw` carries the measured scenes (149 files) and
stays as the machinery layer.

## The rework

One surface, `bough::prelude`, with the design's shape. It is built *on* the kernel, not beside it: an `El`
is interpreted into the same typed builders (`LBox`, `Para`, `BoxOps`, the act table, `mount_dyn`) the
present prelude constructs directly, so pixels, damage and wires are the kernel's own and the measured
scenes (which use `raw`, wires and `Config::once`) do not change by construction.

1. **`&Signal<T>` is a port everywhere.** A compile-time table per owner type (`reflect::cells::<V>()`,
   from `type_info`: every reactive cell reachable through struct, tuple and array fields only, with its
   offset and `TypeId`) proves at mount that a borrowed cell is a field of the owner: `address - owner` is
   looked up; a hit installs the same wire `sig!` would. A miss (a cell behind a `Vec`, in another object)
   is a constant of this description, as today. This applies to the typed builders too, so `sig!` leaves
   the prelude. Soundness: an object of type `C` at an offset where the owner type has a field of type `C`
   is that field.
2. **`El<'f>`**: a move-only, `!Send`, covariant cursor (pointer + generation) to a draft node in a
   thread-local arena. Node chunks are never freed and hold only nodes, so the generation word of a slot is
   always a generation; the kernel resets the arena at describe and turn boundaries and zeroes the
   generations it hands back, so a stale `El<'static>` is a located panic and never an alias. Borrowed
   `&'f str` is stored by pointer; owned strings and formatted numbers are copied into the arena; nothing
   in the arena needs `Drop`. No public signature mentions the arena.
3. **Interpretation.** `impl<V: 'static> Element<V> for El<'_>`: each node constructs the typed builder
   with erased operands (`Operand<P>`: value + optional wire request holding the monomorphic patch function
   the setter chose) and calls its `mount` / `update` / `seal` / `lower`. `BoxOps::{SIZE, PAINT}` are
   associated consts, so four monomorphs are selected from the node's op mask.
4. **Shape.** Every node carries a 64-bit shape (kind, paint flag, text metrics, op mask, wire and route
   identity, folded child shapes), maintained by the setters, O(1) to read. `Option` and iterator children
   are dynamic parts (`describe.rs`), keyed by shape; the view root is one dynamic item.
   **Deviation from L3, recorded:** a shape change at a *fixed* position remounts the nearest enclosing
   dynamic item (the view root, an `Option`/iterator item), not only that position. State kept by the
   kernel below it (hover, focus, scroll offset) restarts. Position-precise replacement is follow-up B2.
5. **Routes carry their receiver.** `.on_click(Counter::inc)` records `TypeId::of::<Counter>()` with the
   trampoline; mount compares it with the mounting owner and panics by name on a mismatch. A capturing
   closure larger than the 16-byte recipe is a compile error whose text names `.with(..)`. Dispatch stays
   root-owner-only in this step (follow-up B3: owner chain, rows and child views, key tables, propagation).
6. **`View`.** `bough::prelude::View` is `fn view(&self) -> El<'_>`; a blanket impl gives every such type the
   kernel's `View`, so `App`, `FlatApp` and `Application::run` take it unchanged.
7. **The typed builders stay** as `bough::raw` machinery (what `El` lowers onto and what the wire-path
   benchmarks are written in). Prelude users are ported; the prelude stops exporting the typed names.

Deferred, each its own change: `text(format!(.., *self.count))` reruns `view` and the tape patches the label
(the design's rung 1 counts zero owner reruns; here it is one, with the same pixels; `text(&self.count)` is
the zero-rerun spelling); `Reader` / `Entity` / `Motion` / token plugs; `boundary()` in `El`; `Handoff`,
`Target`, `#[derive(Delta)]`; `Win::demo`; the trybuild and Miri fixtures of gates 13 and D1.

## Card (frozen 2026-11-22, before building)

**Question.** What does the erased tree cost over the typed one, per re-described node, and does it keep the
typed path's steady state free of allocation?

**Measure.** `crates/bough/examples/redescribe_cost.rs`, extended with `El` twins of its three row shapes
(`owned`, `static`, `bare`) at the same N, same turn (`selected` changes, no frame), instructions per
`App::turn` on the calling thread by slope over two repeat counts, and `HEAP=1` allocation calls per turn.
Same binary, same run, typed and `El` cells interleaved.

**Prediction.** `El` re-description costs between 1.2x and 2.0x the typed path's instructions per turn on
`owned` and `static`, and at most 2.5x on `bare` (the typed `bare` row is nearly free; the node write is
not). Zero allocation calls per steady-state turn for both.

**Kill line.** More than 3.0x on `owned` or `static`, or any allocation call in a steady-state `El` turn. Past
either, `El` does not become the prelude in this form; the fallback is items sealed per row (so unchanged
rows are stepped over without interpretation) before any port.

**Correctness gates** (each with a mutant that must fail it):

* Parity: a scene described with `El` and its typed twin present byte-identical pixels after mount and
  after the same clicks; the repaired frame equals a full redraw. Mutant: one differing radius.
* Ports: publishing a `&self.x`-bound signal under `Config::once()` (no re-description) changes the pixels;
  a signal reached through a `Vec` does not install a wire. Mutant: the cell table offset shifted by one
  field.
* Shape: a branch that swaps a box for a text at a fixed position, and back, matches a fresh mount each
  time. Mutant: shape hash ignoring the node kind (must trip the kernel's tape assertions or the pixel
  compare).
* Stale handle: an `El<'static>` kept across a turn panics by name when used.
* Routes: a method of another type passed to `.on_click` panics at mount naming both types.

**Not claimed.** Nothing about the measured STATUS scenes (they do not use `El`); nothing about frame time.

## Results

(appended below)

### What was built (branch `lane/reactivity/el-surface`)

* `bough::prelude` is the `El` surface: `View::view(&self) -> El<'_>`; `div`/`row`/`column`/`block`/`text`/
  `para`/`button`/`text_input`/`image`; setters taking a constant, `&self.signal` or `self.signal.map(f)`
  (`IntoProp`), integer lengths (`Len`); children through `ChildSeq` (an `El`, `()`, tuples to 12, `Option`,
  `Vec`, arrays, `each`, std iterator adapters); routes as methods (`.on_click(Counter::inc)`,
  `.on_click(Panes::split.with(v))`, `.on_key(Key::Space, ..)`, `.on_any_key`, `.focusable()`), receiver
  checked at mount by `TypeId` and named in the panic. The typed builders moved to `bough::builders`; the
  64 in-tree users (workloads, benches, tests) were switched mechanically.
* `El` is a cursor into a thread-local bump arena (`el/arena.rs`): nodes never freed, a turn number checked on
  every use (a stale handle panics), rewound when the outermost `Walk::enter` boundary (mount, `turn`,
  redescribe, row mounts, entity views, lowering) ends; El's own `Element` impls `Walk::hold`.
* Interpretation (`el/walk.rs`) lowers nodes onto the kernel's typed builders (`LBox`, `Para`, `ImageBox`,
  `TextInput`) with erased operands. **Deviation from the plan, in the design's favour:** every box's children
  are an unsealed dynamic part keyed by `shape()`, so a kind change remounts *that position* (L3), not the
  nearest enclosing dynamic item. Groups (`Option`/`Vec`/iterators) are sealed parts.
* A fast seal check (`Element::seal_eq`, `SEAL_EQ`/`SEAL_HOLDS_KEY`) compares a node's retained seal in
  registers with early exit: one 104-byte head block per node, texts of 8..=16 bytes as two words; a test
  asserts it agrees with the streaming `SealCheck` on every case and that a seal matches only an identical
  description.
* `rt::Recipe` (16 bytes) carries closure captures for wires and routes. A recipe whose type
  `reflect::padding_free` cannot prove padding-free is frozen in two registers by a read-only asm load after
  it is written, so its bytes are always initialised and compare as bytes; `Sealer::recipe` compares them as
  two `u64`s. (The first fix refused such seals and tripled sheet's re-description; the second froze with a
  memory clobber, +8.4%; both measured and replaced.)

### Card

`EL=1 redescribe_cost 1000 10000`, instructions per turn at `19d484c87f` (same binary, interleaved):

| rows | shape | typed | El | El / typed | allocs/turn (typed, El) |
|---|---|---:|---:|---:|---|
| 1000 | owned | 296,413 | 428,596 | 1.45 | 0, 0 |
| 1000 | static | 293,131 | 437,728 | 1.49 | 0, 0 |
| 1000 | bare | 125,231 | 166,573 | 1.33 | 0, 0 |
| 10000 | owned | 2,915,417 | 4,182,478 | 1.43 | 0, 0 |
| 10000 | static | 2,865,620 | 4,272,011 | 1.49 | 0, 0 |
| 10000 | bare | 1,232,057 | 1,616,023 | 1.31 | 0, 0 |

Inside the prediction (1.2–2.0x on `owned`/`static`, ≤2.5x on `bare`, zero allocations); far from the kill
line. The path there: 2.9x first build → 2.18x (direct node seal, register check, bump arena) → 1.44x
(`lane/reactivity/el-perf`: by-pointer check, chunked rewind, one head block, inlined containers).

Correctness gates (`tests/el_surface.rs`, 10 tests): parity with a typed twin, ports under `once`, ports
against re-description, kind change remounts its position (fails under the `mutant_el_shape` feature, as do
"a sibling keeps its routes"), stale handle panics, wrong route owner panics by name, keys and `.with`, the
progressive rungs move no pixels, rows take bound routes. `tests/el_review.rs` holds the review findings
(R1–R4, R6): rewind in rows mounted outside a walk, a wired width with a constant height, an image's spec
change, lowered key/click dispatch, padded bound values, routes on images and inputs, an input's margin.

### The measured scenes (bough-workloads, not claimed by the card, checked anyway)

`19d484c87f` against `06686cd98d`, all six scenes, `MODE=once` and `MODE=redescribe`, `RENDER=0`, median of
3, instructions per step (full table: DeepSeek's run, `el-baseline-results.md` "Final tree (definitive)").
Re-description steady state:

| scene | baseline | final | Δ |
|---|---:|---:|---:|
| sheet | 43,670,656 | 43,271,582 | −0.91% |
| ide | 23,250,759 | 23,329,570 | +0.34% |
| dash | 3,948,654 | 3,932,717 | −0.40% |
| canvas | 633,799 | 635,757 | +0.31% |
| chat | 17,930,974 | 17,981,101 | +0.28% |
| gbcanvas | 5,772,473 | 5,768,874 | −0.06% |

Every mount, hover, theme, resize, resize_anim and anim cell in both modes is within ±1% (sheet anim
redescribe −1.16%). Cells outside: `first_frame` (−48%…+0%) and `canvas`/`gbcanvas` steady once (+14%/−5%)
move non-monotonically commit to commit with the frame total unchanged under `BOUGH_KPROF`: about 5–6k
instructions of the same frame cross the harness's measurement boundary with code layout (bisected; not new
work, and not the arena's TLS, whose first touch lands in mount). `idle` once is +30 instructions on ~550.

Workspace tests: `bough` passes except `startup` in debug (it passes in release, its intended profile). Eight
targets in other crates (`sketch-code`, `bough-dataflow` ×2, `bough-kernel exact`, `bough-paint` ×2,
`bough-text parley_twin`, `bough-zgui drawing`) fail identically at `06686cd98d` (missing fonts, an unrelated
pointer precondition); not touched here.

### Open

* `List<T>` as `El` children (needs a row mounter that is not a cast of the typed one).
* `Reader` / `Entity` / `Motion` / token plugs; `boundary()` in `El`.
* Routes dispatch on the root owner only (`root_owner_only`), rows included; owner chains are B3.
* A map's recipe is not part of `shape()`; a re-description after settle corrects the value.
* A hover colour changed while hovered shows on the next hover.

## Results, part 2: integration and performance (2026-11-23, after review)

The El work above was integrated with the rest of the tree, reviewed again (DeepSeek V4 Pro reviews of every
merged piece; findings in ~/work/lorelei-wt/review-*-deepseek.md), and every benchmark brought back to the
reference. State at the end: `cargo test --workspace` green (310 test binaries, 0 failures; 8 targets failed at the
start), and the native gate (`bench/baseline/gate.sh`) **passes** against its reference (761d227fd4): 791 primary
counters, 481 improved, every exception recorded under the reference's `supplements` or in `limits.json`.

**El review fixes.** Recipes are frozen by a read-only two-register asm load (padding becomes fixed bytes;
padding-free captures skip it; `Sealer::recipe` compares two u64s): the first fix refused seals of every
capturing closure (sheet 3x), the second froze with a memory clobber (+8.4%); final: sheet redescribe 43.27 M vs
43.67 M baseline, all six workload scenes within ±1%. Container-only setters check their element; tests for padded
captures and for key dispatch parity between the kernel and the lowered runtime (400 seeded steps; fails if
`Hnd::hears` passes every key). Two HIGH review findings (asm freeze UB, outermost `hold`) were answered with the
mechanism (asm outputs are initialised; `rewind` is the only reuse and always advances the turn).

**Correctness found on the way.** `Scene::push/set` copied paints through a misaligned `*const u32` (UB; word
copy kept for aligned paints); SVG decode's `CGImageForProposedRect` return encoding; wgsl-rs's CPU expansion now
wraps integer arithmetic as WGSL does (WGSL byte-identical, release instructions unchanged), so debug runs the GPU's
arithmetic instead of panicking; parley: a combining mark after a hard break split into its own cluster (it was
drawn on the newline's line), shaped in a loop (the first version recursed and overflowed the stack on a paste of
1,600 pairs); DejaVu 2.37 vendored in `fonts/dejavu` (twelve tests read one machine's Nix path and silently dropped
Bold/Mono coverage); `pbench canvas 1` dragged a record it does not have; the gate's no-change search cell had
never been re-referenced after its contract changed, so every full gate stopped there.

**Coverage.** bough-damage had no tests: footprint, engine and allocation tests against a brute-force pyramid
oracle, then an exact full-mode oracle (mask = documented model, tile for tile); 33 hand mutants, all caught.
bough-flat backdrop blur bit-exact against the pyramid. Gate cells for flatbench and the damage engine; the
fxbench parser brought to the current output format (it rejected every fxbench cell).

**Performance recovered** (each bisected; several earlier attributions were wrong and were re-measured):
fxbench idle 9.4 K -> 3.0 K (ref 5.4 K; 3.2 K was two counter syscalls inside the measured frame), media scroll
cpu 135 K -> 17 K, metal 589 K -> 93 K (bins rebuilt on a quarter of records moving; buffer lookups per tile);
pbench canvas4 pan/A 5.2 M -> 3.05 M (ref 3.43 M), canvas1 drag/A and hover1/A back to the reference (Scene::update
visits only columns reading a changed word); effects-core/200 +2.6% -> +0.3% (`Tiles::mark`'s one-row path); ops
corpus/search: same-host noise from allocator layout and page faults, made allocation-free once warm (needle
narrowing 2.8 K -> 1.1 K), Join −40–58%, Group back; `part_mark_cell` 59 -> 64 kept (3 of 5 instructions are the
soundness bounds check). The `_x` fixture names left over from bough-x are gone (old receipts and checkouts still
compare and build).

## Reader plugs and `boundary()` in El (card frozen 2026-11-24, before building; branch `lane/reactivity/el-reader`)

**What is built.** `Reader<T>` fields of a view as plugs of the El surface (`self.r.map(|o: Option<&T>| ..)`,
`is_pending(&self.r).map(..)`, `latest(&self.sel)`), proven fields by the cell table as signals are; an El
producer element (`produce(&self.sel, &self.r, request)`); and `boundary().loading(..).failed(..).child(..)`
in El, which guards the readers its child arm reads (collected while the child is described, which by Rust's
evaluation order is after `boundary()` and before `.child(..)` is called). All of it lowers onto the kernel's
own reader machinery (`reader.rs`: ticket, cohort, `patch_rmap*`, clip-switched branches).

**Question.** Does a completion reaching an El reader port, and a boundary's verdict through an El boundary,
cost what the kernel's typed spelling (`sig!(self.r).map(..)`, `boundary(sig!(self.r))`) costs, with no
allocation beyond the completion's own; and do the existing cells stay where they are?

**Measure.** New example `crates/bough/examples/reader_cost.rs`, El and typed twins in one binary, interleaved:
instructions on the calling thread by slope over two repeat counts, allocator calls per cycle counted by a
counting global allocator (as `redescribe_cost`). Cells: (a) *port*: one producer `q -> r`, a text port and an
`is_pending` opacity port on `r`; a cycle is `turn(q += 1)` (request), `run_jobs(1)`, `poll()` (admission,
commit, patches); the request turn and the completion turn reported apart, under `Config::once()` (no
re-description) and under re-description; (b) *boundary*: the same with the ports inside a boundary
(`loading`/`child` arms); per completion the boundary's verdict wire runs (branch unchanged); and the
first-readiness switch (loading -> child) as a single turn on fresh apps (median of 8). Also, unchanged:
`EL=1 redescribe_cost 1000 10000` base (1a090b8b7f) vs head, same host, same session.

**Prediction.** Once mode: El / typed <= 1.05 on every port and boundary cell (the same kernel wires run; the
El boundary's verdict loops over its guards, one here). Re-description mode: El / typed <= 1.3 per turn (El's
view of a handful of nodes is described each turn, as in the main card). Allocator calls per cycle: El equals
typed (the request's boxes, the result's box, the formatted text: the completion's own payload); zero in a
turn with no request and no completion. `redescribe_cost`: every typed and El cell within +-1% of base, zero
allocations per steady turn.

**Kill line.** Any once-mode cell above 1.3x typed, any El allocation per cycle beyond typed's, or any
`redescribe_cost` cell outside +-1%. Past it, the work moves below the surface (the El operand carries the
kernel's request directly) before anything else is added.

**Correctness gates** (`crates/bough/tests/el_reader.rs`, each with a mutant behind a cargo feature that must
fail it): a stale completion (an older request finishing after a newer one) is rejected (mutant
`mutant_reader_stamp`: the ticket check passes everything); drop-cancel: unmounting a branch revokes its
producer and a late completion changes nothing; loading -> content -> revalidation keeps content -> failure
shows the failed arm (policy) -> success heals (mutant `mutant_el_boundary_ready`: the boundary ignores first
readiness and shows loading while any guard is pending); the `is_pending` opacity port; a reader port patches
with zero `view` runs (counted); and the retained pixels equal a fresh mount of the same state after every
step of seeded random scripts of request / complete / fail / supersede / branch flip.

### Results: Reader plugs and `boundary()` (2026-11-24)

Built in `260da4af08` (plugs, `is_pending`/`latest`, `produce`, `boundary()`, kernel revoke/settle fixes, the
three mutants, `tests/el_reader.rs`) and `8446bde194` (wire kinds as tags in the head word: `WireReq` back to 48
bytes; `examples/reader_cost.rs`). Every number below was measured in one session on one host, base `1a090b8b7f`
(binaries from the `rethink` worktree) against head. Instructions are counted on the calling thread.

**Verification.** `cargo test -p bough`: debug 47 targets, 176 passed, 0 failed; `--release` the same.
`cargo build --workspace --all-targets` passes. `cargo clippy -p bough --all-targets` has no finding on a line
this branch wrote. Clippy only runs with `-A clippy::mut_from_ref -A clippy::approx_constant`, because
bough-core, bough-metal and bough-fused's build script fail those deny lints at base.

Mutant proof (`cargo test --release -p bough --test el_reader --features <m>`; 8 tests, all pass without one):

| mutant | what it breaks | failing tests |
|---|---|---|
| `mutant_reader_stamp` | admission passes every ticket | `a_stale_completion_is_rejected` |
| `mutant_el_boundary_ready` | the skeleton shows while any guard is pending (first readiness ignored) | `loading_then_content_then_revalidation_keeps_content` |
| `mutant_el_bound_wire` | a guard's transition does not reach its boundary | `loading_then_content_then_revalidation_keeps_content`, `random_scripts_match_a_fresh_mount_at_every_step` |
| `mutant_cell_offset` (existing) | the cell table's offsets | 7 of 8 (all but `a_producer_of_a_reader_elsewhere_is_refused_by_name`) |

**`reader_cost`** (`cargo run --release -p bough --example reader_cost`). Instructions per turn, median of 3
runs; allocator calls per cycle (`request` and `completion` both show the whole cycle's). `port idle` was added
in this commit (a `poll()` with nothing to admit, on the plain port shape) to isolate the cause below.

| shape | mode | phase | typed | El | El / typed | allocs (typed, El) |
|---|---|---|---:|---:|---:|---|
| signal control | once | first | 16,847 | 16,430 | 0.98 | 7, 7 |
| port | once | request | 4,591 | 4,511 | 0.98 | 8.84, 8.84 |
| port | once | completion | 4,667 | 4,088 | 0.88 | 8.84, 8.84 |
| port | once | first | 18,430 | 18,418 | 1.00 | 8, 8 |
| port | once | idle | 417 | 417 | 1.00 | 0, 0 |
| boundary | once | request | 4,422 | 4,449 | 1.01 | 9, 9 |
| boundary | once | completion | 4,024 | 4,052 | 1.01 | 9, 9 |
| boundary | once | idle | 417 | 417 | 1.00 | 0, 0 |
| boundary | once | first | 42,294 | 42,408 | 1.00 | 12, 12 |
| port | redescribe | request | 5,569 | 7,452 | 1.34 | 10.84, 10.84 |
| port | redescribe | completion | 5,637 | 7,448 | 1.32 | 10.84, 10.84 |
| port | redescribe | first | 20,747 | 22,547 | 1.09 | 9, 9 |
| port | redescribe | idle | 1,688 | 3,596 | 2.13 | **1, 1** |
| boundary | redescribe | request | 4,822 | 9,881 | 2.05 | 9, **11** |
| boundary | redescribe | completion | 4,353 | 9,414 | 2.16 | 9, **11** |
| boundary | redescribe | idle | 817 | 5,916 | 7.24 | 0, **1** |
| boundary | redescribe | first | 35,587 | 48,996 | 1.38 | 12, 13 |

The `first` cells are bimodal from run to run on both sides (boundary once first: 34.8 K or 42.3 K, a fresh
app's first commit). The median is reported.

**`EL=1 redescribe_cost 1000 10000`.** Instructions per turn, three interleaved runs (median). Run-to-run spread
is under 0.1%, except typed `static`, which varies by up to 0.5% on both sides:

| rows | shape | typed base | typed head | Δ | El base | El head | Δ | allocs/turn (all four) |
|---|---|---:|---:|---:|---:|---:|---:|---|
| 1000 | owned | 296,413 | 296,417 | +0.00% | 428,598 | 431,954 | +0.78% | 0 |
| 1000 | static | 291,619 | 291,406 | −0.07% | 437,658 | 441,031 | +0.77% | 0 |
| 1000 | bare | 125,231 | 125,235 | +0.00% | 166,573 | 162,587 | −2.39% | 0 |
| 10000 | owned | 2,915,411 | 2,915,762 | +0.01% | 4,182,363 | 4,212,840 | +0.73% | 0 |
| 10000 | static | 2,865,966 | 2,865,837 | −0.00% | 4,272,461 | 4,302,562 | +0.71% | 0 |
| 10000 | bare | 1,232,058 | 1,232,407 | +0.03% | 1,615,720 | 1,575,563 | −2.47% | 0 |

**Workload scenes** (they use `App::redescribe`, which this branch touches). All six scenes, `MODE=once` and
`MODE=redescribe`, `RENDER=0`, median of 3, base and head interleaved. `comparisons/bough-workloads` is outside the
workspace, so each tree was built from an isolated copy as `comparisons/bough-fx-bench/scripts/build-historical.sh`
builds one. gbcanvas's two scene files are not in the tree; the same copy of `11bc801858`'s files went into both
builds, with their prelude import updated to `bough::builders`. Results:

* Re-description: every steady, hover, theme, resize, resize_anim, anim and idle cell of every scene is within
  ±0.3% (sheet steady 43,261,985 -> 43,262,821; ide 23,330,487 -> 23,331,607; dash 3,890,411 -> 3,890,042; canvas
  583,662 -> 583,796; chat 17,999,199 -> 17,957,782; gbcanvas 6,009,233 -> 6,019,773). Mount is within ±1%.
* Once: every cell is within ±1% except `first_frame` and two steady cells. `first_frame` moves +34% on sheet
  and −10% on ide/canvas: the boundary artifact recorded above (Results, "The measured scenes"). The steady
  cells are sheet steady (1,733 -> 1,924, +191 on each wheel step and 0 on the edit and select steps) and
  gbcanvas steady (5,914 -> 5,676, −4%). The sheet shift is layout: disabling the branch's one kernel-path
  addition (`App::redescribe`'s `rescan` settle, dead in once mode) moves those wheel steps to **−122** below
  base instead. gbcanvas mount is −2.1%.

**Against the card.**

* Once mode: every port and boundary cell is ≤ 1.01 except `port once request`, where one run reached 1.14 (typed
  ran 4,013 in that run against 4,591 in the others). Allocations equal typed's. Inside the prediction.
* `redescribe_cost`: every typed cell is within ±0.1% and every El owned/static cell within +0.8%. El bare is
  **−2.4%, outside ±1%, faster**. It does not come from the new `check_at` arm (moving it behind a guard changed
  nothing) and was not bisected further.
* **Kill line crossed: allocations under re-description.** The El boundary makes 11 allocator calls per cycle
  where typed makes 9, and 1 per idle turn where typed makes 0. The cause is not the boundary's machinery. It is
  the map closure the author wrote (`text(self.r.map(|s| s.cloned().unwrap_or_default()))` builds a `String`),
  which El's plug runs every time `view` runs. The typed twin `sig!(self.r).map(f)` does exactly the same at
  each of its own re-descriptions: the plain-port rows are equal (10.84, 10.84; idle 1, 1). Typed's boundary
  escapes only because it is opaque (`update_opaque`: its arms are mounted once and never re-described). El's
  arms are re-described every turn, and the description is where the closure runs. El map plugs over signals
  have behaved this way since the El surface landed (base); this branch adds reader maps that behave the same.
  **Not fixed here.** The card's remedy is "the El operand carries the kernel's request directly": a map
  operand that keeps the recipe and evaluates it only at mount or when its cell is not a wired field of the
  view. That also requires a field-wired port's seal and update to leave the value out, which changes El's
  seal semantics for every map port. That is a design decision for the El surface, not a finishing step.
* Re-description cost: the prediction was ≤ 1.3x. It measured 1.32–1.34x on port and 2.05–2.16x on boundary
  (idle 7.2x). The boundary refuses its seal (its shown arm is runtime state), so each turn updates its three
  arm parts in full, and the arms are unsealed parts. Sealing the boundary on its guards, its policy and the
  deep seals of its arms would let an unchanged description step over it. That would bring idle toward the port
  figure; the allocation above stays until the operand is lazy.

**Design check** (05-async §2–5, 03-ports §1). The spellings follow the design: `boundary().loading(..)
.failed(..).child(..)` (an arm is an `El` or a `fn() -> El`, as in `.loading(detail_skeleton)`);
`is_pending(&self.results).map(|p| if p { 0.65 } else { 1.0 })`; `latest(&self.selected_id)`; `map` over a
source. Deviations, each with its reason:

1. **Producer:** `produce(&self.src, &self.reader, request)`, an element naming a reader field, instead of
   `selected_id.try_map_async(fetch)` returning a `Reader` (05 §1–2). The reader must be a field of the view so
   its ports are direct (documented on `produce`). Missing: `map_async_on(runtime, f)` and `compute_on(pool, f)`.
   Requests are `fn(&S) -> Request<T>` and failures are `String` (no typed `E`, 05 §6).
2. **A reader's ready view in `map` is `Option<&T>`** (`None` before first readiness), where 03 §1 has `&T`
   behind the Ready/Pending/Fault verdict. A port's closure cannot see a fault. `project` (05 §3), `map_ref`,
   `Settled`/`reader.or(fallback)` (§3.1) and `resolve().await` are not built. A plain `&Reader<T>` does not plug
   into a port without `.map`.
3. **`is_pending` takes `&Reader<T>` only; `latest` takes `&Signal<S>` only.** 03 §1.2 makes both total over
   `impl Source`: a derived reader, and the source write that began the cohort. `Source` here has a GAT
   `Ready<'a>` with neither a `Copy` bound nor a `Value` type. There are no tuple (product) sources and no
   `Sink`/`Cell` split. A map closure captures at most 16 bytes, by copy (as for signals).
4. **How a boundary finds its guards:** it collects them from the ports of its `child` arm (reader value ports,
   not `is_pending`, not through a nested boundary) after the child is described. The design installs the
   boundary before the child is evaluated (05 §4); the observable result is the same. A reader that is not a
   field of the view guards nothing (it is a constant of the description).
5. **Boundary policy:** `replace_on_failure()` names the design's "replace-with-error"; the default is
   keep-stale (05 §6). The failed arm is not given the failure (no failure key or `E`) and offers no retry.
   `reload_on(&self.selected_id)` (05 §4) is not built.
6. **All three arms are mounted** under clips of their own, and in a layout container they all take space (as the
   kernel's boundary does). The El boundary's arms are re-described each turn; the typed boundary is opaque.

**Open.**

* The allocation kill line (above): lazy map operands carrying the kernel's request; a design decision on
  El's seal semantics.
* Sealing an El boundary so that an unchanged description steps over it (re-description 2x -> about 1.3x).
* El bare in `redescribe_cost` −2.4% (faster) and El owned/static +0.7–0.8% are not attributed. sheet steady
  once ±190 instructions per wheel step and gbcanvas steady once −4% follow code layout (shown by the
  dead-code A/B above).
* 05/03 surface not built: `project`, `map_ref`, `Settled`/`or`, `reload_on`, `resolve().await`,
  `try_map_async`/`map_async_on`/`compute_on`, `is_pending`/`latest` over any source, product sources, typed
  failures and retry.
* Formatting: no rustfmt config is checked in, so the branch's commits reflowed unrelated lines in `app.rs`,
  `elem.rs`, `reader.rs` and `el/walk.rs` (no code change). The added example lines keep the file's wide style.
