#!/bin/sh
# The supplementary adapters have narrower semantics than the author program.
set -eu
: "${CARGO_TARGET_DIR:?Set the isolated target directory}"
lock=${BOUGH_BENCH_LOCK:-$HOME/work/locks/bench.lock}
output=${1:?Pass a directory for logs}
mkdir -p "$output"
resolved=$(cd "$output" && pwd)
exec lockf -k "$lock" sh -c '
    out=$1
    target=$CARGO_TARGET_DIR/release
    for n in 200 1000 5000; do
        for cell in startup idle one many theme hover scroll resize video; do
            uptime
            printf "typed-core %s %s\n" "$n" "$cell"
            if "$target/typed-core" check "$n" "$cell" > "$out/core-$n-$cell.gate" 2>&1; then
                echo check_exit=0
                "$target/typed-core" measure "$n" "$cell"
                echo "measure_exit=$?"
            else
                echo "check_exit=$?"
            fi
        done
        uptime
        printf "staged-video %s\n" "$n"
        if "$target/examples/fx_bench" check "$n"; then
            echo check_exit=0
            "$target/examples/fx_bench" measure "$n"
            echo "measure_exit=$?"
        else
            echo "check_exit=$?"
        fi
    done
    uptime
' sh "$resolved"
