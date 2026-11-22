# Where the flat path lives

The work reported in `2026-11-22-rethink-baseline.md` and `2026-11-22-rethink-comparison.md` was reorganised
before it was merged. Those reports use the names it had when it was measured; this is what they are now.

| Then | Now |
| --- | --- |
| `bough_x::damage_engine` | crate `bough-damage` (`rect`, `mask`, `effect`, `blur`, `scroll`, `chooser`, `report`, `engine`) |
| `bough_x::flat` | crate `bough-flat` (`state`, `expr`, `effect`, `store`, `world`, `raster` with `raster/face`, `raster/blur`) |
| `bough-x` feature `flat_mutants`, `tests/flat_*.rs` | `bough-flat` feature `mutants`, `tests/{oracle,alloc,mutants}.rs` |
| `bough::rethink::{FlatApp, ui}`, `bough::rethink::flat` | `bough::flat::{FlatApp, ui}`; the core is re-exported from `bough::flat` |
| `bough::fx_api` | `bough::style` (boxes, gradients, shadows, backdrop blur) and `bough::motion` (specs, the `Motion` bank) |
| `bough::rethink::bridge` | removed; `FlatScene::kind_counts` is the inventory, `bough_inspect::hooks` projects a `DamageReport` |
| `bough-paint` aliases `FlatPaintScene`, `PaintBank` | `FlatScene` |
| `comparisons/bough-fx-bench` bin `rethink-core` | bin `flat-core` (rows `flat_startup`, `flat_idle`, `flat_summary`) |
| `bough` tests `rethink_app`, `rethink_ui`, `fx_pages`, `unified_rethink_e2e` | `flat_app`, `flat_ui`, `pages`, `style_pages_parity` |

Taken out rather than moved:

* The conic-gradient and inset-shadow branches added to the presenter's shaders (`bough-metal` `shader.metal`
  and `oracle.metal`, `bough-fused` `fused.rs`). No record could select them (`deco::gradient_instance` writes
  linear or radial, a shadow instance has no form word) and the kernel executor has no counterpart, so they
  were compiled and never drawn or compared. Conic gradients and inset shadows are drawn, on the CPU and on
  Metal and held to the specification, by the flat bank (`bough-paint` `flat_scene`), which is what
  `bough::style` lowers to.
* `dilate`, `subtract_opaque_rect`, `shift_rect` (`bough-metal` `damage.rs`) and `dilate_px`,
  `clear_opaque_rect` (`bough-core` `damage.rs`): no caller. `bough-damage` does this work.
* `bough::rethink::bridge::sync_flat_scene_to_damage_engine`: it read a scene's untransformed, unclipped
  rects as covers, and nothing but its own test called it.

## The typed core's name

After the move above, the typed core itself was renamed. Dated reports, lane journals, cards, `docs/r8` and
everything under `bench/results` and `bench/baselines` keep the names they were written or measured with.

| Then | Now |
| --- | --- |
| crate `bough-x` (`crates/bough-x`, `bough_x::`) | crate `bough-dataflow` (`crates/bough-dataflow`, `bough_dataflow::`) |
| `comparisons/bough-guibench-x` (`bough_guibench_x::`) | `comparisons/bough-guibench-dataflow` (`bough_guibench_dataflow::`) |
| gui-bench runner framework `bough-x` (`bench/vm`, the registration patch) | `bough-dataflow`; rows named `bough-x` in stored results are the same app |
| features `bough-x/phase_probe`, `bough-x/mutant_*` | `bough-dataflow/phase_probe`, `bough-dataflow/mutant_*` |

The examples keep their names (`sheet_x`, `ops_x`, `anim_x`, `text_x`, `events_x`): they are the cell names of
the native baseline (`bench/baselines/native/reference.json`), and a changed set of names fails its gate.
`bench/baseline/compare.py` reads `-p bough-x` in a stored receipt as `-p bough-dataflow`, and
`bench/baseline/run.py` builds whichever of the two a checkout has, so a baseline taken before the rename
still compares.

## What is an author surface

`bough::prelude` is the only one (`bough::raw` serves the existing scene ports). `bough::flat::ui`,
`bough::style`, `bough::motion` and `bough::pages` are engines: public so their tests and harnesses can hold
them to their oracles, hidden from the documentation, and not to be written against by an application. The
design's style and motion ports (`designs/reactivity/final`) are lowered onto them.
