#!/bin/sh
set -eu
bin=$1
lock=${BOUGH_BENCH_LOCK:-$HOME/work/locks/bench.lock}
exec lockf -k "$lock" sh -c '
    bin=$1
    for size in 200 1000 5000; do
        for cell in startup idle one many hover theme scroll resize video; do
            uptime
            printf "typed-core %s %s\n" "$size" "$cell"
            "$bin" check "$size" "$cell" 40
            rc=$?
            echo "check_exit=$rc"
            if [ "$rc" -eq 0 ]; then
                "$bin" measure "$size" "$cell" 240
                echo "measure_exit=$?"
            fi
        done
    done
    uptime
' sh "$bin"
