# Slice 11 report — resident Metal ranks

Measured at `4d0acaa3e0` on Apple M3 Ultra, macOS 26.7.3. `Gpu::resident` seeds the first possible rank from the event tiles and forced slots. Frames with no seed skip all layer dispatches. Other frames submit the retained suffix through a Metal compute indirect command buffer with explicit barriers. The serial suffix remains available on devices that refuse this path. Layer argument resets run on the GPU. The surface tile count costs a constant-time read. Slot changes and buffer replacements invalidate the command cache.

The resident test exercised 116 drawn frames at surface ages 1–4, with zero failures and 788 frame-thread submissions. The suffix test drew four above-glass frames with three output/reset dispatches each. The missing-seed mutant exited 101 and introduced stale pixels at step 2. The cached-command fixture drew five exact frames across four slot reassignments. The stale-command mutant exited 101 and changed 1,176 words on step 1. Reproduce with `nice -n 15 cargo test --locked --release -p bough-paint --test fx -j 8 -- --nocapture` and the `mutant_rank_seed` and `mutant_rank_stale` features.

Dense Metal one spring improved from 352.8 / 1,132.2 K to **125.5 / 239.3 K**. The ratio is **1.91×** and misses 1.2×. The same command submission serves both sizes. The cell still visits 38 / 188 records during the turn and uploads their damage. At 3,000 dense the fixture has 187 endless shader paints as well as the spring. A change above glass measured **78.1 K** at 3,000 dense with no prefix work, meeting the 120 K limit. A dense scroll still rewrites windows and lists, including command-cache rebuilds when topology changes.

`bench/fx/matrix.sh` replayed the pinned binary with explicit per-process options. The first run leaked `PALETTE` and `REPS` into later configurations. That run is excluded. All 39 corrected matrix configurations printed `mismatching words in total: 0`. All six pbench runs printed `cross-check failures: 0`. A1 stayed within 1.03×. GPU-duration and wall columns were incidental during the instruction runs. No edits outside the paint lane.

## Instruction matrix

Median of three frame medians in K instructions. Each cell lists CPU / Metal / chooser.

| Cell | effects 200 | effects 1000 | effects 5000 |
|---|---:|---:|---:|
| idle | 9.2 / 6.2 / 3.1 | 9.3 / 6.2 / 3.1 | 9.3 / 6.2 / 3.1 |
| one spring | 35,048.1 / 101.6 / 99.5 | 35,047.7 / 100.8 / 98.4 | 35,048.1 / 100.6 / 99.2 |
| all springs | 53,534.5 / 142.0 / 141.6 | 53,524.4 / 142.0 / 139.7 | 53,524.2 / 141.9 / 138.6 |
| scroll 11 px | 992,395.2 / 1,887.1 / 1,882.0 | 992,586.9 / 2,005.3 / 2,014.7 | 992,645.9 / 2,260.7 / 2,264.0 |
| hover above glass | 6,082.1 / 76.2 / 73.2 | 6,082.0 / 76.1 / 73.1 | 6,082.1 / 76.2 / 73.4 |
| hover beneath glass | 41,966.2 / 92.5 / 89.5 | 41,965.8 / 92.0 / 89.3 | 41,965.1 / 92.5 / 89.6 |
| theme | 986,124.6 / 110.0 / 120.2 | 986,074.1 / 110.1 / 119.7 | 986,090.7 / 109.6 / 120.2 |
| glass moves 1 px | 17,788.0 / 126.7 / 126.5 | 17,793.2 / 126.1 / 123.2 | 17,795.4 / 126.6 / 123.7 |

| Cell | effects 600 dense | effects 3000 dense |
|---|---:|---:|
| idle | 9.2 / 6.2 / 3.1 | 9.2 / 6.2 / 3.1 |
| one spring | 199,275.5 / 125.5 / 124.3 | 1,034,405.4 / 239.3 / 241.0 |
| all springs | 341,968.2 / 277.7 / 276.5 | 1,707,252.9 / 882.2 / 881.1 |
| scroll 11 px | 4,769,426.6 / 5,304.4 / 5,313.7 | 24,212,527.4 / 30,534.3 / 30,544.6 |
| hover above glass | 7,313.6 / 78.0 / 74.7 | 12,165.7 / 78.1 / 75.5 |
| hover beneath glass | 240,872.0 / 91.9 / 90.1 | 1,049,874.9 / 92.5 / 91.8 |
| theme | 5,453,344.8 / 110.0 / 119.9 | 26,812,674.0 / 111.5 / 121.6 |
| glass moves 1 px | 85,475.0 / 133.5 / 133.8 | 367,006.3 / 157.9 / 159.3 |

