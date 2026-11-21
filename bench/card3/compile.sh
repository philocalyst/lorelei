#!/bin/sh
# Integration Card 3, workload 3: compile cost of the deep heterogeneous view on the lowered façade.
#   sh bench/card3/compile.sh            (from the repository root; set CARGO_TARGET_DIR; uses nice -n 15, -j ${JOBS:-8})
# Prints: clean release build of `bough` (the crate and what it pulls in that is not yet built stays built: only
# `bough` itself and the two examples are cleaned) with the slice and the deep view; then an incremental rebuild
# after editing one literal in the deep view's leaf. Wall seconds, with the load average beside each.
set -e
export RUSTC_WRAPPER=
J=${JOBS:-8}
F=crates/bough/examples/deep_view.rs
load() { sysctl -n vm.loadavg 2>/dev/null || cat /proc/loadavg; }
now() { python3 -c 'import time; print(time.time())'; }
nice -n 15 cargo build --release -p bough --example deep_view --example slice -j "$J" >/dev/null 2>&1   # dependencies built
cargo clean --release -p bough >/dev/null 2>&1
t0=$(now); nice -n 15 cargo build --release -p bough --example deep_view --example slice -j "$J" 2>&1 | tail -1
t1=$(now); echo "clean build of bough + slice + deep view: $(python3 -c "print(round($t1-$t0,2))") s   load: $(load)"
cp "$F" "$F.orig"
trap 'mv "$F.orig" "$F"' EXIT
sed -i.bak 's/row((text("leaf"),/row((text("leaf edited"),/' "$F" && rm -f "$F.bak"
t0=$(now); nice -n 15 cargo build --release -p bough --example deep_view -j "$J" 2>&1 | tail -1
t1=$(now); echo "incremental leaf edit (deep view example): $(python3 -c "print(round($t1-$t0,2))") s   load: $(load)"
echo "deep_view binary bytes: $(wc -c < "${CARGO_TARGET_DIR:-target}/release/examples/deep_view" | tr -d " ")"
"${CARGO_TARGET_DIR:-target}/release/examples/deep_view"
