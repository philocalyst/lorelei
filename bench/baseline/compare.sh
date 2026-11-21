#!/bin/sh
# The regression gate: compare two recorded baselines, fail beyond the stated thresholds.
#   bench/baseline/compare.sh bench/baselines/macos-71ee3be/71ee3be.json bench/baselines/macos-try/try.json
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
exec python3 "$ROOT/bench/baseline/compare.py" "$@"
