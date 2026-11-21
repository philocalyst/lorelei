# Card: the audio side of the media seam, and audio as the clock that paces video (slice 2)

```
lane:            style-motion-media
state:           CARD
owner:           fx session (Claude, claude-fable-5-2), 2026-11-21
predecessor:     0a92f72e54 (nightly-2026-10-17; release for gates, dist for numbers)
question:        Can audio enter the media seam as a tiny typed callback/lease interface with an explicit clock
                 and latency, such that a video's frames are chosen by its audio clock through the same
                 output-paced Time the animations use, with no audio code in an application that plays none?
change:          A new crate `bough-audio` with no dependencies (inspired by Boilerpipe's surface: a session,
                 ports by flow, a connection whose buffer exists only inside the callback, one sample rate):
                 `Spec`, `Flow`, a render callback over a `Block` (interleaved f32, its first frame's stream
                 position, the latency to the ear), a lock-free single-producer ring a decoder leases into,
                 a `Clock` the callback advances (frames delivered, host time, latency), `sync::Pacer`
                 (presentation instant -> media time -> video frame index; hold on stall), a deterministic
                 `Tone` source, a `NullSink` driven by the test (no thread, no device), and CoreAudio's default
                 output unit behind the `coreaudio` feature. bough-paint gains nothing but a dev-dependency:
                 the media benchmark gives each video tile an audio clock.
implication:     Passing establishes the interface, the clock arithmetic and the A/V pacing rule on synthetic
                 sources, and that a real output device can drive the same clock. It does not establish
                 capture, loopback, application capture, device enumeration, resampling, mixing, or audible
                 quality; nor lip-sync against a real decoder.
platforms:       macOS (CoreAudio, output only, silence, headless); everything else platform-free.
workloads:       tests in crates/bough-audio; `fxbench media N` cell "a/v playback" (N tiles, each with its own
                 audio clock at 48 kHz with a per-tile rate skew of up to +-0.5 %, 30 fps content, 120 Hz output).
gates:           A1 ring: single producer / single consumer over 10^6 frames across two threads, every sample
                    delivered once in order; underrun delivers silence and is counted.
                 A2 clock: position, latency and extrapolation are exact integers; monotone under callback jitter.
                 A3 pacing: the frame shown at every output refresh is floor(media_time * fps) of the audio
                    clock (never the wall clock): with a skewed audio rate the video follows audio; when audio
                    stalls the video holds and publishes nothing; a seek republishes once.
                 A4 negative control: `mutant_wall_clock` (the pacer reads output time instead of the audio
                    clock) must fail A3.
                 A5 CoreAudio (if a default output exists and starts without a prompt): the callback runs, the
                    clock advances at the device rate within 2 % over 300 ms, rendering silence. Skipped, and
                    reported as skipped, otherwise.
                 A6 no dependency: `cargo tree -p bough-paint -e normal` does not contain bough-audio.
                 A7 the media scene with audio clocks stays retained == full redraw, CPU == Metal.
thresholds:      T1 a video tile that shows no new frame at a refresh costs 0 tiles and 0 records.
                 T2 frames published over 2 s of output == sum over tiles of the frames their audio clocks
                    crossed (exactly).
                 T3 pacer cost <= 100 instructions per tile per refresh (it is arithmetic on three words).
measurement:     instructions of the frame thread (pmu); counts are exact.
budget:          this session.
stop conditions: CoreAudio cannot be started headless without a prompt: record METHOD_BOUNDARY for A5, keep
                 the null sink as the gate.
do not revisit:  present paths with large commit costs (AVSampleBufferDisplayLayer is settled against).
```
