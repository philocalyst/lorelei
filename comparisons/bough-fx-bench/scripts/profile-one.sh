#!/bin/sh
set -eu
bin_dir=${1:?directory of binaries}
lock=${BOUGH_BENCH_LOCK:-$HOME/work/locks/bench.lock}
exec lockf -k "$lock" sh -eu -c '
    bin_dir=$1
    for n in 200 1000 5000; do
        uptime
        echo "facade mount $n"
        BOUGH_GPU=cpu BOUGH_KERNEL_EXEC=cpu1 BOUGH_KPROF=1 "$bin_dir/bough-fx-bench" profile-mount "$n" one 1
        echo "facade trace $n"
        BOUGH_GPU=cpu BOUGH_KERNEL_EXEC=cpu1 BOUGH_KPROF=1 "$bin_dir/bough-fx-bench" profile "$n" one 20
        echo "core trace $n"
        "$bin_dir/typed-core" profile "$n" one 20
    done
    uptime
' sh "$bin_dir"
