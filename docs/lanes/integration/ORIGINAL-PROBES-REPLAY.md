# Original independent-review replay — Card 2

The 16 checks in the three original regression files, plus the standalone pool check, were replayed without altering any probe byte. This is **not an unchanged-green result**. The selected tree is the Card 2 branch at `e6eb6ac3a7`, separate from Card 1 and any future integration.

The private runner is `runtime/lanes/integration/replay-harness/run_replay.py` (SHA-256 `caeb778ca3d4baa510efbc6576d3995d5c8d08c6110fb4ee9efce7deaf39e869`). Invoke from the integration checkout:

```sh
python3 runtime/lanes/integration/replay-harness/run_replay.py \
  --worktree runtime/lanes/integration/worktrees/sol-card2
```

It validates every file listed in `original-review/SHA256.json` before and in a finally block after replay, byte-copies all four Rust files into temporary harnesses, and checks copy equality. Frozen relative module paths resolve through scratch symlinks to contemporary sources. A copied workspace lockfile and root patch resolutions constrain offline Cargo resolution. Cargo uses the serialized wrapper and dedicated target; the self-contained pool file uses pinned nightly rustc. Compilation has a 300-second timeout and each individual executable check a 20-second timeout. Logs and scratch are ignored, not committed. No original source is patched, extracted, rewritten, or substituted.

Host: headless macOS aarch64, owner's M3 Pro; nightly-2026-10-17; Cargo harnesses release, standalone pool rustc test default profile. These are correctness executions, not latency measurements.

Core compilation fails with E0507: the original generic helper captures `R: Row<V>` by move in a callback now required to implement `FnMut`. The already-existing adapted test adds a `Copy` bound; this runner does not. All ten core runtime checks are consequently unexecuted. `ApiDrift` describes that whole-file admission failure, not ten proven behavioral failures.

Storage compilation rejects sending thread-bound `Line` because its marker is `!Send`. All three rows below carry that **whole-file** type-safety rejection; the alignment and extent checks are blocked by their sibling test and have no unchanged runtime result. Further diagnostics may become visible only after that blocker is removed; this replay does not remove it.

The pool source embeds the historical implementation with inert scratch/stat stubs. Its runtime assertion fails (exit 101), safely retaining captures until workers finish. `GenuineRegression` below belongs to that embedded historical code, not today's linked pool. Contemporary pool unwind, worker panic, overlap and reentry tests pass.

