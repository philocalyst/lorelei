# Shared tile prefix checkpoints for bough-paint

Proposal for `Scene`, `Bins`, and `Layers` at wave3 `7991e7bd4e`. Implements both conditions of D1. Build this in the paint lane. The measurement lane supplies gate fixtures and verifies phase costs.

## Existing contract and failure mode

`Scene::update` already computes windows, paint-order events, and an ordered layer plan. `Bins::build` rebuilds every tile's record list on any geometry move. `Layers::schedule` scans all events at every reader. `Scene::seg` reuses a lower reader's BGRA8 prefix if its ink and prefix halo cover the entire tile. Partial ink makes the BGRA8 restart lossy outside the lower reader. Dense overlapping halos can therefore replay the same lower records several times on the same tile.

## Records, tiles and events

- Give each live record an ordered key with a generation, or retain the current dense `z` until order edits are admitted. Use record handles in memberships and keep paint order independent of a recycled record slot. A key rebase is an explicit epoch change. For a reorder, emit removal and insertion events using both order positions.
- Add to `Bins` editable ordered paint memberships per tile. A per-tile vector is a reasonable first implementation with a bound on in-tile depth. Use sorted chunks or a tree if insertion copies dominate at depth. A record keeps old and new tile ranges from `win` and old/new opaque `cover`. Repair the union of its old/new tiles and update memberships only in the symmetric difference. Re-evaluate the conservative occlusion floor whenever a cover changes. Maintain the uncut memberships for prefix jobs. A full-output cut above a reader cannot truncate that reader's prefix.
- Maintain a second ordered membership per tile for reader halos, including halo-only tiles and partial right/bottom tiles. Update this index on `Layers::regeo`, insertion, removal, radius and clip edits. Store old and new memberships until old damage has been enqueued. GPU buffers need tile-range or page updates keyed by tile generation. The existing full upload by `bins_epoch` can be kept as a fallback for initial mount or a widespread edit.
- A change is `(tile, lowest_changed_key, old/new dirty bounds)` after projecting the window. Merge duplicate reader/tile reach. A reader at key `k` consumes only damage from `key < k`. Queue reached readers in ascending paint order. When a reader's reconstructed output changes, publish damage of its visible ink at its own key to reached higher readers. Add render-only invalidation when its parameters change without changing its prefix. Continue to dilate from exact touched blocks using the existing tent and bilinear supports. Count comparisons, accepted edges and duplicate events to verify work follows reached memberships rather than the product of all events and layers.

## Suggested storage in the first implementation

```rust
struct TileMembers {
    paints: Vec<PaintKey>,             // all contributors before any output cut
    readers: Vec<ReaderId>,            // sorted by owner paint key, halo coverage
    lowest_changed: Option<PaintKey>,  // folded across old and new ink this turn
    output_cover_floor: Option<PaintKey>,
    membership_generation: u32,
}

struct PrefixSlot {
    tile: TileId,
    boundary: PaintKey,    // accumulator immediately before this key
    boundary_generation: u32,
    surface_epoch: u64,
    generation: u32,       // increments on arena slot reuse
    region: LocalRect,      // extent actually stored on a clipped final tile
    ready_stage: Option<StageId>,
    offset: u32,           // index in the exact Q15 arena
}

struct PrefixJob {
    tile: TileId,
    start: Option<SlotKey>,  // includes the expected slot generation
    first: Bound<PaintKey>,  // Included(start.boundary) when resuming
    end: Bound<PaintKey>,    // Excluded(reader.paint_key), or the end of output
    destination: SlotKey,
    produces_level_zero_for: ReaderId,
}
```

Keep `TileMembers` in `Bins`, the exact arena and boundary map next to `Layers::pool`, and the jobs in `Scene::plan`. Resolve the key interval when emitting a job. The corresponding tile membership must stay immutable through execution. Pass emission to Metal the same first/end offsets and slot generations in the frame plan. For the initial pass, publish one predecessor slot per tile per reader cut and permit a full-fold fallback on every lookup miss. The existing layer table still addresses BGRA8 samples for filter reads.

## Exact restart and execution

For dirty `(tile, reader)` write the state immediately **before** the reader's first primitive. It is the literal premultiplied 4×u16 Q15 accumulator, 8,192 bytes per complete 32×32 tile. Keep the existing BGRA8 level 0 and tent pyramid as the sampling contract. A reader's prefix can be quantized into level 0 once the exact accumulator is saved. Resuming at that reader must execute its flatten and paint operations in the ordinary kernel, including their ordinary ink masks. A partly covered tile preserves Q15 outside ink. Storing only BGRA8 across the halo changes the pixels.

