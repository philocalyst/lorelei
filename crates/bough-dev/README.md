# bough-dev

The developer loop for a lorelei (bough) application: watch, rebuild, reload or restart with typed state
preserved, inspect over a terminal or MCP, and screenshot.

This implementation follows the CLI architecture from GPUI-CE at `6774184` (Apache-2.0).
See `NOTICE` for the upstream repository and commit.

## What is real

- **Watch.** A content scan of the given roots (defaults: `crates/bough-inspect-app/src`,
  `comparisons/bough-guibench-dataflow/src`, `crates/bough-inspect/src`), every 250 ms. Changes are classified: Rust
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

- **In-process view patching.** `--features hot-view` enables a trusted local text-function boundary. Cargo
  compiles the changed source to a `cdylib` exporting `hot_text` and an immutable `HOT_STATE_TOKEN`. The demo
  requires an explicitly configured private `--patch-dir`. The app loads the new function and re-describes the
  same `World`. The test edits one file twice, then changes the fixture's actual field type and verifies that
  `patch_or_restart` restores the non-default registered field after a refusal.

## Scope

- The demo tag is derived from its one-field state declaration, size and alignment. A trusted library's load
  can run initializers before admission. The tag checks compatibility and cannot sandbox native code.
- The single text boundary ships independently of general Subsecond patching. The source watcher still takes
  its existing restart path, and the opt-in patch command reports a refusal to its caller.
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
