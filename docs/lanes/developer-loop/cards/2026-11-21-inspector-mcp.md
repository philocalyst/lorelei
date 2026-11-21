lane:            developer-loop
state:           CARD
owner:           inspector session (lane/developer-loop/inspector-mcp)
predecessor:     71ee3be0aa; nightly-2026-10-17; dev profile (tests), release/dist for the size line
question:        Can a versioned, serialisable inspector schema over the typed core's reflection, an
                 in-process agent/test socket, an MCP server and a state-preserving dev server be
                 built so that (a) every element, operand, dependency, rect, damage tile, counter and
                 accessibility node a running app has is readable, (b) turns can be injected and
                 recorded/replayed pixel-exactly, (c) mutation goes through typed admission and is
                 refused with a reason otherwise, and (d) none of it costs a byte or an instruction in
                 a binary that does not link it?
change:          New crates `bough-inspect` (schema v1, session, Unix-socket JSON-RPC, record/replay,
                 pixel diff, PNG, typed-selector test client, element-layer overlay), `bough-mcp`
                 (stdio + streamable-HTTP MCP over the socket), `bough-dev` (CLI forked from
                 gpui-ce/cli @ 6774184, polling watcher, MCP endpoint, state-preserving restart) and
                 `bough-inspect-app` (headless host for the real apps). Replaces nothing in the core;
                 reads `bough-x::reflect`, `bough-x::world` and `bough-ax`.
implication:     Passing establishes that the developer loop can be built on the typed core without
                 reopening the core's representation: reflection is sufficient to name, locate, read,
                 select and damage records; typed state fields are sufficient for validated mutation;
                 and the inspector is a separate link-time artefact. It does NOT establish: per-leaf
                 (sub-row) source locations beyond the core's row/scope/list sites; in-process Rust code
                 patching (the hot path refuses and falls back to a state-preserving restart);
                 native accessibility adapters; any performance claim about the inspected app beyond
                 the zero-cost line.
platforms:       macOS (Apple M-series, this host), headless; CPU raster (`bough-x::soft` for the
                 overlay, `bough-span` through `bough-guibench-x::scene` for the real apps).
workloads:       comparisons/bough-guibench-x `Rows` 10,000 and `Table` 1,000 (and `Search` where noted),
                 plus a small element-layer view for overlay behaviour. No competitor comparison.
gates:           G1 schema round-trip + byte-stable golden (mutant_schema must fail).
                 G2 tree completeness: one node per live record, locations, operands, stages, reads,
                 rects; stores/lists/damage/counters/ax present (mutant_drop_site must fail).
                 G3 socket end-to-end against two real apps: hello/tree/find/pick/input/counters/shot/diff.
                 G4 record -> replay pixel-exact and node-exact for both apps (mutant_replay must fail).
                 G5 mutation: accept changed field, refuse unknown/read-only/wrong-kind/foreign/no-op;
                 damaged set == dependent set (mutant_accept must fail).
                 G6 MCP: initialize, tools/list, tools/call over stdio (spawned binary) and POST /mcp.
                 G7 overlay: 11 ported behaviours from reactivity gpui inspector tests.
                 G8 zero cost when off: the off build links no bough_inspect/bough_mcp/bough_dev
                 symbols and its text size is unchanged; hot path instruction counts unchanged.
thresholds:      every listed gate passes on this host; off build: 0 inspector symbols, 0 text-byte
                 delta; no gate may pass with its negative control enabled. No latency threshold is
                 claimed; command timings are reported, not ranked.
measurement:     cargo test (per target), the socket/MCP tests spawn the real app binary and a server
                 process, allocation counting with a test-global counting allocator, `size -m` and
                 `nm` for the size line; all runs logged with load average and the exact command.
budget:          local only; no remote host.
stop conditions: record REJECTED for any gate that cannot pass on this host and explain; METHOD_BOUNDARY
                 for in-process code patching if the dylib/subsecond route is out of scope here.
do not revisit:  style editors (owner decision, REPORT 7.5); `bough::raw`/`rt.rs` as a foundation;
                 reflection's per-row/scope/list granularity (only add, never make finer without a core
                 change); no JSON/Rust style editor in the inspector UI.
