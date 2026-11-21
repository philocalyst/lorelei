# First heavy effects results

State `GATED` for the façade subset and `METHOD_BOUNDARY` for comparative ranking. Apple M3 Ultra, arm64 macOS 26.7.3. The executable was built with `cargo build --offline --locked --release --features full-effects --manifest-path comparisons/bough-fx-bench/Cargo.toml -j 8`. `comparisons/bough-fx-bench/scripts/run.sh` ran each cell under the timing lock. Use the DejaVuSans.ttf from the zgx reference's `comparisons/fonts` through `BOUGH_FONT`. SHA-256, raw process measurements, load, gate hashes and the first failing hover/scroll gates are under `bench/results/effects`.

The façade uses offscreen `MemSurfaces`, rounded bordered boxes, translucent overlap, PNG tiles and interruptible curve transitions. The prelude at the frozen base has no gradient and stacked-shadow modifiers. The existing raw canvas ports retain coverage for those primitives. Scroll places the cards in a clipped strip. Sample counts are 20 operations per cell and one idle wait. The startup row measures mount and first frame in process. Spawn-to-frame is available in the historical driver evidence and is a separate metric. Percentiles are nearest rank. Each p99 is the observed maximum. Instruction counts are all-thread process averages per input or tick, with the sampler floor removed. CPU time includes all process threads. Memory reports the maximum sampled current RSS and physical footprint in MiB. The idle count covers the sleep and measurement loop, with zero submitted frames and zero tile repair. Resize damaged area could not be captured from the public headless diagnostic. GPU timestamps are unavailable for this CPU tier. The samples were taken under concurrent host activity and establish an initial diagnostic baseline.

| Cards | Cell | Instructions / op | Wall ms p50/p95/p99 | CPU ms / op | RSS/footprint MiB | Repair area | Load (1 min) |
|---:|---|---:|---|---:|---|---:|---|
| 200 | startup | 20,288,118 | 0.917/3.267/4.811 | 2.141 | 31.9/23.5 | 100.00% | 20.12–20.12 |
| 200 | idle | 45,149 | 1040.826/1040.826/1040.826 | 0.036 | 16.9/8.5 | 0.00% | 20.12–20.03 |
| 200 | one | 2,407,476 | 0.010/0.390/0.418 | 0.195 | 20.1/11.7 | 1.23% | 20.03–20.03 |
| 200 | many | 16,136,801 | 0.438/0.523/0.627 | 1.121 | 27.0/18.6 | 85.00% | 20.03–20.03 |
| 200 | hover | 3,369,041 | 0.070/0.290/0.309 | 0.212 | 20.1/11.7 | 7.28% | 20.03–20.03 |
| 200 | theme | 14,585,646 | 0.218/0.290/0.333 | 0.930 | 20.0/11.5 | 85.00% | 20.03–20.03 |
| 200 | scroll | 8,195,180 | 0.151/0.223/0.230 | 0.560 | 20.0/11.6 | 85.00% | 20.03–20.03 |
| 200 | resize | 14,974,152 | 0.245/0.335/0.336 | 0.946 | 19.8/11.4 | — | 20.03–20.03 |
| 200 | video | Feature unavailable | — | — | — | — | — |
| 1000 | startup | 88,620,041 | 2.217/2.682/2.993 | 5.471 | 46.8/38.4 | 100.00% | 20.03–20.03 |
| 1000 | idle | 22,189 | 1013.530/1013.530/1013.530 | 0.018 | 20.5/12.2 | 0.00% | 20.03–20.03 |
| 1000 | one | 11,362,747 | 0.042/0.850/0.864 | 0.617 | 23.3/14.9 | 1.23% | 19.63–19.63 |
| 1000 | many | 80,208,954 | 1.716/2.038/2.264 | 4.700 | 44.3/35.9 | 85.00% | 19.63–19.63 |
| 1000 | hover | 16,059,144 | 0.218/0.868/0.895 | 0.921 | 23.1/14.7 | 7.28% | 19.63–19.63 |
| 1000 | theme | 70,594,320 | 0.857/0.908/0.950 | 3.865 | 23.4/15.0 | 85.00% | 19.63–19.63 |
| 1000 | scroll | 8,203,958 | 0.155/0.211/0.215 | 0.562 | 23.8/15.3 | 85.00% | 19.63–19.63 |
| 1000 | resize | 72,079,397 | 0.893/1.040/1.065 | 3.891 | 24.8/16.4 | — | 19.63–19.63 |
| 1000 | video | Feature unavailable | — | — | — | — | — |
| 5000 | startup | 410,964,171 | 8.468/9.076/10.004 | 22.538 | 71.4/63.0 | 100.00% | 19.98–19.98 |
| 5000 | idle | 28,763 | 1104.334/1104.334/1104.334 | 0.021 | 33.7/25.3 | 0.00% | 19.98–19.98 |
| 5000 | one | 54,648,555 | 0.205/3.785/3.958 | 2.930 | 36.1/27.7 | 1.23% | 19.82–19.82 |
| 5000 | many | 392,700,025 | 8.971/10.163/10.832 | 22.677 | 121.6/113.3 | 85.00% | 21.03–21.03 |
| 5000 | hover | 76,742,440 | 0.886/3.676/3.749 | 4.020 | 35.8/27.4 | 7.28% | 21.03–21.03 |
| 5000 | theme | 340,290,961 | 3.929/4.032/4.321 | 17.885 | 35.2/26.8 | 85.00% | 21.51–21.51 |
| 5000 | scroll | 8,212,573 | 0.160/0.218/0.223 | 0.582 | 38.4/30.0 | 85.00% | 21.51–21.51 |
| 5000 | resize | 347,128,006 | 4.228/5.259/5.759 | 19.107 | 37.7/29.3 | — | 23.15–23.15 |
| 5000 | video | Feature unavailable | — | — | — | — | — |

