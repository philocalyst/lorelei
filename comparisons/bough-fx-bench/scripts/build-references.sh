#!/bin/sh
set -eu
: "${CARGO_TARGET_DIR:?Set the isolated target directory}"
refs=${BOUGH_REF:?Set BOUGH_REF to the directory of read-only references}
pkg=$(cd "$(dirname "$0")/.." && pwd)
root=$(cd "$pkg/../.." && pwd)
copy=$root/runtime/solfx-refs
mkdir -p "$copy"
export RUSTC_WRAPPER=
export CARGO_NET_OFFLINE=${CARGO_NET_OFFLINE:-true}
framework=${1:?Pass zgui or zgx}
case "$framework" in
    zgui) revision=80baa3c7cb0fbbe1c058f10059e076e41c0a1b9a ;;
    zgx) revision=6dba24de4c1b63d9c84d6acd0e40b1ec7c3f8305 ;;
    *) exit 2 ;;
esac
if [ ! -e "$copy/$framework/.git" ]; then
    git clone --quiet --shared "$refs/$framework" "$copy/$framework"
fi
cd "$copy/$framework"
git checkout --quiet --detach "$revision"
case "$framework" in
    zgui)
        cp "$pkg/references/zgui.rs" crates/zgui-bench/src/bin/solfx.rs
        exec nice -n 15 cargo +nightly-2026-10-17 build --locked --release -p zgui-bench --bin solfx -j 8
        ;;
    zgx)
        cp "$pkg/references/zgx.rs" crates/zgx-desktop/examples/solfx.rs
        cp "$pkg/references/ramp.wgsl" crates/zgx-desktop/examples/ramp.wgsl
        exec nice -n 15 cargo +nightly-2026-10-17 build --locked --profile dist -p zgx-desktop --example solfx -j 8
        ;;
esac
