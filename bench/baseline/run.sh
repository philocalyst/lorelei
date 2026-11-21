#!/bin/sh
# The one command a baseline is taken with: one lock, one run, fail closed.
#   bench/baseline/run.sh --name 71ee3be --worktree "$HOME/work/lorelei-wt/baseline"
# Take the lock so no two timed runs overlap. Every argument goes to run.py.
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
LOCK=${BENCH_LOCK:-$HOME/work/locks/bench.lock}
mkdir -p "$(dirname "$LOCK")" && touch "$LOCK"
export PATH=/opt/homebrew/opt/rustup/bin:$PATH
export RUSTC_WRAPPER=
exec lockf -k "$LOCK" python3 "$ROOT/bench/baseline/run.py" "$@"
