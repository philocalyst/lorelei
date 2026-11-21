//! Audio paces video.
//!
//! At every output refresh the host knows the instant the frame it is drawing will be shown (the same
//! instant `motion::FrameClock::present` turns into the `Time` word). A [`Pacer`] asks the audio clock which
//! frame of sound is audible at that instant and answers with the video frame that belongs to it. A new
//! index is a frame to publish (one record word: the video's window is damaged); the same index is
//! nothing at all. The video never reads a wall clock: a device that runs 0.3 % fast, a stall, a pause and
//! a seek are all just what the audio clock says.
use crate::Clock;

/// A video's frame rate as a ratio, and the last frame it was told to show.
#[derive(Clone, Copy, Debug)]
pub struct Pacer {
    pub fps_num: u32,
    pub fps_den: u32,
    /// Media time (ns) of the stream's frame 0 (a seek moves it).
    pub origin_ns: i64,
    shown: Option<u64>,
    #[cfg(feature = "mutant_wall_clock")]
    wall0: Option<u64>,
}
impl Pacer {
    pub fn new(fps_num: u32, fps_den: u32) -> Pacer {
        Pacer {
            fps_num,
            fps_den: fps_den.max(1),
            origin_ns: 0,
            shown: None,
            #[cfg(feature = "mutant_wall_clock")]
            wall0: None,
        }
    }
    /// The video frame that belongs to media time `media_ns`.
    #[inline]
    pub fn frame_of(&self, media_ns: u64) -> u64 {
        let t = (media_ns as i128 + self.origin_ns as i128).max(0) as u128;
        (t * self.fps_num as u128 / (self.fps_den as u128 * 1_000_000_000)) as u64
    }
    /// The frame to show at an output refresh presented at `present_ns` (host time), by the audio clock.
    /// `Some(index)` when it differs from the one last returned (publish it); `None` when the video keeps
    /// the frame it shows.
    #[inline]
    pub fn at(&mut self, clock: &Clock, present_ns: u64) -> Option<u64> {
        #[cfg(not(feature = "mutant_wall_clock"))]
        let media = clock.media_ns(present_ns);
        #[cfg(feature = "mutant_wall_clock")]
        let media = {
            let _ = clock;
            present_ns - *self.wall0.get_or_insert(present_ns)
        };
        let f = self.frame_of(media);
        if self.shown == Some(f) {
            return None;
        }
        self.shown = Some(f);
        Some(f)
    }
    /// Jump: media time `to_ns` is now what the audio clock's current position plays (the next `at`
    /// republishes).
    pub fn seek(&mut self, clock: &Clock, now_ns: u64, to_ns: u64) {
        self.origin_ns = to_ns as i64 - clock.media_ns(now_ns) as i64;
        self.shown = None;
    }
    pub fn shown(&self) -> Option<u64> {
        self.shown
    }
}
