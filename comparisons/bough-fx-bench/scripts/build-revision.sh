#!/bin/sh
set -eu
: "${CARGO_TARGET_DIR:?Set the isolated target directory}"
revision=${1:?Pass a Git revision}
package=$(cd "$(dirname "$0")/.." && pwd)
root=$(cd "$package/../.." && pwd)
oid=$(git -C "$root" rev-parse --verify "$revision^{commit}")
copy=$root/runtime/round2/tree-$oid
if [ ! -d "$copy" ]; then
    git -C "$root" archive --output="$copy.tar" "$oid"
    mkdir "$copy"
    tar -xf "$copy.tar" -C "$copy"
    rm "$copy.tar"
fi
mkdir -p "$copy/comparisons/bough-fx-bench"
cp -R "$package"/. "$copy/comparisons/bough-fx-bench/"
cd "$copy"
export RUSTC_WRAPPER=
export CARGO_TARGET_DIR=$CARGO_TARGET_DIR/round2-$oid
nice -n 15 cargo build --offline --locked --release --features full-effects,typed-core --manifest-path comparisons/bough-fx-bench/Cargo.toml -j 8
