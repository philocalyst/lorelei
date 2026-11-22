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
