# Independent inspector review, round 2

The submitted branch was clean at `fbbd5918a4`. Wave 3 at `884e8ca38b` merged without conflicts. The replay checkout preserved the submission while the review branch took fixes. All runs used the pinned nightly on headless macOS aarch64.

| Follow-up | Submitted evidence | Review finding |
|---|---|---|
| Leaf locations and IDs | Sibling text and paint sites, a remount, repeated descriptions and slot recycling | Verified for the element layer. The golden adds one generation, 21 kind IDs and 52 node and operand IDs. It removes and changes zero existing fields. Separate processes computed identical IDs, including after reclaiming a killed app's socket. The old schema-v1 reader used zero when a field was absent. It now derives those IDs from the old snapshot. Sites in manual sink and span records still identify their row or list. |
| Live overlay | Five headless gates for closed pixels, Rows and Table, input selection, jump and damage | Replayed with the original paint mutant. `--overlay` mounts the panel next to a real comparison scene in the headless app. Native window presentation is still pending. |
| Printed MCP claims | Official Rust SDK, four hardening tests, and two transport tests | Confirmed the real SDK import. The submission exercised all tools on stdio and only `tree` on HTTP. Both now exercise all tools and resources and assert a successful record and replay. The submitted replay tool schema describes a nested command which the app never emits. Fixed the schema to describe the real flat command variants and checked it with a metaschema and a live log. |
| Off-turn work and frame fuzzing | Slow stdout reader and 400 parser inputs | The submitted stdio and HTTP writers still got serialized bytes from their request loops. The inspector also encoded its screenshot on the app thread. Owned replies now cross a bounded queue to the socket encoder and MCP response writers. The app still owns the render and the diff operation. Verified 10,000 fragmented frames on a Unix stream, the original parser corpus and the slow-reader gate. |
| Stale socket recovery | Bind and drop a listener | The original gate failed once in a concurrent run. The recovered inode is rechecked before unlink, and an additional regression kills and reaps a real application before reconnecting. A live listener and regular file are preserved. |
| Hot patch | Fresh rustc-built dylib changes a text leaf in the same process | Real code replacement at one boundary. The submission used a literal token and a view with no application state, then started another process without restoring fields. The review builds with Cargo, installs two different artifacts from one repeatedly edited file, verifies the same live World and a preserved non-default field, then rejects an actual field-type change and runs the managed restart. |

## Focused gates

The submitted four-crate suite produced 67 passed, 1 failed and 1 ignored. The failure was stale socket recovery. The same suite on the review branch with `bough-dev/hot-view` produced 74 passed and 1 ignored before the final schema and SDK corrections. The final schema and SDK targets added six passing cases on the latest code.

The original site, overlay and no-re-description mutants each reached the intended failing assertion. The review's no-re-description mutant still fails at the visible new literal, the inline socket encoder mutant fails the worker thread identity assertion, and the inline MCP writer mutant fails the slow-reader test after the writer has already emitted 1,644 bytes. The corresponding ordinary gates pass.

## Hot patch receipt

`cargo test --offline --locked -j 8 -p bough-dev --features hot-view --test hotpatch -- --nocapture` built four fresh Cargo dylibs from a single edited source file. The view switched from `Demo A` to `Demo B` to `Demo C`. The first successful patch changed the pixels and kept PID 6157, World 1 and level 27. A change from `i32` to `u32` produced a different state-declaration tag despite unchanged size and alignment. Admission refused it and kept `Demo C`. The managed fallback started PID 7558 and restored level 27, one of one registered fields. The restarted fixture loads its original built-in view. The test does not build a new host with a migrated state schema.

Patch loading is opt-in through `hot-view` and a private `--patch-dir`. The immutable tag describes the demo's declared type and layout. It checks compatibility of trusted developer-built code. The dynamic loader can run initializers before reading the tag. The watcher still uses its regular rebuild path, and general Subsecond-style view replacement is separate work.

The hot-call microprobe executed 100,000 calls in each of three process runs. The median opt-out count was 5,804,048 instructions, against 6,104,047 with the boundary enabled. The enabled call added about 3 instructions per iteration in this fixture. The release opt-out probe and a reference with the same linked dependencies and the conditional boundary removed both had 212,744 bytes of `__text`, SHA-256 `d91fce1674fd1b582c5827f89be1b946a3246c7b091751ce27d5436af233d02d`. An unmatched reference that omitted the optional dependency entirely produced 212,680 bytes and a different hash. Keep the dependency graph fixed when interpreting this probe.

## GPUI test mapping