| Probe file | SHA-256 | Check | Unchanged outcome | Contemporary workspace equivalent | Status |
|---|---|---|---|---|---|
| `repro/core_regressions.rs` | `f0b3270e50998fd83340e58d4e04b687ccd305f4bb0d2b0775b2fd7039554076` | `moving_target_is_picked_at_visible_position` | Compile E0507; runtime not reached | `crates/bough-x/tests/review_core.rs::moving_target_is_picked_at_visible_position (existing adapted test; not run in this card)` | ApiDrift |
| `repro/core_regressions.rs` | `f0b3270e50998fd83340e58d4e04b687ccd305f4bb0d2b0775b2fd7039554076` | `pointer_updates_expression_words` | Compile E0507; runtime not reached | `crates/bough-x/tests/review_core.rs::pointer_updates_expression_words (existing adapted test; not run in this card)` | ApiDrift |
| `repro/core_regressions.rs` | `f0b3270e50998fd83340e58d4e04b687ccd305f4bb0d2b0775b2fd7039554076` | `in_place_shape_changes_update_layout` | Compile E0507; runtime not reached | `crates/bough-x/tests/review_core.rs::in_place_shape_changes_update_layout (existing adapted test; not run in this card)` | ApiDrift |
| `repro/core_regressions.rs` | `f0b3270e50998fd83340e58d4e04b687ccd305f4bb0d2b0775b2fd7039554076` | `focus_follows_item_through_insert` | Compile E0507; runtime not reached | `crates/bough-x/tests/review_core.rs::focus_follows_item_through_insert (existing adapted test; not run in this card)` | ApiDrift |
| `repro/core_regressions.rs` | `f0b3270e50998fd83340e58d4e04b687ccd305f4bb0d2b0775b2fd7039554076` | `press_is_cancelled_when_slot_reused` | Compile E0507; runtime not reached | `crates/bough-x/tests/review_core.rs::press_is_cancelled_when_slot_reused (existing adapted test; not run in this card)` | ApiDrift |
| `repro/core_regressions.rs` | `f0b3270e50998fd83340e58d4e04b687ccd305f4bb0d2b0775b2fd7039554076` | `window_can_grow` | Compile E0507; runtime not reached | `crates/bough-x/tests/review_core.rs::window_can_grow (existing adapted test; not run in this card)` | ApiDrift |
| `repro/core_regressions.rs` | `f0b3270e50998fd83340e58d4e04b687ccd305f4bb0d2b0775b2fd7039554076` | `world_rejects_different_view_type` | Compile E0507; runtime not reached | `crates/bough-x/tests/review_core.rs::world_rejects_different_view_type (existing adapted test; not run in this card)` | ApiDrift |
| `repro/core_regressions.rs` | `f0b3270e50998fd83340e58d4e04b687ccd305f4bb0d2b0775b2fd7039554076` | `changed_geometry_preserves_paint_order` | Compile E0507; runtime not reached | `crates/bough-x/tests/review_core.rs::changed_geometry_preserves_paint_order (existing adapted test; not run in this card)` | ApiDrift |
| `repro/core_regressions.rs` | `f0b3270e50998fd83340e58d4e04b687ccd305f4bb0d2b0775b2fd7039554076` | `same_size_list_type_change_is_not_silently_accepted` | Compile E0507; runtime not reached | `crates/bough-x/tests/review_core.rs::same_size_list_type_change_is_not_silently_accepted (existing adapted test; not run in this card)` | ApiDrift |
| `repro/core_regressions.rs` | `f0b3270e50998fd83340e58d4e04b687ccd305f4bb0d2b0775b2fd7039554076` | `active_ramp_survives_clock_wrap` | Compile E0507; runtime not reached | `crates/bough-x/tests/review_core.rs::active_ramp_survives_clock_wrap (existing adapted test; not run in this card)` | ApiDrift |
| `repro/pool_regression.rs` | `2073ac875d2ca15706fca3f45e8329606881dcdab40970f13ec3b78925afb4d3` | `pool_run_joins_workers_when_caller_panics` | runtime exit 101 | `crates/bough-kernel/tests/pool.rs::run_waits_for_its_workers_when_the_caller_panics (release passed)` | GenuineRegression |
| `repro/storage_regressions.rs` | `4a88ab4ef9a331da98be7ac7ae17ec2699f93ecbb4beca980cfc7428a3d379d3` | `region_carve_respects_element_alignment` | Compile rejects !Send Line; runtime not reached (file-level blocker) | `crates/bough-mem/tests/align.rs::region_carve_respects_element_alignment (not run in this card)` | CompileRejectionDueToTypeSafety |
| `repro/storage_regressions.rs` | `4a88ab4ef9a331da98be7ac7ae17ec2699f93ecbb4beca980cfc7428a3d379d3` | `line_can_be_moved_to_another_thread` | Compile rejects !Send Line; runtime not reached (file-level blocker) | `crates/bough-span/src/line.rs compile_fail Send/Sync examples (not run in this card)` | CompileRejectionDueToTypeSafety |
| `repro/storage_regressions.rs` | `4a88ab4ef9a331da98be7ac7ae17ec2699f93ecbb4beca980cfc7428a3d379d3` | `logical_extent_does_not_wrap_negative` | Compile rejects !Send Line; runtime not reached (file-level blocker) | `crates/bough-lay/tests/extent.rs::uniform_is_exact_past_32_bits_and_refuses_no_height (release passed)` | CompileRejectionDueToTypeSafety |
| `repro/text_regressions.rs` | `1efd2d70cb8471629e5ae4284dd80a4820f4052c21417a49cff9a7991e8c5f32` | `split_rejects_inside_utf8` | runtime exit 0 | `crates/bough-text/src/stage/doc.rs::tests::edits_inside_a_character_are_refused (release passed)` | Pass |
| `repro/text_regressions.rs` | `1efd2d70cb8471629e5ae4284dd80a4820f4052c21417a49cff9a7991e8c5f32` | `splice_rejects_partial_utf8` | runtime exit 0 | `crates/bough-text/src/stage/doc.rs::tests::edits_inside_a_character_are_refused (release passed)` | Pass |
| `repro/text_regressions.rs` | `1efd2d70cb8471629e5ae4284dd80a4820f4052c21417a49cff9a7991e8c5f32` | `numeric_padding_has_leading_zeros` | runtime exit 0 | `crates/bough-face/src/num.rs::tests::padding_beyond_ten_digits_is_zeros (release passed)` | Pass |

Final counts: 3 Pass, 10 ApiDrift, 3 CompileRejectionDueToTypeSafety, 1 GenuineRegression (embedded historical pool), 0 SafeCheckedRefusal. The UTF-8 checks pass by catching their intended checked panics. No category converts a blocked check into a passing check.

All six manifest-listed files (including REPORT and EVIDENCE) matched their frozen sizes and SHA-256 digests before and after each completed replay. First cold replay: 57.45 seconds; offline lockfile-preserving replay: 23.87 seconds; final replay: 228.58 seconds, zero timeouts. The card's unconditional <30-second budget is **not met**. Cargo lock waiting, compilation and execution are included together; these runs do not isolate their timing contributions.

This card does not execute the adapted core, span or memory tests. Their named equivalents are handoff targets for the integration replay; their presence is not a completion receipt.
