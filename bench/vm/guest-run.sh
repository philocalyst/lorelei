#!/bin/sh
# Runs the gui-bench runner inside the guest. Called by run.sh under the batch lock.
#   bench/vm/guest-run.sh <runner arguments...>
set -eu
. "$(dirname "$0")/lib.sh"
guest "cd $GUI && env -u DISPLAY -u WAYLAND_DISPLAY VK_LOADER_DRIVERS_DISABLE= taskset -c 0-5 ./target/release/gui-bench $*"
