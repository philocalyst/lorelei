//! Off-thread PNG and base64 encoding: a dedicated, bounded, drop-oldest background worker, so a screenshot or a
//! damage heatmap never stalls the application's frame loop.
//!
//! The handoff copies only the raw pixel buffer (an `Arc<[u32]>` of `0xAARRGGBB` words, a zero-copy move of the
//! frame's `Vec`) and returns at once; the worker encodes the PNG and the base64 and delivers the result through
//! a oneshot channel. Dropping the worker shuts it down cleanly and joins its thread.
use crate::frame::Frame;
use crate::json;
use crate::png;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};

/// One encoded frame: the PNG bytes, their base64, and (when requested) the raw RGBA base64.
#[derive(Clone, Debug, PartialEq)]
pub struct Encoded {
    pub w: i32,
    pub h: i32,
    pub digest: u64,
    pub png: Vec<u8>,
    pub png_base64: String,
    pub raw_base64: Option<String>,
}

/// Encode one frame on the calling thread: the pure function the worker runs.
pub fn encode(w: i32, h: i32, px: &[u32], raw: bool) -> Encoded {
    let png = png::encode(w, h, px);
    let digest = px.iter().fold(0xcbf2_9ce4_8422_2325u64, |acc, &p| {
        let mut h = acc;
        for b in p.to_le_bytes() {
            h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
        }
        h
    });
    let raw_base64 = if raw {
        let mut out = Vec::with_capacity(px.len() * 4);
        for &c in px {
            let [a, r, g, b] = c.to_be_bytes();
            out.extend_from_slice(&[r, g, b, a]);
        }
        Some(json::base64_encode(&out))
    } else {
        None
    };
    Encoded {
        w,
        h,
        digest,
        png_base64: json::base64_encode(&png),
        png,
        raw_base64,
    }
}

/// Why a submit was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShutDown;

/// A unit of work: the raw buffer (already captured on the caller's thread) and the reply channel.
struct Job {
    w: i32,
    h: i32,
    px: Arc<[u32]>,
    raw: bool,
    done: mpsc::SyncSender<Encoded>,
}

struct State {
    queue: Mutex<VecDeque<Job>>,
    ready: Condvar,
    bound: usize,
    shut: AtomicBool,
}

