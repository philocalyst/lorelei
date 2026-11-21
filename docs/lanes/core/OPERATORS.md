# Relations, operators and bounded deltas in the new core (how to use them, how the façade lowers onto them)

Card: `cards/2026-11-21-incremental-operators.md`. Report: `cards/2026-11-21-incremental-operators-REPORT.md`.
Everything here is `bough-x` (body B). Nothing in `bough::prelude` changed.

## What exists

| piece | where | one line |
|---|---|---|
| `Lane<T>` | `bough-x/src/rel/lane.rs` | a sequence in blocks of 128 under a Fenwick tree: insert and remove move one block; a read after the last read is one compare |
| `Seq<T>` | `bough-x/src/rel/mod.rs` | `Rel` on a lane: `len`, `get`, `edit`, `insert`, `remove`, `push`, and the same `journal` a `List` reads |
| `Table<T>`, `Log`, `Change` | `bough-x/src/rel/mod.rs` | rows under stable ids (slots, reused) with a bounded log of inserts, removes and updates |
| `Filter`, `Sort<K>`, `Group<G, A>`, `Total<A>`, `Join<K>` | `bough-x/src/ops.rs` | each reads a log from its own cursor and keeps an output (ids, pairs or groups in a lane) plus a positional `journal` |
| `Trigrams`, `Substring` | `bough-x/src/index.rs` | a trigram posting index (superset, verified) and the search operator on it |
| `sel`, `near` | `bough-expr/src/lib.rs` | expressions that declare what bounds their change: a key, a reach |
| key index, page boxes | `bough-x/src/store/fast.rs`, `World::delta_fast` | the deltas those declarations admit; built lazily, dropped by any description pass |
| `Tiles::sweep_rects` | `bough-x/src/store.rs` | motion handed to a raster as rects instead of tile bits |

## Rules that hold everywhere

* **No subscriptions.** An operator is a value in the application's state. Whoever edited the table (a handler)
  or whoever is about to settle a turn calls `sync`. A reader that is behind what a log keeps recomputes; that
  recompute is the operator's definition and what its gate compares against.
* **A list reads a journal.** Every operator output has a `journal` (`sink::Journal`): `List { n: op.len(),
  edits: &op.journal, .. }` and the row function reads `op.get(r)` for the id at position `r`. The list
  materialises the rows it exposes and nothing else, as with a `Rel`.
* **Ids are slots.** `Table::insert` returns the id; it names that row until `remove`, then is reused.
* **Predicates and keys are passed at `sync`.** They are not stored (no boxed closures in the operator). A
  changed predicate is `Filter::narrow` (the new one implies the old), or `Filter::refilter` with candidates.

## How the façade's forms lower (for the integration lane; mechanical)

| author writes (façade) | lowers to |
|---|---|
| `each(self.items, row)` over a plain list | `Seq<T>` in the view's state (or `Rel<T>`: the same journal); `List { edits: &seq.journal }` |
| `each(self.items.filter(p), row)` | `Table<T>` + `Filter`; in the turn before settle: `filter.sync(&table, p)`; rows read `table.row(filter.get(r))` |
| `.sort_by_key(k)` | `Sort<K>` reading the table's log, or the filter's (`sort.sync(&filter, \|id\| k(table.row(id)))`) |
| `.group_by(g).sum(v)`, a column total | `Group<G, Sum>` / `Total<Sum>`; a total shown as a number is `total.value()` written to a block word after `sync` |
| a search field over a list | `Substring`: `q.set_needle(&table, \|t\| &t.label, &needle)` in the key handler; `q.sync(..)` after dataset edits |
| `sig!(self.selected).eq(row.id).select(HI, LO)` | `sel::<Selected, _, _, _>(rec(row.id), HI, LO)`: `KEY` is declared by the type, the column is indexed by the world |
| hover-by-id, focus ring, pressed state | the same `sel` over the word that holds the id |
| a paint that follows the pointer within a distance | `near::<R, _, _, _>(rec(centre), inside, outside)` |

`sel` and `near` are only *recognised* when the declaration is certain (`Expr::KEY` / `Expr::NEAR` are `None`
otherwise): the key is one record word that reads nothing of the block, neither branch reads the selecting word,
`outside` does not read the pointer. In every other case the expression still evaluates correctly and its
column takes the page scan.

## Contracts of the fast paths (what they admit, what forces the fallback)

| path | admits | fallback (tested) |
|---|---|---|
| key index | a change of the key's word and of no other word the column reads; no description pass since the index was built | first use after a pass (scan), any other changed word (scan), `World::no_fast` |
| reach | a change of the pointer words only | any other changed word; without boxes the centre test still bounds the evaluation |
| `Filter::refilter` candidates | the output is up to date with the table (`sync` was called) | otherwise the candidates are ignored and the output is recomputed |
| `Log` replay | at most `cap` (1,024) changes since the reader's cursor | recompute |
| `Substring` index | needles of three bytes or more | narrowing the current hits, a kept result, or a scan |
| `Lane` cursor | a read inside the block last located, with no edit since | Fenwick descent |

## Not done here

* A list still re-describes the exposed rows from an insert on (they moved). A slot remap that makes the exposed
  part a rigid move belongs with item identity (REPORT 4.3).
* `Arch::KEYED` / `NEAR` are forwarded by `Fill` only. `Num`, `Label`, `Line` and `Outline` colours need the
  key's word offset within the record (`offset_of!`): three lines each.
* Active sets for bounded transitions (non-endless timed columns): needs page summaries with a hook where
  records are written.
* Posting lists are `u32`; delta-coded blocks would bring the index near one byte per posting on dense lists.
