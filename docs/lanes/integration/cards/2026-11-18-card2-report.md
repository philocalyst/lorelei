# Card 2 implementation report — 2026-11-18

State: GATED for the owned safety changes; METHOD_BOUNDARY for an unchanged-green original review replay. Independent acceptance and whole-workspace integration remain with the lead.

Baseline: frozen card at `362b0c1b45`, based on `e0b7a4c889`. Final source: `e6eb6ac3a7`. Branch: `lane/integration/card2-probes-kernel`. Host: headless macOS aarch64 (M3 Pro), pinned nightly-2026-10-17, serialized nice-15 Cargo with three jobs and the dedicated Card 2 target. No benchmark timing or scanout claim.

## Construction and scope

- K01: retained the admission mutex, unwind drain guard and caught worker panics already present at card start. Added per-thread active-pool tracking so direct caller/worker reentry refuses with a checked panic instead of deadlocking. Zero-worker runs also enter admission. Existing caller panic, worker panic and overlapping-call tests plus the new reentry test pass; the entire pool target takes 0.07 seconds.
- K02: the existing empty-atlas fast-path check and its naive/fast parity test remain; the parity gate passes.
- K03: validates scene record shape, command kind/shift and animation target presence; program header, ordered unique tile origins, list ranges, IDs and operand table ranges; and stride/output capacity before CPU or GPU execution. GPU uploads retain the admitted inputs, atlas sizes are checked at dispatch, and empty atlas buffers are initialized. Targets carry creator identity; Vulkan forget invalidates previous handles. Safe constructors accept only the built-in shader; arbitrary shaders require explicit unsafe constructors with the buffer-schema/address contract. Existing production calls retain their signatures. Custom rounding mutants use the unsafe entry points with a local address-preservation argument.
- K08: derived radius/blur fields, gradient vectors and inverse transform fields interpolate; shadow inverse blur is recomputed at intermediate progress. At ONE the complete command is exactly the staged target. Clock subtraction uses bounded integer operations equivalent to wrapping_sub for the shared Rust/WGSL grammar. The endpoint oracle covers changing shadow blur, gradient direction and transform, including wrapped clocks. This does not establish an independently restaged oracle for every intermediate transformed or gradient frame.
- T04: Run remains repr(C), two u32 words; default empty, len, is_empty and face remain available. g0 now identifies a store descriptor, not a public lane offset. Globally unique checked tokens never wrap or reuse across stores or clears; exhaustion panics rather than aliases. Descriptors verify the complete run and bounds before slicing. The 301-clear and cross-store test passes. This closes handle aliasing, not the wider exposure-aware glyph collector/publication roadmap. bytes includes descriptor payload but does not claim an exact allocator/B-tree overhead ledger.
- L05: Memo records every canonical Nd word, scale bits and input words and verifies exact equality before reusing a hash/width candidate. A collision replaces the one-entry memo cleanly. Hash-only holds remains a candidate query; row performs the structural admission. The forced collision and scale-change oracle matches uncached layout. This adds equality work; no performance improvement is claimed.
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
