# Independent inspector review

Reviewed `d56ea0f5f3` and composed it with Wave 3 (`7991e7bd4e`). The author’s report is preserved verbatim. The review card gives the pinned build and performance thresholds.

## Findings

| Gate | Independent finding |
|---|---|
| G1 Schema | The round trip, pinned version and byte-stable Rows golden pass. This is one fixture and one compiler. The snapshot still includes implementation-derived type names. |
| G2 Tree | The exposed Rows records, operands, dependency words, layouts, accessibility and stores are queryable. The 1,000-row original smoke test reported 77 paint records and 117 AX nodes. The original report cites 41 AX nodes. `Row 00002` resolves to `rows.rs:74:15`, the list declaration. Its text leaf is on line 79. Rows within that declaration share the same location. |
| G3 Socket | Both comparison apps pass their original socket probes. The review adds concurrent peers, bounded nonblocking I/O, schema negotiation, path ownership and shutdown cleanup. Commands themselves still execute on the app thread. |
| G4 Replay | The original in-process Rows and Table scripted probes pass. The original wire recording omitted the schema and pixel digests that `replay` required. The fixed wire log replays on a second Rows process after the recording process resizes, with exact per-command pixel digests and recorded refusals. |
| G5 Mutation | Registered fields pass typed admission and return actionable errors. Fixed integer narrowing, readback after numeric conversion and no-op writes. Record operands remain read-only. |
| G6 MCP | Rows and Table answered all 26 advertised tools over stdio and HTTP. The JSON Schema metaschema accepted every tool input schema. The review corrected tool errors, HTTP request limits, loopback binding, and reconnect after restart. `wait_for_idle` now paints a headless frame and reports pending motion. |
| G7 UI | Terminal commands exist. The element-layer inspector renders and selects in a test harness. The app binary does not mount it as an overlay. The tests adapt a subset of the GPUI behaviours. The semantic publication and multi-slot tests have no direct ports. |
| G8 Off-feature cost | Original Rows hover was 4,211 instructions on and off in all three repeats. The composed `sheet_x`, `events_x` and `anim_x` examples have byte-identical executable `__text` sections to Wave 3. Raw benchmark counts varied in mount and some sheet percentiles across both builds, so the literal equality gate did not pass for every cell. |

`sheet_x`, `events_x` and `anim_x` used `ROWS=10000`, default repetitions, nightly-2026-10-17, and three per-thread instruction runs per build. Their `__text` sizes were 413,716, 343,652 and 321,876 bytes respectively on both builds. The corresponding unstripped Mach-O files grew by 48, 48 and 32 bytes. The deltas all become zero after `strip -S`. No general performance claim follows from three example binaries.

The dev watcher was exercised with a real Cargo app wrapper and the Rows app. A file classified as an asset exercised the reload callback and preserved the process and field values. This adapter has no asset reader. A Rust edit changed the startup viewport from 240×180 to 240×200, Cargo rebuilt the wrapper, the PID changed, and `scroll=64` and `hot=9` survived. The same HTTP MCP endpoint read those values on its first post-restart call. The patch mechanism present in the pinned GPUI-CE CLI is Subsecond function replacement. The current bough code is an independent implementation of the CLI architecture, with upstream provenance recorded and no Subsecond code path.

## Test receipts

Run the workspace gate with `nice -n 15 cargo test --workspace --release --no-fail-fast -j 8`. The intermediate review at `5f5a2bb86d` produced 254 test results with 528 passed, 5 failed and 8 ignored. Four failures were the existing missing-font tests. The fifth was the stale Unix socket left by the adapter’s normal exit. `aeca9a8415` corrected the adapter lifecycle. The final restart gate keeps an MCP session open across the shutdown and new bind, then compares the state returned by its first post-restart call.

The full rerun at `fabaeaed3a` reported 254 targets, 535 passed, 4 failed and 8 ignored. Both inspector lifecycle tests passed. The four failures match the baseline: `find_counts_the_sample`, `code_short_file`, `code_every_ring` and `faces_and_display_text_draw_their_own_font`. The glyph memo test also passed on the merged branch.

Run the four inspector crates with `cargo test --locked -j 8 --no-fail-fast -p bough-inspect-app -p bough-inspect -p bough-mcp -p bough-dev`. The original command passed 39 tests with 1 ignored. The final command passed 48 tests with 1 ignored. The standalone release command passed 47 before adding the Search regression, which then passed as a targeted release test. All six original mutants exited 101 at the intended assertions.

Run the touched original crates with `cargo test -j 8 --no-fail-fast -p bough-x -p bough-span -p bough-ax -p bough-guibench-x`, once with `--release` and once without it. Each passed 110 tests and ignored 1. The single failure in both profiles was `bough-x::text::the_glyph_memo_is_collected_and_frames_stay_exact`. Its memo reached 90,112 bytes against a 24,576-byte budget. Wave 3 changes the glyph storage and adds a diagnostic to this test. The budget assertion stays in place.

