#!/bin/sh
# The one command a baseline is taken with.
#   bench/baseline/run.sh --name 71ee3be --worktree "$HOME/work/lorelei-wt/baseline"
# Instruction runs are unlocked. With --timing the runner locks each timed child.
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
if command -v brew >/dev/null 2>&1 && prefix=$(brew --prefix rustup 2>/dev/null) && [ -x "$prefix/bin/rustc" ]; then
    export PATH="$prefix/bin:$PATH"
fi
export RUSTC_WRAPPER=
exec python3 "$ROOT/bench/baseline/run.py" "$@"
