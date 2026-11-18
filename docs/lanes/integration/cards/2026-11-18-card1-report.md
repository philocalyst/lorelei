# Card 1: typed admission, opaque CPU records and sparse delta

Status: **GATED, including lock-free signal operations and scoped zero-allocation
checks.** No general zero-CPU-cost or complete retained-memory claim is made.
Independent acceptance remains with the coordinator.

Frozen library source for the lock-free replay:
2a2c9362a10a3d4227db3d7ddd71066c8c080fab.
After that complete replay, the allocation probe gained a positive control and
black_box barriers; the final nineteen-probe binary was rerun successfully.
No library source changed after the full replay.

Earlier complete Card 1 source: 76fa3066443689799ed1827ab9addc270cd58872.
Earlier sparse checkpoint: 5d023b9ad2a49abc01ef0a04f57a0a3bc97c6583.

Scope: body B, the new core prototype, on headless macOS aarch64, pinned
nightly-2026-10-17, release, Soft and the existing paint test backends. These are
correctness and build checks, not façade cost measurements, runner rankings,
hardware-GPU performance or scanout measurements.

## Construction

- World<V,B> has a private invariant owner/backend marker. Its state, mounting,
  settling and event calls accept only V. Reflect<V,B> learns through that typed
  world. WorldId distinguishes instances, including after address reuse.
- Place tokens carry WorldId. Foreign tokens are refused by at_token and
  accept_pick. Packed u32 helpers remain for raster indexing and validate
  numeric bounds/liveness; a packed integer alone does not prove provenance.
- Slot fields are private. Its typed constructor installs only a safe Rust
  reader. Gathering checks TypeId and APP_WORDS before executing it; it performs
  no raw-offset copy. At remains a zero-sized FieldProj<V>.
- Field descriptors are interned by owner, value type and nominal offset.
  Repeated syntax for the same nominal field uses its canonical reader.
  Distinct nominal projections colliding modulo APP_WORDS are refused at
  retable. Nominal offsets select uniform words; they are never dereferenced.
- Expr and Arch use Record, separately from Words. Pinned-nightly specialization
  exposes raw words only for types implementing the unsafe Words contract.
  MapRef no longer implements Words. Composite GPU paints explicitly require
  Words operands.
- Column pages store typed MaybeUninit records, with initialization bits and
  typed writes before reads. Record storage permits exclusive-world transition
  updates through UnsafeCell. Reflection uses a typed erased reader: opaque
  CPU closures return empty raw words and retain evaluated operand values.
  Re-description compares evaluated opaque outputs and retains new captures.
- Store structure counters are local to each tree. Relation journals expose
  owned revision watches, so moving the model cannot invalidate a raw journal
  pointer. A world observes only its own relations.
- Signals carry packed slab tokens with no destructor or per-signal allocation.
  The 4,096-slot static slab retires exhausted generations. Generation, shared
  marker and revision are combined in one AtomicU64 (20/1/43 bits), so stale
  updates racing slot reuse cannot increment or mark a replacement world.
  Scalar TLS saves/restores the current token, including nested unwinding.
  Shared signals use the fallback revision only for worlds marked as observers;
  unrelated single-owner worlds remain isolated. Clones detach.
- Slot constructors store typed function pointers in sealed erased form and
  restore their exact signatures after owner TypeId admission. They allocate
  nothing and never derive a read address from a nominal offset.
- Column admission checks the 4,096-column limit before insertion. Record
  capacity/count and packed-place construction check the 20-bit record limit.
  at_place checks bounds and liveness.
- Reader offsets and indices use u32. Multi-word delta performs an epoch-stamped
  sparse union, with scratch allocated at retable and explicit epoch-wrap reset.
  It uses a dense scan when summed incidence lengths exceed the column count.
  Per-delta counters expose incidences, evaluated columns and dense fallbacks.

The required sig!(self.field) forms remain in the existing scenes unchanged:
map, get, num, array at/num and scroll.

## Verification on the frozen candidate

Every cargo invocation used the serialized integration wrapper, this lane's
dedicated target, nice 15 and three jobs. Source remained unchanged throughout
the final batch. All checks below returned zero, except deliberate mutants.

| Gate | Outcome |
|---|---|
| bough-expr release unit tests | 1 passed |
| bough-x release tests on lock-free source | 77 passed (including 3 slab/TLS unit tests) |
| Final strengthened Card 1 probe binary | 19 passed |
| bough-ax release tests | 11 passed; 1 ignored |
| bough-paint release tests | 17 passed |
| Slot raw-descriptor compile-fail documentation | Passed |
| Wrong-owner World compile-fail documentation | Passed |
| macOS workspace check, all tests | Passed again on lock-free source; Cargo reported 2.50 s |
| bough-guibench-x check, including tests | Passed on prior Card 1 source; no caller API changed in this cleanup |
| bough-x release check of tests, -j 3 | Prior Card 1 check passed; Cargo reported 0.87 s |
| Lock-free full release batch compilation | Cargo reported 1 min 30 s |
| Final focused incremental test compilation | Cargo reported 3.89 s |

The last two times are included to avoid presenting the cached check as a full
rebuild. These are Cargo-reported build times, not a randomized leaf-edit
compile-time study.

The nineteen final Card 1 integration probes cover:

