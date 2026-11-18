# Card 2 implementation report — 2026-11-18

State: GATED for the owned safety changes; METHOD_BOUNDARY for an unchanged-green original review replay. Independent acceptance and whole-workspace integration remain with the lead.

Baseline: frozen card at `362b0c1b45`, based on `e0b7a4c889`. Original safety source: `e6eb6ac3a7`; hot-path correction source: `6313cc7f72`. Branch: `lane/integration/card2-probes-kernel`. Host: headless macOS aarch64 (M3 Pro), pinned nightly-2026-10-17, serialized nice-15 Cargo with three jobs and the dedicated Card 2 target. No benchmark timing or scanout claim.

## Construction and scope

- K01: retained the admission mutex, unwind drain guard and caught worker panics already present at card start. Added per-thread active-pool tracking so direct caller/worker reentry refuses with a checked panic instead of deadlocking. Zero-worker runs also enter admission. Existing caller panic, worker panic and overlapping-call tests plus the new reentry test pass; the entire pool target takes 0.07 seconds.
- K02: the existing empty-atlas fast-path check and its naive/fast parity test remain; the parity gate passes.
- K03: validates scene record shape, command kind/shift and animation target presence; program header, ordered unique tile origins, list ranges, IDs and operand table ranges; and stride/output capacity before CPU or GPU execution. GPU uploads retain the admitted inputs, atlas sizes are checked at dispatch, and empty atlas buffers are initialized. Targets carry creator identity; Vulkan forget invalidates previous handles. Safe constructors accept only the built-in shader; arbitrary shaders require explicit unsafe constructors with the buffer-schema/address contract. Existing production calls retain their signatures. Custom rounding mutants use the unsafe entry points with a local address-preservation argument.
- K08: derived radius/blur fields, gradient vectors and inverse transform fields interpolate; shadow inverse blur is recomputed at intermediate progress. At ONE the complete command is exactly the staged target. Clock subtraction uses bounded integer operations equivalent to wrapping_sub for the shared Rust/WGSL grammar. The endpoint oracle covers changing shadow blur, gradient direction and transform, including wrapped clocks. This does not establish an independently restaged oracle for every intermediate transformed or gradient frame.
- T04: Run remains repr(C), two u32 words; default empty, len, is_empty and face remain available. g0 now packs a checked 12-bit store ID and 20-bit dense descriptor slot, not a public lane offset. Slots reuse storage with a fresh per-slot generation, retire before 8-bit wrap, and validate full u32 epoch, descriptor words and slice bounds. Store IDs and epochs never wrap; exhaustion is a checked refusal. Lookup is O(1) through Vec::get, with no descriptor tree. The 301-clear and cross-store test passes. This closes handle aliasing, not the wider exposure-aware glyph collector/publication roadmap. bytes counts the full descriptor Vec capacity; dead descriptor generation history is deliberately retained across clear.
- L05: Memo records every canonical Nd word, scale bits and input words and verifies exact equality before reusing a hash/width candidate. A collision replaces the one-entry memo cleanly. Hash-only holds remains a candidate query; row performs the structural admission. The forced collision and scale-change oracle matches uncached layout. The exact comparison uses a stack Sink visitor without constructing a temporary Vec. Misses clear and refill the kept canonical buffer, reusing capacity; input words are an inline fixed-size array. No performance improvement is claimed.
- The prelude export and façade test inventory for Card 3 is in [FACADE-LOWERING-MATRIX.md](../FACADE-LOWERING-MATRIX.md). It is a blueprint, not a completed façade cutover.

## Verification

The five-crate release suite at d643bb6dd0 passed: **100 passed, 0 failed, 1 ignored**. Command arguments to the prescribed wrapper:

```sh
test --release -p bough-kernel -p bough-face -p bough-lay -p bough-text -p bough-paint
```

After the final kernel-only shader admission changes, the complete kernel release suite at e6eb6ac3a7 passed again: **20 passed, 0 failed**, with --nocapture. It executed the foreign-target and short-output refusal body; Metal parity reported 24 scenes, 9,600 CPU tiles and 3,840 GPU tiles, zero differing words (spec hash 5f1c8e994b7c347e). The existing GPU rounding mutant was caught in all four scenes. These are correctness gates, not GPU performance measurements. No Linux Vulkan execution is claimed.

