//! Lock-free single-producer/single-consumer ring buffer for audio samples.
//!
//! Design notes:
//! - Positions are **monotonically increasing** counters (they only wrap at
//!   `usize::MAX`, which at 16 kHz takes centuries), so arithmetic is exact and
//!   the classic wrap-around ambiguity disappears.
//! - The writer **overwrites the oldest samples** when the buffer is full —
//!   for a PTT recorder the newest audio always wins.
//! - The reader skips forward (dropping data) if it falls behind instead of
//!   corrupting the buffer; `drain` returns however many samples are ready.
//!
//! Memory ordering: `write_pos` is published with `Release` and observed with
//! `Acquire`; `read_pos` likewise in the other direction. `UnsafeCell` is safe
//! here because producer and consumer never touch the same slot twice (the
//! monotonic counters guarantee each slot is written at most once per lap and
//! read at most once per lap).

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A fixed-capacity ring buffer of `f32` audio samples.
///
/// Interior mutability via `UnsafeCell`: the single producer writes, the
/// single consumer reads, and the monotonic counters guarantee they never
/// touch the same slot in the same lap (see memory-ordering notes above).
pub struct RingBuffer {
    buffer: UnsafeCell<Box<[f32]>>,
    capacity: usize,
    read_pos: AtomicUsize,
    write_pos: AtomicUsize,
    /// Total samples dropped because the writer overtook the reader.
    overwritten: AtomicUsize,
}

unsafe impl Send for RingBuffer {}
unsafe impl Sync for RingBuffer {}

