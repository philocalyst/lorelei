#!/bin/sh
# Run the gui-bench matrix in the VM and copy the result directory back.
#   bench/vm/run.sh [runner arguments...]
# Defaults: --quick, rows/spinner/search/table at their smallest sizes, frameworks bough,bough-x,zgui.
# Every argument after the script name goes to the runner unchanged.
# Output: bench/results/vm/<stamp>/ (run.json, per-cell JSON, screenshots, CONTRACT.txt).
set -eu
. "$(dirname "$0")/lib.sh"
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
OUT=results/vm-$STAMP
FRAMEWORKS=${FRAMEWORKS:-bough,bough-x,zgui}
SCENARIOS=${SCENARIOS:-rows,spinner,search,table}

if [ "$#" -eq 0 ]; then
  set -- --quick --frameworks "$FRAMEWORKS" --scenarios "$SCENARIOS" --out "$OUT"
else
  set -- --out "$OUT" "$@"
fi

# The contract of every number this run produces, written beside the results.
CONTRACT="$ROOT/bench/results/vm/$STAMP"
mkdir -p "$CONTRACT"
{
  printf 'run %s\n' "$STAMP"
  printf 'guest %s\n' "$(guest 'uname -srm')"
  printf 'guest-cpus %s\n' "$(guest 'nproc')"
  printf 'guest-memory %s\n' "$(guest 'free -h | sed -n 2p')"
  printf 'vm %s\n' "$(limactl list --format '{{.Name}}|{{.VMTYPE}}|{{.Arch}}|{{.CPUs}}cpus|{{.Memory}}' | grep -E "^$VM\\|" | head -1)"
  printf 'host-macos %s\n' "$(sw_vers -productVersion 2>/dev/null || echo unknown)"
  printf 'host-cpu %s\n' "$(sysctl -n machdep.cpu.brand_string)"
  printf 'toolchain-lorelei %s\n' "$(guest "cd $LOR && rustc -V")"
  printf 'toolchain-gui-bench nightly-2026-04-16 (its rust-toolchain.toml)\n'
  printf 'renderer lavapipe (Mesa software Vulkan; VK_LOADER_DRIVERS_DISABLE is emptied so Mesa device-select does not hide it); bough CPU executor into wl_shm\n'
  printf 'timing fixed 60 Hz (fakeland fake display default); no GB_TIMING patch\n'
  printf 'pinning taskset -c 0-5 for the runner and every app it launches\n'
  printf 'load-before %s\n' "$(guest 'cat /proc/loadavg')"
  printf 'icd %s\n' "$(guest 'ls /usr/share/vulkan/icd.d/ | tr "\n" " "')"
  printf 'runner-sha256 %s\n' "$(guest "sha256sum $GUI/target/release/gui-bench | cut -d' ' -f1")"
  printf 'app-bough-sha256 %s\n' "$(guest "sha256sum $GUI/apps/bough/target/release/bough-bench 2>/dev/null | cut -d' ' -f1")"
  printf 'app-bough-x-sha256 %s\n' "$(guest "sha256sum $GUI/apps/bough-x/target/release/bough-bench 2>/dev/null | cut -d' ' -f1")"
  printf 'app-zgui-sha256 %s\n' "$(guest "sha256sum $GUI/apps/zgui/target/release/zgui-bench 2>/dev/null | cut -d' ' -f1")"
} > "$CONTRACT/CONTRACT.txt"

echo "run: $OUT"
set +e
guest "cd $GUI && env -u DISPLAY -u WAYLAND_DISPLAY VK_LOADER_DRIVERS_DISABLE= taskset -c 0-5 ./target/release/gui-bench $*"
RC=$?
set -e
printf 'runner-exit %s\n' "$RC" >> "$CONTRACT/CONTRACT.txt"
printf 'load-after %s\n' "$(guest 'cat /proc/loadavg')" >> "$CONTRACT/CONTRACT.txt"

# Copy the guest's result directory back through a tar stream (the home mount is read-only). The
# contract file is already in place, so the cell files are unpacked beside it.
guest "cd $GUI/$OUT && tar -cf - ." | tar -C "$CONTRACT" -xf -
echo "run: results copied to bench/results/vm/$STAMP (runner exit $RC)"
[ "$RC" -eq 0 ] || echo "run: the runner reported cell errors; see $STAMP/run.json" >&2
exit "$RC"
