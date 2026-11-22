#!/bin/sh
# Host-only tests for bench/vm: no VM, no lock, no cargo.
#   bench/vm/test.sh
# 1. every script parses; 2. check.py's exit-status contract; 3. the two runner patches apply, in
# the order sync.sh applies them, to a copy of the gui-bench checkout; 4. lorelei source needs no
# patch (the tree has none for it).
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
REFS=${BENCH_REFS:-$HOME/work/reactivity/ref}
for f in "$HERE"/*.sh; do sh -n "$f"; done
python3 "$HERE/test_check.py"
[ ! -e "$HERE/patches/bough-vk-c_char.patch" ] || { echo "stale c_char patch present" >&2; exit 1; }
if [ -d "$REFS/gui-bench/runner" ]; then
  T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
  mkdir -p "$T/runner" && cp -R "$REFS/gui-bench/runner/src" "$T/runner/src"
  (cd "$T" && patch -p1 --silent < "$HERE/patches/gui-bench-bough.patch" && patch -p1 --silent < "$HERE/patches/gui-bench-timing.patch")
  grep -q 'GB_TIMING' "$T/runner/src/cell.rs" && grep -q '"bough-dataflow"' "$T/runner/src/frameworks.rs"
  echo "patches: apply in order, GB_TIMING and bough registered"
else
  echo "patches: skipped (no gui-bench at $REFS)"
fi
echo "bench/vm tests: ok"
