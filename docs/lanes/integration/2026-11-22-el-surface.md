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

## Motion as a plug, map recipes, hover colours (card frozen 2026-11-24, before building)

Branch `lane/reactivity/el-motion` from `1a090b8b7f`. Items: (A) `animate(&self.x).spring(..)` / `.over(ms)` /
`.curve(..)` / `.delay(ms)` as a plug of the f32 and `Color` ports (`opacity`, `translate_x/y`, `w`, `h`, `bg`,
`fg`), lowered onto the kernel's wire transitions (`transition.rs`); (B1) a change of a map's captured recipe
reaches the installed wire on the turn it is described; (B2) a hover colour changed while hovered shows at once;
(C) theme tokens only if the kernel has a mechanism to lower onto.

**Question.** Does a view that uses none of this pay anything, and what does an El motion port cost per frame and
per retarget against the typed `sig!(..).transition(..)` it lowers onto?

**Measure.** (1) `EL=1 redescribe_cost 1000 10000` (instructions per turn and allocator calls per steady turn,
typed and El twins), base `1a090b8b7f` against head, same host, same session. (2) The six workload scenes
(`comparisons/bough-workloads`, `MODE=once` and `MODE=redescribe`, `RENDER=0`, median of 3), base against head,
because the change touches the kernel's `Wire`, `cut_scope` and `Transitions` that their transitions use. (3) A new
example `el_motion_cost`: N animated boxes (N = 1, 1000) in one binary, El `animate(..).curve(c)` against typed
`sig!(..).transition(c)` on the same curve, plus El `animate(..).spring(..)`; instructions per `App::tick` with every
box in flight, instructions per retarget turn (`Config::once`, one signal published), and `HEAP=1` allocator calls
per tick and per retarget.

**Prediction.** (1) and (2): every cell within ±1% of base, zero allocator calls per steady turn. (3) The El eased
port runs the kernel's own transition state, so per tick it is within ±2% of typed; a retarget costs at most 1.3x
typed (an affine map and an equality cut in the patch); a spring tick costs at most 2.5x an eased tick (one `exp` and
one `sin_cos` per box); zero allocator calls per tick and per retarget once warm, for all three.

**Kill line.** Any allocator call in a steady tick or retarget; El eased tick above 1.1x typed; any redescribe_cost
cell or workload cell outside ±1% that is not shown to be code-layout noise by a no-op rebuild. Past any, the motion
port does not land in this form.

**Correctness gates** (each with a mutant feature that must fail it): retarget mid-flight is continuous (C0; C1 for
springs and `Curve::smooth`), mutant retargets from the target instead of the presented value; one timestamp gives
one value at 64 Hz, 128 Hz and an irregular cadence; the settled frame equals a fresh mount at the target (pixels);
a motion port reruns no `view`; idle after settle steps nothing and requests no frame; a branch flip mid-flight
remounts per 02 §6 (the old track is dropped, the new position equals a fresh mount), mutant keeps the old track; a
view with no motion makes no transition state; B1 and B2 each against a fresh mount, each with a mutant.

**Not claimed.** Frame time; the lowered runtime (`lower`) presents a motion port's target (no motion lanes there).

### Results: motion as a plug, map recipes, hover colours (2026-11-24)

Commits on `lane/reactivity/el-motion`: `afbfd79b5f` (motion plug, B1, B2), `cd2ee2f654` (two more B1 oracles),
this section. Built: `animate(&sig)` with `.spring(..)`, `.over(ms)`, `.ease(..)`, `.curve(..)`, `.delay(ms)`, and
on `f32` `.reverse()` / `.scale(k)`, accepted by the `f32` ports (`opacity`, `translate_x/y`, `w`, `h`) and the
`Color` ports (`bg`, `fg`). A motion is a transition in the kernel's own table (`transition.rs`: `Anim<T>`, eased as
`State` is, a closed-form damped spring, a Hermite `Curve::smooth`), stepped by `App::tick`, retargeted by its wire's
patch from the presented value and velocity, and retired when its wire is cut (`Wire::motion`). Timings are sealed
in the `Ext` part (`X_MOTION`), so a node without motion checks nothing more.

