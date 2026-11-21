#!/bin/sh
# Build the apps the matrix launches, inside the VM, and stage them where the runner looks.
#   FRAMEWORKS=bough,bough-x,zgui bench/vm/build.sh
# Every build is release, with the toolchain each tree pins, niced, and into a guest-local target
# directory. Exit status is the first failing build's.
set -eu
. "$(dirname "$0")/lib.sh"
FRAMEWORKS=${FRAMEWORKS:-bough,bough-x,zgui}
JOBS=${JOBS:-3}

case ",$FRAMEWORKS," in *,zgui,*)
  if [ ! -d "$EXTRA/zgui-fakeland-marks" ]; then
    echo 'build: zgui requires the zgui-fakeland-marks checkout' >&2
    exit 2
  fi ;;
esac
"$ROOT/bench/vm/sync.sh"

# lorelei's new-core app (its own crate in the workspace).
# Both crates build a binary named `bough-bench`; stage each one before the next build replaces it
# in the shared target directory.
case ",$FRAMEWORKS," in *,bough-x,*)
  echo "build: bough-guibench-x"
  guest "cd $LOR && export CARGO_TARGET_DIR=$TARGET && nice -n 15 cargo build --release -j $JOBS -p bough-guibench-x"
  guest "mkdir -p $GUI/apps/bough-x/target/release && cp $TARGET/release/bough-bench $GUI/apps/bough-x/target/release/bough-bench" ;;
esac

# lorelei's façade app (its own workspace, its own lock file).
case ",$FRAMEWORKS," in *,bough,*)
  echo "build: bough-guibench"
  guest "cd $LOR && export CARGO_TARGET_DIR=$TARGET && nice -n 15 cargo build --release -j $JOBS --manifest-path comparisons/bough-guibench/Cargo.toml --bins"
  guest "mkdir -p $GUI/apps/bough/target/release && cp $TARGET/release/bough-bench $GUI/apps/bough/target/release/bough-bench" ;;
esac

# The runner, with the registration patch applied by sync.sh.
echo "build: gui-bench runner"
guest "cd $GUI && export CARGO_TARGET_DIR=$GUI/target && nice -n 15 cargo build --release -j $JOBS -p gui-bench"

# zgui's app: its own workspace, path dependencies on the feat/fakeland-marks checkout.
case ",$FRAMEWORKS," in *,zgui,*)
  echo "build: zgui app (may take 10+ minutes first time)"
  guest "cd $GUI/apps/zgui && export CARGO_TARGET_DIR=$GUI/apps/zgui/target && nice -n 15 cargo build --release -j $JOBS" ;;
esac

echo "build: manifest"
guest "cd $GUI && for f in apps/bough/target/release/bough-bench apps/bough-x/target/release/bough-bench apps/zgui/target/release/zgui-bench target/release/gui-bench; do [ -x \$f ] && printf '%-45s %s %s\n' \$f \$(sha256sum \$f | cut -d' ' -f1) \$(stat -c%s \$f); done"
echo "build: done"
