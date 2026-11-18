# Integration Card 2: Unchanged Review Probe Replay Harness, Kernel/Memo/Text Safety Fixes, and Façade Lowering Matrix

```
lane:            integration
state:           CARD
owner:           sol-card2 (reviewed by Carbon)
predecessor:     e0b7a4c8891d5f9b6c67cda44413b0d73c494668 (nightly-2026-10-17, default features, release)
question:        Do the 16 original independent-review probes classify cleanly without modifying frozen probe bytes, and can bough-kernel, bough-face, and bough-lay close open safety/hash/generation findings (K01, K02, K03, K08, T04-gen, L05-collision) with preserved public signatures?
change:          (1) Build an isolated non-mutating replay runner that verifies SHA-256 of the 4 original probe files and classifies all 16 checks honestly (Pass, SafeCheckedRefusal, CompileRejection, ApiDrift, Regression), adding distinct named contemporary regression tests where APIs evolved;
                 (2) Fix K01 in crates/bough-kernel/src/cpu.rs: exclusive run lock + unwind Drop guard joining/draining all active workers before borrowed closure lifetime ends, propagating panics cleanly;
                 (3) Fix K02 (empty glyph atlas fast path returns 0 coverage matching naive CPU), K03 (validate Scene/Prog/Cmd bounds and Target device branding at upload/render admission), and K08 (recompute derived shadow/gradient/transform fields at animation interpolation/endpoint + modular clock-wrap progress);
                 (4) Fix T04 generation/owner hazard in crates/bough-face: prevent 256-clear u8 generation aliasing and cross-Glyphs-store Run confusion while keeping Run 2-word Words layout compatible;
                 (5) Fix bough-lay Memo hash-only hit hazard: store and verify structural shape/scale/input equality on bucket hit so a 64-bit hash collision or scale change never returns stale rectangles;
                 (6) Produce a complete symbol-and-behavior lowering matrix of crates/bough::prelude onto bough-x for Card 3.
implication:     Closes open kernel/face/lay safety and cache-collision defects and establishes honest unchanged replay accounting before façade cutover; does not claim hardware GPU scanout latency.
platforms:       macOS aarch64 (M3 Pro), headless CPU
workloads:       bough-kernel unit/property/executor parity tests, bough-face shaping/memo stress (300 clears + cross-store handles), bough-lay random tree & forced hash-collision tests, isolated 16-probe replay suite.
gates:           - Original probe files retain exact SHA-256 digests before and after replay.
                 - K01 panic-in-worker(0) and panic-in-worker(k>0) tests complete cleanly in < 2.0 s without UB, hang, or leaked active count; concurrent Pool::run calls serialize or refuse safely.
                 - K02 empty-atlas glyph command matches naive CPU oracle bit-for-bit (no panic).
                 - K03 malformed Prog/Cmd offsets and cross-device Targets are refused safely before unsafe execution.
                 - K08 animated shadow blur/gradient/transform at t = 1.0 equals directly staged target command bit-for-bit, and survives u32 clock wrap.
                 - bough-face Run after >= 256 clears and across two Glyphs instances never resolves stale/foreign glyph slices.
                 - bough-lay Memo forced-hash-collision test returns exact layout matching uncached place_root.
                 - Negative control: mutant `mutant_memo_hash_only` (skips equality check on forced collision) and `mutant_anim_stale_derived` must fail their respective tests.
thresholds:      - Zero regressions across all existing bough-kernel, bough-face, and bough-lay test suites.
                 - Replay harness completes deterministically in < 30 s with zero timeouts.
measurement:     Deterministic test pass/fail matrix, SHA-256 verification, and Memo hit/collision counters.
budget:          Single isolated worktree, nice -n 15, <= 3 cargo jobs, one cargo at a time.
stop conditions: Stop and report if fixing Run generation requires changing Run from two u32 words (must stay ABI-compatible with bough-x::text::RunW).
do not revisit:  docs/SETTLED.md (no update-path string-hash shaping cache, no per-tile sequential v7 bin scan).
```