**(1) Zero tax: `EL=1 redescribe_cost 1000 10000`**, instructions per turn, base `1a090b8b7f` against head, same
session, two runs each (run 2 shown; run 1 agrees within 0.2%). Allocator calls per steady turn: 0.00 in every
cell, base and head (`HEAP=1` tables are identical byte for byte).

| rows | shape | typed base | typed head | Δ | El base | El head | Δ |
|---:|---|---:|---:|---:|---:|---:|---:|
| 1000 | owned | 296,413 | 296,412 | −0.00% | 428,599 | 429,049 | +0.10% |
| 1000 | static | 291,619 | 291,618 | −0.00% | 437,660 | 438,100 | +0.10% |
| 1000 | bare | 125,231 | 125,230 | −0.00% | 166,573 | 166,625 | +0.03% |
| 10000 | owned | 2,915,679 | 2,915,412 | −0.01% | 4,182,698 | 4,182,801 | +0.00% |
| 10000 | static | 2,865,618 | 2,866,058 | +0.02% | 4,272,670 | 4,272,260 | −0.01% |
| 10000 | bare | 1,232,059 | 1,232,055 | −0.00% | 1,615,724 | 1,615,600 | −0.01% |

**(2) The six workload scenes** (`RENDER=0`, median of 3, base and head interleaved, instructions per step). Built
from `git archive` exports of base and head with `comparisons/bough-workloads` added to the workspace members and
the `gui-bench-scenes` / `bough-guibench-scenes` sources copied in (identical for both). Every cell, both modes, is
within ±1% (largest: redescribe chat steady −0.37%, gbcanvas steady once +0.41%) except `first_frame`. Steady,
redescribe:

| scene | base | head | Δ |
|---|---:|---:|---:|
| sheet | 43,262,957 | 43,262,830 | −0.00% |
| ide | 23,332,722 | 23,331,521 | −0.01% |
| dash | 3,908,065 | 3,914,662 | +0.17% |
| canvas | 635,752 | 635,774 | +0.00% |
| chat | 18,024,199 | 17,956,720 | −0.37% |
| gbcanvas | 5,768,450 | 5,766,647 | −0.03% |

Mount cells are within ±0.17%; hover, theme, resize, resize_anim, anim and idle within ±0.25%. `first_frame` is
code-layout noise, shown by a no-op rebuild: base built again with only `-C symbol-mangling-version=v0` gives
sheet 13,366 -> 55,204 (+313%), with the other scenes taking the same two values (≈13.4 K or ≈19.2 K) as the
base and head binaries, in a different arrangement (base: sheet 13,366, ide 13,450, dash 19,184, canvas 19,129,
chat 13,333, gbcanvas 19,203; head: 19,173, 13,355, 13,366, 13,388, 14,127, 14,127). This is the boundary
effect recorded in "The measured scenes" above.

**(3) `el_motion_cost`** (instructions; `HEAP=1` allocator calls per tick, per retarget and per re-describing
tick are 0.00 in every cell):

| boxes | spelling | tick | ×typed | retarget | ×typed | tick, re-describing | ×typed |
|---:|---|---:|---:|---:|---:|---:|---:|
| 1 | typed `sig!(..).transition(c)` | 2,381 | 1 | 2,222 | 1 | 3,365 | 1 |
| 1 | El `animate(..).curve(c)` | 2,371 | 0.996 | 2,258 | 1.016 | 4,345 | 1.29 |
| 1 | El `animate(..).spring(..)` | 2,540 | 1.07 | 2,534 | 1.14 | 4,513 | 1.34 |
| 1000 | typed | 318,635 | 1 | 135,089 | 1 | 792,192 | 1 |
| 1000 | El eased | 308,635 | 0.969 | 171,089 | 1.27 | 895,982 | 1.13 |
| 1000 | El spring | 477,635 | 1.50 | 447,265 | 3.31 | 1,065,154 | 1.34 |

Against the prediction: El eased tick 0.97x typed (outside the predicted ±2%, on the cheap side; kill line 1.1x
not reached); retarget 1.27x at 1000 boxes (predicted ≤ 1.3x); spring tick 1.55x the eased tick (predicted ≤
2.5x); zero allocations everywhere. Not predicted: the re-describing tick is 1.13x typed at 1000 boxes and 1.29x
at one box (the El walk and seal of the owner, as for any El view: the redescribe_cost El/typed ratios are
1.31–1.50), and a spring retarget costs 3.3x a typed eased retarget (it samples the spring's value and velocity
in closed form and re-solves its initial conditions per box).

