#!/bin/sh
# Build the staged seam benchmark against an archived tree (default: wave3 7991e7bd4e) into its own target.
# Usage: fx-seam-build.sh [ARCHIVE_DIR]   env: TARGET (default targets/solfx/fx-seam-r3)
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$(dirname "$0")/../../.." && pwd)
archive=${1:-$root/runtime/round3/fx-seam-7991e7bd4e}
export CARGO_TARGET_DIR=${TARGET:-$HOME/work/targets/solfx/fx-seam-r3}
export RUSTC_WRAPPER=
if [ ! -d "$archive" ]; then
    mkdir -p "$(dirname "$archive")"
    mkdir "$archive"
    git -C "$root" archive 7991e7bd4e | tar -x -C "$archive"
fi
if [ "${FX_LOCKED:-}" != 1 ]; then FX_LOCKED=1 exec lockf -k "$HOME/work/locks/bench.lock" nice -n 15 "$0" "$@"; fi
# (re-entered under the bench lock so the copied source is the newest one when the build starts)
cp "$here/fx-seam.rs" "$archive/crates/bough-paint/examples/fx_bench.rs"
python3 - "$archive/crates/bough-paint/Cargo.toml" <<'PY'
from pathlib import Path
import sys
p = Path(sys.argv[1]); s = p.read_text()
if 'bough-sys = ' not in s:
    p.write_text(s.replace('[dev-dependencies]\n', '[dev-dependencies]\nbough-sys = { path = "../bough-sys" }\n'))
PY
cd "$archive"
exec cargo build --offline --release -p bough-paint --example fx_bench -j 3
