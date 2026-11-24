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

**Correctness.** `tests/el_motion.rs`, 13 tests (15 after the review fixes below), pass in debug and `--release`. Mutants (each a cargo feature),
every one fails its tests:

| mutant | what it breaks | fails |
|---|---|---|
| `mutant_motion_from_target` | a retarget starts from the target, not the presented value | `a_retarget_mid_flight_is_continuous`, `an_eased_motion_presents_what_the_typed_transition_does` |
| `mutant_motion_snap` | an update does not show the motion's presented value | `a_motion_port_reruns_no_view`, `the_settled_frame_equals_a_fresh_mount_at_the_target` |
| `mutant_motion_keep_track` | a cut wire keeps its motion (02 §6) | `a_branch_flip_drops_the_track` |
| `mutant_el_recipe` | an updated map's wire keeps its mount's captures | `a_map_whose_captures_changed_is_right_at_once`, `…_on_the_same_frame`, `…_past_many_ports` |
| `mutant_el_hover` | a hover colour re-described while hovered waits for the next entry | `a_hover_colour_changed_while_hovered_shows_at_once` |
| `mutant_row_keep_track` | a released virtual-list row keeps its motions (review fix) | `rows::a_scrolled_out_row_drops_its_track` (also fails under `mutant_motion_keep_track`) |
| `mutant_opacity_hover` | an opacity write over a hovered box paints its base colour (review fix) | `an_animated_opacity_keeps_the_hover_colour` |

