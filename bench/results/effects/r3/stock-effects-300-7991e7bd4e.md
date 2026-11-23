# Stock effects 300 on 7991e7bd4e

24 frames per cell. Calling-thread CPU1 instruction medians, alongside the original 6c5fd7d42f capture. All 24 replay rows across CPU1, no-cache CPU1 and Metal produced zero final retained/full mismatching words.

| Cell | Original K instr | Replay K instr | Output tiles/frame | Prefix tiles/frame | Tent blocks/frame |
|---|---:|---:|---:|---:|---:|
| idle | 5.4 | 5.5 | 0.0 | 0.0 | 0.0 |
| one spring | 67,390.1 | 67,466.1 | 59.8 | 6.0 | 154.0 |
| all springs | 86,300.9 | 86,859.4 | 89.7 | 6.0 | 154.0 |
| scroll 11 px | 1,221,213.8 | 1,219,786.1 | 1000.0 | 348.5 | 1342.8 |
| hover above glass | 11,221.3 | 11,221.3 | 6.0 | 0.0 | 0.0 |
| hover beneath glass | 85,439.5 | 85,439.5 | 36.0 | 27.0 | 337.0 |
| theme | 1,223,475.4 | 1,223,715.4 | 1000.0 | 367.0 | 1439.0 |
| glass moves 1 px | 55,756.4 | 55,763.7 | 29.7 | 8.0 | 189.3 |