Individual negative controls both compiled and failed their intended runtime assertions (exit 101):

```sh
test --release -p bough-kernel --features mutant_anim_stale_derived --test admission animation_endpoint_matches_every_staged_field
test --release -p bough-lay --features mutant_memo_hash_only --lib memo_collision_and_scale_match_uncached
```

The animation mutant retained old inverse-blur values; the memo mutant returned [0,0,10,10] instead of [0,0,30,10] on the forced collision. These feature gates ran at d643bb6dd0; subsequent production changes affect GPU constructors, not either mutated path.

Unchanged probe bytes and all manifest digests were verified before/after replay. [ORIGINAL-PROBES-REPLAY.md](../ORIGINAL-PROBES-REPLAY.md) gives all 17 checks, exact source hashes, compile/runtime classifications and contemporary equivalents. It explicitly records the historical embedded pool failure and file-level blockers. The <30-second unconditional replay budget missed on cold and final runs; no timeout occurred.

## Failed attempts retained

Private scratch retains the first release log (obsolete run-offset reuse assertion failed after the new descriptor representation), second release log (cast-comparison parse errors), corrected five-crate suite, final kernel suite, individual mutant logs and all replay attempts. A first 1-second wrapper invocation timed out without a conclusion; it was not counted as a passing gate. The obsolete workspace assertion now requires unique tokens while retaining its stale-handle refusal checks. Frozen original probe bytes were never edited.

## Not yet verified

Independent review; the composed Card 1 + Card 2 tree; full workspace/standalone application ports; façade execution on the new core; adapted core/span/memory review gates; Linux/Vulkan runtime and target retirement; Miri/Loom/sanitizers; full intermediate-frame restaging and exact retained-capacity accounting. The existing ignored text test remains ignored. The lead must replay these changes before integration; this session does not accept its own result.

## Carbon adversarial hot-path correction

Carbon rejected the original per-hit canonical Vec allocation and per-line BTreeMap lookup. The follow-up replaces both while retaining the safety oracles:

- Memo uses inline `[u32; bough_expr::WORDS]` input storage. A stack `MatchSink` compares each emitted Nd and the final node count against the kept canonical slice. Only misses populate the canonical Vec, with clear/refill and capacity reuse.
- A test-only allocator counts allocation/reallocation calls on the test thread. `memo_hits_reuse_storage_and_inputs_invalidate` confirms **zero allocations across 1,000 hits**, stable canonical/rect pointer and capacity, input-word invalidation, and rejection of shorter/longer shape streams. The collision and scale oracle remains.
- Glyph descriptors are a dense `Vec<RunSlot>`, indexed directly after the store ID check. Each slot stores start, complete n_face, full epoch and an 8-bit generation. Clear increments the checked epoch and resets the cursor without visiting descriptors. Reuse increments the slot generation; generation 255 retires the slot permanently. IDs 1–4095, 1,048,576 descriptor slots per store, 255 uses per slot and u32 epoch are explicit checked admission limits.
- The descriptor reuse test confirms stable capacity through generations 1–255, unchanged dead descriptors immediately after clear, retirement before reuse could wrap, default-empty refusal and memo-hit handle identity. The 301-clear and cross-store test still passes.
- Descriptor metadata cannot be freed without losing generation history. The memory-release test now counts retained descriptor capacity and requires clearing the text/table/lanes to release at least three quarters of the original large workload's bytes. It no longer claims a universal 64 KiB post-clear bound.
- The first formatting attempt rejected unescaped `gen`, reserved in Rust 2024. This was corrected to `r#gen` before any build gate; the failed commit remains in history.

Frozen at `6313cc7f72`, the prescribed serialized release command `test --release -p bough-face -p bough-lay` passed **15 tests, 0 failed** (plus zero-test doctest targets). The individual `test --release -p bough-lay --features mutant_memo_hash_only --lib` gate compiled and failed both the intended forced-collision assertion and input invalidation assertion (exit 101). Private logs: `card2-hotpath-release.log`, `card2-hotpath-mutant.log` under the existing replay scratch directory.

The previous five-crate/kernel/replay results above remain historical receipts at their named source revisions. This follow-up rechecks face/layout and the memo mutant as requested; it does not claim a new whole-workspace or unchanged-probe replay. All six original-review manifest digests were independently verified again after these edits.
