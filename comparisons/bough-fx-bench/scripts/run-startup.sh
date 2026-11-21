#!/bin/sh
set -eu
: "${CARGO_TARGET_DIR:?Set the isolated target directory}"
lock=${BOUGH_BENCH_LOCK:-$HOME/work/locks/bench.lock}
exec lockf -k "$lock" sh -c '
    set -eu
    bin=$CARGO_TARGET_DIR/release/bough-fx-bench
    for n in 200 1000 5000; do
        uptime
        i=0
        while [ "$i" -lt 20 ]; do
            "$bin" cold "$n" startup 1
            i=$((i + 1))
        done
        uptime
    done
'
