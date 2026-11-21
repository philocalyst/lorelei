#!/bin/sh
# Run the gui-bench matrix in the VM and copy the result directory back.
#   bench/vm/run.sh [runner arguments...]
# Defaults: --quick, rows/spinner/search/table at their smallest sizes, frameworks bough,bough-x,zgui.
# Every argument after the script name goes to the runner unchanged.
# Output: $BENCH_RESULTS/<stamp>/ (default bench/results/vm/; run.json, per-cell JSON, screenshots,
# CONTRACT.txt, VALIDITY.txt/json).
# Environment: GB_TIMING=fixed|unthrottled|vrr (default fixed), GB_PIN=<cpu list> (default 0-5),
# STRICT_VALIDITY=1 (exit 4 when any report is invalid or unchecked).
# Exit status: the runner's if non-zero, else check.py's (2 = cell errors / missing cells / no
# run.json, 4 = strict validity). The runner itself returns 0 on cell errors, so the status of the
# runner alone is not evidence of a good run.
set -eu
. "$(dirname "$0")/lib.sh"
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
OUT=results/vm-$STAMP
FRAMEWORKS=${FRAMEWORKS:-bough,bough-x,zgui}
SCENARIOS=${SCENARIOS:-rows,spinner,search,table}
GB_TIMING=${GB_TIMING:-fixed}
GB_PIN=${GB_PIN:-0-5}
case "$GB_TIMING" in fixed|unthrottled|vrr) ;; *) echo "run: GB_TIMING=$GB_TIMING: expected fixed, unthrottled or vrr" >&2; exit 2 ;; esac
export GB_TIMING GB_PIN
RESULTS=${BENCH_RESULTS:-$ROOT/bench/results/vm}

if [ "$#" -eq 0 ]; then
  set -- --quick --frameworks "$FRAMEWORKS" --scenarios "$SCENARIOS" --out "$OUT"
else
  # An explicit runner argument list still gets the selected frameworks and scenarios unless it
  # names its own, so a one-cell invocation cannot silently launch every registered app.
  case " $* " in *" --frameworks"*) ;; *) set -- "$@" --frameworks "$FRAMEWORKS" ;; esac
  case " $* " in *" --scenarios"*) ;; *) set -- "$@" --scenarios "$SCENARIOS" ;; esac
  set -- --out "$OUT" "$@"
fi

# The contract of every number this run produces, written beside the results.
CONTRACT="$RESULTS/$STAMP"
mkdir -p "$CONTRACT"
{
  printf 'run %s\n' "$STAMP"
  printf 'guest %s\n' "$(guest 'uname -srm')"
  printf 'guest-cpus %s\n' "$(guest 'nproc')"
  printf 'guest-memory %s\n' "$(guest 'free -h | sed -n 2p')"
  printf 'vm %s\n' "$(limactl list --format '{{.Name}}|{{.VMType}}|{{.Arch}}|{{.CPUs}}cpus|{{.Memory}}' | grep -E "^$VM\\|" | head -1)"
  printf 'host-macos %s\n' "$(sw_vers -productVersion 2>/dev/null || echo unknown)"
  printf 'host-cpu %s\n' "$(sysctl -n machdep.cpu.brand_string)"
  printf 'toolchain-lorelei %s\n' "$(guest "cd $LOR && rustc -V")"
  printf 'toolchain-gui-bench nightly-2026-04-16 (its rust-toolchain.toml)\n'
  printf 'renderer lavapipe (Mesa software Vulkan; VK_LOADER_DRIVERS_DISABLE is emptied so Mesa device-select does not hide it); bough CPU executor into wl_shm\n'
  printf 'timing %s (GB_TIMING; fixed = fakeland fixed rate of each cell, 60 Hz by default)\n' "$GB_TIMING"
  printf 'pinning taskset -c %s for the runner and every app it launches\n' "$GB_PIN"
  printf 'lorelei-head %s%s\n' "$(git -C "$WT" rev-parse HEAD)" "$(git -C "$WT" status --porcelain -- crates bench/vm | head -1 | sed 's/.*/ (dirty)/')"
  printf 'guest-base %s\n' "$GB"
  printf 'runner-args %s\n' "$*"
  printf 'load-before %s\n' "$(guest 'cat /proc/loadavg')"
  printf 'icd %s\n' "$(guest 'ls /usr/share/vulkan/icd.d/ | tr "\n" " "')"
  printf 'runner-sha256 %s\n' "$(guest "sha256sum $GUI/target/release/gui-bench | cut -d' ' -f1")"
  printf 'app-bough-sha256 %s\n' "$(guest "sha256sum $GUI/apps/bough/target/release/bough-bench 2>/dev/null | cut -d' ' -f1")"
  printf 'app-bough-x-sha256 %s\n' "$(guest "sha256sum $GUI/apps/bough-x/target/release/bough-bench 2>/dev/null | cut -d' ' -f1")"
  printf 'app-zgui-sha256 %s\n' "$(guest "sha256sum $GUI/apps/zgui/target/release/zgui-bench 2>/dev/null | cut -d' ' -f1")"
} > "$CONTRACT/CONTRACT.txt"

echo "run: $OUT"
# The batch lock: no other timed run overlaps this one on the host.
LOCK=${BENCH_LOCK:-$HOME/work/locks/bench.lock}
mkdir -p "$(dirname "$LOCK")" && touch "$LOCK"
set +e
lockf -k "$LOCK" "$ROOT/bench/vm/guest-run.sh" "$@"
RC=$?
set -e
printf 'runner-exit %s\n' "$RC" >> "$CONTRACT/CONTRACT.txt"
printf 'load-after %s\n' "$(guest 'cat /proc/loadavg')" >> "$CONTRACT/CONTRACT.txt"

# Copy the guest's result directory back through a tar stream (the home mount is read-only). The
# contract file is already in place, so the cell files are unpacked beside it.
archive=$(mktemp "$CONTRACT/transfer.XXXXXX")
trap 'rm -f "$archive"' EXIT
if ! guest "cd $GUI/$OUT && tar -cf - ." > "$archive" || ! tar -C "$CONTRACT" -xf "$archive"; then
  echo "run: could not copy the guest results" >&2
  [ "$RC" -eq 0 ] || exit "$RC"
  exit 2
fi
echo "run: results copied to $CONTRACT (runner exit $RC)"
[ "$RC" -eq 0 ] || echo "run: the runner failed (exit $RC); see $CONTRACT/run.json" >&2

# Audit: the runner exits 0 with failed cells, so read the cells themselves.
FW=$FRAMEWORKS; SC=$SCENARIOS
prev=
for a in "$@"; do
  case "$prev" in --frameworks) FW=$a ;; --scenarios) SC=$a ;; esac
  prev=$a
done
STRICT=
[ "${STRICT_VALIDITY:-0}" = 1 ] && STRICT=--strict-validity
set +e
python3 "$ROOT/bench/vm/check.py" "$CONTRACT" --frameworks "$FW" --scenarios "$SC" $STRICT
CK=$?
set -e
printf 'check-exit %s\n' "$CK" >> "$CONTRACT/CONTRACT.txt"
[ "$RC" -eq 0 ] || exit "$RC"
exit "$CK"
