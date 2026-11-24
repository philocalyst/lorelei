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

## Diagnostics corpus (gate D1, `final/10-quality.md` §4)

`crates/bough/tests/diagnostics.rs` runs a `trybuild` corpus (`tests/ui/`) whose message text is snapshot-tested.
The El traits carry `#[diagnostic::on_unimplemented]` text in the design's vocabulary: `IntoProp` ("cannot feed
this property", with the feed forms and the by-reference note), `ChildSeq` ("is not a child", strings are text
leaves), `Route` ("is not a route", `.with(value)`), `Len`, and a new sealed `Sink` for the two-way port
(`text_input(&self.reader)` now fails as "is not a writable `String` cell" instead of a type mismatch). Cases: a
signal by value, a string child, a method taking a value as a route, a colour signal into a width, an `f64`
length, a `Reader` in a two-way port, an escaped borrow, a map closure capturing `&self`; one pass case.

`text(&self.count)` (an integer `Signal` field as a text port, the zero-rerun spelling the plan deferred) now
exists: `Apply::apply_int` formats into a stack buffer for the text applier, so a publish allocates nothing
(`el_surface::an_integer_signal_is_a_text_port`: port under `once`, agrees with re-description, equals a fresh
mount; `u32::MAX`, `i64::MIN`).

Not in the trybuild corpus, recorded: a route closure over 16 bytes is a post-monomorphisation error (`cargo
check` accepts it, `cargo build` refuses it), so it is a `compile_fail,E0080` doctest on `Route` with a passing
16-byte twin; a container setter on a text (`text("a").gap(8)`) is a located panic at the setter, not a compile
error, because `El` is one type (the design's "wrong layout-family method" is typed per family; here the
element's kind is a runtime value).

## Miri (gate 13, first run)

`MIRIFLAGS=-Zmiri-ignore-leaks cargo miri test -p bough --lib -- el:: rt:: seal:: reflect:: signal:: list:: describe::
changed:: store:: ledger:: mlist:: entity::` (nightly-2026-10-17, Stacked Borrows): 10 passed, 1 ignored, 473 s.
`-Zmiri-ignore-leaks` because the El arena's chunks are never freed by design (`arena.rs`: a stale handle must
always read a turn word). Integration tests (`tests/*.rs`) drive `App`, which calls the platform's frameworks, and
cannot run under Miri.

Found and fixed: **`DNode::block` was undefined behaviour under Stacked Borrows.** The seal's 104-byte head block
was read through a pointer made from `&self.kind`, whose provenance covers one byte; it is now derived from the
whole node (same address, same code). Recorded, not changed: the recipe freeze is inline asm, which Miri cannot
execute (`padded_captures_seal_by_value` is ignored under Miri with that reason); and Rust does not promise that a
function cast to a pointer twice has one address (Miri gives a fresh one each time), so equal descriptions may
seal apart there. A seal holds patch and trampoline addresses, so this is a missed skip, never a wrong one: two
functions can share an address only when their code is identical. `the_fast_check_agrees_with_the_seal` asserts
the weaker law under Miri.
