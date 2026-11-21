# Slice 10 report — motion hulls

Measured on `47902dcba1` (implementation `a78b35048b`, frozen card `67dbb1a8a6`). Apple M3 Ultra, macOS 26.7.3, headless. The tables use frame-thread instructions. The raw logs retain the incidental timing fields and record load before and after every run. The run used the instruction-count exception to the bench lock.

`Expr::time_hull` provides optional inclusive component bounds. Springs find the discrete extrema of the actual fixed-point leaf through settle, including the start before a staggered launch. A proven zero tail is omitted. Progress and cubic eases use closed ranges. Paint lifts these through checked rectangle arithmetic. The scene parks invisible timed geometry if its swept ink and halo miss the frame and indexes the swept rows for pan. Unknown or unusually large sweeps take the safe fallback. A backwards clock seek resets the future-only proof.

## Gates

```
nice -n 15 cargo test --locked --release -p bough-paint -p bough-expr -p bough-kernel -j 8 -- --nocapture
spring hull: 400 remaining-motion intervals match the discrete leaf extrema
motion hull: 240 CPU frames and 125 Metal draws at ages 1..4, zero mismatching words
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Paint lib 2, exact 5, fx 13, review 8, scenes 2 and doctests 2 passed. Expr and kernel passed. The new negative control also ran with `--test fx --features mutant_hull_small` and exited 101 with 11 passed and 2 failed. It reduced the exact interval `Some((47648, 54268))` to `Some((47648, 47648))`. The pan fixture also caught it failing to wake the reader.

## Thresholds

Sparse Metal all springs measured **241.7 / 241.9 / 240.0 K**, a **1.008×** maximum-to-minimum ratio with **18 / 18 / 18** steady visits. Both limits held. Slice 9 measured 266 / 402 / 1,105 K and visited 31 / 131 / 631 records. Pane all-springs also stopped growing with world size. The bulk-retarget frame still grows with the number of rewritten laws. The exact hull searches the reachable integer ticks per law, which made the 5,000-card retarget cost about 97 M instructions. This is a separate measured cost and motivates an indexed or interval-based extremum search if launch latency is a priority.

All 39 `fxbench` runs printed `mismatching words in total: 0`. All six `pbench` runs printed `cross-check failures: 0`. Every A1 sample was within 1.03× the baseline. The minimum and maximum ratios were 0.945 and 1.018. Reproduction uses `bench/fx/matrix.sh` with a lane target directory. The dense rows have six measured frames per repetition and the remaining rows have 24. Raw output is in `bench/fx/results/slice10.txt` and `bench/fx/results/pbench-slice10.txt`.

## Open costs at this revision

- Dense Metal one spring measured 352.8 / 1,132.2 K at 600 / 3,000. Layer rank encoding remained on the frame thread.
- The CPU repaired 570 prefix tiles in six one-spring frames at 600 dense. Of these 534 were unchanged. At 3,000 dense the corresponding counts were 3,348 and 3,150.
- Sparse scroll still measured 1,477 / 1,608 / 1,859 K on Metal. Its indexed pass rewrites exposed windows and bins. The 3,000 dense scroll measured 24.0 M on Metal. CPU whole-frame sparse repairs still measured about 986–993 M.
- Retained shadow coverage, scroll uniforms, groups, author shader modules, image decode, SVG and Vulkan parity were outside this card. The staged adapter gates and the standing mutant replay run separately. GPU timings under the lock and independent replay remain outstanding.

The only implementation edit outside the lane is the seven added lines in `crates/bough-expr/src/lib.rs:758`–`764` defining the default `Expr::time_hull`. No existing signature changed.

## Instruction matrix

Median of three frame medians in K instructions. Each cell lists CPU / Metal / chooser.

| Cell | effects 200 | effects 1000 | effects 5000 |
|---|---:|---:|---:|
| idle | 9.3 / 6.2 / 3.3 | 9.2 / 6.2 / 3.3 | 9.3 / 6.2 / 3.3 |
| one spring | 35,048.5 / 198.7 / 196.8 | 35,052.8 / 198.7 / 197.3 | 35,047.7 / 197.6 / 196.4 |
| all springs | 53,524.5 / 241.7 / 239.5 | 53,528.6 / 241.9 / 238.4 | 53,526.9 / 240.0 / 239.1 |
| scroll 11 px | 992,345.1 / 1,477.0 / 1,489.0 | 992,689.0 / 1,607.5 / 1,620.1 | 992,677.0 / 1,859.1 / 1,868.1 |
| hover above glass | 6,082.2 / 189.8 / 187.4 | 6,082.1 / 189.6 / 187.1 | 6,082.2 / 188.9 / 187.3 |
| hover beneath glass | 41,965.0 / 189.8 / 187.8 | 41,964.8 / 189.7 / 187.9 | 41,964.5 / 189.7 / 187.7 |
| theme | 986,059.5 / 207.9 / 218.0 | 986,070.2 / 207.4 / 218.3 | 986,115.9 / 207.3 / 217.9 |
| glass moves 1 px | 17,783.4 / 223.2 / 221.6 | 17,780.1 / 223.3 / 221.0 | 17,781.4 / 222.8 / 220.8 |

| Cell | effects 600 dense | effects 3000 dense |
|---|---:|---:|
| idle | 9.4 / 6.2 / 3.5 | 9.4 / 6.2 / 4.7 |
| one spring | 199,288.0 / 352.8 / 353.1 | 1,034,466.2 / 1,132.2 / 1,134.7 |
| all springs | 341,958.0 / 512.7 / 512.9 | 1,707,644.1 / 1,809.4 / 1,824.0 |
| scroll 11 px | 4,769,281.2 / 4,406.7 / 4,417.4 | 24,213,339.7 / 24,037.3 / 24,038.7 |
| hover above glass | 7,313.7 / 323.4 / 322.7 | 12,165.8 / 1,012.1 / 1,016.3 |
| hover beneath glass | 240,876.5 / 325.5 / 323.2 | 1,049,857.8 / 1,015.6 / 1,016.7 |
| theme | 5,453,856.2 / 341.7 / 353.2 | 26,813,695.6 / 1,030.3 / 1,050.2 |
| glass moves 1 px | 85,531.0 / 362.2 / 361.8 | 366,975.3 / 1,079.5 / 1,079.6 |

| Cell | pane 300 | pane 3000 |
|---|---:|---:|
| idle | 9.3 / 6.2 / 3.1 | 9.3 / 6.2 / 3.1 |
| one spring | 422.4 / 92.5 / 90.4 | 422.4 / 97.1 / 90.2 |
| all springs | 19,324.9 / 134.7 / 133.9 | 19,324.8 / 133.4 / 134.5 |
| hover above glass | 9,448.0 / 89.8 / 87.3 | 9,447.9 / 90.3 / 87.2 |
| hover beneath glass | 32,947.9 / 90.2 / 88.1 | 32,946.5 / 90.2 / 88.0 |
| glass moves 1 px | 16,971.0 / 126.0 / 121.4 | 16,973.5 / 124.3 / 121.9 |

| Cell | media 4 | media 24 | media 48 |
|---|---:|---:|---:|
| idle | 9.2 / 6.0 / 3.0 | 9.2 / 6.0 / 3.0 | 9.2 / 6.0 / 3.0 |
| one video frame | 11,171.7 / 90.8 / 89.4 | 14,368.1 / 91.1 / 89.4 | 14,368.1 / 90.7 / 89.2 |
| video under glass | 23,830.7 / 91.1 / 88.9 | 25,464.5 / 91.2 / 89.0 | 25,473.7 / 91.1 / 88.9 |
| all videos (every 4th) | 9.4 / 6.0 / 3.0 | 9.4 / 6.0 / 3.0 | 9.0 / 6.0 / 3.0 |
| a/v playback (audio clocks) | 9.4 / 6.0 / 3.0 | 188,190.5 / 124.6 / 129.2 | 294,450.2 / 147.0 / 152.8 |
| scroll 11 px | 135.0 / 605.9 / 132.5 | 240,583.7 / 829.5 / 832.3 | 589,606.4 / 1,147.9 / 1,158.4 |
| hover above glass | 28,507.3 / 91.1 / 88.9 | 35,035.0 / 92.4 / 90.3 | 35,034.3 / 94.7 / 92.5 |
| theme | 77,051.2 / 107.6 / 117.7 | 388,675.3 / 107.7 / 117.7 | 578,991.9 / 107.7 / 117.8 |
| all springs | 2,415.3 / 97.1 / 94.6 | 22,989.5 / 139.3 / 137.4 | 30,406.8 / 152.5 / 154.1 |
| glass over playing video | 9,450.0 / 131.9 / 130.7 | 10,546.4 / 132.6 / 131.1 | 10,551.8 / 133.9 / 133.0 |

| Cell | palette 200 | palette 1000 | palette 5000 |
|---|---:|---:|---:|
| idle | 9.3 / 6.2 / 3.3 | 9.3 / 6.2 / 3.3 | 9.3 / 6.2 / 3.3 |
| theme | 987,264.7 / 207.6 / 218.2 | 987,230.6 / 207.4 / 217.9 | 987,244.3 / 207.1 / 217.9 |
| palette switch | 981,547.3 / 207.3 / 218.4 | 981,458.0 / 207.3 / 218.3 | 981,471.9 / 207.3 / 218.2 |
| palette fade | 981,541.0 / 212.3 / 223.7 | 981,518.2 / 212.1 / 223.4 | 981,513.6 / 212.0 / 224.1 |
| hover above glass | 6,082.1 / 189.8 / 187.1 | 6,082.2 / 189.0 / 187.0 | 6,082.2 / 189.0 / 187.0 |
| scroll 11 px | 994,629.0 / 1,626.2 / 1,638.5 | 994,957.2 / 1,776.1 / 1,786.1 | 994,938.4 / 2,024.7 / 2,036.4 |

## Launch frames

The Metal all-springs retarget uses the regular frame counter and occurs during warmup in this trace.

| Scene | Three launch samples in instructions |
|---|---:|
| effects 200 | 3,218,318 / 3,212,090 / 3,212,799 |
| effects 1000 | 18,892,739 / 18,875,068 / 18,936,613 |
| effects 5000 | 97,101,987 / 97,158,553 / 97,118,748 |
| effects 600 dense | 6,471,789 / 6,460,503 / 6,460,952 |
| effects 3000 dense | 32,006,461 / 32,155,873 / 31,979,195 |
| pane 300 | 5,159,638 / 5,147,305 / 5,161,085 |
| pane 3000 | 57,892,798 / 57,929,656 / 57,908,990 |
| media 4 | 116,402 / 113,547 / 100,991 |
| media 24 | 165,798 / 151,761 / 150,011 |
| media 48 | 652,251 / 650,191 / 659,410 |
