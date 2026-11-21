//! The audio gates (card 2026-11-21-audio-seam-slice2): the ring, the clock, the pacer.
use bough_audio::ring::ring;
use bough_audio::sync::Pacer;
use bough_audio::{Block, Clock, NullSink, Spec, Tone};

const SPEC: Spec = Spec::STEREO_48K;

/// A1: one producer thread, one consumer thread, a million samples: each delivered once, in order. Then an
/// underrun: what is missing stays silence.
#[test]
fn ring_delivers_every_sample_once_in_order() {
    let (mut tx, mut rx) = ring(1024);
    const N: usize = 1_000_000;
    let producer = std::thread::spawn(move || {
        let mut i = 0;
        while i < N {
            let w = tx.write_with((N - i).min(300), |k| (i + k) as f32);
            i += w;
            if w == 0 {
                std::thread::yield_now();
            }
        }
    });
    let (mut next, mut buf) = (0usize, [0f32; 257]);
    while next < N {
        let n = rx.read(&mut buf);
        for &s in &buf[..n] {
            assert_eq!(s, next as f32, "sample {next} out of order");
            next += 1;
        }
        if n == 0 {
            std::thread::yield_now();
        }
    }
    producer.join().unwrap();
    assert_eq!(rx.available(), 0);
    // underrun: the callback's block keeps its silence past what the ring had
    let (mut tx, mut rx) = ring(64);
    tx.write(&[1.0; 10]);
    let mut block = [0f32; 32];
    assert_eq!(rx.read(&mut block), 10);
    assert!(block[..10].iter().all(|&s| s == 1.0) && block[10..].iter().all(|&s| s == 0.0));
    // a full ring refuses the rest rather than overwrite
    assert_eq!(tx.write(&[2.0; 100]), 64);
    assert_eq!(tx.write(&[3.0; 1]), 0);
}

/// A2: the clock is exact integer arithmetic, holds when the callback stops coming, and a reader on another
/// thread never sees half a stamp.
#[test]
fn clock_is_exact_and_holds_on_a_stall() {
    let mut sink = NullSink::new(SPEC, 480, |b: &mut Block| b.samples.fill(0.25));
    let clock = sink.clock();
    assert_eq!(clock.audible_frame(123), 0);
    // 10 ms blocks, each finished exactly on time
    for k in 1..=100u64 {
        sink.pump(480, k * 10_000_000);
    }
    assert_eq!(sink.position(), 48_000);
    // at the stamp: delivered less latency
    assert_eq!(clock.audible_frame(1_000_000_000), 48_000 - 480);
    // 5 ms later: 240 frames on
    assert_eq!(clock.audible_frame(1_005_000_000), 48_000 - 480 + 240);
    assert_eq!(clock.media_ns(1_005_000_000), (48_000u64 - 480 + 240) * 1_000_000_000 / 48_000);
    // a second later with no callback: one period on, no more (the stream is not playing)
    assert_eq!(clock.audible_frame(2_000_000_000), 48_000 - 480 + 480);
    // monotone under jitter: callbacks early and late by up to 3 ms
    let mut last = 0;
    let mut g = 12345u64;
    for k in 101..=400u64 {
        g = g.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let jitter = (g >> 40) % 6_000_000;
        let t = k * 10_000_000 + jitter - 3_000_000;
        sink.pump(480, t);
        let a = clock.audible_frame(t);
        assert!(a >= last, "the clock went back at block {k}");
        last = a;
    }
    // a torn read would show a frame count from one stamp with a host time from another
    let c2 = clock.clone();
    let reader = std::thread::spawn(move || {
        for _ in 0..200_000 {
            let r = c2.read();
            assert_eq!(r.host_ns % 1000, r.frames % 1000, "torn stamp: {r:?}");
        }
    });
    let writer = Clock::clone(&clock);
    for k in 0..200_000u64 {
        writer.stamp(k, 5_000_000 + k, SPEC, 0, 480);
    }
    reader.join().unwrap();
}

/// What a 120 Hz output would show of a 30 fps video paced by `clock`, refresh by refresh.
fn play(sink: &mut NullSink<impl bough_audio::Render>, pacer: &mut Pacer, refreshes: u64, audio_ns_per_block: u64, stall: std::ops::Range<u64>) -> (Vec<u64>, u64) {
    let clock = sink.clock();
    let (mut shown, mut published, mut next_block) = (Vec::new(), 0, 0u64);
    for r in 0..refreshes {
        let present = r * 1_000_000_000 / 120;
        // the device's callbacks up to this instant (none during the stall)
        while next_block <= present {
            if !stall.contains(&r) {
                sink.pump(480, next_block);
            }
            next_block += audio_ns_per_block;
        }
        if let Some(f) = pacer.at(&clock, present) {
            published += 1;
            shown.push(f);
        } else {
            shown.push(pacer.shown().unwrap_or(0));
        }
    }
    (shown, published)
}

