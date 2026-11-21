# Developer-loop follow-ups: report

Lane `developer-loop`, branch `lane/developer-loop/inspector-followups`, base `a8bfa2e12b` plus the independent
review `docs/lanes/independent-review/2026-11-21-inspector-replay.md`. Card:
`cards/2026-11-21-followups.md` (frozen before building; one recorded amendment about the id generation).
Host: Apple M-series, macOS, headless. Toolchain nightly-2026-10-17; `RUSTC_WRAPPER=` and a private
`CARGO_TARGET_DIR`.

**State: the four tasks are built where their gates say so; the boundaries are stated below rather than
implied.** TASK 1 and TASK 2 are gated. TASK 3 is gated except that PNG/base64 encoding still runs on the
application thread (serialization and the client write are off it). TASK 4 delivers the smallest real
in-process patch (one hot-call boundary), not a Subsecond-style general patcher.

## TASK 1 — per-leaf source locations and stable ids

What was built.

- `crates/bough-lay` (feature `inspect-metadata`): `LBox` carries a `#[track_caller]` site; `column`, `row`,
  `div`, `block` and `.bg` capture it; `bg_at` lets a façade method pass the author's site.
- `crates/bough-x` (feature `inspect-metadata`): `Text` carries the `text(..)` site, `Scroll` the `.scroll(..)`
  site; a new `Leaf::at` event is emitted in `Row::visit`; `LearnLeaf` and `DLeaf` record it per leaf. Nothing
  is emitted in a build without the feature.
- `crates/bough-inspect`: `Node.id`, `Node.kind_id`, `Prop.id`, `AppInfo.generation`. The id is FNV-1a over the
  leaf's call site, the store path, `row`/`pos` (logical slots, never the paint index) and the host's world
  generation; it is written as 16 hex digits and read back. `Col.ty` and `Prop.expr` remain display-only, with a
  stable id beside them.

Gates and commands.

```
cargo test -p bough-inspect-app --test leaf_sites
  sibling_leaves_report_their_own_written_lines ... ok        (1 passed)
cargo test -p bough-inspect-app --test leaf_sites --features bough-x/mutant_same_site
  FAILED: the first text leaf names its own call site
cargo test -p bough-inspect-app --test stable_ids
  5 passed: two descriptions agree; relearning agrees; a scroll that recycles a slot keeps the row's id and the
  row now in that slot has another; ids are unique; a remount changes the generation and the ids
```

