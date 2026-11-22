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
