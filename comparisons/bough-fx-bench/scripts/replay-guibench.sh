#!/bin/sh
set -eu
: "${CARGO_TARGET_DIR:?Set the isolated target directory}"
lock=${BOUGH_BENCH_LOCK:-$HOME/work/locks/bench.lock}
exec lockf -k "$lock" sh -c '
    bin=$CARGO_TARGET_DIR/release/headless
    run() {
        uptime
        printf "case %s\n" "$*"
        "$bin" "$@"
        printf "exit=%s\n" "$?"
    }
    run --scenario search --rows 10000 --op filter --repeats 24
    run --scenario typing --rows 10000 --op typing --repeats 24
    run --scenario rows --rows 10000 --op scroll --repeats 24
    run --scenario table --rows 10000 --variant coarse --op clear --repeats 1
    run --scenario spinner --rows 10000 --op spinner --repeats 24
    run --scenario ticker --rows 10000 --variant coarse --op ticker --repeats 24
    uptime
'
