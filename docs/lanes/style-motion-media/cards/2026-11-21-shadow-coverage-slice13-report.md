# Slice 13 report — retained shadow coverage

Measured at `ada22d9d83` on Apple M3 Ultra, macOS 26.7.3. The complete coverage table stores Q15 values in the scene. The key includes evaluated radius, blur, all four edge phases, and the dimensions of the bounded patch. Integer translation changes its origin. A changed phase takes the exact analytic fallback. The span mask still skips coverage under a certified opaque core.

The 600-dense one-spring CPU cell dropped from 185.61 M to 40.14 M frame-thread instructions. Theme dropped from approximately 5.45 G to 1.09 G. Both meet 0.75×. The matched 3,000-dense one-spring CPU cells went from 928.41 M to 188.43 M. The number of changed and unchanged prefix checks was identical. Sparsely laid out theme frames still perform about 216 M instructions on the CPU, so the chooser continues sending them to Metal. Retained coverage measured 530,272 bytes for regular effects and dense scenes, 421,228 for pane, and 69,296 for media.

`nice -n 15 cargo test --locked --release -p bough-paint -j 8 -- --nocapture` passed lib 2, exact 5, fx 17, review 8 and scenes 2. The independent analytic fixture passed five CPU and Metal frames and used 105,172 retained bytes. The new `--features mutant_shadow_patch` fixture exited 101. It failed at `retained shadow samples at 0, 0`. The initial suite showed the old corner-fill negative control was hidden by complete patches. Its dedicated fixture now selects the corner path and measures 24 differing words with that source mutation.

`bench/fx/matrix.sh slice13` ran all 39 configurations with zero differing words. The six pbench runs printed `cross-check failures: 0`. A1 ratios range from 0.099× to 1.018×. The matrix reports frame-thread instructions. Wall and GPU timing columns are incidental. See the raw samples under `bench/fx/results/`.

Whole-pixel motion reuses complete coverage, and motion to a different subpixel phase uses the analytic path. Layered scroll and Vulkan exactness are still pending. This card made no edits outside the paint lane.

## Instruction matrix

Median of three frame medians in K instructions. Each cell lists CPU / Metal / chooser.

| Cell | effects 200 | effects 1000 | effects 5000 |
|---|---:|---:|---:|
| idle | 9.3 / 6.2 / 3.1 | 9.3 / 6.2 / 3.1 | 9.4 / 6.2 / 3.1 |
| one spring | 10,130.2 / 100.5 / 99.2 | 10,130.2 / 101.1 / 98.7 | 10,132.0 / 100.3 / 98.9 |
| all springs | 13,262.3 / 140.9 / 140.0 | 13,262.5 / 141.4 / 140.1 | 13,262.2 / 141.3 / 139.5 |
| scroll 11 px | 209,178.7 / 1,878.2 / 1,885.8 | 209,461.4 / 2,014.6 / 2,025.8 | 209,422.2 / 2,254.2 / 2,262.7 |
| hover above glass | 2,070.3 / 76.3 / 73.3 | 2,070.3 / 75.8 / 73.3 | 2,070.3 / 76.5 / 73.5 |
| hover beneath glass | 14,872.4 / 92.4 / 89.9 | 14,873.1 / 92.7 / 90.9 | 14,869.6 / 92.6 / 90.1 |
| theme | 216,275.1 / 110.7 / 121.2 | 216,281.0 / 110.7 / 120.5 | 216,272.7 / 110.7 / 120.9 |
| glass moves 1 px | 8,163.2 / 126.2 / 124.4 | 8,163.9 / 126.4 / 125.6 | 8,161.0 / 126.6 / 123.4 |

| Cell | effects 600 dense | effects 3000 dense |
|---|---:|---:|
| idle | 9.5 / 6.2 / 3.1 | 9.5 / 6.2 / 3.1 |
| one spring | 40,139.9 / 125.4 / 124.4 | 188,425.3 / 243.2 / 241.9 |
| all springs | 57,790.8 / 277.5 / 275.3 | 270,521.9 / 888.1 / 882.6 |
| scroll 11 px | 901,936.5 / 5,330.8 / 5,344.2 | 4,443,798.1 / 30,619.1 / 30,641.2 |
| hover above glass | 2,067.9 / 76.3 / 76.8 | 3,301.1 / 78.0 / 75.9 |
| hover beneath glass | 45,509.6 / 92.3 / 89.8 | 55,848.2 / 92.7 / 93.2 |
| theme | 1,088,707.8 / 112.2 / 119.6 | 5,246,912.6 / 111.8 / 121.1 |
| glass moves 1 px | 9,846.1 / 134.6 / 132.5 | 12,518.1 / 166.2 / 158.2 |

