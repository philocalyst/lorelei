#!/bin/sh
set -eu
binary=${1:?Pass the historical CPU workload binary}
lock=${BOUGH_BENCH_LOCK:-$HOME/work/locks/bench.lock}
exec lockf -k "$lock" sh -eu -c '
    bin=$1
    for backend in cpu1 cpun; do
        for off in 1 0 0 1; do
            if [ "$off" = 1 ]; then export BOUGH_SHADOW_CACHE_OFF=1; else unset BOUGH_SHADOW_CACHE_OFF; fi
            for size in .2 1; do
                uptime
                echo "backend=$backend off=$off size=$size"
                BOUGH_GPU=cpu BOUGH_KERNEL_EXEC=$backend RENDER=1 MODE=redescribe SCENE_SCALE=$size REPS=8 PHASES=cold,hover,theme,resize "$bin" gbcanvas
                echo "full backend=$backend off=$off size=$size"
                SOLFX_FULL_REPLAY=1 BOUGH_GPU=cpu BOUGH_KERNEL_EXEC=$backend RENDER=1 MODE=redescribe SCENE_SCALE=$size REPS=6 PHASES=theme "$bin" gbcanvas
            done
        done
    done
    uptime
' sh "$binary"
