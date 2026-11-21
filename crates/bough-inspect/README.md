# bough-inspect

The inspector, agent/test socket and schema over the typed core (`bough-x`) reflection. It is a separate artefact:
a binary that does not link it contains none of it, and `bough-x` has no conditional compilation on its behalf.

## Schema (version 1)

One `Snapshot` (`src/schema.rs`) describes one world at one moment:

- `app`: name, view type, size, scale, revision, input revision, store shape;
- `stores`: the root, scopes and lists — space clip and origin, the scroll word, capacity and exposed window,
  columns (paint kind, Rust type, stage, the block words they read, pages, records, bytes), moved rows;
- `lists`: row count, row height, location, the row shape's rects, exposed window, extent, scroll word;
- `nodes`: every live record in paint order: entity key (`path`, `row`, `pos`, plus its materialisation `col`,
  `rec`), kind, Rust type, **source location** (`file:line:col` from `#[track_caller]` at the view's row,
  scope or list call), rect / screen rect / clip, visible, moving, paint offset, handlers, its typed operands
  with values and the staged expression each came from (`expr` is the compiler's type name: the lenses and
  functions), the block words it reads, and its content text;
- `damage`: the last commit's marked tiles (bit rows and, when few, rects) and the sweeps still to come;
- `counters`: delta evaluations, retables, retained records/pages/bytes, commands, frames, last command's
  settle/paint microseconds, and the block words the last command changed;
- `words`: block words with their registered name, value, kind, and how many columns read them;
- `fields`: the application's registered typed state, with values and whether each is writable;
- `pointers`: the pointer position, pick, press and focus, each named as a node of the same snapshot;
- `ax`: the accessibility projection (`bough-ax`) with roles, names, bounds, states, actions and location.

`to_json` is canonical (insertion order, no whitespace); `stable_json` drops the process-specific world identity
and the timings, and is what the golden (`crates/bough-inspect-app/tests/golden/rows.json`) holds. `from_json`
reads a snapshot back, so a description can be recorded, stored and analysed offline.

## Session and mutation

`Program` is what an application exposes: its view and world, its settle policy, its frame, its size and resize,
its input path, a clock, a reload, a way to build a fresh instance for replay, and its registered `FieldDef`s.
A `FieldDef` has a typed `get`/`set` over the view — the application's own lens or `Signal`. `Session::set`
checks the field exists and is writable, checks the value's kind, calls the setter, commits through the
application's settle, and reads the value back; an unknown, read-only, wrong-kind, out-of-range or ineffective
write is refused with a stable code and a reason. **A record's operand is read-only**: it is a staged expression
of state, and the refusal says so.

## Socket

A Unix domain socket, length-framed JSON-RPC 2.0 (4-byte big-endian length, then the message). The first request
on a connection must be `hello` (and a client naming another schema is refused); one connection at a time (a
second is told so). Methods: `hello`, `methods`, `tree`, `node`, `find`, `pick`, `deps`, `why`, `counters`,
`state`, `set`, `pointer`, `down`, `up`, `click`, `key`, `text`, `wheel`, `tick`, `resize`, `settle`, `reload`,
`shot`, `diff`, `record_start`, `record_stop`, `replay`, `shutdown`.

The server is pumped from the application's own thread (`Server::pump`) or run blocking for a headless app
(`Server::serve`); a world's stores are pointers into its own allocations and are never moved across threads.

## Record and replay

A `Command` is a turn: pointer, button, key, text, wheel, tick, resize, settle, typed set. `record` runs a script
and keeps each command's description digest and pixel digest; `replay` runs it on a fresh application and compares
every description, every pixel and every refusal. The app must be deterministic (the benchmark apps are).

## Pixels

`png` writes and reads 8-bit RGBA PNG with a stored-block zlib stream, so a golden is byte-exact without a
compressor dependency. `diff` reports differing pixels, the bounding box and the maximum channel delta. `Frame`
words are `0xAARRGGBB`.

## Overlay

`ui::Inspector` is a `bough_x::el` view: the tree (with a damage marker and a selection highlight), details
(source location, rects in both spaces, text, handlers, operands with stage/expression/reads, the store chain),
counters and timings. A click selects; a source location is left in `jump` for the host's editor integration.
No style editors (the owner's decision).

## Limits, stated

- **Twelve bytes of text per record.** The core's `reflect::Val::Text` holds a `Txt` (three words). Element text
  and span text now implement `OPS`/`val` (this lane's small core additions), so `find --text` and the
  accessibility names work for text up to twelve bytes; a longer label is drawn in full but read back truncated.
- **Source locations are per row/scope/list call site**, as the core's `#[track_caller]` records them. A record's
  position within the row (`key.pos`) distinguishes its elements; there is no per-leaf location without a core
  change.
- **Item identity in a list is positional.** `Key` names `(path, row, pos)`; the core does not yet carry an item
  key and generation, so a row that is removed from the middle of a relation is indistinguishable from a shift.
  A row that leaves the exposed window is kept as the same entity (it is still the same row).
- **Element lists can be learned but not described on demand**: `Reflect::rows` supports hand-written lists; the
  element layer's `each` is learned (`Learn::rows`) but `Reflect::rows` has no layout for it yet.