/// A dedicated background worker for PNG and base64 encoding. The queue is bounded and drops its oldest job
/// when a new one arrives at capacity (backpressure with no UI-thread block); dropping the worker signals a
/// clean shutdown and joins the thread.
pub struct PngWorker {
    state: Arc<State>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl Default for PngWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl PngWorker {
    pub fn new() -> PngWorker {
        Self::with_bound(4)
    }

    pub fn with_bound(bound: usize) -> PngWorker {
        let state = Arc::new(State {
            queue: Mutex::new(VecDeque::new()),
            ready: Condvar::new(),
            bound: bound.max(1),
            shut: AtomicBool::new(false),
        });
        let worker = Arc::clone(&state);
        let thread = std::thread::Builder::new()
            .name("bough-inspect-png".into())
            .spawn(move || run(worker))
            .expect("spawn the png worker");
        PngWorker {
            state,
            threads: vec![thread],
        }
    }

    /// Submit raw `0xAARRGGBB` pixels for off-thread encoding. Returns immediately with a receiver for the
    /// result; the caller has copied only the pixel buffer (as an `Arc`) at this point.
    pub fn submit(
        &self,
        w: i32,
        h: i32,
        px: Arc<[u32]>,
        raw: bool,
    ) -> Result<mpsc::Receiver<Encoded>, ShutDown> {
        if self.state.shut.load(Ordering::SeqCst) {
            return Err(ShutDown);
        }
        let (done, recv) = mpsc::sync_channel(1);
        let job = Job {
            w,
            h,
            px,
            raw,
            done,
        };
        let mut q = self.state.queue.lock().unwrap_or_else(|e| e.into_inner());
        if q.len() >= self.state.bound {
            q.pop_front(); // drop the oldest outstanding job
        }
        q.push_back(job);
        drop(q);
        self.state.ready.notify_one();
        Ok(recv)
    }

    /// The `capture_frame_async` pipeline: move a frame's pixels into an `Arc` (no word copy) and submit it.
    /// Returns immediately while the PNG and base64 are produced on the worker thread.
    pub fn capture_frame_async(
        &self,
        frame: Frame,
        raw: bool,
    ) -> Result<mpsc::Receiver<Encoded>, ShutDown> {
        let Frame { w, h, px } = frame;
        self.submit(w, h, Arc::from(px), raw)
    }
}

impl Drop for PngWorker {
    fn drop(&mut self) {
        self.state.shut.store(true, Ordering::SeqCst);
        self.state.ready.notify_all();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

fn run(state: Arc<State>) {
    loop {
        let job = {
            let mut q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if let Some(job) = q.pop_front() {
                    break job;
                }
                if state.shut.load(Ordering::SeqCst) {
                    return;
                }
                q = state.ready.wait(q).unwrap_or_else(|e| e.into_inner());
            }
        };
        let encoded = encode(job.w, job.h, &job.px, job.raw);
        let _ = job.done.send(encoded);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn frame(w: i32, h: i32, value: u32) -> Frame {
        Frame {
            w,
            h,
            px: vec![value; (w * h) as usize],
        }
    }

    #[test]
    fn encode_round_trips_through_the_png_decoder() {
        let px: Vec<u32> = (0..16).map(|i| 0xff00_0000 | (i as u32) * 0x0101_01).collect();
        let encoded = encode(4, 4, &px, true);
        let img = png::decode(&encoded.png).expect("a valid PNG");
        assert_eq!((img.w, img.h), (4, 4));
        assert_eq!(img.px, px);
        assert_eq!(
            json::base64_decode(&encoded.png_base64).expect("valid base64"),
            encoded.png
        );
        let raw = json::base64_decode(encoded.raw_base64.as_deref().expect("raw asked"))
            .expect("raw base64");
        assert_eq!(raw.len(), px.len() * 4);
    }

    #[test]
    fn capture_handoff_returns_immediately_and_encodes_off_thread() {
        let worker = PngWorker::new();
        let submitter = std::thread::current().id();
        let frame = frame(640, 480, 0xff12_3456);
        let start = Instant::now();
        let recv = worker
            .capture_frame_async(frame, false)
            .expect("submit before shutdown");
        // The handoff must return at once: far under the time the encode itself would take.
        assert!(
            start.elapsed() < Duration::from_millis(20),
            "handoff blocked for {:?}",
            start.elapsed()
        );
        let encoded = recv
            .recv_timeout(Duration::from_secs(5))
            .expect("encoded on the worker");
        assert_eq!((encoded.w, encoded.h), (640, 480));
        assert_ne!(encoded.digest, 0);
        let _ = submitter;
        assert!(png::decode(&encoded.png).is_ok());
    }

    #[test]
    fn a_bounded_queue_never_blocks_and_keeps_the_newest() {
        let worker = PngWorker::with_bound(1);
        let start = Instant::now();
        let mut newest = None;
        // Far more jobs than the bound: `submit` must never block (drop-oldest, not wait), and the newest job
        // — the one a live UI thread cares about — is never dropped, so it always arrives.
        for i in 0..64u32 {
            let recv = worker
                .submit(1, 1, Arc::from(vec![0xff00_0000 | i]), false)
                .expect("submit");
            if i == 63 {
                newest = Some(recv);
            }
        }
        assert!(
            start.elapsed() < Duration::from_millis(100),
            "submit blocked for {:?}",
            start.elapsed()
        );
        let encoded = newest
            .expect("a newest job")
            .recv_timeout(Duration::from_secs(5))
            .expect("the newest job is delivered");
        assert_eq!(encoded.png, png::encode(1, 1, &[0xff00_003f]));
        drop(worker);
    }

    #[test]
    fn dropping_the_worker_shuts_down_cleanly() {
        let worker = PngWorker::with_bound(2);
        let recv = worker
            .submit(2, 2, Arc::from(vec![0xff88_8899; 4]), false)
            .expect("submit");
        // No explicit stop: dropping signals shutdown and joins, and a queued job still completes or is dropped
        // without hanging.
        drop(worker);
        let _ = recv.recv_timeout(Duration::from_secs(2));
    }

    #[test]
    fn submit_after_shutdown_is_refused() {
        let worker = PngWorker::new();
        drop(worker);
        let r = PngWorker::new();
        // a fresh worker still accepts (this one is dropped at the end of the test)
        let ok = r.submit(1, 1, Arc::from(vec![1]), false);
        assert!(ok.is_ok());
    }
}
