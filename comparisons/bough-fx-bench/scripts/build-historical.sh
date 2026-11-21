#!/bin/sh
set -eu
: "${CARGO_TARGET_DIR:?Set the isolated target directory}"
export RUSTC_WRAPPER=
root=$(cd "$(dirname "$0")/../../.." && pwd)
copy=$root/runtime/solfx-replay
mkdir -p "$copy/comparisons/bough-workloads"
cp -R "$root/comparisons/bough-workloads/src" "$copy/comparisons/bough-workloads/"
for path in comparisons/bough-guibench-scenes/src/canvas.rs comparisons/gui-bench-scenes/overlay/apps/shared/scenes.rs; do
    mkdir -p "$copy/$(dirname "$path")"
    git -C "$root" show "11bc801858:$path" > "$copy/$path"
done
# The historical crate is excluded from the current workspace. Use the lane's crates as
# direct dependencies of an isolated copy, so those crates still inherit their own workspace.
sed "s|../../crates/|$root/crates/|g" "$root/comparisons/bough-workloads/Cargo.toml" > "$copy/comparisons/bough-workloads/Cargo.toml"
{
    printf '[workspace]\nresolver = "3"\nmembers = ["comparisons/bough-workloads"]\n\n'
    sed -n '7,35p' "$root/Cargo.toml" | sed "s|path = \"vendor/|path = \"$root/vendor/|g"
} > "$copy/Cargo.toml"
cp "$root/Cargo.lock" "$copy/Cargo.lock"
if [ "${SOLFX_CPU_REPLAY:-0}" = 1 ]; then
    # Attach the existing offscreen CPU target used by crates/bough/tests/cpu_target.rs.
    python3 - "$copy/comparisons/bough-workloads/src/harness.rs" <<'PYTHON'
from pathlib import Path
import sys
path = Path(sys.argv[1])
source = path.read_text()
old = "let mut app = app.unwrap();"
assert source.count(old) == 1
source = source.replace(old, old + """
    app.target = Some(Box::new(bough::backend::MemSurfaces::new(
        app.rt.win_w as u32, app.rt.win_h as u32, 4,
    )));""")
source = source.replace("App::new(model, c, cfg.render)", "App::new(model, c, false)")
source = source.replace("let frame = move |app: &mut App<S>| {", """let force_full = std::env::var_os("SOLFX_FULL_REPLAY").is_some();
    let frame = move |app: &mut App<S>| {
        if force_full {
            app.rt.bough.damage_rect(app.rt.bough.window());
        }""")
path.write_text(source)
PYTHON
fi
if [ "${SOLFX_DROP_SHADOWS:-0}" = 1 ]; then
    # Diagnostic control. Its pixels intentionally differ and are excluded from comparison rows.
    sed 's/\.shadows(s)/.shadows(\&[])/; s/\.shadows(\&FRAME_SHADOW)/.shadows(\&[])/' \
        "$copy/comparisons/bough-guibench-scenes/src/canvas.rs" > "$copy/without-shadows.rs"
    mv "$copy/without-shadows.rs" "$copy/comparisons/bough-guibench-scenes/src/canvas.rs"
fi
exec nice -n 15 cargo build --offline --manifest-path "$copy/Cargo.toml" --release -p bough-workloads --bin workload --no-default-features --features gbcanvas,canvas,sheet -j 8