- Arbitrary nominal offset on a padded owner: safe projection value is gathered.
- Foreign gathered owner: checked refusal before its reader executes.
- Out-of-block slot: checked refusal at construction.
- Distinct nominal projections sharing a word: diagnostic refusal.
- Padded closure capturing bool/u8/u32, NonZeroU32 and a static string: mount,
  once mode, re-description, immediate-paint equality and reflection snapshots.
- Independent apps: mutating B causes zero extra view descriptions and zero
  retables in A; external setters still work after moves.
- Foreign WorldId place and reflection refusal.
- Independent relation edits: zero extra A descriptions and retables.
- Signal extraction moves and drops its value exactly once.
- Signal<u32>, Signal<f32> and the numeric sheet owner have no drop glue.
- A thread-local counting allocator observes zero allocation calls over 1,000
  signal set/deref/once-settle steps; view descriptions and retables also stay flat.
- Typed and projected Slot construction/read performs zero allocations across
  1,000 iterations, with black_box keeping the operations observable.
- A deliberate allocation is detected by the counting allocator's positive control.
- Shared signals notify all three observing worlds, including a world observing
  an already-shared sentinel; an unrelated single-owner app remains unchanged.
- Stale signal tokens are inert after their world drops and rebind on observation.

Three additional library unit probes verify stale CAS updates racing release/reuse,
terminal generation retirement without issuing the shared sentinel, and restoration
of nested/unwound scalar TLS input tokens.
- Packed column/record overflow and 4,097-column admission refusal.
- Sparse slope, large incidence storage and the retained sheet ledger.

| Sparse matrix | Visited incidences | Evaluated columns | Dense fallbacks |
|---|---:|---:|---:|
| C=64, two changed words | 2 | 2 | 0 |
| C=256, two changed words | 2 | 2 | 0 |
| C=1,024, two changed words | 2 | 2 | 0 |
| C=2,048, two changed words | 2 | 2 | 0 |
| C=1,500, 48 readers each, one changed word | 1,500 | 1,500 | 0 |
| Same 72,000-incidence matrix, two changed words | 0 | 1,500 | 1 |

Sparse damage equals the tree-walk oracle. The crossover is an operation-count
rule, not an empirically calibrated CPU timing crossover.

sheet_el at 10,000 rows, 2560x1600, scale 2, Soft:
**764 records, 14 columns, 32 pages, 51,456 column bytes**. Both preregistered
record/column-storage limits pass. The earlier checkpoint used 49,408 column
bytes; initialized-page metadata adds 2,048 bytes here.

All seven mutants ran separately on the preceding full Card 1 source, compiled
successfully and failed their named oracle with exit 101. The requested
mutant_sparse_union replay on the lock-free source also compiled successfully
and failed the same damage oracle (exit 101):

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
[1], expected [3]. No compile failure is counted as mutant detection.

## Costs and limits

The lock-free follow-up removes the signal watch mutex, hash map,
Arc/Weak subscriptions and Signal's Drop implementation. A thread-local counting
allocator observes zero allocations across 1,000 set/deref/once-settle steps,
and across 1,000 typed/projected Slot constructions and reads. Signal<u32>,
Signal<f32> and the numeric sheet owner need no drop. These are scoped operation
gates, not proof that all framework operations allocate nothing or a CPU timing
comparison against the predecessor. Signal remains Clone rather than Copy and
contains a four-byte token in addition to its value.

Each world has at most 2^43-1 local input revisions before checked refusal.
World slot generations never wrap: the terminal generation is retired and never
issued, including the combination that would equal the shared-signal sentinel.
If all slots are occupied or retired, world construction refuses cleanly.
The existing cold field-admission registry still uses a mutex/map; no signal
setter, observer, dereference, or ordinary gathered slot read consults it.
Slot::typed and Slot::projected use no Box::leak or heap allocation. Their sealed
function-pointer trampolines restore the exact typed signatures only after the
owner TypeId check; no integer-to-function-pointer round trip or Any downcast
is needed.

Exact model, static slab, field-registry, incidence scratch, code-size and total
retained-memory costs were not measured. The column ledger excludes them.

The field-reader registry retains one canonical reader per admitted nominal
projection. Canonicalization is by owner/type/offset, not proof of equivalence of
arbitrary user-supplied reader functions. Public typed constructors cannot grant
raw-memory read authority.

No Miri, sanitizer, independent replay, calibrated crossover benchmark, complete
retained-state budget or general speedup is claimed. The branch must be reviewed
with these limits and the Signal trait change visible. No remote host, visible
window or synthetic global input was used.

## Reproduction and rejected attempts

Run these arguments through cargo-locked.sh with a dedicated target:

- test -p bough-expr -p bough-x -p bough-ax -p bough-paint --release
- test -p bough-x --release --test card1_soundness_and_sparse -- --nocapture
- check -p bough-x --tests --release -j 3
- check -p bough-guibench-x --tests
- check --workspace --tests
- test -p bough-x --release --features FEATURE --test TARGET ORACLE
  (one mutant table row per invocation)

Failed attempts remain in ignored runtime logs: an initial mutant batch timed
out waiting on the shared cargo lock; a private retry script initially computed
the wrong wrapper location; the first padded probe used the wrong snapshot
method name and then exposed the reflecting sink's missing element environment.
The first inline signal-watcher representation overflowed the default test
thread stack when moving the large sheet. Replacing it with a four-byte identity
passed the default-stack gate. The first isolation assertion accidentally counted
the explicit reflection description; the corrected test separately checks idle
settle and reflection.

All owned background processes completed. No goal was marked complete: Nori
reported no active goal, and independent acceptance belongs to the coordinator.