| Reference assertion | Equivalent exercised here | Remaining authority |
|---|---|---|
| `multi_slot_inspector` starts with the old caption, records one source operation, prepares a frame, updates both slots and replaces the caption | The port starts with the second row hot, transfers the word to the first, checks both colors, exactly one command, nonzero damage, and exact before and after caption strings | One command is the local admission analogue. The host needs prepared and acknowledged presentation tokens for the pre-ACK assertion. |
| `multi_slot_inspector` compares normalized exact/coarse nodes and pixels | The captured operands and final pixels match the app pointer path. The retained snapshot preserves its old operands. | Failed or abandoned presentation-candidate checks need host acknowledgement. |
| `direct_name_and_description_share_one_read_and_promotion_without_visual_work` | The AX name and node ID remain stable across an unrelated paint edit. A repeated write returns a refusal and changes zero commands and revisions. | A distinct semantic name/description source and semantic-only publish counters are missing. |
| `metadata_extends_pending_complete_sheet_but_never_relabels_an_older_failed_candidate` | Refusal leaves the old name, bounds and revision unchanged. A retained snapshot preserves its values. | Prepared and rejected candidate epochs, deferred relabeling and visual acknowledgement. |
| `surface_loss_and_failed_preparation_keep_names_frozen_until_the_right_ack` | No equivalent gate on the headless host. | Surface loss, allocation failure and presentation acknowledgement. |
| `typed_contributions_preserve_inheritance_and_explicit_name_absence` and `cleared_description_inherits_the_latest_source_without_unmasking_early` | No typed semantic contribution API in this inspector. | Explicit declarations, inheritance, clearing and restoration. |
| `contained_direct_sources_route_semantic_ports_without_parent_or_sibling_redescriptions` | The two-slot test verifies unchanged retables for its paint source. | Per-owner semantic port routing and no neighbouring re-description. |
| `list_position_count_and_key_are_frozen_with_p_not_read_from_logical_s` | The new test checks row indices and count against the exposed grid parent. Scrolling advances the indices while a retained snapshot keeps the old rows. | Stable relation keys across reorder and staged count changes with presentation ACK. |
| `nested_realized_rows_keep_their_immediate_frozen_list_parent` | Immediate parent and row count checked for one live grid. | Nested realized lists and frozen ancestor assignments. |
| `real_apps_have_explicit_names_separate_from_values_and_play_labels` | The live Rows projection publishes the row label. | Explicit editable name, description and value declarations. |
| `published_semantic_reads_allocate_nothing_and_use_the_same_static_inbox_owner` | The existing allocation probe checks repeated idle inspector refreshes. | Native semantic query allocation and inbox ownership. |
| `a_panicking_semantic_projector_quarantines_the_same_owner_and_focus` | No equivalent semantic projector API. | Poisoned owner and retained frame policy. |
| `query_dimensions_are_conjunctive_and_order_independent` | Live RPC tests combine kind, text and store in both orders and check contradictory filters. Out-of-range stores and incompatible paint/AX filters now return errors. | A combined explicit semantic name, description and text query awaits the associated semantic fields. |

## Outside the developer-loop crates

- `bough-lay/Cargo.toml` introduces the opt-in site feature. `bough-lay/src/el.rs` gates the `LBox` location and `track_caller` on that feature. `div` and `block` still delegate through the original constructors. Location survives the layout-only builders and follows the paint call.
- `bough-x/Cargo.toml` forwards the feature and records its site-loss mutant. `bough-x/src/el.rs` supplies leaf metadata for element boxes, text and scroll paint, including the border and translation builders. `bough-x/src/sink.rs` adds the default site event. `bough-x/src/reflect.rs` resets the event after each emitted leaf and uses the existing row/list location when the authoring layer has no site.
- `Cargo.lock` records the SDK test dependency graph and `bough-dev`'s use of the existing libc version. No AX or comparison app implementation was changed outside the submitted and Wave 3 commits.
- The three final `dist` examples match `884e8ca38b` byte-for-byte in executable text. `sheet_x` is 449,960 bytes, `events_x` is 304,180 bytes, and `anim_x` is 302,760 bytes. The unmerged submission has smaller binaries with different hashes because it predates the newer integration's core work.

## Follow-up gates

1. Add native host publication epochs spanning inspector captions and AX. Begin with the GPUI two-slot pre-ACK case, then exercise rejection, surface loss and an older acknowledgement. Gate the published name and pixels together.
2. Add explicit AX declarations for name, description and value, then test inheritance, semantic-only publication, nested realized list parents and owner quarantine. Extend selector tests to those fields.
3. In `bough-dev`, route watched patch roots through `patch_or_restart` and build the replacement host before stopping the old one. Gate a migrated state schema with two retained fields and an explicit refused field. Extend the text-specific demo to a general view boundary only after proving its ABI.
4. Test Linux SDK and stale socket behaviour. Give long-running encoding and diff jobs an aggregate byte budget and confirm normal turns survive a saturated reply queue. Validate canonical source paths under path remapping and identities across keyed relation changes.
