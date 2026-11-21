#!/bin/sh
# Run all author cells. Refuse timing for any cell whose oracle failed.
set -eu
: "${CARGO_TARGET_DIR:?Set the isolated target directory}"
lock=${BOUGH_BENCH_LOCK:-$HOME/work/locks/bench.lock}
samples=${BOUGH_FX_SAMPLES:-240}
bin=$CARGO_TARGET_DIR/release/bough-fx-bench
out=${1:?Pass a directory for logs}
mkdir -p "$out"
shasum -a 256 "$bin" | awk '{print $1}' > "$out/binary.sha256"
uname -sm > "$out/host.txt"
if command -v sw_vers >/dev/null 2>&1; then sw_vers >> "$out/host.txt"; fi
for n in 200 1000 5000; do
    for cell in startup idle one many hover theme scroll resize video; do
        stem=$out/$n-$cell
        if "$bin" check "$n" "$cell" 20 > "$stem.gate" 2>&1; then
            count=$samples
            if [ "$cell" = idle ]; then count=1; fi
            if lockf -k "$lock" sh -c 'uptime; "$@"; r=$?; uptime; exit "$r"' sh \
                    "$bin" measure "$n" "$cell" "$count" > "$stem.csv" 2>&1; then
                printf '%s,%s,measured\n' "$n" "$cell"
            else
                printf '%s,%s,timing_failed,%s\n' "$n" "$cell" "$?"
            fi
        else
            printf '%s,%s,gate_refused_or_failed,%s\n' "$n" "$cell" "$?"
        fi
    done
done
if "$bin" mutant 200 theme 20 > "$out/mutant.gate" 2>&1; then
    echo 'mutant unexpectedly passed' >&2
    exit 1
fi
if ! rg -q 'trace never changed pixels' "$out/mutant.gate"; then
    echo 'mutant failed for an unrelated reason' >&2
    exit 1
fi
