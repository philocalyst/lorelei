#!/bin/sh
# The style-motion-media lane's benchmarks (card 2026-11-21-layers-motion-media-slice1), one command.
# Builds bough-paint's fxbench and pbench in the dist profile and runs every cell under the shared bench
# lock, writing raw output to runtime/fx/. Set CARGO_TARGET_DIR to your own target directory.
#   bench/fx/run.sh [tag]        (tag defaults to the short commit)
set -eu
cd "$(dirname "$0")/../.."
tag="${1:-$(git rev-parse --short HEAD)}"
out=${FX_BENCH_OUT:-runtime/fx}
lock="${BENCH_LOCK:-$HOME/work/locks/bench.lock}"
mkdir -p "$out" "$(dirname "$lock")"
export RUSTC_WRAPPER=
lockf -k "$lock" nice -n 15 cargo build --locked --profile dist -p bough-paint --example fxbench --example pbench -j "${JOBS:-3}"
bin="${CARGO_TARGET_DIR:-target}/dist/examples"
status=0
run_cell() {
  if lockf -k "$lock" "$@"; then
    return 0
  else
    code=$?
    printf '## FAILED: %s (exit %s)\n' "$*" "$code"
    if [ "$status" -eq 0 ]; then
      status=$code
    fi
  fi
}
{
  for a in "effects 300" "effects 3000" "pane 300" "pane 3000" "media 4" "media 24" "media 48"; do
    echo "## fxbench $a @$tag $(uptime)"
    run_cell "$bin/fxbench" $a
  done
  # every card in the frame: depth grows with n (50 glass panels, about six deep). Fewer frames: the control
  # redraws every layer of a deep stack every frame. (3,000 dense, 31 deep, was stopped: seconds per frame.)
  echo "## fxbench effects 600 dense (REPS=6) @$tag $(uptime)"
  REPS=6 run_cell "$bin/fxbench" effects 600 dense
} > "$out/fxbench-$tag.txt" 2>&1
{
  for a in "canvas 4" "anim 600"; do
    echo "## pbench $a @$tag $(uptime)"
    run_cell "$bin/pbench" $a
  done
} > "$out/pbench-$tag.txt" 2>&1
echo "wrote $out/fxbench-$tag.txt $out/pbench-$tag.txt"
exit "$status"
