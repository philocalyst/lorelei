# Glass over video on staged CPU1

Both timing binaries use paint at wave3 `7991e7bd4e`. The generated arm includes synthetic NV12 generation, publication, backdrop repair, and output shading. The predecoded arm precomputes 120 NV12 images (6,635,520 bytes). Its timed interval includes copying the next decoded image into the lease, publication, backdrop repair, and output shading. The synthetic source has no codec.

Changing ticks use sequence IDs 0–60 and submit 240 output frames. Both arms run the four-age retained/full gate, fresh mount, independent pixel probes, and dropped-input and dropped-damage mutants. The separate parity test compares exact frame bytes at all three sizes. The two timing captures ran in separate time blocks under different host load. The wall samples support a scoped comparison only.

| Cards | Generated p50/p95/p99 (ms) | Predecoded p50/p95/p99 (ms) | Frames per arm |
|---:|---:|---:|---:|
| 200 | 7.390/35.532/61.402 | 7.219/41.850/78.595 | 240/240 |
| 1,000 | 30.778/64.298/65.647 | Pending | — |
| 5,000 | Pending | Pending | — |

[Generated input receipts](staged-final) · [Predecoded input receipts](staged-predecoded) · [Exact source parity](staged-source-parity-7991e7bd4e.txt)