The façade passed 96 checks across the 24 available cells and four ring ages. Each active check compares every sampled frame to full redraw and compares the resting frame to a fresh mount. The color-change mutant failed with `trace never changed pixels`. Before the opacity fix, each hover and scroll cell stopped at fresh-mount equality. `8037225e25` restores opacity when the pointer enters and leaves a card.

The single-moving-card cost misses both locality targets. The instruction average grows 22.7× from 200 to 5,000 cards, with 1.23% repair area at all sizes. The test increases the number of translucent layers inside tiles at fixed dimensions. Every changing tile includes the stack of unchanged contributing cards. Test paint replay below the changed record and measure it against this trace and the existing independent raster fixtures. One moving card reaches p99 3.958 ms at 5,000. The 5,000-card many-motion p95 is 10.163 ms on this CPU tier. The GPU budget remains a different cell. Check section “Contract” of the frozen spec for each target.

## Historical replay

Commands and measured subsets are in `bench/results/effects/{historical.tsv,canvas-cpu1.tsv,guibench.tsv,glyph-capacity.tsv,sheet-abba.tsv}`. The recovered historical canvas view reproduced the scaling loss in the current single-thread CPU path on this host. Removing its shadow work lowered measured instructions by roughly 89% in the four target cells. The current Metal offscreen method saw p50s of 1.113 ms for 1,000 hover, 1.418 ms for 1,000 theme, 2.976 ms for 5,000 theme and 8.762 ms for 5,000 resize. These numbers include a different rendering backend from the Linux scoreboard.

The historical façade sheet mounted 240,055 records at 10,000 rows. It used 2.08 G instructions and 139.7 ms to mount in the damage-only mode at 2× device scale. Hover, theme, mixed edit/select/scroll and resize took 62.1 M, 96.5 M, 43.6 M and 167.0 M instructions. Metal and damage-only invocations completed. Connection reset remains for VM reproduction. The handwritten typed-core `sheet_x` fixture, with prepared Latin-1 text, opaque paints and arithmetic placement, has 764 exposed records and 43,776 bytes of columns. In its release `dist` replay the edit costs 2,902 instructions. Watcher reuse removes 20 instructions and passes 12 release tests. The reported historical reference was 1,433 instructions.

The façade's configured 524,288-slot glyph column reproduced two process failures. Search filter and coarse-table clear/undo needed 524,304 and 524,320 slots. With the existing diagnostic override of 1,048,576 slots, both one-pair replays finished. Four idle reclamation frames also let the filter run at the default cap. Direct typing, scroll and spinner/ticker ticks completed. Compositor timing and Linux window lookup require a VM. The spinner and ticker's 16 ms schedule limits callback frequency to 62.5 Hz in the corresponding presets.

## Typed core diagnostic

`typed-core check` passed 15 cells across four surface ages. This adapter uses the `bough-x` software renderer. It admits opaque rectangles, integer arithmetic placement, theme changes and colour-only transitions. Text, opacity, images, shadows and rounded edges are outside this adapter. The startup line has one observation at each size. The idle line has one sleep observation, zero submitted frames and zero damage. The table shows average process instructions for one, many and theme, and one sample of total startup instructions.

