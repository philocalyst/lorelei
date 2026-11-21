#!/bin/sh
# Shared settings for the VM jobs. Sourced by the other scripts; POSIX sh.
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
VM=${BENCH_VM:-benchvm}
# Host reference checkouts (read-only): gui-bench and fakeland by zortax, and the two extra clones.
REFS=${BENCH_REFS:-$HOME/work/reactivity/ref}
EXTRA=${BENCH_EXTRA:-$HOME/work/ref-extra}
# The lorelei checkout the VM is built from. Defaults to this worktree.
WT=${BENCH_WORKTREE:-$ROOT}
# Guest locations and the guest-only target directory (never a host target).
U=$(id -un)
# BENCH_GUEST_BASE moves every guest path (sources, build targets) to another directory, so a second
# user of the same VM never rsync --delete's or rebuilds the first one's trees.
GB=${BENCH_GUEST_BASE:-/home/$U.guest}
SRC=$GB/src
TARGET=$GB/targets/lorelei-release
GUI=$SRC/gui-bench
LOR=$SRC/lorelei

# Run a command in the guest with cargo on PATH and the pinned wrapper cleared.
guest() {
  limactl shell "$VM" -- bash -lc "set -eo pipefail; if [ -f \"\$HOME/.cargo/env\" ]; then source \"\$HOME/.cargo/env\"; fi; export RUSTC_WRAPPER=; $*"
}

# A one-line description of a binary: sha256 and bytes.
digest() {
  guest "sha256sum $1 2>/dev/null | cut -d' ' -f1; stat -c%s $1 2>/dev/null"
}