B1 (a map's captured recipe): the recipe is sealed, so a changed capture (`self.n.map(move |n| n * k)`, another
`k`) fails the seal fast check and the item is updated; the update gives each installed wire the description's
recipe (`motion::sync_ports`, matched by cell and patch in install order within the element's own scope; one
element has at most six wired ports), so the frame of that very turn equals a fresh mount (tested with and
without motion in the scene, and with 80 ports of one list item each capturing its own factor). Not tested: a
later publish that runs the wires alone (`Config::once`) after a capture change. B2 (hover colour): `set_routes` paints the new hover colour at once when the act is
hovered (`input::show_hover_bg`, over the element's opacity, as `hover_set` does); leaving shows the base.
The seal fast check (`check_at`) is unchanged for nodes without `Ext`; motion timings are checked only under
`X_MOTION` in the cold `check_progs`.

**Suite (rechecked at `e3fcde8e35`, after the review fixes).** `cargo test -p bough` green in debug and
`--release`: 47 result blocks each, 185 passed, 0 failed, 12 ignored. The first release build was killed by
macOS during compilation (exit 137, no test failure); rerun with `CARGO_BUILD_JOBS=1` and `nice -n 15`, green.
`cargo build --workspace --all-targets` ok with the same one-job limit. `cargo clippy -p bough --all-targets
--no-deps`: no finding on a line
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

### Review fixes (DeepSeek Flash review, `review-el-motion-flash.md`)

**HIGH: a released list row kept its motions.** `lr_release_row` (listview.rs) marked its row's wires dead and
cleared the scope's list itself, so `cut_scope` (then the only caller of `Transitions::retire`) never retired a
virtual-list row's motion: scrolled-out rows left running tracks that painted their value onto the node ids the
rows scrolling in recycle, and the table grew. Both paths now cut through one `Rt::cut_wire` (dead + retire), and
`Transitions::retire` frees an id once when it is retired twice (`Running::retire` returns `None` for a motion
already retired: the review's `run_head` note, a wire index recycled while an old scope still lists it).
Test `rows::a_scrolled_out_row_drops_its_track`: 200 `List<Row>` rows with `animate(&fade).over(400)`, all fades
published, scrolled out mid-flight, then every frame against a fresh mount at the scroll position (pixels), no
motion running, as many live motions as the same scroll with nothing in flight, and scrolled back = a fresh mount.
Without the fix it fails at the first frame after scroll-out (pixels). `transition::retire_tests` covers the
double retire. (The review's probe compared against a run whose fades were never published, so its 72,000
differing bytes mix state and leak; the leak itself, 7 orphans stepped each tick, was real.)

**MED: an opacity write wiped the hover colour.** `ApplyOpacity::apply` repainted the fill from the described
colour while `NK_HOVERED`, so every motion frame of a hovered box showed its base. The applier's body is base's;
after it, a hovered box (one test of the kind already loaded) takes a cold fix-up that repaints the act's hover
colour through the new opacity (`opacity_hovered`). Test `an_animated_opacity_keeps_the_hover_colour`: hovered,
opacity in flight, hover colour changed while hovered at frame 6, 30 frames each against a fresh hovered mount at
that frame's presented opacity, settled, then left = the base. A first version that routed every non-plain quad
through a cold function (one test on the hot path) cost dash steady redescribe +1.1% (5 runs; codegen around the
inlined applier, 330–680 calls a step), so it was replaced.

**Measured after the fixes** (head `e3fcde8e35`, same session as base). `EL=1 redescribe_cost 1000 10000`, two runs,
every cell within ±0.11% of base (largest: 1000 static El +0.10%), allocator calls 0.00 per turn everywhere, `HEAP=1`
tables identical. Workload scenes (median of 3, both modes): every cell within ±1% except `first_frame` (the layout
cell above), `ide steady once` (+2.06% in the table run) and `gbcanvas steady once` (−8.2%); both are bimodal in
every binary, base included: over five runs base gives ide 38,786 / 39,815 and gbcanvas 6,199–6,715, head 38,795 /
39,819 and 6,035–6,040, base built with v0 mangling 38,791–39,815 and 6,041–6,337. Dash steady redescribe
3,914,722 vs 3,920,813 (−0.16%). `el_motion_cost` after the fixes (allocator calls 0.00 everywhere):

| boxes | spelling | tick | retarget | tick, re-describing |
|---:|---|---:|---:|---:|
| 1 | typed | 2,384 | 2,222 | 3,371 |
| 1 | El eased | 2,374 | 2,258 | 4,348 |
| 1 | El spring | 2,543 | 2,534 | 4,516 |
| 1000 | typed | 321,635 | 135,089 | 798,192 |
| 1000 | El eased | 311,635 | 171,089 | 898,982 |
| 1000 | El spring | 480,635 | 447,059 | 1,068,154 |

The hovered test costs every opacity write 3 instructions: a 1000-box tick +3,000 (+0.94%) for typed and El alike
(this applier is the kernel's, so typed `sig!(..).transition(..)` opacity pays it too); no workload anim cell moved
(dash anim −0.2%). Recorded, not hidden: it is the one cost of the fix.

Open from the review: a wire index that `run_head` recycles while an old scope still lists it can still be cut by
that scope after a new motion took it, retiring the new motion (the guard stops the double free, not the
misattribution; it needs a moved `Signal` head). `Transitions::retire` acts on motions only (`State`, the typed
`.transition(..)`'s, returns `None`); whether a typed transition on a released virtual-list row is dropped some
other way was not checked here.

The suite recheck above used two build jobs for debug (green) and one for release and the workspace build. The
initial two-job release build was killed during compilation, before any test ran; no source change was made for
the successful one-job rerun, and the resulting code head is `e3fcde8e35`.
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

**Verification.** `cargo test -p bough --release`: 49 targets, 188 passed, 0 failed, 13 ignored. Debug: 49
targets, 186 passed, 0 failed, 13 ignored with the two very slow `pages` transition tests skipped; the
unskipped run reached those tests with every preceding target green (they also ran for over ten minutes in
earlier attempts on this host). `cargo build --workspace --all-targets` passes. Clippy on the library and all
touched tests/examples has no finding on a line this branch wrote. It runs with
`-A clippy::mut_from_ref -A clippy::approx_constant -A clippy::suspicious_assignment_formatting`: the first
two are needed by bough-core, bough-metal and bough-fused's build scripts; the last by the unchanged
`typed_words`/`typed_twin` tests. Both denials were reproduced on `1a090b8b7f`.

Mutant proof (`cargo test --release -p bough --test el_reader --features <m>`; the nine reader tests pass without one):

| mutant | what it breaks | failing tests |
|---|---|---|
| `mutant_reader_stamp` | admission passes every ticket | `a_stale_completion_is_rejected` |
| `mutant_el_boundary_ready` | the skeleton shows while any guard is pending (first readiness ignored) | `loading_then_content_then_revalidation_keeps_content` |
| `mutant_el_bound_wire` | a guard's transition does not reach its boundary | `loading_then_content_then_revalidation_keeps_content`, `random_scripts_match_a_fresh_mount_at_every_step`, `a_first_failure_shows_the_failed_arm`, `a_listed_boundary_is_stepped_over_and_switched_by_its_wires` |
| `mutant_cell_offset` (existing) | the cell table's offsets | 8 of 9 (all but `a_producer_of_a_reader_elsewhere_is_refused_by_name`) |

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
* **Kill line crossed: allocations under re-description** (closed by lazy port values: "Lazy port values and a
  sealed boundary" below). The El boundary makes 11 allocator calls per cycle
  where typed makes 9, and 1 per idle turn where typed makes 0. The cause is not the boundary's machinery. It is
  the map closure the author wrote (`text(self.r.map(|s| s.cloned().unwrap_or_default()))` builds a `String`),
  which El's plug runs every time `view` runs. The typed twin `sig!(self.r).map(f)` does exactly the same at
  each of its own re-descriptions: the plain-port rows are equal (10.84, 10.84; idle 1, 1). Typed's boundary
  escapes only because it is opaque (`update_opaque`: its arms are mounted once and never re-described). El's
  arms are re-described every turn, and the description is where the closure runs. El map plugs over signals
  have behaved this way since the El surface landed (base); this branch adds reader maps that behave the same.
  The card's remedy is "the El operand carries the kernel's request directly": a map operand that keeps the
  recipe and evaluates it only at mount or when its cell is not a wired field of the view, with a field-wired
  port's seal and update leaving the value out. That was decided and built afterwards (below).
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
   Requests are `fn(&S) -> Request<T>`, where `Request` owns a boxed `FnOnce`; failures are `String` (no typed
   `E`, 05 §6). `Reader<T>` is not `Copy` as 05 §1 specifies: it owns its committed value and private slots.
2. **A reader's ready view in `map` is `Option<&T>`** (`None` before first readiness), where 03 §1 has `&T`
   behind the Ready/Pending/Fault verdict. A port's closure cannot see a fault. `project` (05 §3), `map_ref`,
   `Settled`/`reader.or(fallback)` (§3.1) and `resolve().await` are not built. A plain `&Reader<T>` does not plug
   into a port without `.map`.
3. **`is_pending` takes `&Reader<T>` only; `latest` takes `&Signal<S>` only.** 03 §1.2 makes both total over
   `impl Source`: a derived reader, and the source write that began the cohort. `Source` here has a GAT
   `Ready<'a>` with neither a `Copy` bound nor a `Value` type. There are no tuple (product) sources and no
   `Sink`/`Cell` split. A map closure captures at most 16 bytes, by copy (as for signals). This lane's port
   descriptors carry `Impact` only implicitly in their patch (`Apply`), not as a sealed `const IMPACT`.
4. **How a boundary finds its guards:** it collects them from the ports of its `child` arm (reader value ports,
   not `is_pending`, not through a nested boundary) after the child is described. The design installs the
   boundary before the child is evaluated (05 §4); the observable result is the same. A reader that is not a
   field of the view guards nothing (it is a constant of the description).
5. **Boundary policy:** `replace_on_failure()` names the design's "replace-with-error"; the default is
   keep-stale (05 §6). The failed arm is not given the failure (no failure key or `E`) and offers no retry.
   `reload_on(&self.selected_id)` (05 §4) is not built. Guards stop at nested boundaries (each boundary
   evaluates its own child readers), rather than the design's nested-boundary join. A reader not directly a
   field of the view guards nothing and is evaluated as a constant on each description; it cannot arm a
   boundary's first-readiness transition through a completion alone.
6. **All three arms are mounted** under clips of their own, and in a layout container they all take space (as the
   kernel's boundary does). The typed boundary is opaque (its arms are never re-described); the El boundary is
   sealed (stepped over when described as before) and its arms are sealed parts of one item each, so an arm
   described as before is stepped over and a changed one is updated (below).

**Open.**

* (Closed below: the allocation kill line, by lazy port values; sealing the El boundary.)
* El bare in `redescribe_cost` −2.4% (faster) and El owned/static +0.7–0.8% are not attributed. sheet steady
  once ±190 instructions per wheel step and gbcanvas steady once −4% follow code layout (shown by the
  dead-code A/B above).
* 05/03 surface not built: `project`, `map_ref`, `Settled`/`or`, `reload_on`, `resolve().await`,
  `try_map_async`/`map_async_on`/`compute_on`, `is_pending`/`latest` over any source, product sources, typed
  failures and retry.
* `Source`'s associated `Ready<'a>` is not `Copy` as 03 §1 specifies (a reader map's ready view is
  `Option<&T>`). The current `Source` also has no `Value` associated type; `map`'s output type is inferred by
  the port. The public `Reader<T>` itself is not a port; a direct port is `self.r.map(..)`.
* Formatting: no rustfmt config is checked in, so the branch's commits reflowed unrelated lines in `app.rs`,
  `elem.rs`, `reader.rs` and `el/walk.rs` (no code change). The added example lines keep the file's wide style.

### Lazy port values and a sealed boundary (2026-11-24, after the results above)

The allocation kill line is closed. This is the card's remedy ("the El operand carries the kernel's request
directly"), built after the results above.

**What changed.**

* **Plugs** (`el/plug.rs`). Every plug with a wire request (a signal, `latest`, a reader map, an `is_pending`
  map, each for every scalar port and text) stores a fixed placeholder in the node and an eval fn in its
  `WireReq`, monomorphised with the patch. Building a description runs no closure and reads no cell.
* **When the value is evaluated** (`el/walk.rs`, "Lazy port values"). The walk evaluates a port only where its
  value is needed: at mount; at an update whose port has no installed wire; at an update whose installed wire
  computes with other captures; when lowering; and in the seal of a port that is not a field.
* **Field-ness** is the one rule `elem::field_offset::<V>(owner, cell, ty)`. `Mounter::wire_field` installs by
  it. The seal asks it through the walk's owner (set by the root `El`'s `mount`/`update`) and records the
  answer as the port's first word (`WIRED`, or `CONST` followed by the value). A retained seal is only checked
  by updates of the owner that wrote it, and an owner does not move while mounted, so the fast check trusts a
  retained `WIRED` whose identity (cell address, patch, recipe) matches.
* **Updates** (`resolve_ports`). Before a node's builder runs, each port is matched against the wires its
  item's mount installed (same head, same patch, in mount order). A match with the same recipe is kept
  (`P_KEEP`, `IntoProp::keep`/`TextSrc::keep`: neither computed nor applied). A match with a new recipe gets
  the description's recipe and is evaluated and applied this turn; no match is a constant of this description
  (`P_CONST`) and is evaluated and applied. The request's state also tells the fast check whether to compare
  a value: a `WIRED` identity with a value-less seal is followed by one exactly when this update resolved the
  port as `P_CONST`. The kernel's `LBox::update`/`Para::update` skip a kept operand (a bordered box reads its
  applied fill, `elem::applied_bg`). `keep` is `false` for every typed operand.
* **The boundary** (`K_BOUND`). It now seals as its policy, its guard readers and its three arms, so a boundary
  described as before is stepped over; which arm shows is its wires' state. Its arms are sealed one-item parts,
  so inside a box (an unsealed part) its unchanged arms are stepped over.
* **The fast check** (`check_at`). A node with a fill, ink or text port is checked out of line
  (`check_ported`). The first build inlined the port checks, whose constant-value evals are calls, into every
  leaf. A list's plain rows have no ports but paid 5–7 instructions each (+1.2–1.9% on `redescribe_cost`
  owned/static), bisected to that inlining; the split took it back to base.

**Gate** (final implementation `078ac8fbad`, base `1a090b8b7f`; Apple M3 Ultra, macOS 26.7.3,
nightly-2026-10-17, release `thin` LTO, calling-thread instruction counters, no renderer; one session,
interleaved, median of 3). `reader_cost`, re-description (once mode: every cell ≤ 1.01 of typed, allocations
equal):

| shape | phase | typed | El before | El now | El / typed now | allocs (typed, El before, El now) |
|---|---|---:|---:|---:|---:|---|
| port | request | 5,569 | 7,452 | 6,993 | 1.26 | 10.84, 10.84, **8.84** |
| port | completion | 5,564 | 7,448 | 6,986 | 1.26 | 10.84, 10.84, **8.84** |
| port | idle | 1,688 | 3,596 | 3,070 | 1.82 | 1, 1, **0** |
| port | first | 20,672 | 22,547 | 22,059 | 1.07 | 9, 9, 8 |
| boundary | request | 4,822 | 9,881 | 7,857 | **1.63** (was 2.05) | 9, 11, **9** |
| boundary | completion | 4,353 | 9,414 | 7,388 | **1.70** (was 2.16) | 9, 11, **9** |
| boundary | idle | 817 | 5,916 | 3,825 | 4.68 (was 7.24) | 0, 1, **0** |
| boundary | first | 35,597 | 48,996 | 50,756 | 1.43 | 12, 13, **23** |

The boundary's allocations equal typed's (9 per cycle, 0 idle), and El ports now allocate less than typed (a
typed map port still runs its closure at each re-description). `boundary first` is the one worse cell. It is
the first re-description after mount, when the three now-sealed arms write their seals for the first time
(their storage is allocated once); every later turn of the same app allocates nothing. `boundary once first`
is bimodal from run to run on both sides (34.7 K or 42.2 K), as recorded above.

The re-description prediction (≤ 1.3x) **is missed**: port 1.26x, boundary request/completion 1.63–1.70x,
and boundary idle 4.68x (817 typed instructions). The boundary's request/completion ratio is down from
2.05–2.16x and its idle ratio from 7.24x. The remaining work is the El description and seal check of the
boundary's arms (about ten nodes); the typed boundary is opaque. Allocation equality, the crossed kill line,
is met. In once mode every cell is ≤ 1.01x typed and allocations are equal.

`EL=1 redescribe_cost 1000 10000` (zero allocations per steady turn in all twelve cells):

| rows | shape | typed base | typed head | Δ | El base | El head | Δ |
|---|---|---:|---:|---:|---:|---:|---:|
| 1000 | owned | 296,413 | 296,236 | −0.06% | 428,598 | 428,993 | +0.09% |
| 1000 | static | 291,619 | 291,611 | −0.00% | 437,658 | 438,943 | +0.29% |
| 1000 | bare | 125,231 | 125,235 | +0.00% | 166,573 | 161,622 | −2.97% |
| 10000 | owned | 2,915,411 | 2,915,535 | +0.00% | 4,182,185 | 4,182,654 | +0.01% |
| 10000 | static | 2,865,857 | 2,865,840 | −0.00% | 4,272,297 | 4,282,642 | +0.24% |
| 10000 | bare | 1,232,056 | 1,232,061 | +0.00% | 1,615,721 | 1,565,770 | −3.09% |

**Workload scenes** (base vs head, both modes, `RENDER=0`, median of 3; the kernel's `LBox`/`Para` update and
`wire_field` changed):

* Re-description: 51 of 54 cells are within ±1%. The three exceptions are canvas steady's harness p50
  (+7.36%), canvas first_frame (−1.32%) and canvas hover (−1.15%). Canvas steady is bimodal on the base
  itself (per-step p50 0.590 M or 0.636 M across five interleaved runs); the median sum over all 48 steps is
  40.856 M base and 40.899 M head (+0.11%), with 27 of 48 paired steps identical in the first run. The other
  two are faster. gbcanvas steady is +0.81%.
* Once: 51 of 54 cells are within ±1%. The exceptions are all favorable or boundary-sized: sheet steady
  −6.92% (1,733 -> 1,613 instructions on each wheel step), gbcanvas mount −1.18%, and ide first_frame
  +1.16% (7,352 -> 7,437 instructions). No steady cell regressed.

**Tests.**

* `tests/el_lazy.rs` (new, 4 tests):
  * a wired map's closure runs once at mount and once per write of its signal, never for a re-description
    (12 turns of a plain field, text, fill and opacity ports, in a box and in a list);
  * maps over signals behind a `Vec` (text, fill, opacity; in a box's children and in a list's items) show
    every new value on re-description;
  * el-motion's capture tests without motion: `a_map_whose_captures_changed_is_right_on_the_same_frame` and
    `..._past_many_ports` (80 wired ports of one item, each its own capture).
* `walk::tests::ports_seal_by_the_field_rule` (new): a field's new value seals alike, a value behind a `Vec`
  seals apart, another capture seals apart, and the fast check (trusting `WIRED`) agrees with the streaming
  check (asking the rule again).
* `walk::tests::the_fast_check_agrees_with_the_seal` now includes a field text/fill map and a non-field map.
  After resolution, a field's new value does not enter a `WIRED` seal, while a `P_CONST` port's new value does.
  Under the eager mutant the same test requires the opposite field-value verdict, and
  `the_eager_mutant_makes_a_fields_new_value_part_of_its_seal` is its direct scalar probe.
* `tests/el_reader.rs::a_listed_boundary_is_stepped_over_and_switched_by_its_wires` (new): boundaries as list
  items, sealed by two idle polls, then switched by first readiness, failure and healing against a fresh mount.
* `tests/el_fuzz.rs` is taken from `lane/reactivity/el-surface` at `ae1221bb98`, the version written against
  this base: 8 pass, and the 2 `#[ignore]`d regressions it documents are unchanged.

**Mutant proof.**

| change | failing tests |
|---|---|
| `mutant_el_eager_port` (new: every port is a constant, its value evaluated into its seal and at every update) | `a_wired_map_runs_only_when_its_signal_moves` (12 closure runs on the first re-description, where 0 pass), and `the_fast_check_agrees_with_the_seal` (the opposite field-value verdict) |
| `mutant_el_bound_wire` (existing) | now also `a_listed_boundary_is_stepped_over_and_switched_by_its_wires`: a stepped-over boundary is switched only by its wires |
| hand check, not kept: no recipe sync on an installed wire | both capture tests |
| hand check, not kept: an uninstalled port kept | `a_map_over_a_signal_behind_a_vec_shows_on_redescription`, both capture tests, `el_fuzz`'s differential loop |

**Design note.** A non-field `Signal<String>` text port evaluates by borrowing (no allocation). A non-field map
port runs its closure at each check of its seal, as the eager plug did at each description. A field port's
closure runs only on its wire.

**Still open from this item.** The boundary's first-re-description seal allocations (23 vs 12, once per
mounted boundary). Re-description of the El boundary is 1.63–1.70x typed and its idle turn 4.68x (817
instructions of typed; El describes and checks about ten nodes). The card predicted ≤ 1.3x. The two slow
debug-only `pages` transition tests were not completed in the time available; they are unrelated to this
branch's changed files and pass in release.