The leaf test asserts `file:line:col` against *its own source text* (`include_str!`), including a `.bg(..)` line
(a method call's caller column is the method name). The fill that used to share the list's location now names the
line that wrote it.

Off-feature cost. The three `bough-x` examples were built at the branch point and on this branch,
`--profile dist`, and their executable `__text` compared:

```
                      a8bfa2e12b                                   branch
sheet_x   sha256 c4ac5b25318ec34ccc41892a60da74fb9d304fff…   c4ac5b25318ec34ccc41892a60da74fb9d304fff…
          __text 441,192 bytes                                441,192 bytes
events_x  sha256 0398221af83a8d540a81f73c2074b48f8a3d50d4…   0398221af83a8d540a81f73c2074b48f8a3d50d4…
          __text 297,268 bytes                                297,268 bytes
anim_x    sha256 451f26e83c8a21ff85afe7ede9ccb87acd169f5…   451f26e83c8a21ff85afe7ede9ccb87acd169f5…
          __text 297,192 bytes                                297,192 bytes
```

(sha256 of the hex words of `otool -s __TEXT __text`, excluding the per-line address column.)

Amendment. The card first said the id's generation was the store's creation generation. A process-global
counter makes the committed golden and record→replay order-dependent. The generation is the host's world
generation (`Program::generation`: 0 for a fresh app, +1 on a remount), which keeps the word "generation" in the
id, makes two identical apps describe the same ids for replay, and still separates a remounted world.

## TASK 2 — the inspector beside a live app

`WithOverlay<V>` (`crates/bough-inspect-app/src/overlay_host.rs`) is a `Program`: it mounts the element-layer
`Inspector` as a side panel, publishes the session's snapshot only while the panel is open (`Program::overlay`
and `Program::publish` are additive defaults; `Session::shot` asks for the snapshot only then), paints the
selection's rect and the last turn's damage on the app's own pixels, and translates pointer input over the panel
into the panel's coordinates so the overlay's own handlers run. Keys are real injected input: `` ` `` toggles,
Esc closes, Up/Down move, Left selects the parent, Enter leaves `Inspector::jump` (`file:line:col`). The demo
binary takes `--overlay`.

```
cargo test -p bough-inspect-app --test overlay_live           # 5 passed
  closed pixels are the app's own (byte-identical to Lab::shot)
  a pointer click on a tree line selects that node (through the overlay's click handler)
  a key moves the selection and Enter leaves a source location
  a state write flashes the damage of that turn (0xffe06020 on the app's pixels, 0xff5a9bff outline)
  Table mounts the same panel
cargo test -p bough-inspect-app --test overlay_live --features mutant_overlay_paint
  FAILED: a closed panel changed 385584 pixels
```

```
bough-inspect-app --scenario rows --rows 40              (no inspector feature)  digest 42da5b8af87798c0
bough-inspect-app --scenario rows --rows 40              (inspector, closed)      digest 42da5b8af87798c0
bough-inspect-app --scenario rows --rows 40 --overlay    (open)                   digest 888e569ea212a5c7
```

Ported behaviours (mapping named in the tests).

- `reactivity/crates/gpui/tests/multi_slot_inspector.rs`
  (`one_source_two_slot_transfer_cannot_publish_ahead_of_inspector_caption`) →
  `ported_inspector::one_source_two_slots_and_the_inspector_caption`. One block word feeds two row slots; the
  exact write is `Session::set` and the coarse write is the app's own pointer path; both records and both frames
  agree; an older `Snapshot` is a value and is not relabelled. There is no presentation token or failed-candidate
  state in the typed core, so that half is not claimed.
- `reactivity/crates/gpui/tests/semantic_metadata.rs` (the name/value publication, refusal and identity cases) →
  `ported_inspector::semantic_publication_is_not_relabelled_by_an_unrelated_turn`. The accessibility cell's name
  and the node's id survive an unrelated visual change and a refused write; a repeated identical write is a
  no-op. The "deferred label across a failed canvas" half has no analogue and is not claimed.

## TASK 3 — MCP and protocol hardening

- Every nested input schema is concrete: `set_field.value` is a `oneOf` over int/float/bool/color/text objects,
  `replay.log` names its members and each step's, `node.path` is an integer array (max 6). A test walks all tool
  schemas and rejects a nested object without properties or an array without items. The server advertises no
  `outputSchema` (results are JSON text content), so there is no bare output object; that is stated here rather
  than added.
- A standard MCP client (`rmcp` 3.10, client feature only) drives the server: `initialize`, `tools/list`, every
  tool call, `resources/list` and every `resources/read` over stdio against the real `Rows` app; a second
  pristine `Table` app records and replays. The same client uses the HTTP transport
  (`StreamableHttpClientTransport`) against the HTTP server.
  `cargo test -p bough-mcp --test standard_client` → 2 passed.
- `serve_stream_owned` serializes and writes replies on a thread with a 64-reply queue. The slow-reader gate
  uses a writer that takes 10 ms per byte for its first 100 bytes and asserts the third request is served while
  fewer than 100 bytes were written. HTTP success bodies are written on detached threads, bounded at 64.
- Fuzz: 400 deterministic random JSON-ish messages, truncated requests and a 65 MiB line; no panic, no hang,
  RSS growth bounded by the test's check.
- `Server::bind` reclaims a socket file of this user with no listener and refuses a live one or another user's.
- Added dev-dependencies `rmcp`, `tokio` and `serde_json` to `crates/bough-mcp`; `Cargo.lock` grew by ~829 lines.
  Normal builds do not link any of them.

```
cargo test -p bough-mcp --offline
  binary 2, mcp 3, hardening 4, standard_client 2 ... all ok
```

Boundary: PNG and base64 encoding of a screenshot still run on the application thread inside `rpc::shot`; the
serialization and the write do not, and the socket's writes were already non-blocking and bounded. Moving the
encoder needs a deferred reply (the reply currently carries its bytes); recorded as the next card item.

## TASK 4 — hot patching

What is real.

- `bough_dev::hotview`: the demo view's variable text is one `extern "C"` function pointer. `install` dlopens a
  `cdylib` and reads its `hot_state_token` **before running any of its code**; a mismatch is refused with both
  tokens named. `bough_dev::patch` builds the `cdylib` with `rustc`, applies it through the inspector's new
  `hotpatch` method (`Program::hot_patch`, an additive default), and reports `Outcome::needs_restart`.
- `crates/bough-dev/src/bin/hotdemo.rs` is a real element-layer view serving the inspector socket; its
  `view()` reads the boundary on every description and `hot_patch` re-describes in the same `World`.
- The CLI has `bough-dev hotpatch --socket S --source FILE`.

Gates and commands.

```
cargo test -p bough-dev --test hotpatch
  a_view_patch_changes_the_text_in_the_same_process_and_world ... ok
    "Demo A" -> "Demo B", same PID, same world id, same process
    a patch compiled against another state token: Refused, text still "Demo B", world unchanged, needs_restart
    the fallback really starts another process
cargo test -p bough-dev --features mutant_no_redescribe --test hotpatch
  FAILED: the running view shows the new literal (left "Demo A", right "Demo B")
```

By hand, against the built demo:

```
bough-dev hotpatch --socket …/h.sock --source …/v.rs
  patched (revision 2, c514e178cda4f7ce)
bough-dev inspect find --kind label --socket …/h.sock
  #2 label   Demo CLI  crates/bough-dev/src/bin/hotdemo.rs:24
```

Boundaries. No Subsecond dependency, no jump table, no executable-memory write. Only this one boundary is
patched; every other Rust change is a state-preserving restart. The state token is a constant the demo and the
patch source agree on, not a digest of the real state type. The watcher does not yet route a changed source to
`hotpatch` by itself (the library and CLI step is real; the classification is the next card). macOS/aarch64 only.

## Workspace replay

`nice -n 15 cargo test --workspace --release --no-fail-fast -j 8` — totals and every failure are in
`notes/reports/flashinsp-final.md` (outside the repository), which also lists every edit outside the four
developer-loop crates.