/// A3: video follows the audio clock. A device running fast or slow drags the video with it; a stall holds
/// the frame and publishes nothing; a seek republishes once.
#[test]
fn video_frames_follow_the_audio_clock() {
    // nominal: 480 frames every 10 ms; 0.5 % fast: every 9.95 ms of host time; 0.5 % slow: 10.05 ms
    for (name, block_ns) in [("nominal", 10_000_000u64), ("fast", 9_950_000), ("slow", 10_050_000)] {
        let mut tone = Tone::new(440, SPEC.rate, 8000);
        let mut sink = NullSink::new(SPEC, 0, move |b: &mut Block| tone.fill(b));
        let mut pacer = Pacer::new(30, 1);
        let (shown, published) = play(&mut sink, &mut pacer, 1200, block_ns, 0..0);
        // 10 s of output: the video is where the audio is, not where the wall clock is
        let audio_s = sink.position() as f64 / SPEC.rate as f64;
        let last = *shown.last().unwrap();
        eprintln!("{name}: {} frames of audio ({audio_s:.3} s) in 10 s of output; video at frame {last}, {published} published", sink.position());
        assert!((last as f64 - audio_s * 30.0).abs() <= 1.5, "{name}: video frame {last} against audio {audio_s:.3} s");
        if name != "nominal" {
            assert!((last as i64 - 299).abs() >= 1, "{name}: the video ignored the device's rate");
        }
        // never backwards, never skipping more than one frame at 120 Hz
        assert!(shown.windows(2).all(|w| w[1] >= w[0] && w[1] - w[0] <= 1), "{name}: a jump");
        assert_eq!(published, last + 1, "{name}: every frame up to the last was published exactly once");
    }
    // a stall: refreshes 240..480 (two seconds) deliver no audio
    let mut sink = NullSink::new(SPEC, 0, |_: &mut Block| {});
    let mut pacer = Pacer::new(30, 1);
    let (shown, published) = play(&mut sink, &mut pacer, 720, 10_000_000, 240..480);
    let (before, during_first, during_last, after) = (shown[239], shown[245], shown[479], shown[719]);
    eprintln!("stall: frame {before} before, {during_first}..{during_last} during, {after} at the end; {published} published");
    assert!(during_last - during_first == 0 && during_first - before <= 1, "the video ran on without audio");
    // and it resumes from where the audio stopped: four seconds of audio in six of output
    assert!((119..=121).contains(&after) && published == after + 1);
    // a seek: the next refresh shows the target's frame, once
    let clock = sink.clock();
    let now = 6_000_000_000;
    pacer.seek(&clock, now, 60_000_000_000);
    assert!(matches!(pacer.at(&clock, now), Some(1799..=1800)));
    assert_eq!(pacer.at(&clock, now), None);
    // the same script twice renders the same samples (the source and the sink are deterministic)
    let run = || {
        let mut tone = Tone::new(997, SPEC.rate, 30_000);
        let mut sink = NullSink::new(SPEC, 64, move |b: &mut Block| tone.fill(b));
        (0..50).for_each(|k| sink.pump(441, k * 9_187_500));
        sink.hash
    };
    assert_eq!(run(), run());
}

/// A5: CoreAudio's default output drives the same clock (silence; no capture, so no permission).
#[cfg(all(target_os = "macos", feature = "coreaudio"))]
#[test]
fn coreaudio_output_advances_the_clock() {
    use bough_audio::coreaudio::{Output, host_now_ns};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
    let calls = Arc::new(AtomicU64::new(0));
    let c = calls.clone();
    // the block arrives zeroed and is left so: nothing is heard
    let Some(out) = Output::start(SPEC, move |b: &mut Block| {
        assert!(b.samples.iter().all(|&s| s == 0.0));
        c.fetch_add(1, Relaxed);
    }) else {
        eprintln!("coreaudio: no default output could be started: SKIPPED");
        return;
    };
    let clock = out.clock();
    std::thread::sleep(std::time::Duration::from_millis(150));
    let (t0, f0) = (host_now_ns(), clock.read().frames);
    std::thread::sleep(std::time::Duration::from_millis(400));
    let (t1, f1) = (host_now_ns(), clock.read().frames);
    let rate = (f1 - f0) as f64 * 1e9 / (t1 - t0) as f64;
    eprintln!("coreaudio: {} callbacks, period {} frames, latency {} frames, measured {rate:.0} frames/s against {}", calls.load(Relaxed), clock.read().period, out.latency, SPEC.rate);
    assert!(calls.load(Relaxed) > 4, "the render callback did not run");
    assert!((rate / SPEC.rate as f64 - 1.0).abs() < 0.05, "the clock ran at {rate:.0} frames/s");
    let mut pacer = Pacer::new(30, 1);
    assert!(pacer.at(&clock, host_now_ns()).is_some());
    drop(out);
    let after = calls.load(Relaxed);
    std::thread::sleep(std::time::Duration::from_millis(60));
    assert_eq!(calls.load(Relaxed), after, "the callback ran after the output was dropped");
}
