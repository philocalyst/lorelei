# Slice 14 report — world-relative scroll

`Scene::uniform_pan` retains one rigid scroll space in overlapping world-cell lists. A pure pan updates the block and damages the viewport. The CPU and Metal merge fixed frame lists with one world list per frame tile in paint order. GPU storage uploads once per geometry version. The admission excludes scenes with clocks, backdrop layers, or incompatible spaces. Combined changes re-enter normal scene repair.

`nice -n 15 cargo test --locked --release -p bough-paint -p bough-expr -p bough-ax -j 8 -- --nocapture` passed. The paint suite ran 18 FX tests. The new fixture drew seven exact CPU and Metal frames. Its five pure pans visited zero records and rewrote no bins. Metal uploaded once in each of two geometry epochs. `mutant_world_origin` exited 101 with 10,349 different words on Metal world frame 1. The full workspace all-targets check passed.

The standing effects, pane and media scenes use their ordinary admission. All 39 matrix configurations printed zero mismatches and all six pbench runs passed cross-checks. Counts use the frame-thread PMU. The matrix did not measure a dedicated admitted-world timing cell. The layered pan still needs frame-grid-preserving layer dependencies. Output band copies and layer-free reuse of unaffected pixels remain open.

## Instruction matrix

Median of three frame medians in K instructions. Each cell lists CPU / Metal / chooser.

| Cell | effects 200 | effects 1000 | effects 5000 |
|---|---:|---:|---:|
| idle | 9.2 / 6.2 / 3.1 | 9.3 / 6.2 / 3.1 | 9.4 / 6.2 / 3.1 |
| one spring | 10,130.7 / 100.5 / 99.3 | 10,130.6 / 100.6 / 98.0 | 10,130.6 / 100.3 / 99.1 |
| all springs | 13,263.3 / 141.8 / 141.1 | 13,262.6 / 142.5 / 139.3 | 13,262.7 / 140.5 / 139.4 |
| scroll 11 px | 209,189.4 / 1,894.8 / 1,902.4 | 209,423.6 / 2,021.5 / 2,029.6 | 209,440.1 / 2,262.5 / 2,275.0 |
| hover above glass | 2,070.4 / 75.9 / 73.3 | 2,070.4 / 75.9 / 73.6 | 2,070.4 / 76.3 / 73.3 |
| hover beneath glass | 14,872.3 / 93.0 / 89.6 | 14,872.2 / 92.1 / 89.5 | 14,872.2 / 91.9 / 89.8 |
| theme | 216,288.1 / 110.1 / 120.4 | 216,291.0 / 109.9 / 119.9 | 216,288.6 / 110.2 / 120.3 |
| glass moves 1 px | 8,163.8 / 127.8 / 124.4 | 8,174.9 / 125.8 / 124.4 | 8,161.2 / 127.0 / 123.4 |

| Cell | effects 600 dense | effects 3000 dense |
|---|---:|---:|
| idle | 9.6 / 6.2 / 3.1 | 10.1 / 6.2 / 3.1 |
| one spring | 40,133.6 / 125.6 / 123.7 | 188,421.3 / 240.0 / 240.5 |
| all springs | 57,791.6 / 278.2 / 276.1 | 270,544.6 / 884.7 / 883.5 |
| scroll 11 px | 901,896.8 / 5,365.9 / 5,373.8 | 4,443,812.0 / 30,840.8 / 30,859.9 |
| hover above glass | 2,068.1 / 81.0 / 75.3 | 3,301.2 / 77.3 / 75.1 |
| hover beneath glass | 45,509.9 / 93.9 / 89.5 | 55,859.0 / 92.6 / 98.3 |
| theme | 1,088,707.5 / 109.8 / 122.5 | 5,247,164.6 / 109.2 / 124.4 |
| glass moves 1 px | 9,851.3 / 133.7 / 129.8 | 12,518.3 / 159.3 / 156.7 |

