#!/bin/sh
# Copy the source trees into the VM and register lorelei's two apps with the runner.
#   bench/vm/sync.sh
# Reads the host's home through lima's read-only mount, which the guest sees at the same path, so
# no host-only location is written into a committed file. Nothing is built here.
set -eu
. "$(dirname "$0")/lib.sh"

[ -d "$REFS/gui-bench" ] || { echo "sync: no gui-bench at $REFS (set BENCH_REFS)" >&2; exit 2; }
[ -d "$REFS/fakeland" ] || { echo "sync: no fakeland at $REFS (set BENCH_REFS)" >&2; exit 2; }
[ -d "$EXTRA/zgui-fakeland-marks" ] || echo "sync: warning: $EXTRA/zgui-fakeland-marks missing; the zgui app will not build" >&2

# fakeland's checkout under reactivity/ref is missing crates/fakeland-scenario/src/target (a
# mid-copy deletion). The fresh clone in ref-extra is preferred when it is there.
if [ -d "$EXTRA/fakeland/crates/fakeland-scenario/src/target" ]; then FKL=$EXTRA/fakeland; else FKL=$REFS/fakeland; fi

# Build directories are excluded by anchored patterns only: an unanchored `--exclude target` would
# also drop fakeland's crates/fakeland-scenario/src/target module, which is source.
SKIP="--exclude /target/ --exclude /apps/*/target/ --exclude /comparisons/*/target/ --exclude /crates/*/target/ --exclude .git"
guest "mkdir -p $SRC"
guest "rsync -a --delete $SKIP --exclude reactivity --exclude designs --exclude archive '$WT/' $LOR/"
guest "rsync -a --delete $SKIP '$REFS/gui-bench/' $GUI/"
guest "rsync -a --delete $SKIP '$FKL/' $SRC/fakeland/"
if [ -d "$EXTRA/zgui-fakeland-marks" ]; then
  guest "rsync -a --delete $SKIP '$EXTRA/zgui-fakeland-marks/' $SRC/zgui-fakeland-marks/"
fi

# The runner does not know lorelei: register the two rows, and let GB_TIMING choose the display
# timing (fixed, the default and the old behaviour, | unthrottled | vrr). Applied to a freshly synced
# tree every time. lorelei's own source is built as committed: no patch touches it.
guest "cd $GUI && patch -p1 --silent < $LOR/bench/vm/patches/gui-bench-bough.patch && patch -p1 --silent < $LOR/bench/vm/patches/gui-bench-timing.patch"

echo "sync: $LOR and $GUI up to date"
