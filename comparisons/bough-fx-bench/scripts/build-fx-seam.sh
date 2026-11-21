#!/bin/sh
set -eu
: "${CARGO_TARGET_DIR:?Set the isolated target directory}"
package=$(cd "$(dirname "$0")/.." && pwd)
root=$(cd "$package/../.." && pwd)
revision=64101ac59d
copy=$root/runtime/solfx-fx-$revision
if [ ! -d "$copy" ]; then
    git -C "$root" archive --output="$copy.tar" "$revision"
    mkdir "$copy"
    tar -xf "$copy.tar" -C "$copy"
    rm "$copy.tar"
fi
cp "$package/references/fx-seam.rs" "$copy/crates/bough-paint/examples/fx_bench.rs"
python3 - "$copy/crates/bough-paint/Cargo.toml" <<'PY'
from pathlib import Path
import sys
p = Path(sys.argv[1])
s = p.read_text()
if 'bough-sys = ' not in s:
    p.write_text(s.replace('[dev-dependencies]\n', '[dev-dependencies]\nbough-sys = { path = "../bough-sys" }\n'))
PY
cd "$copy"
export RUSTC_WRAPPER=
exec nice -n 15 cargo build --offline --release -p bough-paint --example fx_bench -j 8
