#!/bin/sh
# Instruction matrix for the retained effects lane. Wall and GPU columns are incidental.
set -eu
cd "$(dirname "$0")/../.."
: "${CARGO_TARGET_DIR:?Set the lane target directory}"
fx_tag=${1:-$(git rev-parse --short=10 HEAD)}
fx_out=${FX_MATRIX_OUT:-runtime/fx}
mkdir -p "$fx_out"
export RUSTC_WRAPPER=
nice -n 15 cargo build --locked --profile dist -p bough-paint --example fxbench --example pbench -j 8
fx_bin=$CARGO_TARGET_DIR/dist/examples
fx_status=0
run_cell() {
    fx_mode=$1
    shift
    printf '## %s %s @%s %s\n' "$fx_mode" "$*" "$fx_tag" "$(uptime)"
    if env NOCONTROL=1 "$fx_bin/$fx_mode" "$@"; then
        echo '## exit 0'
    else
        fx_status=$?
        echo "## exit $fx_status"
    fi
    uptime
}
{
    sw_vers
    sysctl -n machdep.cpu.brand_string
    uname -m
    rustc --version
    shasum -a 256 "$fx_bin/fxbench" "$fx_bin/pbench" | awk '{print $1}'
    for fx_rep in 1 2 3; do
        echo "## repetition $fx_rep"
        for fx_n in 200 1000 5000; do run_cell fxbench effects "$fx_n"; done
        for fx_n in 600 3000; do REPS=6 run_cell fxbench effects "$fx_n" dense; done
        for fx_n in 300 3000; do run_cell fxbench pane "$fx_n"; done
        for fx_n in 4 24 48; do run_cell fxbench media "$fx_n"; done
        for fx_n in 200 1000 5000; do PALETTE=1 run_cell fxbench effects "$fx_n"; done
    done
} > "$fx_out/$fx_tag.txt" 2>&1
{
    for fx_rep in 1 2 3; do
        echo "## repetition $fx_rep"
        run_cell pbench canvas 4
        run_cell pbench anim 600
    done
} > "$fx_out/pbench-$fx_tag.txt" 2>&1
exit "$fx_status"
