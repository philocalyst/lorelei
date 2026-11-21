#!/bin/sh
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(git -C "$here" rev-parse --show-toplevel)
tree=${1:?Pass the path for a new instrumented archive}
mkdir "$tree"
git -C "$root" archive 7991e7bd4e | tar -x -C "$tree"
cd "$tree"
git apply "$here/instrumentation.patch"
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$HOME/work/targets/solfx/round3-profile-replay}
export RUSTC_WRAPPER=
exec lockf -k "${BENCH_LOCK:-$HOME/work/locks/bench.lock}" nice -n 15 \
    cargo build --offline --profile dist -p bough-paint --example fxprof -j 3
