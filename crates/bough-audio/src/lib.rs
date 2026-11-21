//! Audio for the media seam.
//!
//! The shape is Boilerpipe's (codeberg.org/AnEthernetCable/boilerpipe), taken for what it leaves out: a
//! stream has one sample rate, a direction ([`Flow`]), and **one callback**; the buffer exists only inside
//! the callback ([`Block`]) and is never cached across calls; everything a callback needs comes in with it.
//! There is no mixer graph, no device model in the way of the first sound, and no state the application
//! must mirror. What this crate adds to that shape is what a UI framework needs from audio and Boilerpipe
//! leaves to the application:
//!
//! * **An explicit clock** ([`Clock`]). Every block says where in the stream its first frame is and how
//!   long until it is heard; the sink stamps the clock with it. Anyone, on any thread, can ask which frame
//!   is audible at a given host instant. Nothing else in the crate keeps time.
//! * **Audio as the master clock of video** ([`sync::Pacer`]). The frame a video shows at an output refresh
//!   is the frame its audio is playing at that refresh's presentation instant: the same instant the frame
//!   clock turns into the `Time` word animations read. If audio stalls, video holds, and a tile that shows
//!   no new frame costs the raster nothing.
//! * **A lease for the producer** ([`ring`]). A decoder thread writes samples into slots the callback
//!   cannot yet see and commits them; the callback never blocks and never allocates. An underrun is
//!   silence and a counter, not a wait.
//! * **Sinks that are not devices** ([`NullSink`]): the test drives the callback and the clock by hand, so
//!   every gate is deterministic. A device ([`coreaudio`], behind a feature) is the same callback with a
//!   real clock.
//!
//! An application that plays no sound does not depend on this crate; `bough-paint` does not.
#[cfg(all(target_os = "macos", feature = "coreaudio"))]
pub mod coreaudio;
pub mod ring;
pub mod sync;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering::*};

/// A stream's format: frames per second, and samples per frame (interleaved 32-bit floats).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    pub rate: u32,
    pub channels: u16,
}
impl Spec {
    pub const STEREO_48K: Spec = Spec { rate: 48_000, channels: 2 };
}

/// Which way audio flows, from the stream's side (Boilerpipe's words).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    /// The stream receives audio from the application: playback.
    Receives,
    /// The stream sends audio to the application: capture (of a device, an output's loopback, or another
    /// application). Declared so the surface has its place; no sink here implements it yet.
    Sends,
}

/// What a callback is handed: the samples to fill (playback) for this call only, and where they are in time.
pub struct Block<'a> {
    /// `frames * channels` interleaved samples, zeroed (silence) on entry.
    pub samples: &'a mut [f32],
    pub spec: Spec,
    /// The stream position of the first frame: frames delivered before this block.
    pub position: u64,
    /// Frames between a sample being written here and being heard (the device's and the sink's delay).
    pub latency: u32,
}
impl Block<'_> {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.spec.channels.max(1) as usize
    }
}

