# Inspector, agent/test socket, MCP and dev server: report (2026-11-21)

Lane `developer-loop`; card `cards/2026-11-21-inspector-mcp.md` (frozen at `bdef2ac112` before any code).
Predecessor `71ee3be0aa`; toolchain nightly-2026-10-17; `CARGO_TARGET_DIR` per session. Host: Apple M-series,
headless, macOS. Every number below is from a command in this report.

**State: GATED** for G1-G8 on this host. Not `MEASURED` in the lane's sense (no performance claim), and not
`INDEPENDENT_REPLAY` (the `independent-review` lane has not replayed it).

## What exists

| crate | what |
|---|---|
| `bough-inspect` | schema v1 over `bough-x::reflect` + `bough-ax`; snapshot builder and reader; typed state mutation; Unix-socket JSON-RPC server/client (length-framed); record/replay; PNG write/read and pixel diff; typed selectors; element-layer overlay |
| `bough-inspect-app` | the real comparison apps (`Rows`, `Table`, `Search`, `Spinner` from `comparisons/bough-guibench-x`) on a headless host, with the socket; built `--no-default-features` it is the zero-cost control |
| `bough-mcp` | MCP over stdio (newline-framed JSON-RPC) and over HTTP `POST /mcp`; 26 tools, 3 resources |
| `bough-dev` | watch (content scan), reload, rebuild + state-preserving restart, terminal client, MCP endpoint; forked from `gpui-ce/cli` @ `6774184` (Apache-2.0, `NOTICE`) |

## Gates, commands and results

### G1 schema round trip and stability

```
cargo test -p bough-inspect-app --test schema_golden
running 3 tests
test the_version_is_pinned ... ok
test the_golden_is_byte_stable ... ok
test a_description_reads_back_and_writes_the_same_bytes ... ok
test result: ok. 3 passed; 0 failed
```
Golden: `crates/bough-inspect-app/tests/golden/rows.json` (35,597 bytes; `Rows` 20 at 200x160). It holds no
absolute path (asserted in the test).

### G2 tree completeness

`cargo test -p bough-inspect-app --test inspect_core` — `rows=1000 nodes=77 stores=3 lists=1 ax=41 records=77`;
every node has `kind`, a `file.rs` location with `line > 0`.

### G3 socket end to end, two real apps

```
cargo test -p bough-inspect-app --test socket
running 3 tests
test handshake_is_enforced_and_a_second_client_is_refused ... ok
test table_over_the_socket ... ok
test rows_over_the_socket ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.64s
```
`rows_over_the_socket` covers hello, tree (3 stores: root, body scope, list of 1,000), pick (label over its row,
the row fill hears the pointer), pointer → `hot = 5`, typed `scroll` write, `find --text "Row 00042"` (1 match),
`deps --field hot`, `why` (command `set`, changed words, damage > 0), shot (valid 800x600 PNG, stable across two
paints), a screenshot by element, diff against itself (match) and against a one-pixel mutant (1 differing pixel,
max channel delta), counters, a method refusal (-32601) and an admission refusal (-32010, distinguishable).

### G4 record → replay

```
cargo test -p bough-inspect-app --test replay
test rows_replay_is_pixel_and_node_exact ... ok
test table_replay_is_pixel_and_node_exact ... ok
```
16 and 9 commands; every description digest, pixel digest and revision equal on a fresh app.

### G5 mutation

`cargo test -p bough-inspect-app --test overlay` (refusals leave the digest and revision unchanged; a write
damages its readers, no retables; every registered field reads/writes/reads back; read-only `len` refuses) and
`inspect_core::a_typed_field_write_damages_exactly_its_readers` (wrong kind, unknown field, negative offset).

### G6 MCP

```
cargo test -p bough-mcp
tests/binary.rs: 2 passed   (stdio binary, --http binary)
tests/mcp.rs:    3 passed   (shapes, tools/list, resources, image content, refusal as a tool error, stdio loop, POST /mcp, GET 405)
cargo test -p bough-inspect-app --test mcp_e2e
test mcp_drives_the_rows_app ... ok
```
The real `Rows` app is driven through MCP: tree, find by text, screenshot (2x2… window 800x600 PNG), click,
typed set, why-repaint, deps, refusal of `len` as a tool error, diff, node by index.

### G7 overlay and ported behaviours

```
cargo test -p bough-inspect-app --test overlay
running 13 tests ... test result: ok. 13 passed
cargo test -p bough-inspect-app --test alloc_zero
test idle_refresh_allocates_nothing ... ok
```
The idle gate is a counting global allocator in a test binary of its own: **0 allocations over 128 refreshes**,
with and without a selection; invalidating and refreshing a changed world allocates > 0 (not vacuous).

### G8 zero cost when off

```
cargo tree -p bough-inspect-app --no-default-features -e normal | tail -n +2 | grep -c 'bough-inspect\|bough-mcp\|bough-dev'
0
cargo build --release -p bough-inspect-app                    # inspector on
cargo build --release -p bough-inspect-app --no-default-features
ls -l: 2,240,336 bytes (on)   1,157,872 bytes (off)
size -m __text: 1,079,484 (on)   435,220 (off)
nm -C | grep -E ' bough_inspect::' | grep -vc bough_inspect_app
52 (on)   0 (off)
cargo test --release -p bough-inspect-app --test hot_path -- --nocapture
hot-path hover: median 4211 instructions (n=2000)
cargo test --release -p bough-inspect-app --test hot_path --no-default-features -- --nocapture
hot-path hover: median 4211 instructions (n=2000)
```
The hot-path probe measures the app's own hover turn (`Lab::input` + settle) and is identical to the instruction
with the inspector linked and without it. (Caveat: the off binary's main is not the same program — it paints one
frame and prints a digest; the size delta is not "the inspector's cost", only evidence that the off build links
none of it. The instruction line is the meaningful one.)

