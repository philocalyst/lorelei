# El surface fuzz test — findings

Branch `lane/reactivity/el-tests`, base `1a090b8b7f`. These findings come from
`crates/bough/tests/el_fuzz.rs` (seeded randomized differential tests of the `bough::prelude` `El`
surface). Each bug below is kept as an `#[ignore = "BUG: …"]` test in that file; run
`cargo test -p bough --test el_fuzz -- --ignored` to see both fail, and
`cargo test -p bough --test el_fuzz` to see the rest of the suite pass.

The fuzz oracle is byte-exact: after every turn, `retained.presented_pixels()` must equal the pixels of a
fresh `App` mounted on the same state, and a repaired frame must equal a from-scratch full redraw
(`reference_pixels()`). Two cases at this base violate the first oracle by one LSB of anti-aliasing.

## 1. A non-integer corner radius anti-aliases one LSB differently after a re-description than on a fresh mount

`#[ignore]` test: `a_fractional_radius_redescribes_pixel_identical`.

- **Input.** A column whose first child is `if kind_flip { text("flip") } else { div().size(60, 14).bg(..) }`
  and whose second child is `div().size(50, 30).bg(rgb(0x885500)).translate_x(0.9).rounded(5.6)`. Mount with
  `kind_flip = true`, then `turn(|s| s.kind_flip = false)`.
- **Expected.** The re-described frame is byte-identical to a fresh mount with `kind_flip = false`
  (the oracle: identity is parent × ordinal × kind, so the sibling kind change remounts only the first child,
  and the translated rounded box is untouched).
- **Actual.** One byte differs: pixel `(196, 60)`, blue channel `0x23` (retained) vs `0x24` (fresh).
- **Cause.** The kind change shortens the first child's height, shifting the rounded box; its anti-aliased
  arc's coverage lands one LSB differently when the layout is recomputed incrementally than when it is laid
  out from scratch. Quantising the radius to an even integer removes the difference, so the fuzz view's
  `radius` is whole points.

## 2. A text whose width changes shifts its siblings, and the shifted glyphs anti-alias away from a fresh mount

`#[ignore]` test: `a_text_change_redescribes_pixel_identical`.

- **Input.** The full fuzz view (a row of six fixed-width texts, three box rows, an `Option`, iterator and
  `Vec` child). After a kind change and a fade write, `text(self.idx)` goes from a one-digit to a three-digit
  value (`idx = 777`).
- **Expected.** The re-described frame is byte-identical to a fresh mount of the same state.
- **Actual.** ~200 bytes differ, all at glyph edges (e.g. `(48, 4)` reads `[fc, fc, fc]` retained vs
  `[ff, ff, ff]` fresh): the integer text's rendered width changes, which shifts the sibling texts, and their
  re-shaped glyphs land one LSB away from the fresh mount's. A `String` text changed to the same rendered
  width does *not* diverge, so it is specifically the width change, not the text port.
- **Consequence for the fuzz loop.** The differential loop therefore keeps text *content* fixed while still
  rendering every text form (`&str`, `String`, integer, `Signal<String>` field, mapped `Signal<String>` and
  mapped `Signal<u32>`); the text ports are exercised by the ignored regression above.

## Not claimed

- Nothing about wall time; the fuzz suite is debug-fast (see the commit message timings).
- One further observation from the fuzz loop (a translated *rounded* box whose paint offset changes repaired
  six bytes away from a full redraw) was not minimised to a deterministic reproduction, so it is reported
  here only as an unminimised instance of the same rounded-corner anti-aliasing family, not as a separate
  gate.
