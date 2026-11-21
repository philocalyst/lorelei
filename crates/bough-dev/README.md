# bough-dev

The developer loop for a lorelei (bough) application: watch, rebuild, reload or restart with typed state
preserved, inspect over a terminal or MCP, and screenshot.

Forked from [`gpui-ce/cli`](https://github.com/gpui-ce/cli) at `6774184` (Apache-2.0). See `NOTICE`.

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

## What is not

- **No in-process Rust code patching.** The upstream CLI compiles changed functions into shared libraries and
  patches a running process through Subsecond's jump table. This fork does not; it does not depend on Subsecond
  and does not write executable memory. A change to `view()` is new code and needs a new binary, which is why the
  restart path exists and is what `run` uses for Rust files. (The owner's brief allows the hot path to refuse as
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
