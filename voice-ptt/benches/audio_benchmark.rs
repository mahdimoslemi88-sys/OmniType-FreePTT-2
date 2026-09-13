//! Ring-buffer and processing benchmarks (criterion).
//!
//! Run with: `cargo bench`

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::sync::Arc;
use std::thread;

use voice_ptt::audio::RingBuffer;
use voice_ptt::processing::dictionary::Dictionary;
use voice_ptt::processing::normalizer::Normalizer;
use voice_ptt::vad::rms::RmsVad;
use voice_ptt::vad::VadConfig;

fn bench_ring_buffer(c: &mut Criterion) {
    let rb = Arc::new(RingBuffer::new(16_000 * 30));
    let chunk = vec![0.1f32; 256];

    c.bench_function("ring_buffer/write_256", |b| {
        b.iter(|| rb.write(black_box(&chunk)))
    });

    c.bench_function("ring_buffer/write_read_256", |b| {
        let mut out = vec![0.0f32; 256];
        b.iter(|| {
            rb.write(black_box(&chunk));
            rb.read(&mut out)
        })
    });
}

fn bench_spsc_throughput(c: &mut Criterion) {
    c.bench_function("ring_buffer/spsc_10s_audio", |b| {
        b.iter(|| {
            let rb = Arc::new(RingBuffer::new(16_000 * 30));
            let w = rb.clone();
            let r = rb.clone();
            let producer = thread::spawn(move || {
                let chunk = vec![0.5f32; 256];
                for _ in 0..625 {
                    w.write(&chunk);
                }
            });
            let mut consumed = 0usize;
            let mut buf = vec![0.0f32; 4096];
            while consumed < 160_000 {
                consumed += r.read(&mut buf);
            }
            producer.join().unwrap();
            consumed
        })
    });
}

fn bench_rms_vad(c: &mut Criterion) {
    let mut vad = RmsVad::new(&VadConfig::default());
    let chunk: Vec<f32> = (0..512).map(|i| ((i as f32) * 0.05).sin() * 0.2).collect();
    c.bench_function("vad/rms_512", |b| {
        b.iter(|| vad.process(black_box(&chunk)))
    });
}

fn bench_text_processing(c: &mut Criterion) {
    let n = Normalizer::new();
    let d = Dictionary::with_defaults();
    let text = "من با پاتون روی ویندوز کار میکنم و جاوا اسکریپت بلدم. ۱۲۳ تست ، سلام";

    c.bench_function("text/normalize", |b| {
        b.iter(|| n.normalize(black_box(text)))
    });
    c.bench_function("text/dictionary_correct", |b| {
        b.iter(|| d.correct(black_box(text)))
    });
}

criterion_group!(
    benches,
    bench_ring_buffer,
    bench_spsc_throughput,
    bench_rms_vad,
    bench_text_processing
);
criterion_main!(benches);
