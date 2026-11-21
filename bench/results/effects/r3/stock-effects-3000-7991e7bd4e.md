# Stock effects 3000 on 7991e7bd4e

24 frames per cell. Calling-thread CPU1 instruction medians alongside the original 6c5fd7d42f capture. The stock script includes the time-dependent shader paints. All 24 replay rows across CPU1, no-cache CPU1 and Metal produced zero final retained/full word differences.

| Cell | Original K instr | Replay K instr | Output tiles/frame | Prefix tiles/frame | Tent blocks/frame |
|---|---:|---:|---:|---:|---:|
| idle | 5.8 | 5.5 | 0.0 | 0.0 | 0.0 |
| one spring | 67,792.9 | 67,854.8 | 59.8 | 6.0 | 154.0 |
| all springs | 86,977.8 | 87,582.8 | 89.7 | 6.0 | 154.0 |
| scroll 11 px | 1,224,531.1 | 1,224,923.9 | 1000.0 | 348.5 | 1342.8 |
| hover above glass | 11,444.1 | 11,444.3 | 6.0 | 0.0 | 0.0 |
| hover beneath glass | 85,663.5 | 85,662.7 | 36.0 | 27.0 | 337.0 |
| theme | 1,223,392.7 | 1,224,445.9 | 1000.0 | 367.0 | 1439.0 |
| glass moves 1 px | 56,133.7 | 56,131.2 | 29.7 | 8.0 | 189.3 |