| Cell | pane 300 | pane 3000 |
|---|---:|---:|
| idle | 9.4 / 6.2 / 3.1 | 9.3 / 6.2 / 3.1 |
| one spring | 108.6 / 79.3 / 77.2 | 108.5 / 80.1 / 76.4 |
| all springs | 2,978.6 / 120.7 / 123.5 | 2,978.5 / 119.4 / 120.1 |
| hover above glass | 3,303.8 / 76.0 / 75.3 | 3,303.8 / 75.9 / 74.6 |
| hover beneath glass | 10,163.2 / 94.5 / 91.4 | 10,163.1 / 92.2 / 90.4 |
| glass moves 1 px | 7,357.6 / 109.8 / 107.7 | 7,358.8 / 110.4 / 107.3 |

| Cell | media 4 | media 24 | media 48 |
|---|---:|---:|---:|
| idle | 9.4 / 6.0 / 2.9 | 9.2 / 6.0 / 2.9 | 9.4 / 6.0 / 2.9 |
| one video frame | 8,387.8 / 77.7 / 80.5 | 9,058.8 / 77.6 / 77.1 | 9,058.8 / 77.6 / 76.0 |
| video under glass | 18,006.4 / 93.8 / 91.7 | 18,199.7 / 94.1 / 92.1 | 18,198.8 / 94.7 / 90.7 |
| all videos (every 4th) | 9.4 / 6.0 / 2.9 | 9.4 / 6.0 / 2.9 | 9.4 / 6.0 / 2.9 |
| a/v playback (audio clocks) | 9.4 / 6.0 / 2.9 | 103,938.8 / 130.5 / 133.6 | 158,692.9 / 153.3 / 158.6 |
| scroll 11 px | 135.1 / 591.8 / 132.4 | 147,628.3 / 845.4 / 850.2 | 373,767.2 / 1,185.1 / 1,196.0 |
| hover above glass | 19,601.6 / 94.0 / 90.5 | 21,642.3 / 95.9 / 92.3 | 21,645.3 / 96.6 / 95.1 |
| theme | 53,293.8 / 109.7 / 119.5 | 246,785.4 / 110.6 / 120.1 | 367,511.2 / 110.0 / 120.0 |
| all springs | 1,942.3 / 84.0 / 82.0 | 11,377.1 / 123.4 / 123.2 | 14,460.6 / 139.7 / 138.0 |
| glass over playing video | 9,457.7 / 127.2 / 124.4 | 9,624.8 / 121.8 / 119.6 | 9,629.9 / 125.5 / 119.0 |

| Cell | palette 200 | palette 1000 | palette 5000 |
|---|---:|---:|---:|
| idle | 9.6 / 6.2 / 3.1 | 9.3 / 6.2 / 3.1 | 9.4 / 6.2 / 3.1 |
| theme | 217,424.6 / 109.5 / 119.7 | 217,437.5 / 110.4 / 119.8 | 217,439.0 / 110.7 / 120.9 |
| palette switch | 211,652.0 / 110.2 / 119.7 | 211,641.7 / 111.3 / 120.0 | 211,647.2 / 111.3 / 121.4 |
| palette fade | 211,714.5 / 113.9 / 125.7 | 211,723.4 / 116.0 / 125.7 | 211,714.3 / 115.1 / 126.0 |
| hover above glass | 2,070.4 / 76.1 / 73.2 | 2,070.4 / 76.5 / 74.0 | 2,070.4 / 76.5 / 73.7 |
| scroll 11 px | 210,755.5 / 2,009.2 / 2,016.4 | 211,042.5 / 2,159.2 / 2,165.2 | 211,027.3 / 2,402.8 / 2,420.5 |

## Launch frames

The Metal all-springs retarget uses the regular frame counter and occurs during warmup in this trace.

| Scene | Three launch samples in instructions |
|---|---:|
| effects 200 | 3,114,989 / 3,110,795 / 3,117,828 |
| effects 1000 | 18,774,533 / 18,813,646 / 18,863,229 |
| effects 5000 | 97,017,366 / 97,020,728 / 97,016,449 |
| effects 600 dense | 6,213,326 / 6,213,682 / 6,229,658 |
| effects 3000 dense | 31,011,352 / 31,071,381 / 30,993,380 |
| pane 300 | 5,128,297 / 5,199,006 / 5,161,074 |
| pane 3000 | 57,878,801 / 57,899,610 / 57,938,542 |
| media 4 | 108,106 / 150,644 / 105,283 |
| media 24 | 147,131 / 196,500 / 175,970 |
| media 48 | 646,304 / 649,851 / 698,791 |
