# Effects benchmark through the staged paint seam

CPU1 at wave3 `7991e7bd4e`. A row enters timing after the four-ring-age retained and full-redraw comparison, fresh mount, independent pixel constraint, and both applicable dropped-input and dropped-damage controls. Startup uses the full-frame specification and independent probes. Idle uses a frozen clock and only the input mutant. The reduced scenes include dense translucent stacks, gradients, two shadows, rectangular clips and built-in shader nodes. The video row adds four backdrops over synthetic NV12.

The staged seam cannot express the shared PNG tile, stroked cubic and even-odd filled paths, rounded clips, alpha masks, isolated subtree filters, or the three registered author shaders. Retained resize requires a resizable scene. The scrolled strip keeps all records mounted. The manifest names each unsupported part and each substitution. [Capability manifest](staged-final/manifest.tsv)

Wall timings include all observed virtual ticks. The count gives output submissions over observations. Idle has 240 observed ticks. The startup wall column uses warm-cache fresh-process child mount timings when available. Instructions and footprint use repeated in-process mounts for all startup rows. Physical footprint is the largest sampled `ri_phys_footprint`. The mean tile count uses the actual cumulative count, including partial edge tiles. The GPU budget and presentation cadence require their respective timing boundaries.

| Cards | Cell | Gate and wall p50/p95/p99 (ms) | Process instructions p50 | Peak footprint (MiB) | Mean output tiles |
|---:|---|---|---:|---:|---:|
| 200 | startup | 31.545/51.466/72.822 (240/240) | 528,762,027 | 30.41 | 260.0 |
| 200 | idle | 0 frames | 4,983 | 22.61 | 0.0 |
| 200 | one | 1.751/2.015/10.813 (238/240) | 34,498,818 | 24.63 | 16.9 |
| 200 | many | 54.548/97.256/122.522 (240/240) | 608,984,596 | 24.92 | 258.7 |
| 200 | hover | 0.846/2.878/10.522 (240/240) | 16,753,130 | 24.61 | 11.2 |
| 200 | theme | 30.405/50.899/64.979 (240/240) | 526,302,370 | 24.72 | 260.0 |
| 200 | scroll | 23.081/56.779/77.794 (240/240) | 318,290,651 | 24.70 | 260.0 |
| 200 | resize | not expressible (requires remount) | — | — | — |
| 200 | video | 7.390/35.532/61.402 (240/240) | 133,547,794 | 24.77 | 90.3 |
| 1,000 | startup | 133.796/135.964/148.159 (240/240) | 2,564,620,719 | 27.42 | 260.0 |
| 1,000 | idle | 0 frames | 5,082 | 21.97 | 0.0 |
| 1,000 | one | 7.911/8.123/8.199 (238/240) | 153,056,149 | 24.41 | 16.9 |
| 1,000 | many | 150.565/153.921/156.924 (240/240) | 2,825,152,985 | 25.67 | 259.8 |
| 1,000 | hover | 4.377/9.159/10.409 (240/240) | 87,059,002 | 25.11 | 11.2 |
| 1,000 | theme | 129.546/133.103/138.013 (240/240) | 2,560,761,255 | 22.13 | 260.0 |
| 1,000 | scroll | 16.643/16.970/17.092 (240/240) | 318,730,301 | 22.19 | 260.0 |
| 1,000 | resize | not expressible (requires remount) | — | — | — |
| 1,000 | video | pending | — | — | — |
| 5,000 | startup | pending | — | — | — |
| 5,000 | idle | pending | — | — | — |
| 5,000 | one | pending | — | — | — |
| 5,000 | many | pending | — | — | — |
| 5,000 | hover | pending | — | — | — |
| 5,000 | theme | pending | — | — | — |
| 5,000 | scroll | pending | — | — | — |
| 5,000 | resize | not expressible (requires remount) | — | — | — |
| 5,000 | video | pending | — | — | — |

[Raw per-tick rows, gate receipts, and mutant output](staged-final)
