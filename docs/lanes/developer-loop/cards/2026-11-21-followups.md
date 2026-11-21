lane:            developer-loop
state:           CARD
owner:           flashinsp session (lane/developer-loop/inspector-followups)
predecessor:     a8bfa2e12b + independent review `docs/lanes/independent-review/2026-11-21-inspector-replay.md`;
                 nightly-2026-10-17; tests in dev, `--profile dist` for the off-feature size gate,
                 `--release` for the workspace replay.
question:        Can the reviewed inspector close its four accepted gaps: (a) a source location per leaf record
                 and a node identity that survives re-description and slot recycling, (b) the inspector mounted
                 beside a live comparison app and driven by real injected input, (c) an MCP server exercised by a
                 standard MCP client with bounded off-turn work and a fuzzed parser, and (d) an in-process
                 function patch that changes a running view's output without changing its PID, world or state?
change:          Add, in that order:
                 1. `bough_x::el` leaf sites: a `#[track_caller]` location carried by each element leaf constructor,
                    stored cold and mode-gated by `inspect-metadata`; `bough_x::sink::Leaf::at` and
                    `bough_x::reflect::SiteInfo::loc` use it. `Store` gains a mode-gated creation generation;
                    `bough-inspect::schema::Node` gains `id` (call site + store path + row/pos + store generation,
                    never paint index) and every `Prop` gains an operand `id`; both are serialised and read back.
                 2. An overlay host: the inspector UI mounted beside the real `Rows` and `Table` apps behind a
                     keybinding/flag, fed by the session's own snapshots, selection by pointer and key, a jump
                     request, damage flashes and per-turn counters, with the overlay-closed frame byte-identical to
                     a build without the overlay. Port the GPUI rework's `multi_slot_inspector.rs` and
                     `semantic_metadata.rs` behaviours with the mapping stated.
                 3. MCP hardening: a standard MCP client drives initialize -> tools/list -> every tool -> resources
                     over stdio and HTTP; nested schemas stop using a bare `object`; reply serialisation and PNG
                     encoding move to a worker so the turn never waits on a slow reader; random/truncated frames
                     are refused without panic, hang or unbounded memory; a stale socket left by a killed process
                     is reclaimed on start.
                 4. `bough-dev` hot patching: a view entry reached through a hot-call boundary; a source change to
                     its reachable code builds a patch dylib, the running app loads it and re-describes with the
                     same PID, `World` and state. A patch whose state type token differs is refused and the
                     existing state-preserving restart is used.
implication:     Passing establishes that a developer loop can locate a leaf (not only a row), name a node
                 stably, see and drive the inspector beside a real app, be driven by a standard MCP client without
                 stalling the app, and change a running view's code without losing the world. It does NOT
                 establish: item key/generation inside a relation (an insert in the middle still renumbers rows);
                 a full Subsecond-style jump table (only the demo view's hot-call boundary is patched); patch
                 support on any platform but this macOS/aarch64 host; native window hosts; any performance
                 claim beyond the numbers reported.
platforms:       macOS (Apple M-series, this host), headless; CPU raster; no window host, no GPU.
workloads:       comparisons/bough-guibench-x `Rows` 1,000-10,000 and `Table` 1,000; the element-layer overlay; a
                 small element-layer hot-patch demo view; `bough-x` examples `sheet_x`, `events_x`, `anim_x`
                 built `--profile dist` for the off-feature text gate.
gates:           T1 sibling leaves on different source lines report their own file:line:col, asserted against the
                    test's own source text; node ids equal over two descriptions and over a scroll that recycles
                    slots, and distinct for distinct nodes; off-feature `__text` sha256 of `sheet_x`,
                    `events_x`, `anim_x` equal to a8bfa2e12b (both hashes recorded). Negative control: a mutant
                    that reuses one site for every leaf must fail the leaf gate.
                 T2 the overlay closes and the app's frame is byte-identical to the app built without the overlay
                    feature; a real pointer click over the drawn tree selects a node; a key moves selection; the
                    jump returns the selected node's file:line; damage flashes and counters appear; the two GPUI
                    tests' behaviours pass with the mapping named. Negative control: a mutant that makes the
                    closed overlay paint must fail the identity gate.
                 T3 a standard client completes the protocol over stdio and HTTP against the real Rows and Table
                    apps; every advertised tool is called; every input schema validates against its required
                    fields and contains no bare `object`; a client reading one byte per 10 ms does not delay the
                    app's answer to a subsequent request beyond the stated bound; 10,000 random/truncated frames
                    leave the server alive with memory bounded; a stale socket is reclaimed. Negative control:
                    a mutant that blocks the pump on the client write must fail the slow-reader gate.
                 T4 a string literal in the demo view changes on disk; the running process re-describes and shows
                    the new text with the same PID and the same world identity token; a patch with a different
                    state token is refused and the restart path restores fields. Negative control: a patch with
                    the same token but no re-description must not change the text.
thresholds:      every gate above passes on this host; no latency threshold is claimed except the slow-reader
                 bound, stated with the measurement; off-feature `__text` delta is exactly zero.
measurement:     cargo test per target; sha256 of `__text` from `otool -s __TEXT __text` for both builds;
                 PID and world id printed by the test; wall time and CMSG for the slow-reader bound; memory from
                 the fuzz loop's own allocation counter. No bench lock: no reported column is wall time except the
                 slow-reader bound, which is compared against itself (mutant) on the same machine.
budget:          local only; no remote host.
stop conditions: record REJECTED for a gate that cannot pass here and say why; METHOD_BOUNDARY for a patch that
                 the macOS/aarch64 linker refuses, after pasting the exact command and error, with the furthest
                 working stage and its test delivered.
do not revisit:  style editors; `bough::raw`/`rt.rs` as a foundation; item key and generation inside `Rel`; the
                 façade author API; reflection's per-row/scope/list granularity for non-element layers (only add
                 leaf sites where the element layer owns the constructor).

## Reformulation considered (standing direction 2026-11-21)

The reviewed gaps are representation gaps, not constant factors:
- **Location**: asking the sink for a location can only name the row/scope/list call because the sink has no
  per-leaf channel. The reformulation is to make the *element value* carry its construction site (one word,
  cold under the mode) and to give `Leaf` a site event, so the description nowhere derives a leaf's site.
- **Identity**: a paint index is a position; the reformulation is a content hash over the persistent identity
  the core already has (`path,row,pos`), the call site and the store's creation generation, so scrolled slots and
  re-descriptions compare equal without any bookkeeping per turn.
- **Overlay**: a second runtime or a compositor is rejected; the reformulation is to let the overlay be another
  `View` over an owned `Snapshot`, and to hand the host a switch that decides *whether the overlay view is
  described at all*. Then "closed = zero cost" is one branch, not a compositing pass.
- **Slow client**: waiting is the wrong unit. The reformulation is to make the reply a value produced by the
  turn and written by a different thread (one bounded queue), so the turn's cost is independent of the reader.
- **Hot patch**: recompiling and restarting is a fallback, not the answer. The reformulation is to make the
  view's variable text cross one **hot-call boundary** (an atomic function pointer) so new code can be a
  replacement at that boundary, keeping the process and world; everything else stays on the restart path.
