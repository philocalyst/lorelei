# bough-dev

The developer loop for a lorelei (bough) application: watch, rebuild, reload or restart with typed state
preserved, inspect over a terminal or MCP, and screenshot.

This implementation follows the CLI architecture from GPUI-CE at `6774184` (Apache-2.0).
See `NOTICE` for the upstream repository and commit.

## What is real

- **Watch.** A content scan of the given roots (defaults: `crates/bough-inspect-app/src`,
  `comparisons/bough-guibench-x/src`, `crates/bough-inspect/src`), every 250 ms. Changes are classified: Rust
  source, configuration (manifest/lock/build/shader), asset.
- **Reload (the hot path).** An asset change is sent to the running application as `reload`: the app re-reads its
  own inputs and re-describes; **state is not touched and nothing is rebuilt**. The lab apps have no files of
  their own; their `reload` re-describes from the state they hold.
- **Rebuild and state-preserving restart (the fallback).** A Rust or configuration change runs `cargo build`, then
  snapshots the application's typed state through the inspector's admission, stops it, starts the new binary,
  writes the state back through the same admission, reloads and compares the two descriptions' digests. A failed
  build keeps the old application and its state; the next valid save retries.
- **State files**: `bough-dev state --file f` and `bough-dev restore --file f` expose the same snapshot/restore as
  data, for a restart by hand or for a test.
- **Inspect**: `tree`, `node`, `find`, `pick`, `deps`, `why`, `counters`, `state`, `set`, `click`, `pointer`,
  `key`, `type`, `tick`, `resize`, `reload`, `shot`, `diff`, `stop`. `shot` writes a PNG; `diff` compares the
  current frame to a golden PNG and fails when they differ.
- **MCP**: `bough-dev mcp --socket …` serves MCP over stdio, or `--http ADDR` at `POST /mcp`. `run --http` starts
  it beside the watched application.

- **In-process view patching (the smallest real version).** A view whose variable text crosses the hot-call
  boundary (`bough_dev::hotview`) can be patched without a new process: `bough_dev::patch::build` compiles a
  small `cdylib` exporting `hot_text` and `hot_state_token`, the running application loads it through the
  inspector's `hotpatch` method, and the view is re-described in the same `World`. The demo is
  `cargo run -p bough-dev --bin hotdemo -- --socket /tmp/hot.sock`; `tests/hotpatch.rs` changes a string
  literal on disk and asserts the new text, the same PID and the same world. A patch whose state token differs
  is refused before any of its code runs.

## What is not

- **Not a general function patcher.** There is no Subsecond dependency, no jump table and no executable-memory
  write. Only the one hot-call boundary is patched; any other Rust change still takes the state-preserving
  restart. The source watcher does not yet choose between `hotpatch` and a rebuild by itself, and only
  macOS/aarch64 was exercised. The state token is a compile-time constant the demo and the patch source must
  agree on; it is not derived from the real state type. (The owner's brief allows the hot path to refuse as
  long as the state-preserving fallback exists; this is that boundary, stated.)
- No cross-compilation, Windows, web or mobile.
- The watcher does not follow changes into a local path dependency outside the roots unless that root is passed
  with `--watch`.

## Use

```sh
# start a demo app (headless in the benchmark adapter), watch it and print an MCP address
cargo run -p bough-dev -- run --app target/debug/bough-inspect-app --http 127.0.0.1:0 -- --scenario rows --rows 10000

# in another terminal
cargo run -p bough-dev -- inspect tree --socket /tmp/bough-dev-<pid>-run.sock
cargo run -p bough-dev -- inspect find --kind text --text "Row 00042" --socket …
cargo run -p bough-dev -- inspect set scroll int 1344 --socket …
cargo run -p bough-dev -- inspect shot /tmp/frame.png --socket …
```

A windowed application that embeds the inspector serves the same socket from its own loop
(`bough_inspect::socket::Server::pump` between frames); `--socket-args` (in the library's `AppSpec`) lets a host
name its own flag.