## Negative controls (each run, each failed its gate)

```
cargo test -p bough-inspect-app --test schema_golden --features bough-inspect/mutant_schema
  the_golden_is_byte_stable ... FAILED ; a_description_reads_back_and_writes_the_same_bytes ... FAILED
cargo test -p bough-inspect-app --test inspect_core --features bough-inspect/mutant_drop_site
  a_snapshot_names_the_tree ... FAILED
cargo test -p bough-inspect-app --test replay --features bough-inspect/mutant_replay
  rows_replay_is_pixel_and_node_exact ... FAILED ; table_replay_is_pixel_and_node_exact ... FAILED
cargo test -p bough-inspect-app --test inspect_core --features bough-inspect/mutant_accept
  a_typed_field_write_damages_exactly_its_readers ... FAILED
cargo test -p bough-inspect-app --test alloc_zero --features bough-inspect/mutant_inspector_cache
  idle_refresh_allocates_nothing ... FAILED
cargo test -p bough-inspect-app --test socket rows_over --features bough-inspect/mutant_png
  rows_over_the_socket ... FAILED
```

## Whole new-crate suite

```
cargo test -p bough-inspect-app -p bough-inspect -p bough-mcp -p bough-dev   (exit 0)
passed=39 failed=0 ignored=1
```

## Core crates I touched

```
cargo test -p bough-x -p bough-span -p bough-ax -p bough-guibench-x              (debug)
cargo test --release -p bough-x -p bough-span -p bough-ax -p bough-guibench-x
```
Debug: all green after two test-harness fixes (below). Release: one failure,
`bough-x --test text the_glyph_memo_is_collected_and_frames_stay_exact` ("the memo reached 90112 bytes under a
24576-byte budget") — **pre-existing**: the same failure is in `~/work/notes/baseline-tests.log` at line 2038.

## Edits outside this lane (all small, additive, listed)

- `crates/bough-x/src/reflect.rs`: `Learn::rows` — an element-layer `each` list could not be learned at all; it
  is now described exactly as a hand-written `list`. 45 lines.
- `crates/bough-x/src/el.rs`: `OPS`/`val` for `StrRec`/`NumRec` — element text and numbers were opaque to
  reflection (and therefore to the accessibility projection); this makes them readable (12-byte text).
- `crates/bough-x/src/kinds.rs`: `Txt::from_bytes` (12-byte, char-boundary cut).
- `crates/bough-span/src/kinds.rs`: `OPS`/`val` for the span `Text` kind (text + colour); `TextOf` remains
  opaque (its text lives in a `Line`, not in the record).
- `crates/bough-x/scenes/sheet.rs`: the column total saturates instead of overflowing `i32` at 100,000 rows
  (debug builds panicked; release wrapped).
- `crates/bough-x/tests/sheet_el.rs`, `tests/card1_soundness_and_sparse.rs`: the deep element-sheet bodies run
  on a 32 MiB test thread (the default 2 MiB test thread overflows in debug; the evidence baseline was a
  release build).
- `comparisons/bough-guibench-x/src/scene.rs`: `world.set_scale(geo.k)` on mount — the adapter passed the scale
  to layout but never to the block, so a scale-2 mount laid out text at scale 1.
- `Cargo.toml`: four new workspace members. `Cargo.lock`: their path entries.

## Not verified / boundaries

- **No in-process Rust code patching.** A Rust change is a state-preserving restart; the subsecond jump-table
  route of the gpui-ce CLI is not implemented and not depended on. The boundary is stated in `bough-dev/README.md`.
- **Text in a description is up to 12 bytes** (`reflect::Val::Text` is a `Txt`). Drawn text is full; searched,
  named and diffed text is truncated at twelve bytes.
- **Source locations are per row/scope/list call site** (the core's `#[track_caller]`), with `key.pos` naming the
  element in the row. No per-leaf location without a core change.
- **List identity is positional** (`path,row,pos`); the core has no item key/generation yet, so a row removed
  from the middle of a relation is indistinguishable from a shift. A row outside the exposed window is kept as
  the same entity.
- **Element lists**: learned, but `Reflect::rows` (rows on demand) has no layout for them yet; only hand-written
  `list`s support on-demand rows.
- **macOS only** (headless); no native window host was exercised. The socket server is pumped from the app's
  thread; the benchmark adapter serves blocking.
- No Miri/Loom/sanitizer run on the new crates (X28 is still open for the core too).
- The `bough-dev` `run` loop's rebuild path was exercised indirectly (state-preserving restart and reload against
  the real app); a full `cargo build`-triggered watch cycle was not run end to end in a test (the watcher and the
  restart were tested separately).

## Next card

1. Per-leaf source locations: a `Location` on a record's site (the element layer already has the call sites in
   `view`; the core's `Site` would carry one per leaf) — this is what an inspector UI's "jump" wants.
2. Item key and generation in `Rel` (the core's open decision): then selection, focus and accessibility ids name
   items, and "removed" stops being indistinguishable from "shifted".
3. `Reflect::rows` for element lists (needs layout for a row nobody materialised) and the on-demand accessibility
   rows for element views.
4. A windowed app's `pump` in its frame loop on Linux/Wayland (the `bough-host` path), so the socket is served by
   a real application, not only the headless adapter.
5. A real hot-patch experiment: measure what a dylib/jump-table patch would cost against this restart path before
   building one (prediction: a restart with state is 10-100x the patch and state-identical; kill if it is not).
