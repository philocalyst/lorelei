# Slice 12 report — CPU prefix checkpoints

Measured at `e7463cbdbb` on Apple M3 Ultra, macOS 26.7.3. The CPU routes each event tile to the closest later layer that holds the full tile prefix. A changed prefix forwards the box of changed words. The layer also forwards damage from its sampled pyramid. The eager plan retains conservative scheduling before repair results are available.

The six 600-dense one-spring frames dropped from 570 checks (534 unchanged) to **456 checks (420 unchanged)**, with the same 36 changed prefixes. The 3,000-dense counts dropped from 3,348 checks (3,150 unchanged) to 2,622 (2,424 unchanged). Thus the fact about an unchanged checkpoint holds and the improvement misses both the 250 target and the 400 kill line. CPU frame instructions improved from 199.28 M to 185.61 M at 600 dense and from 1,034.41 M to 928.41 M at 3,000 dense. A single remaining prefix check can still repaint a tile between consecutive readers.

`nice -n 15 cargo test --locked --release -p bough-paint -j 8 -- --nocapture` passed lib 2, exact 5, fx 16, review 8 and scenes 2. The exact fixture forwarded a one-byte level-0 change through two prefix tiles even though all recomputed pyramid blocks stayed equal. `--features mutant_prefix_lost` exited 101 and failed `the first prefix's unmagnified change must reach the next prefix`. The earlier CPU/Metal ring-age and independent specification gates passed. The corrected 39-run matrix printed zero mismatches in every run. The six pbench runs had zero cross-check failures, and A1 stayed within 1.03×.

A cross-shaped opacity certificate remains an independent experiment. The dense evidence already shows the remaining first checks still dominate, so measure certified events before adding a production lane. Scroll and shadows are separate costs. No edits outside the paint lane.

## Instruction matrix

Median of three frame medians in K instructions. Each cell lists CPU / Metal / chooser.

| Cell | effects 200 | effects 1000 | effects 5000 |
|---|---:|---:|---:|
| idle | 9.3 / 6.2 / 3.1 | 9.5 / 6.2 / 3.1 | 9.3 / 6.2 / 3.1 |
| one spring | 35,049.6 / 100.7 / 98.4 | 35,056.0 / 101.0 / 97.8 | 35,049.1 / 100.6 / 98.6 |
| all springs | 53,524.5 / 141.1 / 139.8 | 53,525.6 / 141.4 / 138.6 | 53,526.4 / 141.0 / 138.1 |
| scroll 11 px | 992,570.2 / 1,877.1 / 1,882.9 | 992,745.4 / 2,005.5 / 2,019.4 | 992,700.3 / 2,244.8 / 2,259.7 |
| hover above glass | 6,082.2 / 76.0 / 73.1 | 6,082.1 / 75.9 / 72.9 | 6,082.2 / 75.7 / 73.2 |
| hover beneath glass | 41,973.1 / 91.7 / 89.2 | 41,972.9 / 91.9 / 89.1 | 41,973.5 / 91.7 / 89.1 |
| theme | 986,085.2 / 109.6 / 119.4 | 986,083.2 / 109.6 / 119.5 | 986,103.2 / 109.4 / 119.7 |
| glass moves 1 px | 17,779.6 / 126.0 / 123.6 | 17,787.6 / 126.1 / 124.1 | 17,787.0 / 126.3 / 123.5 |

| Cell | effects 600 dense | effects 3000 dense |
|---|---:|---:|
| idle | 9.3 / 6.2 / 3.1 | 9.5 / 6.2 / 3.1 |
| one spring | 185,609.9 / 125.0 / 123.4 | 928,410.2 / 240.5 / 241.1 |
| all springs | 328,292.8 / 279.6 / 276.7 | 1,624,397.9 / 891.0 / 878.6 |
| scroll 11 px | 4,758,974.5 / 5,306.3 / 5,315.4 | 23,751,520.5 / 30,529.5 / 30,542.6 |
| hover above glass | 7,313.6 / 77.1 / 73.6 | 12,165.8 / 76.5 / 78.7 |
| hover beneath glass | 161,166.7 / 101.9 / 93.1 | 209,419.8 / 101.7 / 102.9 |
| theme | 5,453,403.6 / 109.8 / 124.2 | 26,812,403.5 / 112.9 / 132.3 |
| glass moves 1 px | 23,277.8 / 135.8 / 133.6 | 32,038.4 / 156.9 / 155.9 |

