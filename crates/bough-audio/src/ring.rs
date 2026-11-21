//! A single-producer, single-consumer ring of samples between a decoder and the callback.
//!
//! The producer **leases**: it writes into slots past the committed head, which the consumer cannot see,
//! and then commits them with one store. The consumer (the audio callback) takes what is committed and
//! never waits: what is missing is silence, counted as an underrun by the caller. Samples are stored as
//! their bits in atomics, so the whole ring is safe code; the two ends are separate owned values, which is
//! what makes "single producer, single consumer" a property of the types.
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering::*};

struct Shared {
    buf: Box<[AtomicU32]>,
    /// Samples committed in all (wrapping), and samples consumed in all.
    head: AtomicUsize,
    tail: AtomicUsize,
}
pub struct Producer(Arc<Shared>);
pub struct Consumer(Arc<Shared>);

/// A ring holding `capacity` samples (not frames; rounded up to a power of two).
pub fn ring(capacity: usize) -> (Producer, Consumer) {
    let cap = capacity.max(2).next_power_of_two();
    let s = Arc::new(Shared { buf: (0..cap).map(|_| AtomicU32::new(0)).collect(), head: AtomicUsize::new(0), tail: AtomicUsize::new(0) });
    (Producer(s.clone()), Consumer(s))
}

impl Producer {
    /// Samples that can be written now.
    pub fn free(&self) -> usize {
        self.0.buf.len() - self.0.head.load(Relaxed).wrapping_sub(self.0.tail.load(Acquire))
    }
    /// Lease up to `n` samples, fill them through `f` (given each sample's index in the lease), and commit.
    /// Returns how many were written (fewer than `n` when the ring is that full: the decoder keeps the rest).
    pub fn write_with(&mut self, n: usize, mut f: impl FnMut(usize) -> f32) -> usize {
        let s = &*self.0;
        let (head, mask) = (s.head.load(Relaxed), s.buf.len() - 1);
        let n = n.min(self.free());
        for i in 0..n {
            s.buf[head.wrapping_add(i) & mask].store(f(i).to_bits(), Relaxed);
        }
        s.head.store(head.wrapping_add(n), Release);
        n
    }
    pub fn write(&mut self, samples: &[f32]) -> usize {
        self.write_with(samples.len(), |i| samples[i])
    }
}
impl Consumer {
    /// Samples committed and not yet taken.
    pub fn available(&self) -> usize {
        self.0.head.load(Acquire).wrapping_sub(self.0.tail.load(Relaxed))
    }
    /// Take up to `out.len()` samples into the front of `out`; the rest of `out` is left as it was (the
    /// callback's block is silence there). Returns how many were taken.
    pub fn read(&mut self, out: &mut [f32]) -> usize {
        let s = &*self.0;
        let (tail, mask) = (s.tail.load(Relaxed), s.buf.len() - 1);
        let n = out.len().min(self.available());
        for (i, o) in out[..n].iter_mut().enumerate() {
            *o = f32::from_bits(s.buf[tail.wrapping_add(i) & mask].load(Relaxed));
        }
        s.tail.store(tail.wrapping_add(n), Release);
        n
    }
}
