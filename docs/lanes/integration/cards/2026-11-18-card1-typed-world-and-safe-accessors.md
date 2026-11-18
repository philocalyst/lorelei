# Integration Card 1: Typed `World<V, B>`, Safe Field Accessors, Closure Padding Soundness, and Sparse Multi-Word Delta

```
lane:            integration
state:           CARD
owner:           astra-card1 (reviewed by Carbon)
predecessor:     e0b7a4c8891d5f9b6c67cda44413b0d73c494668 (nightly-2026-10-17, default features, release)
question:        Can bough-x close all safe raw-read, wrong-owner, closure-padding, and global-counter soundness/isolation defects while making multi-word delta O(reached incidences) with zero steady-state tax?
change:          (1) Parameterize World<V, B> and Reflect<V, B> with runtime WorldId instance branding;
                 (2) Replace raw-offset copy_nonoverlapping in el::View::gathered and unsealed Slot/prove authority with sealed safe typed FieldProj<V> accessors preserving sig!(self.field) syntax;
                 (3) Remove unsound raw &[u32] slice casts over padded/arbitrary closures in el::MapRef and reflect;
                 (4) Replace process-global SETS/SHAPE/REL_EPOCH shortcuts with world-local revisions and checked place/incidence bounds;
                 (5) Upgrade World::delta multi-word path from O(all columns) scan to sparse epoch-deduplicated reader-list union over u32 incidence offsets with explicit dense crossover and counters.
implication:     Establishes memory-safe, owner-branded typed core admission and sparse multi-word invalidation in bough-expr/bough-x/bough-ax; does not by itself migrate crates/bough (covered in Card 3).
platforms:       macOS aarch64 (M3 Pro), headless CPU (Soft / Immediate / Spans)
workloads:       bough-x sheet (20 and 10,000 rows, once and redescribe), el_small (5,000 rows), multi-world isolation (2 concurrent worlds), sparse multi-word synthetic matrix (columns C = 64, 256, 1024, 2048; 2 changed words affecting 2 columns; high-incidence > 65,536 reader edges).
gates:           - Pixel & oracle exactness: retained == full redraw == immediate == fresh mount on sheet, sheet_el, el_small, anim, text, table, events, reflect.
                 - Safety & provenance probes (no UB executed): direct gathered with forged/foreign slots refused cleanly; arbitrary safe FieldProj/prove never performs raw OOB/padding reads; MapRef with (bool, u8, u32) padded closure captures passes cleanly without raw word reads; wrong-owner World calls rejected at compile time (compile-fail test) and foreign-WorldId tokens refused at runtime; >4,096 columns or >20-bit record place checked before partial publication; >65,536 reader incidences supported via u32 offsets.
                 - Negative controls: all 6 existing crate mutants (mutant_delta, mutant_journal, mutant_moves, mutant_sweep, mutant_line, mutant_reflect) run individually and fail; plus new mutant feature `mutant_sparse_union` (drops second changed word in sparse union) verified to fail pixel/delta oracle.
thresholds:      - Sparse multi-word work slope: for 2 changed words read by 2 columns total, candidate columns evaluated must stay <= 4 across C = 64..2048 (flat in unrelated C; dense scan mutant visits all C).
                 - Multi-world isolation: mutating World B must cause 0 extra view descriptions and 0 retables in World A (`once` mode).
                 - Compile time (release check of bough-x tests, -j 3): <= 20.0 s incremental.
                 - Retained exposure on 10,000-row sheet_el: < 860 records, < 320 KiB column bytes.
measurement:     Exact operation/column/incidence counters + Cargo test/example runs under exclusive lock when timing; no concurrent builds during timed runs.
budget:          Single isolated worktree, nice -n 15, <= 3 cargo jobs, one cargo at a time.
stop conditions: Stop and report METHOD_BOUNDARY if typed FieldProj accessors break existing sig!(self.field) syntax in sheet_el / el_small or regress steady-state hover/edit column count.
do not revisit:  docs/SETTLED.md (no per-row hover signals, no cx threaded through author code, no public 'ui lifetime brand, no monomorphized recursive layout).
```