| Cell | pane 300 | pane 3000 |
|---|---:|---:|
| idle | 9.2 / 6.2 / 3.1 | 9.4 / 6.2 / 3.1 |
| one spring | 422.3 / 79.5 / 77.2 | 422.4 / 78.8 / 76.1 |
| all springs | 19,327.0 / 119.6 / 119.2 | 19,324.4 / 119.2 / 122.7 |
| hover above glass | 9,447.8 / 75.8 / 73.2 | 9,447.8 / 76.5 / 73.7 |
| hover beneath glass | 32,946.8 / 92.7 / 89.6 | 32,947.1 / 92.3 / 89.3 |
| glass moves 1 px | 16,974.7 / 112.9 / 108.0 | 16,970.4 / 110.4 / 111.1 |

| Cell | media 4 | media 24 | media 48 |
|---|---:|---:|---:|
| idle | 9.2 / 6.0 / 2.9 | 9.0 / 6.0 / 2.9 | 9.0 / 6.0 / 2.9 |
| one video frame | 11,171.7 / 78.3 / 76.2 | 14,368.1 / 78.1 / 76.4 | 14,368.1 / 78.3 / 77.5 |
| video under glass | 23,846.0 / 93.7 / 90.7 | 25,462.8 / 93.6 / 90.7 | 25,458.9 / 93.2 / 91.7 |
| all videos (every 4th) | 9.0 / 6.0 / 2.9 | 9.0 / 6.0 / 2.9 | 9.0 / 6.0 / 2.9 |
| a/v playback (audio clocks) | 9.0 / 6.0 / 2.9 | 188,186.2 / 130.9 / 133.5 | 294,450.0 / 153.7 / 162.5 |
| scroll 11 px | 135.0 / 595.2 / 132.4 | 240,617.2 / 844.3 / 847.4 | 589,558.7 / 1,183.7 / 1,194.9 |
| hover above glass | 28,507.9 / 93.6 / 99.9 | 35,033.3 / 94.8 / 93.0 | 35,032.6 / 98.0 / 95.8 |
| theme | 77,031.7 / 110.1 / 119.7 | 388,679.1 / 110.3 / 119.8 | 579,020.5 / 110.2 / 120.6 |
| all springs | 2,415.4 / 84.8 / 81.3 | 22,984.0 / 125.4 / 122.5 | 30,406.3 / 139.1 / 140.6 |
| glass over playing video | 9,460.7 / 125.3 / 117.0 | 10,556.9 / 125.3 / 121.2 | 10,546.4 / 121.3 / 128.9 |

| Cell | palette 200 | palette 1000 | palette 5000 |
|---|---:|---:|---:|
| idle | 9.2 / 6.2 / 3.1 | 9.3 / 6.2 / 3.1 | 9.3 / 6.2 / 3.1 |
| theme | 987,362.0 / 119.6 / 125.6 | 987,247.5 / 113.0 / 124.3 | 987,270.6 / 111.4 / 120.8 |
| palette switch | 981,508.3 / 112.7 / 132.2 | 981,521.2 / 113.9 / 120.8 | 981,453.6 / 110.5 / 121.9 |
| palette fade | 981,547.1 / 120.8 / 133.0 | 981,497.3 / 115.5 / 125.6 | 981,510.8 / 115.2 / 125.7 |
| hover above glass | 6,082.1 / 81.7 / 77.1 | 6,082.1 / 77.2 / 74.1 | 6,082.2 / 77.4 / 73.8 |
| scroll 11 px | 994,758.8 / 2,012.1 / 2,046.3 | 995,013.4 / 2,153.7 / 2,166.4 | 994,985.0 / 2,406.8 / 2,409.4 |

## Launch frames

The Metal all-springs retarget uses the regular frame counter and occurs during warmup in this trace.

| Scene | Three launch samples in instructions |
|---|---:|
| effects 200 | 3,116,009 / 3,129,444 / 3,116,158 |
| effects 1000 | 18,785,731 / 18,780,487 / 18,779,963 |
| effects 5000 | 97,010,377 / 97,023,372 / 97,028,572 |
| effects 600 dense | 6,218,722 / 6,254,175 / 6,239,545 |
| effects 3000 dense | 31,176,439 / 31,043,976 / 31,050,202 |
| pane 300 | 5,140,066 / 5,139,961 / 5,176,126 |
| pane 3000 | 57,899,143 / 57,875,764 / 57,902,252 |
| media 4 | 114,137 / 143,133 / 103,786 |
| media 24 | 166,743 / 161,544 / 149,612 |
| media 48 | 660,084 / 648,867 / 660,872 |