| Cell | 200 | 1,000 | 5,000 |
|---|---:|---:|---:|
| Startup instructions | 5,286,965 | 32,453,244 | 418,381,559 |
| Idle instructions including sleep bracket | 17,607 | 17,749 | 17,606 |
| One moving instructions / tick | 740,665 | 3,048,891 | 14,596,481 |
| Many moving instructions / tick | 5,110,243 | 22,676,863 | 110,506,736 |
| Theme instructions / step | 4,430,043 | 19,281,655 | 93,548,182 |
| Hover | Unavailable | Unavailable | Unavailable |
| Scroll | Unavailable | Unavailable | Unavailable |
| Resize | Unavailable | Unavailable | Unavailable |
| Video | Unavailable | Unavailable | Unavailable |

Wall, CPU, RSS, footprint, tile counts and loads are in `bench/results/effects/typed-core.tsv`.

## Staged paint and reference coverage

The `bough-paint` seam at `64101ac59d` has a CPU staged adapter for glass over synthetic NV12 video, brightness and saturation, four rounded panels, two built-in shader paints and spring motion. The card bodies use translucent solid fills. It includes video generation in each publication step. The other eight staged scene cells, application shader registration, subtree filters, path and mask coverage, compressed video decode and Metal execution remain open. The first two staged sizes passed the independent whole-frame spec at all sampled ages. The 5,000-card independent spec loop was terminated after 4m10s. The retry passed all three sizes. It checks the independent whole-frame specification on the initial frame and compares the retained surfaces against an uncached full redraw from a second scene on every frame. Each video update repaired 90 output tiles, rebuilt 32 prefix tiles and 168 pyramid blocks. A motion-only step repaired 40 output tiles.


| Staged video cards | Instructions / tick | Wall ms p50/p95/p99 | Load around cell |
|---:|---:|---|---|
| 200 | 19,991,395 | 0.720/2.184/2.188 | 17.13 |
| 1,000 | 29,703,172 | 1.064/3.243/3.303 | 17.13 |
| 5,000 | 78,274,339 | 2.811/8.869/8.869 | 17.13–17.79 |

The two pinned reference probes ran a single-moving-card trace over stacked shadows and gradient fills. The zgui timing boundary covers the virtual-clock turn and pump. The zgx timing boundary covers scene flush and GPU submission. Neither boundary includes a per-step GPU completion fence. zgui reported 12 redraw requests after startup and 20 samples. zgx used the same offscreen renderer configuration to check its final retained frame against a full redraw and witnessed pixel changes. All six jobs exited 0. The compiled zgui and zgx probes have different effect coverage and there is no matched cross-framework ranking. The table covers every reference probe that ran. Startup, idle, many, hover, theme, scroll, resize and glass over video remain unavailable for both reference adapters.

| Reference | Cards | Offscreen step wall p50/p95/p99 ms | Load around cell |
|---|---:|---|---|
| zgui, initial single-card probe | 200 | 0.111/1.448/3.599 | 13.72–13.26 |
| zgui, initial single-card probe | 1,000 | 0.214/1.611/11.276 | 13.26 |
| zgui, initial single-card probe | 5,000 | 0.909/3.190/79.989 | 13.26 |
| zgx single-card probe | 200 | 0.661/1.086/1.150 | 13.26 |
| zgx single-card probe | 1,000 | 1.013/1.428/1.509 | 13.26 |
| zgx single-card probe | 5,000 | 2.970/4.821/51.927 | 13.26 |

## Glyph pressure fix

Default-cap 10,000-row filter and clear aborted in both baseline blocks and finished in both candidate blocks. Filter cost 401,075,076 and 400,648,999 calling-thread instructions. Clear cost 416,175,773 and 413,787,493. Load was 13.64. The changed runtime reclaims retired glyph blocks when a label allocation approaches the column's tail. The 2,048-slot regression fixture passed eight removal and remount steps at each age 1–4. The baseline specimen aborted at `glyph_x` capacity 2048, need 2064. The test and the existing CPU ring oracle passed with the change. See `bench/results/effects/glyph-pressure-abba.tsv`.

The next card should bind the staged effects to the author frontend, align one reference trace down to identical assets and submitted frame counts, and test tile-prefix reuse against the measured one-moving-card slope. Reproduce the failing Wayland transport cells in the VM before selecting a host-side fix.

## Fresh-process frontend startup

`comparisons/bough-fx-bench/scripts/run-startup.sh` measured 20 independent process launches per size. The OS page cache was left in its usual state. The process clock includes font loading, scene mount and the first paint. Load was 18.10 during the runs and 17.13 after the final block. The 228.904 ms p99 at 200 cards is one observed maximum. Per-process instruction and memory samples are in `bench/results/effects/spawn-first.tsv`.

| Cards | Spawn to first ms p50/p95/p99 | Average process instructions |
|---:|---|---:|
| 200 | 11.660/15.404/228.904 | 60,793,477 |
| 1,000 | 13.539/14.408/14.443 | 129,971,120 |
| 5,000 | 20.919/22.026/23.516 | 460,055,809 |
