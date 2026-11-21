#!/bin/sh
# Integration Card 3: instructions per operation, legacy façade against lowered façade, on the vertical slice.
#   sh bench/card3/cost.sh [out.tsv]     (from the repository root; set CARGO_TARGET_DIR)
# The reported columns are instruction counts, which load does not disturb: no bench lock is taken (the lock is
# for wall time, GPU time and RSS). The load average is printed with the table; repeat the run to see a
# disturbed sample.
set -e
export RUSTC_WRAPPER=
nice -n 15 cargo build --release -p bough --example card3_cost -j "${JOBS:-8}" >/dev/null 2>&1
BIN="${CARGO_TARGET_DIR:-target}/release/examples/card3_cost"
run() { "$@"; }
BOUGH_REPAIR=kernel run "$BIN"
if [ -n "$1" ]; then BOUGH_REPAIR=kernel run "$BIN" tsv > "$1"; fi
