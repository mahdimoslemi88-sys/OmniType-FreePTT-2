//! Integration tests for the audio layer (public API).

use std::sync::Arc;
use std::thread;
use voice_ptt::audio::{list_input_devices, RingBuffer};

#[test]
fn ring_buffer_threaded_pipeline_preserves_order() {
    let rb = Arc::new(RingBuffer::new(16_000 * 30));
    let w = rb.clone();
    let r = rb.clone();

    let producer = thread::spawn(move || {
        for lap in 0..50u32 {
            w.write(&vec![lap as f32; 512]);
            // Paced like real-time capture so the consumer never falls behind.
            thread::sleep(std::time::Duration::from_millis(1));
        }
    });

    let consumer = thread::spawn(move || {
        let mut seen = Vec::new();
        let mut buf = vec![0.0f32; 2048];
        let mut total = 0usize;
        while total < 25_600 {
            let n = r.read(&mut buf);
            if n > 0 {
                seen.push(buf[0]);
                total += n;
            } else {
                thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        seen
    });

    producer.join().unwrap();
    let seen = consumer.join().unwrap();

    // Every chunk boundary must be observed in order (monotonic laps).
    let mut last = -1.0;
    for &v in &seen {
        assert!(v >= last, "order violation: {v} after {last}");
        last = v;
    }
}

#[test]
fn ring_buffer_overwrite_policy_bounds_memory() {
    let rb = RingBuffer::new(16_000); // 1 second
    // Write 5 seconds of audio — memory must stay bounded by capacity.
    for _ in 0..5 {
        rb.write(&vec![0.1f32; 16_000]);
    }
    assert_eq!(rb.available(), 16_000);
    assert_eq!(rb.total_overwritten(), 4 * 16_000);
}

/// Device enumeration must not panic even on machines without mics.
#[test]
fn device_listing_is_safe() {
    let devices = list_input_devices().unwrap();
    // We can't assert non-empty (CI/VMs may lack mics), just well-formedness.
    for d in &devices {
        assert!(!d.name.is_empty());
    }
}
