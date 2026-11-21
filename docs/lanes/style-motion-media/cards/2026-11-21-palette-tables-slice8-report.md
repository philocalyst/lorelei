# Report: resident palette tables read by an expression; a fade on a derived clock word (slice 8, D4)

Card: `2026-11-21-palette-tables-slice8.md` (frozen at 11ac4f42cd; a draft of the `bough-expr` edit existed
uncommitted when it was frozen, nothing was built or measured). Raw output: `bench/results/fx/slice8.txt`,
`slice8-noshaders.txt`, `pbench-slice8.txt`. Dist profile, frame-thread instructions, three repetitions, no
bench lock.

## What was built

* **`bough-expr`** (`crates/bough-expr/src/lib.rs`, the only edit outside this lane's area; four hunks):
  - lines 663 to 665: `Code::Pal(&'static Code)` with its doc comment;
  - lines 668 to 669: the doc comment of `Cx` (one line became two);
  - line 674: field `pub pal: &'a [u32]` in `Cx`;
  - lines 680 to 716: `Cx::of` now also sets `pal: &[]`; new `Cx::at(u, rect)` and `Cx::with_pal(self, pal)`;
    new `pub struct Pal<I>(pub I)` with `unsafe impl Words` and `impl Expr` (`READS = I::READS`, `CODE =
    Code::Pal(I::CODE)`, `eval` = `cx.pal.get(index)` or 0, `until` and `slots` forwarded).
  `Cx::of` is unchanged for callers. No crate outside `bough-paint` builds a `bough_expr::Cx` by literal;
  `cargo check --workspace --all-targets` passes. **One hazard left for the core lane:** `bough-ax`'s
  `eq_words` matches `Code` with a wildcard arm, so it compiles, but it does not descend into `Code::Pal`
  and would miscount record words for an expression that holds a `Rec` under a `Pal`. Not edited (not mine).
* **`bough-paint`**: 16 tables of 256 slots in the scene's arena, ending where the corner tables begin (so
  Metal reads them through the pointer it already has, with no copy and no new buffer); every evaluation
  context carries them (`ColView::pal`); `emit` renders `Code::Pal` (`pal_at(TAB, index)`) and gives every
  function of an archetype that reads a table the table pointer; leaf `x_palix(table, slot)` and
  `expr::PalIx`; `Scene::set_palette`, `Scene::palettes`.
* **The derived clock word**: `Scene::clock(word, start, dur)`. `update` computes the word from `Time`; its
  readers read an ordinary block word, which the host declares broad. `Scene::block()` is the block a frame
  is drawn under (`Scene::frame` and `Mixed::frame` use it; a host driving an executor itself must).
  `Scene::animating()` holds while a clock runs.

Decisions inside the slice: a table *write* re-mounts the lanes (`first`), because `Pm<Pal<Rec>>` reads no
block word and so may be an opaque cover whose alpha lives in the table; choosing a table by a block word is
never a cover (`Pm` over a block read is not opaque by type; `Pmo` is, whatever the table holds).

## Gates

`cargo test --release -p bough-paint -j 8`: lib 2, exact 5, fx 11, review 8, scenes 2, all passed.
`cargo test --release -p bough-expr`: passed. `cargo check --workspace --all-targets`: finished.

```
palette tables: 4 resident tables, 90 steps, 17 fade frames with 0 records visited, 11 switches; 0 failing steps on the CPU, 0 on Metal
palette mutant "the Metal read ignores the table": 78 failing steps
```

| gate | result |
|---|---|
| P1 paletted retained == literal fresh == literal specification (CPU) | 90 steps over table switches, timed fades, hovers that choose between two slots, table rewrites: 0 failing |
| P2 Metal resident == the literal CPU frame | 0 failing |
| P3 mutants | `mutant_pal_stale` (the context sees only the first table): 78 failing steps on the CPU, 23 on Metal; source mutant (the Metal read drops the table): 78 |
| P4 earlier gates; workspace; pbench A1 | pass; builds; 0.945 to 1.017x of baseline, 0 cross-check failures |

## Thresholds (effects with `PALETTE=1`: a paletted chip on every card, three tables; 200 / 1,000 / 5,000)

| | card | measured | verdict |
|---|---|---|---|
| F-a fade frame turn within 1.2x of theme's, flat, 0 records | theme 2,467 | without endless shader paints: 2,474 / 2,474 / 2,474, 0 records. With them: 10,628 / 32,866 / 125,415 and 12 / 62 / 312 records | held for the fade itself; the scene's shader paints are visited on every frame the clock moves (the known item, next card) and a fade frame carries them |
| F-b fade frame on Metal within 1.1x of theme | theme 207 / 208 / 207 K | without shaders 207 / 208 / 208 K; with 218 / 240 / 334 K | held for the fade itself; same caveat |
| F-c table switch == theme | | turn 2,467; Metal 208 / 207 / 207 K; chooser 218 K | held |

0 mismatching words in all 18 runs. Other cells with `PALETTE=1` (K, median; CPU / Metal / chooser):
idle 9.1 / 6.0 / 3.1; theme 987,289 / 207 / 218; hover above glass 6,082 / 190 / 188; scroll 994,994 /
1,633-2,018 / 1,645-2,029 (the chips are one more scrolled column).

## Not verified

Vulkan; a geometry operand that reads a table (allowed by the types; exercised by no gate); `bough-ax` with a
paletted expression; GPU time under the lock.
