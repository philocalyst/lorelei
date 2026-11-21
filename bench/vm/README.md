# bench/vm — the Linux aarch64 VM for zortax's gui-bench/fakeland matrix

Zortax's `gui-bench` runner and `fakeland` compositor are Linux/Wayland. This directory creates a
Lima VM (Apple Virtualization, `vz`) that runs them from this Mac, registers lorelei's two apps, and
copies self-describing results back. Verified on macOS 26.7.3, Apple M3 Ultra, lima 2.3.0.

## Jobs

```sh
bench/vm/up.sh                                   # idempotent: lima + Ubuntu guest + rustup toolchains
FRAMEWORKS=bough,bough-x,zgui bench/vm/build.sh   # sync, register, build, stage, print sha256
FRAMEWORKS=bough,bough-x,zgui SCENARIOS=rows,table,search bench/vm/run.sh
```

`run.sh` passes everything it is given to the runner (defaults: `--quick` over rows, spinner, search
and table for `bough,bough-x,zgui`), writes `CONTRACT.txt` beside the cells, and unpacks the result
tree into `bench/results/vm/<stamp>/`. It takes the batch lock. See `../RUNBOOK.md` for how to read
a cell and the validity rules.

## What it installs

Guest: Ubuntu (lima's default template), build-essential, pkg-config, cmake, ninja, **lld** (the
workspace's aarch64 linker flag), `libwayland-dev`, `wayland-protocols`, `libxkbcommon-dev`,
`libvulkan-dev`, **`mesa-vulkan-drivers` (lavapipe, the only Vulkan device)**, EGL/GLES, fontconfig,
freetype, GTK/glib (the GUI apps' platform deps), libdrm/gbm/udev/input/seat, **dbus** (fakeland
starts a private bus), DejaVu fonts. Rust: `nightly-2026-10-17` (lorelei) and `nightly-2026-04-16`
(gui-bench's and fakeland's own `rust-toolchain.toml`).

## Reference checkouts and the gaps found

`BENCH_REFS` defaults to `$HOME/work/reactivity/ref`; `BENCH_EXTRA` to `$HOME/work/ref-extra`.

* `gui-bench` is used as-is from the ref checkout at `13f3cb61687c` (the commit its README pins).
* `fakeland`'s checkout there has `crates/fakeland-scenario/src/target/{mod,position,roi,window}.rs`
  deleted in the working tree (a mid-copy state): it does not build. `sync.sh` prefers a fresh
  upstream clone at `$BENCH_EXTRA/fakeland` (b06eace, then `origin/main`, which also had to be
  fetched because b06eace's `fakeland` refers to trace events that commit does not define).
* `gui-bench/apps/zgui` depends on a sibling checkout `zgui-fakeland-marks` on branch
  `feat/fakeland-marks`; the ref `zgui` has only `main`. Clone it into
  `$BENCH_EXTRA/zgui-fakeland-marks` (`git clone -b feat/fakeland-marks https://github.com/zortax/zgui
  zgui-fakeland-marks`). Without it, build `FRAMEWORKS=bough,bough-x` and the zgui cells are absent.

## Patches (applied to the VM's copies of gui-bench only, never to the repository)

| patch | why |
|---|---|
| `patches/gui-bench-bough.patch` | registers `bough` and `bough-x` in `runner/src/frameworks.rs` and the runner's default list. Both crates build a binary named `bough-bench`, so the registration overrides `Framework::binary` for those two names. |
| `patches/gui-bench-timing.patch` | `GB_TIMING=fixed` (default, the previous behaviour) `\| unthrottled \| vrr` (48-144 Hz) selects fakeland's display timing for every cell; any other value panics, never falls back to fixed. |

lorelei's own source is built exactly as committed. (The former `bough-vk-c_char.patch` is gone:
`crates/bough-vk` now names `core::ffi::c_char`, which is `u8` on aarch64 Linux and `i8` elsewhere.)

## Exit status and validity

`run.sh` exits with the runner's status if it is non-zero, else with `check.py`'s: 0 clean; 2 a
cell has errors or skipped phases, a requested scenario x framework has no cell, or `run.json` is
missing (the runner itself returns 0 for failed cells); 4 (`STRICT_VALIDITY=1` only) some report is
invalid or has no `validity`. `VALIDITY.txt/json` in the result directory list every invalid or
unchecked report; an invalid report is reported, never ranked. `bench/vm/test.sh` runs the host-side
tests.

Environment: `GB_TIMING`, `GB_PIN` (cpu list, default `0-5`), `BENCH_RESULTS` (result root),
`BENCH_GUEST_BASE` (guest directory for sources and targets, so a second user of the VM does not
overwrite the first one's trees).

## Timing contract

Fixed 60 Hz by default (`GB_TIMING=fixed`, recorded in `CONTRACT.txt`; `unthrottled` and `vrr` are separate contracts, never pooled with it). `taskset -c 0-5` pins the
runner and every app it launches. Software rendering only: Vulkan apps on lavapipe, bough's façade
and new core on the CPU into `wl_shm`; `environment.renderer: "pixman"` in a report is the
compositor's renderer. `VK_LOADER_DRIVERS_DISABLE=` is set empty: with it unset, Mesa's device-select
layer disables lavapipe and wgpu (zgui) sees no adapter. An interactive cell whose `validity.valid` is
false (compositor wake lateness above 2 ms) is reported, never ranked. VM numbers are never compared
with native macOS numbers.

## Guest layout

`~/src/{lorelei,gui-bench,fakeland,zgui-fakeland-marks}`, built into `~/targets/lorelei-release`
(lorelei) and each app's own `target`; lorelei's binaries are staged under
`~/src/gui-bench/apps/{bough,bough-x}/target/release/bough-bench`. Source arrives over lima's
read-only home mount by `rsync`; results return by a `tar` stream.
