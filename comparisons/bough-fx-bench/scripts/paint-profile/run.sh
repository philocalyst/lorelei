#!/bin/sh
set -eu
here=$(cd "$(dirname "$0")" && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$HOME/work/targets/solfx/round3-profile-replay}
output=${1:?Pass a fresh output directory}
mkdir "$output"
mkdir "$output/profile-results"
bin=$CARGO_TARGET_DIR/dist/examples/fxprof
lock=${BENCH_LOCK:-$HOME/work/locks/bench.lock}
shasum -a 256 "$bin" > "$output/binary.sha256"
run() {
    path=$output/profile-results/$1
    shift
    rc=0
    (uptime; lockf -k "$lock" nice -n 15 perl -e 'alarm shift; exec @ARGV' "${LIMIT:-3000}" "$bin" "$@") > "$path" 2>&1 || rc=$?
    printf 'exit %s\n' "$rc" >> "$path"
    uptime >> "$path"
}
for n in 300 600 3000; do
    REPS=12 CELLS=idle,spring,springs,above,beneath,glass run "pane-$n.txt" pane "$n"
    REPS=12 CELLS=idle,spring,above,beneath,glass run "effects-$n.txt" effects "$n"
done
REPS=6 WARM=8 CELLS=idle,spring,above,beneath,glass run dense-600.txt effects 600 dense
NOSHADER=1 REPS=6 WARM=8 CELLS=idle,spring,above,beneath,glass run dense-600-noshader.txt effects 600 dense
for n in 1000 1500 2000; do
    REPS=2 WARM=3 CELLS=spring,above LIMIT=1500 run "dense-$n.txt" effects "$n" dense
    NOSHADER=1 REPS=2 WARM=3 CELLS=spring,above LIMIT=1500 run "dense-$n-noshader.txt" effects "$n" dense
done
REPS=2 WARM=2 CELLS=spring LIMIT=3400 run dense-3000.txt effects 3000 dense
NOSHADER=1 REPS=2 WARM=2 CELLS=spring LIMIT=3400 run dense-3000-noshader.txt effects 3000 dense
python3 "$here/table.py" "$output"
