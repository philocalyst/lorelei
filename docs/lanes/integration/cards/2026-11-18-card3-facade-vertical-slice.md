# Integration Card 3: Shipping `bough::prelude` Vertical Slice on Typed Core (`World<V, B>`)

```
lane:            integration
state:           CARD
owner:           astra-card3 (after Cards 1 & 2 merge; reviewed by Carbon)
predecessor:     lane/integration/typed-continuation post-Card-1+2 merge (nightly-2026-10-17, release)
question:        Can shipping crates/bough (`bough::prelude`) lower directly onto `World<V, B>`, `bough-lay`, unified `bough-face`/shaping fallback, and `bough-kernel` with one authoritative runtime, passing pixel/semantic/focus/resize gates within strict compile-time and retained-memory budgets?
change:          Wire `crates/bough` to `bough-x`, `bough-lay`, and `bough-face`; lower `bough::prelude` (`View`, `changed`, `Signal`, `List`, `Slot`, `Reader`, `div`, `row`, `column`, `block`, `para`, `text`, `button`, `text_input`, `image`, `each`, `list`, `Curve`) into `World<V, B>` schema, columns, lexical ancestry, and generic retained fallback kernels inside the same `World<V, B>` runtime (no delegation to legacy `Rt` for lowered prelude apps).
implication:     Converts `bough-x` from a standalone prototype into the actual backing engine of `bough::prelude`, enabling all runner scenarios and the five heavy scenes (`sheet`, `chat`, `ide`, `dashboard`, `canvas`) in Wave 3.
platforms:       macOS aarch64 (M3 Pro), headless CPU + Metal/kernel parity where headless-safe
workloads:       (1) Canonical prelude vertical-slice app (2 Signals + opaque state + header + keyed scrollable list with Unicode/ASCII text, click/hover/key handlers, insert/remove/reorder, focus continuity, live resize, plus second World isolation control);
                 (2) Existing `crates/bough/tests/*` suite (`counter`, `describe`, `layout`, `lay_diff`, `list`, `list_window`, `owners`, `reflect`, `scroll_box`, `select_all`, `startup`);
                 (3) Deep heterogeneous component compile-cost fixture (depth 16, mixed flex/text/handlers/transitions).
gates:           - Pixel & state exactness: retained == full redraw == immediate == fresh mount across ring ages 1..=4 on prelude vertical slice and migrated tests.
                 - Runtime provenance counter proves 100% of mount/dispatch/settle/paint passes run through `World<V, B>` and 0 through legacy `Rt`.
                 - Non-Latin1 / Unicode / emoji text and translucent alpha paints render accurately via integrated fallback within the same runtime (never dropped or forced opaque).
                 - Resize preserves `World<V, B>`, focus, held interaction state, and scroll anchor without remounting.
                 - Negative control: sabotage/mutant features (`mutant_delta`, `mutant_moves`, `sabotage_copy`) falsify pixel gates.
thresholds:      - Deep-view release compile time (`-j 3`, pinned nightly): clean `crates/bough` + slice <= 35.0 s, incremental leaf edit <= 8.0 s (hard kill if > 30.0 s incremental).
                 - Materialized records for 100,000-item prelude `list`: <= 400 records (`O(exposed)`).
                 - Paired-block headless turn latency (executed by DeepSeek under bench.lock): hover & single-signal update p50/p95 <= 1.00x legacy `bough` façade on matched prelude slice, virtual 100k mount <= 0.50x legacy `bough`.
measurement:     Paired randomized blocks via DeepSeek benchmark runner under exclusive `bench.lock`; compiler wall/RSS via `-Z time-passes` / `/usr/bin/time -l`; full retained-state memory ledger.
budget:          Single isolated worktree, nice -n 15, <= 3 cargo jobs, one cargo at a time.
stop conditions: Stop and refactor lowering to type-erased `Flat` layout descriptors immediately if monomorphized element types cause codegen blowup (> 30 s incremental).
do not revisit:  docs/SETTLED.md (no component library in core, no per-view static mode switch, no monomorphized layout solver).
```
