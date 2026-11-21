# Report: the audio seam (card 2026-11-21-audio-seam-slice2)

State reached: **MEASURED** (not independently replayed). Host as slice 1 (M3 Ultra; instructions ranked).
Raw output, two files, because the first run stopped itself:

* `bench/results/fx/fxbench-media-audio-aborted.txt` (`REPS=240`, dist, under the bench lock, load 17 to 19):
  the source of every instruction count below. Each of its three runs **panicked after the a/v cell's CPU
  row**: my own T1 assertion also fired in the redraw-every-layer control, which does work on idle refreshes
  by construction. So that file has no control or Metal row for the a/v cell and no final validity line.
* `bench/results/fx/fxbench-media-audio-validity.txt` (`REPS=24`, assertion scoped to the cached executors,
  **without the bench lock**, load 42: the lock had a queue of ten by then): every cell on all three
  executors, exit 0, "mismatching words in total: 0" at 4 / 24 / 48 tiles. Its timings are not used.

Reproduce: `cargo test --release -p bough-audio` (add `--features coreaudio` on a Mac with an output device;
`--features mutant_wall_clock` must fail); `REPS=240 fxbench media 24`.

## Gates

| gate | result |
|---|---|
| A1 ring, two threads, 10^6 samples | in order, once each; underrun leaves silence; a full ring refuses (`ring_delivers_every_sample_once_in_order`) |
| A2 clock | exact at the stamp and between stamps; holds one period after the last callback; monotone under +-3 ms callback jitter; 200,000 concurrent reads, none torn. The first version (acquire/release orderings) **did** tear on this machine; the lock is now sequentially consistent. |
| A3 pacing | 10 s at 120 Hz: nominal device 480,000 frames -> video frame 300; 0.5 % fast 482,400 -> 301; 0.5 % slow 477,600 -> 298; every frame published once, none skipped. Stall of 2 s: frame 60 held throughout, 120 at the end (four seconds of audio in six of output). Seek republishes once. |
| A4 `mutant_wall_clock` | fails A3 ("fast: video frame 299 against audio 10.050 s") |
| A5 CoreAudio | ran headless with no prompt: 70 callbacks in about 0.55 s, period 512 frames, reported latency 0, 48,405 frames/s measured against 48,000 (0.8 %); silence rendered; no callback after drop |
| A6 no dependency | `cargo tree -p bough-paint -e normal` has no bough-audio (it is a dev-dependency for the benchmark) |
| A7 media scene with audio clocks | 0 mismatching words against a full redraw on CPU, control and Metal at 4 / 24 / 48 tiles (the validity run; 31 refreshes each) |

## Thresholds

| | prediction | measured | verdict |
|---|---|---|---|
| T1 a refresh with no new frame | 0 tiles, 0 records | asserted on every such refresh of the CPU run: 173 / 101 / 101 of 247 at 4 / 24 / 48 tiles; and on CPU and Metal in the validity run (23 / 16 / 16 of 31) | held |
| T2 frames published == frames the audio clocks crossed | exact | 249 == 249, 1,491 == 1,491, 2,981 == 2,981 (247 refreshes); 32, 192, 384 in the validity run | held |
| T3 pacer cost per tile per refresh | <= 100 instructions | 99.0 (126.0 before the clock stopped reading its stamp twice and dropped a 128-bit product) | held, barely |

a/v playback, CPU one core, mean K instructions per refresh (medians are near-idle refreshes at 4 tiles):
17,726 (4 tiles, 27.3 tiles shaded per refresh), 125,785 (24; 200.2), 193,698 (48; 299.6; a third of the tiles
are below the frame). The tiles' devices differ by up to 0.5 %, so their frames do not land on the same
refresh: the work per refresh is the tiles whose frame changed.

**A correction to slice 1's P6.** Its 4-video cell was void (the script named a video that scene lacks). Fixed
(the cell now publishes video 3 in every scene) and re-measured: 11,141 K at 4, 14,338 K at 24, 14,338 K at
48. So 24 vs 48 is 1.0001x, but **4 vs 48 is 1.29x and misses the 1.15x threshold**: not because of the count
but because in the larger scenes the cards of the next row cast shadows into that video's tiles. The cell
does not isolate "number of videos" as the card intended.

## Not verified

Metal timing for the a/v cell (the timed run stopped before it); a timed re-run under the lock.

Capture, loopback, application capture, device enumeration, resampling, mixing; audible output (only silence
was rendered); a real decoder; lip-sync against real content; CoreAudio's reported latency (it said 0, which
is the unit's own, not the device's: the device property is not read yet); behaviour when the default
device changes; any platform but macOS for the device backend.

## Measurement lane's findings that land in this lane

| finding | answer |
|---|---|
| Canvas 1,000 / 5,000 hover, theme, resize: 89 % of CPU instructions are shadows | Agreed, and outside what slice 1 changed. Their exact corner-coverage cache (`canvas-exact-shadow-cache.patch`) is applied here unchanged (commit "bough-kernel: exact CPU corner coverage reuse"): bough-kernel's suite is green with it and its mutant fails `tests/shadow_cache.rs`. Not re-measured here; their numbers (theme 162.71 -> 132.27 ms at 1,000, +13 MiB at 5,000 on N cores) stand as theirs. The larger answer is a retained shadow coverage as a layer (DESIGN section 1, last paragraph): not built. |
| One card, façade raster: a thick translucent stack dominates per-tile work; prefix checkpoint design | The staged path's stored prefix is that checkpoint for tiles inside a layer record's window (`Scene::seg`); it does not exist in the façade's generic executor, and general checkpoints (not tied to a backdrop) are the "deep stacks" item of slice 1's next card. |
| Spinner / ticker pacing (62.5 fps) | Owned by core in their index. This lane supplies `motion::FrameClock` and `Scene::animating()`; the façade's app clock is not in this lane's files. |
