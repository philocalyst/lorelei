# Lane: developer-loop

State: open. The inspector card is frozen and the first slices are built:
`cards/2026-11-21-inspector-mcp.md` (question, gates G1-G8, negative controls) and its report
`2026-11-21-inspector-report.md`. Backlog: `docs/AVENUES.md`; earlier lane work: binary size
(`attribution.md`, `cards/2026-11-11-binary-size.md`).

Crates this lane owns: `crates/bough-inspect` (schema, session, socket, record/replay, PNG, typed selectors,
element-layer overlay), `crates/bough-mcp` (MCP over stdio and `POST /mcp`), `crates/bough-dev` (the developer
loop, forked from gpui-ce/cli), `crates/bough-inspect-app` (the real comparison apps on a headless host).

Local state goes in the ignored `runtime/lanes/developer-loop/` (`STATE.md`).
