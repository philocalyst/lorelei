#!/bin/sh
set -eu
: "${CARGO_TARGET_DIR:?Set the isolated target directory}"
output=${1:?Pass output directory}
mkdir -p "$output"
lock=${BOUGH_BENCH_LOCK:-$HOME/work/locks/bench.lock}
if [ "${2:-}" != --locked ]; then
    exec lockf -k "$lock" "$0" "$1" --locked
fi
for framework in zgui zgx; do
    if [ "$framework" = zgui ]; then
        binary=$CARGO_TARGET_DIR/release/solfx
    else
        binary=$CARGO_TARGET_DIR/dist/examples/solfx
    fi
    shasum -a 256 "$binary" | awk '{print $1}' > "$output/$framework.sha256"
    for n in 200 1000 5000; do
        for cell in startup idle one many hover theme scroll resize video; do
            base=$output/$framework-$n-$cell
            sh -c '
                framework=$1 binary=$2 n=$3 cell=$4 base=$5
                uptime
                "$binary" "$n" "$cell" 40 check > "$base.retained" 2>&1
                rc=$?
                echo "check_exit=$rc"
                if [ "$rc" -ne 0 ]; then exit 0; fi
                if [ "$framework" = zgui ] && [ "$cell" != startup ] && [ "$cell" != idle ]; then
                    ZGUI_FULL_DAMAGE=1 "$binary" "$n" "$cell" 40 check > "$base.full" 2>&1
                    rc=$?
                    echo "full_exit=$rc"
                    if [ "$rc" -ne 0 ]; then exit 0; fi
                    if ! rg "^zgui_pixel," "$base.retained" > "$base.retained.hash" || ! rg "^zgui_pixel," "$base.full" > "$base.full.hash"; then
                        echo comparison_exit=2
                        exit 0
                    fi
                    cmp -s "$base.retained.hash" "$base.full.hash"
                    rc=$?
                    echo "comparison_exit=$rc"
                    if [ "$rc" -ne 0 ]; then exit 0; fi
                fi
                "$binary" "$n" "$cell" 240
                echo "measure_exit=$?"
                uptime
            ' sh "$framework" "$binary" "$n" "$cell" "$base" > "$base.out" 2>&1
        done
    done
done
