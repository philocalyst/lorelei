# Card 1 implementation candidate

The source now implements typed field-reader admission, opaque CPU records,
World<V,B>/WorldId/Reflect<V,B>, local shape/relation/input revisions, checked
packed-place admission and sparse delta. Fourteen focused release probes pass.
Final full-suite and mutant replay results will be appended after the frozen
candidate is checked. This is not an independent acceptance receipt.

Important deviations from the frozen prediction:

- Signal carries a four-byte stable identity, is Clone rather than Copy, and
  stores weak world subscriptions in a registry. Copies made by Clone are
  independent. This preserves all required sig! syntax, but does not preserve
  the previous Copy trait or the claim that a signal is only its value.
- Input observation and setters use a registry mutex. No zero-cost or zero
  steady-state-tax claim is made. Signal<u32> now occupies eight bytes on this
  host; the sheet's model storage increases accordingly, separately from its
  retained column ledger.
- Record uses pinned nightly specialization to restrict raw words to Words.
  Opaque CPU record equality uses evaluated operands; new captures are retained
  even when their current output agrees.
- Sparse/dense crossover is by exact incidence count, not calibrated wall time.
- Field reader descriptors are interned by owner, value type and nominal offset.
  The offset selects a uniform word; it is never a raw read address. Repeated
  syntax for the same field uses its canonical reader. Distinct nominal
  projections colliding modulo APP_WORDS are refused at retable.
- Place tokens carry WorldId. The packed u32 helper remains available for raster
  indexing, with bounds/liveness checks; only the token API proves provenance.

Latest focused ledger: sheet_el, 10,000 rows, 2560x1600, scale 2, Soft:
764 records, 14 columns, 32 pages, 51,456 column bytes. This excludes the model,
signal registry, field-reader registry, incidence scratch and other allocations.

The initial inline signal watcher representation overflowed the default test
thread's stack when moving the large sheet. It was replaced with the four-byte
identity representation; the corrected default-stack probe passes. The initial
padded snapshot probe also exposed the reflecting sink's missing element
environment; reflection now learns from the typed world's gathered block.
All failed attempts remain in ignored runtime logs.

The earlier checkpoint evidence follows for provenance; its incomplete status
and old ledger apply only to that checkpoint.

---

# Card 1 checkpoint: sparse delta and packed places

Status: **INCOMPLETE**. This checkpoint implements only the sparse invalidation
and part of the structural bounds work in the frozen Card 1. It does not close
the card's safety gates and must not be represented as completion of Card 1.

Scope: body B, the new core prototype, on headless macOS aarch64 with Soft;
nightly-2026-10-17, release, default features except each named mutant.
The predecessor is the frozen card's baseline. No display, GPU or scanout
measurement was made. Existing author syntax is unchanged.

## Changes

- Reader incidence offsets and column indices use u32. The prior 65,536-edge
  limit is removed; the existing 4,096-column limit bounds total incidences.
- Multi-word delta walks the changed words' incidence lists, deduplicating columns
  with an epoch-stamped vector. Epoch wrap clears that vector before reuse.
- When the sum of reached incidence lengths exceeds the column count, delta
  uses its dense scan. This is an operation-count crossover, not an empirically
  calibrated CPU timing crossover.
- Public per-delta counters expose visited incidences, evaluated columns and
  dense fallbacks. The scratch vector is sized at retable, not per delta.
- place checks 12-bit column and 20-bit record limits before packing.
  at_place additionally checks record liveness.
- mutant_sparse_union deliberately omits the second changed word.

## Verification

All cargo invocations used the integration cargo-locked wrapper and this lane's
dedicated target directory, nice 15, with three jobs. No source changed during
a running build/test. The new test file is
crates/bough-x/tests/card1_soundness_and_sparse.rs.

| Gate | Result |
|---|---|
| bough-x release suite | 62 passed, zero failed before adding the ledger test |
| bough-ax release suite | 11 passed, zero failed, one ignored |
| Final Card 1 checkpoint test binary | Five passed |
| Sparse C=64,256,1024,2048 | Exactly two evaluated columns and two incidences at every C; damage equals tree oracle |
| 1,500 columns reading 48 words | 72,000 incidences admitted; single changed word visits/evaluates 1,500 |
| Same matrix, two changed words | One dense fallback; 1,500 evaluated columns |
| Packed place column/record overflow | Both panic with the expected diagnostic |
| sheet_el, 10,000 rows, 2560x1600, scale 2 | 764 records; 14 columns; 32 pages; 49,408 column bytes |
| Release check of bough-x tests, -j 3 | Passed; Cargo reported 0.83 s |
| Incremental release build of final checkpoint test | Cargo reported 2.62 s |

Cargo-reported times describe these builds only. They are not a paired leaf-edit
compile benchmark or proof of the full card's compile-time threshold. The
retained ledger excludes the new incidence/epoch vectors, model storage and
other allocations; it is explicitly column storage, not total retained memory.

Each mutant ran separately and compiled successfully, then failed its named
oracle (exit 101):

| Feature | Test target / oracle |
|---|---|
| mutant_delta | sheet / short_sheet |
| mutant_journal | sheet / short_sheet |
| mutant_moves | sheet / short_sheet |
| mutant_sweep | sheet / short_sheet |
| mutant_line | text / rows_with_text_mount_once |
| mutant_reflect | reflect / an_entity_survives_scrolling |
| mutant_sparse_union | card1_soundness_and_sparse / sparse_multiword_slope_and_damage_oracle |

The sparse mutant fails the damage comparison at 64 columns: actual tile bits
[1], expected [3]. The other mutants fail their existing semantic/pixel oracles,
not compilation.

An initial mutant batch timed out after 200 seconds waiting on the shared cargo
lock, before compiling its first mutant. Its owned orphan wrapper was stopped.
A private log-preserving runner initially used an incorrect relative wrapper
location and failed before cargo; corrected retry produced the results above.
Private operational logs remain ignored under runtime.

Reproduction through the required wrapper:

- test -p bough-x -p bough-ax --release
- test -p bough-x --release --test card1_soundness_and_sparse -- --nocapture
- check -p bough-x --tests --release -j 3
- test -p bough-x --release --features FEATURE --test TARGET ORACLE
  (one table row per invocation)

## Outstanding work and coordination

Not implemented: safe typed FieldProj/Slot gathering; padded MapRef support;
World<V,B>, WorldId and Reflect<V,B>; world-local shape/relation/set revisions;
the corresponding safe-client, compile-fail and two-world isolation probes;
checked structural admission before partial publication; foreign-world picks.

The current Expr: Words and Arch: Words contracts propagate raw-word authority
through composite operands. Merely overriding MapRef.words would leave raw
casts in Fill, transitions and reflection unsound. Splitting opaque CPU operands
from true Words operands also affects bough-paint's generic implementations
(expr.rs and paint.rs), which are outside this lane's assigned ownership.
The coordinator was asked to coordinate that scope. This is a limitation of
the current implementation plan, not a proof that the requested API is impossible.

No workspace-wide check, bough-guibench-x check, independent replay, Miri or
sanitizer run was performed. No safety finding is claimed closed by these
sparse/bounds tests. No goal was marked complete; Nori reported no active goal.