| Cell | pane 300 | pane 3000 |
|---|---:|---:|
| idle | 9.5 / 6.2 / 3.1 | 9.4 / 6.2 / 3.1 |
| one spring | 422.4 / 80.2 / 75.9 | 422.3 / 79.1 / 85.4 |
| all springs | 19,324.6 / 119.8 / 120.8 | 19,327.7 / 122.8 / 118.5 |
| hover above glass | 9,448.0 / 76.8 / 76.6 | 9,448.0 / 77.3 / 75.5 |
| hover beneath glass | 32,950.7 / 92.5 / 90.6 | 32,951.2 / 92.5 / 89.6 |
| glass moves 1 px | 16,970.9 / 109.8 / 107.5 | 16,974.8 / 109.8 / 108.3 |

| Cell | media 4 | media 24 | media 48 |
|---|---:|---:|---:|
| idle | 9.2 / 6.0 / 2.9 | 9.1 / 6.0 / 2.9 | 9.1 / 6.0 / 2.9 |
| one video frame | 11,171.6 / 78.5 / 76.0 | 14,368.1 / 79.1 / 80.8 | 14,368.1 / 79.4 / 77.3 |
| video under glass | 23,830.8 / 93.1 / 90.6 | 25,464.7 / 93.6 / 94.6 | 25,463.5 / 94.1 / 90.8 |
| all videos (every 4th) | 9.0 / 6.0 / 2.9 | 9.0 / 6.0 / 2.9 | 9.0 / 6.0 / 2.9 |
| a/v playback (audio clocks) | 9.0 / 6.0 / 2.9 | 188,196.3 / 131.3 / 134.0 | 294,434.9 / 161.5 / 159.5 |
| scroll 11 px | 134.9 / 593.5 / 132.2 | 240,599.0 / 866.7 / 865.5 | 589,661.0 / 1,191.0 / 1,206.7 |
| hover above glass | 28,510.9 / 92.7 / 90.9 | 35,036.8 / 101.5 / 93.5 | 35,038.1 / 108.0 / 96.1 |
| theme | 77,033.0 / 110.4 / 120.0 | 388,718.1 / 113.8 / 129.8 | 579,016.5 / 125.0 / 122.8 |
| all springs | 2,415.3 / 85.0 / 80.6 | 22,984.0 / 129.8 / 124.6 | 30,411.5 / 144.2 / 140.5 |
| glass over playing video | 9,448.0 / 132.5 / 119.9 | 10,549.4 / 125.9 / 135.9 | 10,549.2 / 130.3 / 131.6 |

| Cell | palette 200 | palette 1000 | palette 5000 |
|---|---:|---:|---:|
| idle | 9.3 / 6.2 / 3.1 | 9.5 / 6.2 / 3.1 | 9.4 / 6.2 / 3.1 |
| theme | 987,326.2 / 114.1 / 120.5 | 987,225.7 / 113.4 / 119.6 | 987,253.1 / 109.8 / 119.8 |
| palette switch | 981,504.1 / 111.0 / 120.3 | 981,496.2 / 110.0 / 119.5 | 981,426.1 / 109.7 / 119.8 |
| palette fade | 981,507.1 / 114.5 / 124.7 | 981,533.3 / 114.0 / 124.7 | 981,501.7 / 114.0 / 124.6 |
| hover above glass | 6,082.1 / 76.0 / 73.3 | 6,082.2 / 75.9 / 73.6 | 6,082.2 / 75.7 / 72.9 |
| scroll 11 px | 994,887.1 / 2,012.8 / 2,024.8 | 995,045.5 / 2,152.0 / 2,171.9 | 995,103.6 / 2,399.4 / 2,414.5 |

## Launch frames

The Metal all-springs retarget uses the regular frame counter and occurs during warmup in this trace.

| Scene | Three launch samples in instructions |
|---|---:|
| effects 200 | 3,121,072 / 3,121,331 / 3,118,239 |
| effects 1000 | 18,782,078 / 18,775,251 / 18,779,262 |
| effects 5000 | 97,000,963 / 97,014,077 / 97,019,017 |
| effects 600 dense | 6,217,693 / 6,221,023 / 6,219,119 |
| effects 3000 dense | 31,045,372 / 30,989,992 / 31,021,386 |
| pane 300 | 5,133,271 / 5,175,924 / 5,263,681 |
| pane 3000 | 57,883,456 / 57,886,330 / 57,919,629 |
| media 4 | 109,674 / 101,573 / 123,770 |
| media 24 | 161,996 / 171,724 / 165,103 |
| media 48 | 647,226 / 646,653 / 647,690 |