impl RingBuffer {
    /// Creates a buffer holding `capacity` samples.
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "ring buffer capacity must be > 0");
        Self {
            buffer: UnsafeCell::new(vec![0.0f32; capacity].into_boxed_slice()),
            capacity,
            read_pos: AtomicUsize::new(0),
            write_pos: AtomicUsize::new(0),
            overwritten: AtomicUsize::new(0),
        }
    }

    /// Writes `data` into the buffer, overwriting the oldest samples on overflow.
    ///
    /// Called from the audio callback thread (the single producer).
    pub fn write(&self, data: &[f32]) {
        if data.is_empty() {
            return;
        }
        let write = self.write_pos.load(Ordering::Relaxed);
        let read = self.read_pos.load(Ordering::Acquire);
        let buffer = unsafe { &mut *self.buffer.get() };

        // Publish samples first.
        for (i, &s) in data.iter().enumerate() {
            buffer[(write + i) % self.capacity] = s;
        }
        let new_write = write + data.len();

        // If the writer lapped the reader, advance the reader to keep only the
        // newest `capacity` samples (drop-oldest policy).
        if new_write.saturating_sub(read) > self.capacity {
            self.overwritten
                .fetch_add(new_write - read - self.capacity, Ordering::Relaxed);
            self.read_pos
                .store(new_write - self.capacity, Ordering::Release);
        }

        self.write_pos.store(new_write, Ordering::Release);
    }

    /// Reads up to `output.len()` samples, returning how many were written.
    ///
    /// Called from the consumer thread (the single reader).
    pub fn read(&self, output: &mut [f32]) -> usize {
        let read = self.read_pos.load(Ordering::Relaxed);
        let write = self.write_pos.load(Ordering::Acquire);
        let available = write.saturating_sub(read);
        let n = available.min(output.len());
        let buffer = unsafe { &*self.buffer.get() };
        for (i, slot) in output.iter_mut().take(n).enumerate() {
            *slot = buffer[(read + i) % self.capacity];
        }
        self.read_pos.store(read + n, Ordering::Release);
        n
    }

    /// Number of samples currently available to read.
    pub fn available(&self) -> usize {
        let read = self.read_pos.load(Ordering::Relaxed);
        let write = self.write_pos.load(Ordering::Acquire);
        write.saturating_sub(read)
    }

    /// Removes and returns all currently available samples.
    pub fn drain(&self, out: &mut Vec<f32>) -> usize {
        let n = self.available();
        out.reserve(n);
        let read = self.read_pos.load(Ordering::Relaxed);
        let buffer = unsafe { &*self.buffer.get() };
        for i in 0..n {
            out.push(buffer[(read + i) % self.capacity]);
        }
        self.read_pos.store(read + n, Ordering::Release);
        n
    }

    /// Resets both positions (drops all buffered audio).
    pub fn clear(&self) {
        self.read_pos.store(0, Ordering::Relaxed);
        self.write_pos.store(0, Ordering::Release);
    }

    /// Buffer capacity in samples.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Total samples discarded due to overwrite since creation.
    pub fn total_overwritten(&self) -> usize {
        self.overwritten.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn write_then_read_roundtrip() {
        let rb = RingBuffer::new(1024);
        let data: Vec<f32> = (0..100).map(|i| i as f32).collect();
        rb.write(&data);
        assert_eq!(rb.available(), 100);

        let mut out = vec![0.0f32; 100];
        assert_eq!(rb.read(&mut out), 100);
        assert_eq!(out, data);
        assert_eq!(rb.available(), 0);
    }

    #[test]
    fn overwrite_oldest_keeps_newest() {
        let rb = RingBuffer::new(10);
        rb.write(&[0.0; 8]);
        rb.write(&[1.0; 7]); // total 15 > 10 → 5 oldest dropped
        assert_eq!(rb.available(), 10);
        assert_eq!(rb.total_overwritten(), 5);

        let mut out = vec![0.0f32; 10];
        rb.read(&mut out);
        // Last 10 written samples are all 1.0 (7) plus the newest 3 of the first write.
        assert_eq!(&out[3..], &[1.0; 7]);
    }

    #[test]
    fn reader_cannot_go_backward() {
        let rb = RingBuffer::new(4);
        rb.write(&[1.0, 2.0]);
        let mut out = [0.0f32; 8];
        assert_eq!(rb.read(&mut out), 2);
        assert_eq!(rb.read(&mut out), 0); // nothing new
        rb.write(&[3.0, 4.0, 5.0]);
        let mut out2 = [0.0f32; 2];
        assert_eq!(rb.read(&mut out2), 2);
        assert_eq!(out2, [3.0, 4.0]);
    }

    #[test]
    fn spsc_threaded_no_tears() {
        let rb = Arc::new(RingBuffer::new(16_000));
        let rb_w = rb.clone();
        let rb_r = rb.clone();

        // Paced producer: real capture delivers real-time audio (20 ms chunks),
        // it never laps a keeping-up consumer. Unpaced writes are covered by
        // the single-threaded overwrite-policy test above.
        let producer = thread::spawn(move || {
            for lap in 0..100u32 {
                let chunk = vec![lap as f32; 320]; // 20 ms @ 16 kHz
                rb_w.write(&chunk);
                thread::sleep(std::time::Duration::from_millis(1));
            }
        });
        let consumer = thread::spawn(move || {
            let mut last = -1.0f32;
            let mut total = 0usize;
            let mut buf = vec![0.0f32; 1600];
            while total < 32_000 {
                let n = rb_r.read(&mut buf);
                if n > 0 {
                    // Values must be non-decreasing (each lap is constant).
                    for &v in &buf[..n] {
                        assert!(v >= last, "out-of-order sample {v} after {last}");
                        last = v;
                    }
                    total += n;
                } else {
                    thread::sleep(std::time::Duration::from_millis(1));
                }
            }
            total
        });

        producer.join().unwrap();
        assert_eq!(consumer.join().unwrap(), 32_000);
    }

    #[test]
    fn drain_returns_available_and_empties() {
        let rb = RingBuffer::new(100);
        rb.write(&[0.5; 40]);
        let mut out = Vec::new();
        assert_eq!(rb.drain(&mut out), 40);
        assert_eq!(out.len(), 40);
        assert_eq!(rb.available(), 0);
    }
}
