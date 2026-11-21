#!/bin/sh
# Build, verify and measure the full native matrix, then compare with its committed reference.
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
reference=${BENCH_REFERENCE:-$root/bench/baselines/native/reference.json}
limits=${BENCH_THRESHOLDS:-$root/bench/baselines/native/limits.json}
out=${BENCH_GATE_OUT:-}
source=${BENCH_WORKTREE:-$root}
if [ "$#" -ne 0 ]; then
    echo 'usage: bench/baseline/gate.sh (uses BOUGH_FONT and optional BENCH_REFERENCE, BENCH_THRESHOLDS, BENCH_WORKTREE, BENCH_GATE_OUT)' >&2
    exit 2
fi
if [ ! -f "$reference" ] || [ ! -f "$limits" ]; then
    echo 'native gate: the reference and thresholds files are required' >&2
    exit 2
fi
if [ -z "$out" ]; then
    mkdir -p "$root/runtime/baseline"
    out=$(mktemp -d "$root/runtime/baseline/integration.XXXXXX")
fi
"$root/bench/baseline/run.sh" --worktree "$source" --name integration --out "$out" --reps 3 \
    --expected-inputs-from "$reference"
exec "$root/bench/baseline/compare.sh" "$reference" "$out/integration.json" \
    --thresholds "$limits" --require-same-cells