## Outside the inspector crates

| File | Review decision |
|---|---|
| `bough-x/src/el.rs` | Keep the element text and number operands behind `inspect-metadata`. A text operand must not present its interned address as raw reflected words. |
| `bough-x/src/kinds.rs` | Keep UTF-8 truncation for the optional 12-byte text preview. Revert unrelated formatting. |
| `bough-x/src/reflect.rs` | Keep the optional cold `Learn::rows` support. Use saturating extent arithmetic. |
| `bough-span/src/kinds.rs` | Feature-gate the span-text operand projection and revert formatting of unrelated raster paths. |
| `bough-x/scenes/sheet.rs` | Restore the one-word wrapping total used in the Wave 3 release fixture. Give debug builds those same overflow semantics. Saturating only the initial total would give inconsistent later edits. |
| `bough-x/tests/sheet_el.rs` | Use a 32 MiB worker stack in debug for the existing long sheet scripts and retained-space probe. All workloads and assertions stay the same. |
| `bough-x/tests/card1_soundness_and_sparse.rs` | Use the larger debug stack for the retained ledger test. The release test invokes the same body directly. |
| `bough-guibench-x/src/scene.rs` | Scope the adapter scale fix to `inspect-metadata`. The overlay scale test compares the physical and logical coordinate spaces. |
| `Cargo.toml`, affected manifests and `Cargo.lock` | Keep the four workspace members and opt-in metadata features. The lock also records a dependency on the existing `libc` version for effective-UID admission. |

## Follow-up tasks for a smaller model

1. **Leaf sites and cold metadata.** In `bough-x/src/{el,reflect,sink}.rs`, extend `Learn` with an optional cold table from leaf position to source site. Resolve workspace-relative filenames and expose a stable kind/expression ID beside any display-only Rust type strings in `bough-inspect/src/{schema,schema_read}.rs`. Gate with sibling leaves declared on different lines, equal release `__text` in the off-feature operation examples, and two path-remapped builds of the same golden. Cover full UTF-8 reflected text, a `TextOf` shaped run and layout-backed offscreen `each` rows.
2. **Published frame and mutation contract.** In `session.rs`, `rpc.rs` and the app adapters, extend the headless frame barrier to report host publication and wait for an epoch with a deadline. Keep the Rows, Spinner and Search checks when introducing the native host. Define a validate/apply contract for application setters before claiming atomic refusal for arbitrary `Program`s. Add a state snapshot format that covers model-owned collection state and uses explicit versioned restoration.
3. **Native inspector and GPUI parity.** Mount `ui.rs` beside a real app via the author layer. Drive actual selection, geometry highlight, jump activation, damage flashes and edit refusals by input in a headless harness. Port `multi_slot_inspector.rs`’s one-source/two-slot certificate and the `semantic_metadata.rs` publication/failure/identity cases into `bough-inspect-app/tests`. Map the authority to typed-core publication and document any prerequisites for cases that cannot be translated. The current source-jump test writes the jump directly. Drive that through a click and assert the jump request.
4. **MCP client interoperability.** In `bough-mcp/src/lib.rs` and `bough-mcp/tests`, test through a standard MCP client. Add whole-schema validation and meaningful schemas for nested selectors, set values and logs. Return structured results and cover externally encoded PNG fixtures. Exercise both Rows and Table through both binaries, preserving session behaviour after reconnect. Use `fakeland-mcp`’s typed parameters, scoped screenshots and state-based wait as references. Keep Lorelei’s source, staged dependency and repaint data.
5. **Bounded UI-thread work.** Measure `Server::pump` under simultaneous input and screenshot/replay requests. Move serialization and PNG work to a bounded worker using owned snapshots and pixels. Keep the world on its owning thread. Add an absolute deadline and total session memory/response budget, and fuzz parsing, PNG, replay and arbitrary JSON-RPC shapes. Verify same-user admission and cleanup on macOS and Linux, and define managed stale-socket recovery after a killed child. A connected slow reader must leave the host’s ordinary turn admission responsive.
6. **GPUI-CE function patching.** Start from the pinned CLI’s `compiler.rs`, `native.rs`, `server.rs` and hot-patch fixture. Integrate the compiler capture and explicit hot-call boundary for a narrow `view()` edit in `bough-dev`. Prove the PID, world identity and app-owned state survive successful patching while new view output appears. Refuse a schema-changing patch, then prove fallback restarts and restores fields. Gate with a real asset reader and prove that modifying the file changes rendered output with no PID change.
7. **Release reproducibility.** Keep a stable PMU harness for A/B same-binary replicates and report all instruction deltas. Match Mach-O paths or compare stripped artifacts. Run normal and negative controls on the same frozen build. Keep missing-font failures visible and obtain the documented fonts before expecting a green workspace suite.