/// The one callback. It runs on the sink's thread (a real-time thread for a device): no locks, no
/// allocation, no blocking. A closure is a `Render`.
pub trait Render: Send {
    fn render(&mut self, block: &mut Block<'_>);
}
impl<F: FnMut(&mut Block<'_>) + Send> Render for F {
    fn render(&mut self, block: &mut Block<'_>) {
        self(block)
    }
}

/// A stream's clock: the sink writes it after every block, anyone reads it. A sequence lock over plain
/// atomics: a reader never blocks the callback and never sees half a stamp.
#[derive(Default)]
struct Stamp {
    seq: AtomicU64,
    /// Frames delivered to the sink so far.
    frames: AtomicU64,
    /// The host time (ns) at which that count was taken.
    host_ns: AtomicU64,
    /// Rate (low 32 bits) and latency in frames (high 32 bits).
    rate_latency: AtomicU64,
    /// Frames of the last block: how far past a stamp the position may be extrapolated.
    period: AtomicU64,
    underruns: AtomicU64,
}
#[derive(Clone, Default)]
pub struct Clock(Arc<Stamp>);

/// One consistent reading of a clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading {
    pub frames: u64,
    pub host_ns: u64,
    pub rate: u32,
    pub latency: u32,
    pub period: u32,
}
impl Clock {
    pub fn new() -> Clock {
        Clock::default()
    }
    /// The sink's side: `frames` have been delivered in all as of `host_ns`.
    pub fn stamp(&self, frames: u64, host_ns: u64, spec: Spec, latency: u32, period: u32) {
        // Sequentially consistent throughout: with weaker orderings a reader's second look at the sequence
        // may be satisfied before its reads of the fields (seen as a torn stamp on arm64 by the gate).
        let s = &*self.0;
        s.seq.fetch_add(1, SeqCst);
        s.frames.store(frames, SeqCst);
        s.host_ns.store(host_ns, SeqCst);
        s.rate_latency.store(spec.rate as u64 | (latency as u64) << 32, SeqCst);
        s.period.store(period as u64, SeqCst);
        s.seq.fetch_add(1, SeqCst);
    }
    pub fn read(&self) -> Reading {
        let s = &*self.0;
        loop {
            let a = s.seq.load(SeqCst);
            let rl = s.rate_latency.load(SeqCst);
            let r = Reading { frames: s.frames.load(SeqCst), host_ns: s.host_ns.load(SeqCst), rate: rl as u32, latency: (rl >> 32) as u32, period: s.period.load(SeqCst) as u32 };
            if a & 1 == 0 && s.seq.load(SeqCst) == a {
                return r;
            }
            std::hint::spin_loop();
        }
    }
    /// The frame that is audible at host time `at_ns`: the delivered count, less the latency, carried
    /// forward at the stream's rate from the stamp, by at most one period (a stream whose callback has
    /// stopped coming is not playing: its position holds). 0 before anything is audible.
    pub fn audible_frame(&self, at_ns: u64) -> u64 {
        Clock::audible(&self.read(), at_ns)
    }
    #[inline]
    fn audible(r: &Reading, at_ns: u64) -> u64 {
        if r.rate == 0 {
            return 0;
        }
        // at most one period ahead, so the product stays far inside 64 bits
        let ns = at_ns.saturating_sub(r.host_ns).min(r.period as u64 * 1_000_000_000 / r.rate as u64 + 1);
        (r.frames + (ns * r.rate as u64 / 1_000_000_000).min(r.period as u64)).saturating_sub(r.latency as u64)
    }
    /// The same as media time in nanoseconds.
    pub fn media_ns(&self, at_ns: u64) -> u64 {
        let r = self.read();
        if r.rate == 0 {
            return 0;
        }
        (Clock::audible(&r, at_ns) as u128 * 1_000_000_000 / r.rate as u128) as u64
    }
    pub fn note_underrun(&self) {
        self.0.underruns.fetch_add(1, Relaxed);
    }
    pub fn underruns(&self) -> u64 {
        self.0.underruns.load(Relaxed)
    }
}

/// A sink with no device and no thread: the caller pumps it. Each pump is what a device's callback thread
/// would do (hand the callback a zeroed block, stamp the clock), at a host time the caller names, so a test
/// decides exactly when audio runs, at what rate, and when it stalls.
pub struct NullSink<R: Render> {
    pub spec: Spec,
    pub latency: u32,
    render: R,
    clock: Clock,
    position: u64,
    scratch: Vec<f32>,
    /// A fingerprint of every sample rendered (the gates compare runs).
    pub hash: u64,
}
impl<R: Render> NullSink<R> {
    pub fn new(spec: Spec, latency: u32, render: R) -> NullSink<R> {
        NullSink { spec, latency, render, clock: Clock::new(), position: 0, scratch: Vec::new(), hash: 0xcbf2_9ce4_8422_2325 }
    }
    pub fn clock(&self) -> Clock {
        self.clock.clone()
    }
    pub fn position(&self) -> u64 {
        self.position
    }
    /// Deliver one block of `frames` frames, finished at host time `host_ns`.
    pub fn pump(&mut self, frames: u32, host_ns: u64) {
        self.scratch.clear();
        self.scratch.resize(frames as usize * self.spec.channels as usize, 0.0);
        self.render.render(&mut Block { samples: &mut self.scratch, spec: self.spec, position: self.position, latency: self.latency });
        for s in &self.scratch {
            self.hash = (self.hash ^ s.to_bits() as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
        self.position += frames as u64;
        self.clock.stamp(self.position, host_ns, self.spec, self.latency, frames);
    }
}

/// A deterministic source: a triangle tone from an integer phase (the same bits on every machine).
#[derive(Clone, Copy, Debug)]
pub struct Tone {
    phase: u32,
    step: u32,
    /// Peak amplitude in 1 / 32768.
    pub level: u16,
}
impl Tone {
    pub fn new(hz: u32, rate: u32, level: u16) -> Tone {
        Tone { phase: 0, step: ((hz as u64) << 32).checked_div(rate as u64).unwrap_or(0) as u32, level }
    }
    #[inline]
    pub fn next(&mut self) -> f32 {
        let p = self.phase >> 16;
        self.phase = self.phase.wrapping_add(self.step);
        let tri = if p < 32768 { p as i32 - 16384 } else { 49152 - p as i32 };
        (tri * self.level as i32) as f32 / (16384.0 * 32768.0)
    }
    /// Fill a block, every channel the same.
    pub fn fill(&mut self, b: &mut Block<'_>) {
        let ch = b.spec.channels.max(1) as usize;
        for f in b.samples.chunks_mut(ch) {
            f.fill(self.next());
        }
    }
}
