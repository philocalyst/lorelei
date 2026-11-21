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

`to_json` retains the full live snapshot. `stable_json` serializes the description contract in canonical order
for the golden and the replay description digest. `from_json` reads both forms and supplies defaults for
omitted diagnostics.

The description contract covers app identity and geometry, host remount epoch, node and operand IDs, kinds,
source sites, properties, rectangles, text, logical store and list relations, registered state, pointer
targets and accessibility. The remount epoch begins at zero for each new host and advances when the host
replaces its world. It serves as an identity input. `lists[].shape` describes row geometry.

`schema::VOLATILE_SNAPSHOT_FIELDS` defines the live diagnostic fields omitted from comparisons.

- `app.world`, `app.revision`, `app.input_revision` and `app.shape` track the live instance and its revisions.
- `counters`, `damage` and `ax.projected` describe the work a turn performed, including its timings.
- Store capacity, seen rows, move history and retained storage totals describe materialization. Column page,
  record and byte counts, list capacity and seen rows, and word reader counts describe the same runtime work.
- `col` and `rec` on node and pointer keys name current materialization slots. Logical paths, rows and
  positions stay in the comparison. The live snapshot publishes no raw addresses.

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
on a connection must be `hello` with the supported schema. `methods` may be queried before negotiation. Up to
eight same-user connections share the same world and their commands execute serially. An existing path is
never removed at bind. Remove a known stale socket before reusing its address. Methods: `hello`, `methods`, `tree`, `node`, `find`, `pick`, `deps`, `why`, `counters`,
`state`, `set`, `pointer`, `down`, `up`, `click`, `key`, `text`, `wheel`, `tick`, `resize`, `settle`, `wait_for_idle`, `reload`,
`shot`, `diff`, `record_start`, `record_stop`, `replay`, `shutdown`.

The server is pumped from the application's own thread (`Server::pump`) or run blocking for a headless app
(`Server::serve`). A world stays on its own thread. Network I/O is nonblocking and bounded per pump.
Commands such as screenshots still execute synchronously on the application thread. `wait_for_idle`
paints at the current clock and reports the frame and pending animation. It reports only work owned
by the headless session.

## Record and replay

Start recording on a fresh app before injecting turns. `record_stop` returns the complete versioned log.
Use that object as the `log` parameter to `replay`. Replay compares a fresh app with the same startup
configuration, even if the app that recorded the log has since been resized.

A `Command` injects pointer, button, key, text, wheel, tick, resize, settle or typed set. Replay compares the
description digest, pixel digest and refusal code after each command. The log retains damage, revision and
refusal text for diagnostics. Log schema 2 uses the description contract above. Log schema 1 remains readable
and compares the historical digest, which includes core revision values. Replay a version 1 log on its original
core to check that recording, or create a new version 2 log to carry comparisons across core refactors.

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
