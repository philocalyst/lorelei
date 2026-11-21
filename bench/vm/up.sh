#!/bin/sh
# Bring up the Linux aarch64 VM the gui-bench matrix runs in. Idempotent.
#   bench/vm/up.sh
# Installs lima with Homebrew if it is missing, then creates and starts the VM under Apple
# Virtualization (vz) if it does not exist. The image is the upstream Ubuntu template; the guest's
# toolchains are installed on first use.
set -eu
. "$(dirname "$0")/lib.sh"

if ! command -v limactl >/dev/null 2>&1; then
  echo "up: installing lima with Homebrew"
  brew install lima
fi
# lima 2.x takes the instance name as an argument; `--filter name=...` is a syntax error there.
if limactl list --format '{{.Name}}' | grep -qx "$VM"; then
  echo "up: $VM exists"
  STATUS=$(limactl list --format '{{.Status}}' "$VM")
  if [ "$STATUS" != "Running" ]; then
    limactl start "$VM"
  fi
else
  cpus=${BENCH_VM_CPUS:-10}
  case "$cpus" in ''|0*|*[!0-9]*) echo 'up: BENCH_VM_CPUS must be a positive integer' >&2; exit 2 ;; esac
  echo "up: creating $VM (Ubuntu, vz, $cpus vCPU, 16 GiB, 80 GiB)"
  limactl start --name="$VM" --vm-type=vz --cpus="$cpus" --memory=16 --disk=80 --tty=false template:ubuntu
fi

# The guest packages every app needs: a C toolchain and lld (the workspace links with lld on
# aarch64), the Wayland client/server headers smithay and the apps build against, Mesa's software
# Vulkan (lavapipe, the only Vulkan device in the VM), EGL/GL for the OpenGL paths, fonts, and the
# usual build helpers. Installed once; apt is idempotent.
guest 'sudo DEBIAN_FRONTEND=noninteractive apt-get update -qq && sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential pkg-config cmake ninja-build git curl ca-certificates python3 python3-venv unzip jq rsync lld libwayland-dev wayland-protocols libxkbcommon-dev libvulkan-dev vulkan-tools mesa-vulkan-drivers libegl1-mesa-dev libgles2-mesa-dev libfontconfig-dev libfreetype-dev libgtk-3-dev libglib2.0-dev libdrm-dev libgbm-dev libudev-dev libinput-dev libseat-dev dbus fonts-dejavu-core 2>&1 | tail -3'

# rustup and the two pinned toolchains: lorelei's, and gui-bench's and fakeland's.
guest 'if ! command -v rustup >/dev/null 2>&1; then curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none; fi'
guest 'rustup toolchain install nightly-2026-10-17 --profile minimal -c rustfmt 2>&1 | tail -2'
guest 'rustup toolchain install nightly-2026-04-16 --profile minimal -c rustfmt 2>&1 | tail -2'

echo "up: $VM ready"
limactl list "$VM"