| Cell | pane 300 | pane 3000 |
|---|---:|---:|
| idle | 9.5 / 6.2 / 3.1 | 9.2 / 6.2 / 3.1 |
| one spring | 108.6 / 79.4 / 76.0 | 108.5 / 79.5 / 75.8 |
| all springs | 2,978.8 / 118.6 / 118.9 | 2,978.7 / 118.2 / 118.5 |
| hover above glass | 3,304.0 / 76.4 / 73.3 | 3,303.9 / 76.1 / 73.4 |
| hover beneath glass | 10,163.3 / 92.4 / 89.7 | 10,163.0 / 92.1 / 89.2 |
| glass moves 1 px | 7,353.8 / 109.6 / 107.3 | 7,353.1 / 109.8 / 107.8 |

| Cell | media 4 | media 24 | media 48 |
|---|---:|---:|---:|
| idle | 9.2 / 6.0 / 2.9 | 9.2 / 6.0 / 2.9 | 9.2 / 6.0 / 2.9 |
| one video frame | 8,388.0 / 77.6 / 75.8 | 9,059.0 / 77.9 / 75.8 | 9,059.1 / 77.4 / 75.9 |
| video under glass | 17,996.4 / 93.1 / 91.0 | 18,198.7 / 93.2 / 91.1 | 18,198.7 / 93.0 / 90.8 |
| all videos (every 4th) | 9.0 / 6.0 / 2.9 | 9.0 / 6.0 / 2.9 | 9.0 / 6.0 / 2.9 |
| a/v playback (audio clocks) | 9.0 / 6.1 / 3.0 | 103,949.9 / 131.1 / 134.7 | 158,688.2 / 153.7 / 160.1 |
| scroll 11 px | 135.1 / 592.8 / 132.5 | 147,615.2 / 847.7 / 851.7 | 373,710.7 / 1,191.1 / 1,202.2 |
| hover above glass | 19,603.3 / 92.9 / 90.3 | 21,644.0 / 94.7 / 92.5 | 21,645.8 / 97.3 / 94.7 |
| theme | 53,300.4 / 109.8 / 119.5 | 246,768.6 / 109.7 / 119.6 | 367,508.3 / 109.8 / 119.5 |
| all springs | 1,942.4 / 83.3 / 80.8 | 11,360.6 / 122.3 / 122.9 | 14,443.3 / 136.9 / 138.7 |
| glass over playing video | 9,447.9 / 118.7 / 118.8 | 9,619.4 / 119.2 / 118.6 | 9,625.3 / 121.8 / 118.1 |

| Cell | palette 200 | palette 1000 | palette 5000 |
|---|---:|---:|---:|
| idle | 9.3 / 6.2 / 3.1 | 9.3 / 6.2 / 3.1 | 9.3 / 6.2 / 3.1 |
| theme | 217,443.4 / 109.8 / 120.0 | 217,449.7 / 110.2 / 120.0 | 217,457.4 / 111.1 / 119.6 |
| palette switch | 211,682.8 / 109.9 / 120.0 | 211,657.5 / 110.0 / 120.1 | 211,673.5 / 109.9 / 120.0 |
| palette fade | 211,725.7 / 114.4 / 125.1 | 211,706.2 / 115.2 / 125.5 | 211,717.3 / 114.5 / 124.8 |
| hover above glass | 2,070.5 / 75.9 / 73.4 | 2,070.4 / 75.7 / 73.2 | 2,070.5 / 76.0 / 73.2 |
| scroll 11 px | 210,774.5 / 2,018.7 / 2,028.3 | 211,015.9 / 2,169.8 / 2,177.5 | 211,057.6 / 2,423.6 / 2,421.5 |

## Launch frames

The Metal all-springs retarget uses the regular frame counter and occurs during warmup in this trace.

| Scene | Three launch samples in instructions |
|---|---:|
| effects 200 | 3,134,985 / 3,112,943 / 3,122,309 |
| effects 1000 | 18,831,118 / 18,773,669 / 18,774,773 |
| effects 5000 | 97,010,626 / 97,256,049 / 97,010,806 |
| effects 600 dense | 6,215,767 / 6,218,494 / 6,228,103 |
| effects 3000 dense | 30,992,563 / 31,014,493 / 31,052,647 |
| pane 300 | 5,129,700 / 5,128,952 / 5,129,393 |
| pane 3000 | 57,874,608 / 57,876,479 / 57,902,213 |
| media 4 | 103,952 / 104,576 / 123,223 |
| media 24 | 148,067 / 162,171 / 160,420 |
| media 48 | 647,659 / 648,047 / 647,149 |