Use tagged checkpoint entries containing `(surface_epoch, tile, boundary_key, boundary_generation, valid_subset, ready_stage)`. The validity subset may start as the whole active frame tile. The existing layer store supplies the level windows, and the new checkpoint map maps `(tile, boundary)` to an arena slot. Track a per-tile lowest changed key. A carried entry is usable if its boundary is at or below that key and the covered pixels and boundary are still valid. A freshly produced entry can be used higher in the same plan after its producer completes. To protect direct readers of media or atlas data, project a resource-generation change through all affected record memberships with their paint keys. Surface resize, scale, kernel semantic changes, and untracked resource changes invalidate by epoch. Pool relocation must preserve content or invalidate the moved entries and bump their slot generations.

On each dirty tile, visit requested reader cut points in paint order. Select the highest valid checkpoint at or below the first cut, seek to its boundary in the ordered record list, and append only the interval ending before the requested reader. Publish that checkpoint, then build the reader's pyramid using the completed prefix jobs over its halo. Higher jobs continue from the checkpoint, execute the lower reader using its completed pyramid, and replay only the intervening records. At a given cut this also works if the lower reader's visible ink misses the tile. Advance the cursor past that hole with the normal coverage logic. For final output, use the highest valid checkpoint for that tile and the same interval rules. Preserve the current opaque prepass for any interval whose cover proof applies to its initial state and readers. Fall back to the established full fold on a missing or evicted checkpoint.

Schedule cross-tile read dependencies explicitly. A reader can sample several tiles from its halo, so its output cannot feed any higher prefix until its relevant lower prefix and downsample blocks have finished. CPU workers get barriers between dependent stages. On Metal, dispatch disjoint jobs at the same dependency rank, with synchronization for writes read at a later rank. A tile-local cursor cannot use a neighboring halo tile that is still stale. Record prefix interval entries executed, starts at zero, checkpoints produced/reused, pyramid bytes and dependency stages. For fixed tile damage and a complete chain, record shading should cost the first surviving suffix plus the sum of records between reachable cut points. Pyramid work and reached readers remain proportional to their actual dependencies.

## Capacity

At 640×400, one frame of exact samples costs 2,048,000 bytes with clipped edge storage or 2,129,920 bytes with 260 full padded tiles. A separate full exact frame at every reader can exhaust the budget. Stream transient checkpoints within a frame through tile/rank lifetimes. Retain exact tiles for readers with repeated lower-prefix demand under a byte budget. Share one exact slot when two boundaries fold the same per-tile record interval with no changes, and use the existing BGRA8 restart where its whole-tile flatten certificate applies. Report exact arena live/reserved/peak bytes alongside the existing BGRA8 levels and allocator slack. Include a forced-budget fallback benchmark. Avoid a scaling claim at depths that exceed the measured budget.

## Gates and mutants

1. Lower a translucent record under two partially overlapping backdrops, move it away, then repaint after wrap and re-entry. Compare every retained step with a fresh full frame and an independent spot constraint. Mutants omit old ink and accept an older stale prefix.
2. Move a rounded or clipped reader on a partial window at the frame edge. Mutants quantize the whole Q15 checkpoint to BGRA8 and skip the lower reader at resume. Both must fail exact comparison.
3. Edit a record at the checkpoint boundary. Swap two translucent records, remove one, and recycle its slot. Mutants use the wrong inclusive endpoint, preserve membership order or omit the generation.
4. Change clip, radius, atlas and video generations without moving the sampled record. Mutants omit each invalidation. Inject an edit at each side of a tent halo at every level, including odd frame dimensions.
5. Put a video above a moving spring and below two readers straddling a tile edge. Delay a lower halo producer and confirm CPU single worker, pool and staged Metal all match full replay. A mutant schedules the upper consumer before its lower pyramid.
6. Force slot eviction, frame resize, and device buffer reallocation. A stale ready-stage or old slot-generation mutant must fail. A budget miss must take the full-fold fallback and preserve bytes.
7. Use the independent dense profile and the sparse one-spring profile. Freeze active record count and touched output tiles while increasing disjoint scene size. Then freeze reached tiles while increasing crossed reader depth. Report bin memberships edited, reached edges, records executed per interval, prefix bytes and end-to-end instructions. Include below/above-glass hover and full redraw controls.
