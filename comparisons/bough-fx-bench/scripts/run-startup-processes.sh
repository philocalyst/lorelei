#!/bin/sh
# Capture a fresh process on each iteration with the machine's caches warm.
set -eu
: "${CARGO_TARGET_DIR:?Set the isolated targets directory}"
framework=${1:?Pass facade, core, zgui, or zgx}
output=${2:?Pass output directory}
count=${BOUGH_FX_SAMPLES:-240}
lock=${BOUGH_BENCH_LOCK:-$HOME/work/locks/bench.lock}
if [ "${3:-}" != --locked ]; then
    exec lockf -k "$lock" "$0" "$framework" "$output" --locked
fi
case "$framework" in
    facade) binary=$CARGO_TARGET_DIR/release/bough-fx-bench ;;
    core) binary=$CARGO_TARGET_DIR/release/typed-core ;;
    zgui) binary=$CARGO_TARGET_DIR/release/solfx ;;
    zgx) binary=$CARGO_TARGET_DIR/dist/examples/solfx ;;
    *) exit 2 ;;
esac
mkdir -p "$output"
shasum -a 256 "$binary" | awk '{print $1}' > "$output/$framework.sha256"
for n in 200 1000 5000; do
    log=$output/$framework-$n.out
    status=0
    (
        uptime
        case "$framework" in
            facade|core) "$binary" check "$n" startup 1 ;;
            *) "$binary" "$n" startup 1 check ;;
        esac
    ) > "$log.gate" 2>&1 || status=$?
    printf 'check_exit=%s\n' "$status" > "$log"
    if [ "$status" -ne 0 ]; then
        continue
    fi
    i=0
    uptime >> "$log"
    while [ "$i" -lt "$count" ]; do
        case "$framework" in
            facade) "$binary" cold "$n" startup 1 >> "$log" 2>&1 || status=$? ;;
            core) "$binary" measure "$n" startup 1 >> "$log" 2>&1 || status=$? ;;
            *) "$binary" "$n" startup 1 >> "$log" 2>&1 || status=$? ;;
        esac
        if [ "$status" -ne 0 ]; then break; fi
        i=$((i + 1))
    done
    printf 'measure_exit=%s\n' "$status" >> "$log"
    uptime >> "$log"
done
