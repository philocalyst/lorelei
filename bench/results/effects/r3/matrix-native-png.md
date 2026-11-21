# Native-PNG reference matrix with the completed Round 2 Lorelei captures

Lorelei façade columns use `71ee3be0aa` and `62d53d3abe`. The typed `Soft` column is the baseline diagnostic at `71ee3be0aa`. This interim table uses the new reference captures while the updated wave3 and staged runs proceed. See the Round 3 report for timer boundaries and workload differences.

The timings are p50/p95/p99 milliseconds with draws or repair steps per observation. zgui counts the increase in redraw requests after the first sampled tick. The two `many` gate failures were wgpu validation errors in readback at 1,000 and 5,000 cards. Startup in the reference suite has one observation per size, with separate process repeats queued.

| Cards | Cell | Lorelei baseline | Lorelei wave3 | Lorelei typed Soft | zgui | zgx |
|---:|---|---:|---:|---:|---:|---:|
| 200 | startup | 0.617/0.693/0.941 (240/240) | 0.894/1.422/1.782 (240/240) | 0.324 (1 observation) | 41.151 (1 observation) | 38.410 (1 observation) |
| 200 | idle | 0 frames | 0 frames | 0 repairs | 0 frames | 0 frames |
| 200 | one | 0.014/0.018/0.256 (240/240) | 0.013/0.024/0.494 (240/240) | 0.003/0.003/0.261 (240/240) | 0.057/0.764/3.058 (Δ131/240) | 0.415/0.500/0.644 (221/240) |
| 200 | many | 0.199/0.365/0.471 (240/240) | 0.495/1.048/1.495 (240/240) | 0.228/0.274/0.542 (240/240) | 6.513/21.206/32.569 (Δ131/240) | 0.479/0.568/0.640 (221/240) |
| 200 | hover | pixel gate failed | 0.069/0.138/0.487 (229/240) | unavailable | 0.159/0.658/0.856 (Δ239/240) | 0.424/0.512/0.605 (229/240) |
| 200 | theme | 0.186/0.209/0.246 (240/240) | 0.420/0.637/0.818 (240/240) | 0.201/0.270/0.436 (240/240) | 2.370/3.564/4.735 (Δ239/240) | 0.444/0.567/0.629 (240/240) |
| 200 | scroll | pixel gate failed | 0.296/0.536/0.697 (240/240) | unavailable | 0.215/0.656/1.008 (Δ239/240) | 0.482/0.679/0.761 (240/240) |
| 200 | resize | 0.213/0.232/0.280 (240/240) | 0.442/0.724/0.828 (240/240) | unavailable | 0.286/0.728/1.007 (Δ239/240) | 0.561/0.676/0.783 (240/240) |
| 200 | video | unavailable | unavailable | unavailable | unavailable | 0.676/0.838/1.011 (240/240) |
| 1,000 | startup | 1.937/2.031/2.223 (240/240) | 2.947/3.866/4.260 (240/240) | 1.718 (1 observation) | 82.518 (1 observation) | 43.694 (1 observation) |
| 1,000 | idle | 0 frames | 0 frames | 0 repairs | 0 frames | 0 frames |
| 1,000 | one | 0.057/0.061/0.884 (240/240) | 0.060/0.117/1.404 (240/240) | 0.011/0.012/0.852 (240/240) | 0.092/0.566/11.659 (Δ131/240) | 0.467/0.694/1.357 (221/240) |
| 1,000 | many | 0.877/1.641/1.804 (240/240) | 1.636/4.131/5.226 (240/240) | 0.982/1.217/2.023 (240/240) | gate failed | 0.686/0.886/1.723 (221/240) |
| 1,000 | hover | pixel gate failed | 0.336/0.766/1.570 (229/240) | unavailable | 0.246/0.452/0.789 (Δ239/240) | 0.479/0.611/0.939 (229/240) |
| 1,000 | theme | 0.783/0.823/0.868 (240/240) | 1.889/2.737/3.150 (240/240) | 0.824/1.170/2.077 (240/240) | 14.057/16.185/16.760 (Δ239/240) | 0.636/0.751/0.812 (240/240) |
| 1,000 | scroll | pixel gate failed | 0.314/0.613/0.972 (240/240) | unavailable | 0.132/0.505/0.723 (Δ239/240) | 0.478/0.647/0.751 (240/240) |
| 1,000 | resize | 0.881/0.928/0.962 (240/240) | 1.796/3.092/4.125 (240/240) | unavailable | 0.190/0.602/1.085 (Δ239/240) | 0.743/1.005/2.031 (240/240) |
| 1,000 | video | unavailable | unavailable | unavailable | unavailable | 0.671/0.839/1.119 (240/240) |
| 5,000 | startup | 8.047/8.295/8.548 (240/240) | 10.702/13.020/13.610 (240/240) | 19.513 (1 observation) | 296.733 (1 observation) | 40.146 (1 observation) |
| 5,000 | idle | 0 frames | 0 frames | 0 repairs | 0 frames | 0 frames |
| 5,000 | one | 0.207/0.232/3.625 (240/240) | 0.289/0.542/5.303 (240/240) | 0.055/0.055/3.814 (240/240) | 0.290/2.185/95.298 (Δ131/240) | 0.665/0.788/0.938 (221/240) |
| 5,000 | many | 4.212/8.849/9.235 (240/240) | 6.725/14.099/18.634 (240/240) | 4.597/4.886/6.178 (240/240) | gate failed | 1.608/2.058/2.708 (221/240) |
| 5,000 | hover | pixel gate failed | 1.085/2.108/4.641 (229/240) | unavailable | 1.319/1.696/1.932 (Δ239/240) | 0.686/0.813/0.899 (229/240) |
| 5,000 | theme | 3.625/3.949/4.035 (240/240) | 5.923/6.829/7.634 (240/240) | 3.857/4.518/5.091 (240/240) | 112.916/124.330/143.459 (Δ239/240) | 1.326/1.484/1.551 (240/240) |
| 5,000 | scroll | pixel gate failed | 0.272/0.438/0.638 (240/240) | unavailable | 0.278/0.465/0.770 (Δ239/240) | 0.508/0.660/0.981 (240/240) |
| 5,000 | resize | 4.075/4.189/4.234 (240/240) | 6.138/7.828/8.611 (240/240) | unavailable | 0.319/0.758/1.018 (Δ239/240) | 1.795/2.026/3.762 (240/240) |
| 5,000 | video | unavailable | unavailable | unavailable | unavailable | 0.927/1.137/1.221 (240/240) |