**Correctness.** `tests/el_motion.rs`, 13 tests, pass in debug and `--release`. Mutants (each a cargo feature),
every one fails its tests:

| mutant | what it breaks | fails |
|---|---|---|
| `mutant_motion_from_target` | a retarget starts from the target, not the presented value | `a_retarget_mid_flight_is_continuous`, `an_eased_motion_presents_what_the_typed_transition_does` |
| `mutant_motion_snap` | an update does not show the motion's presented value | `a_motion_port_reruns_no_view`, `the_settled_frame_equals_a_fresh_mount_at_the_target` |
| `mutant_motion_keep_track` | a cut wire keeps its motion (02 §6) | `a_branch_flip_drops_the_track` |
| `mutant_el_recipe` | an updated map's wire keeps its mount's captures | `a_map_whose_captures_changed_is_right_at_once`, `…_on_the_same_frame`, `…_past_many_ports` |
| `mutant_el_hover` | a hover colour re-described while hovered waits for the next entry | `a_hover_colour_changed_while_hovered_shows_at_once` |

B1 (a map's captured recipe): the recipe is sealed, so a changed capture (`self.n.map(move |n| n * k)`, another
`k`) fails the seal fast check and the item is updated; the update gives each installed wire the description's
recipe (`motion::sync_ports`, matched by cell and patch in install order within the element's own scope; one
element has at most six wired ports), so the frame of that very turn equals a fresh mount (tested with and
without motion in the scene, and with 80 ports of one list item each capturing its own factor). Not tested: a
later publish that runs the wires alone (`Config::once`) after a capture change. B2 (hover colour): `set_routes` paints the new hover colour at once when the act is
hovered (`input::show_hover_bg`, over the element's opacity, as `hover_set` does); leaving shows the base.
The seal fast check (`check_at`) is unchanged for nodes without `Ext`; motion timings are checked only under
`X_MOTION` in the cold `check_progs`.

**Suite.** `cargo test -p bough` green in debug and `--release` (47 result blocks each, 0 failures);
`cargo build --workspace --all-targets` ok. `cargo clippy -p bough --all-targets --no-deps`: no finding on a line
this branch wrote (its findings are older lines; the one error is in the untouched `tests/typed_words.rs`, and
without `--no-deps` clippy stops on deny lints in `bough-core`, `bough-metal` and `bough-fused`'s build script).

**Deviations from `designs/reactivity/final/03-ports.md` §4.**
* Durations are `u32` milliseconds (`.over(300)`, `.delay(100)`), not `2.s()` unit values: the tree has no
  duration type yet.
* Motions live in the kernel's transition table (`Transitions`), one per port, stepped by `App::tick`, and not
  in kind-segregated lanes. `reverse()` / `scale(k)` make their own motion per port (a spring is linear, so the
  mapped motion is the motion of the mapped value). A view without motion makes no motion state (tested).
* Only `f32` and `Color` (per channel) interpolate; no lengths, angles, transforms, brushes, paths or shader
  fields, and no `Interpolatable` for user types.
* A delay holds the presented value, so a delayed motion retargeted mid-flight is continuous in value only (C0,
  not C1).
* Every spring keeps velocity on retarget; `preserve_velocity()` is accepted and does nothing.
* The lowered runtime (`lower`) presents a motion port's target (no motion there).

**Open.**
* (C) Theme tokens: not started (skipped by the task); no token plug.
* From 03 §4: keyframes, sequence / parallel / race, repeat (the `rotate(repeat(..))` of the example), gesture
  handoff and the scroll / media / demo timelines, `jump(target)`, playback control (pause, seek, rate, fill),
  composition other than replace, reduced motion, completion as a queued output, structural enter/exit/hero.
* Motion on the lowered runtime.
* A spring retarget costs 3.3x a typed eased retarget, and the re-describing tick of an El motion view is
  1.13–1.29x typed (the El walk); neither is a kill line, both are recorded.
* The "Open" entries "A map's recipe is not part of `shape()` …" and "A hover colour changed while hovered shows
  on the next hover" above are closed by this branch.
