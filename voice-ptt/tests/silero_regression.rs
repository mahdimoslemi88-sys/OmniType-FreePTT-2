//! Regression tests for the Silero VAD calling convention.
//!
//! These reproduce (and guard against) the field bug where every utterance
//! was discarded as "not enough speech": the model graph requires a
//! 64-sample context prefix (576-sample input) and a 0-d `sr` scalar;
//! feeding bare 512-sample windows yields a constant ~0.001 probability or
//! an STFT shape error. The fixture is a 9.3 s real-speech f32 file
//! (`assets/test-speech-16k.f32`); tests skip gracefully when the model or
//! the fixture is missing so CI without local assets still passes.

use voice_ptt::vad::{Endpoint, SileroVad, VadConfig};

/// Loads the speech fixture, or returns None (skip) when absent.
fn load_speech_fixture() -> Option<Vec<f32>> {
    let raw = std::fs::read("assets/test-speech-16k.f32").ok()?;
    Some(
        raw.chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect(),
    )
}

fn find_model() -> Option<std::path::PathBuf> {
    [
        std::path::Path::new("assets/silero_vad.onnx"),
        std::path::Path::new("../voice-ptt-dist/assets/silero_vad.onnx"),
    ]
    .into_iter()
    .find(|p| p.exists())
    .map(std::path::Path::to_path_buf)
}

/// REAL SPEECH through the app wrapper: SAPI-synthesized wav converted to
/// 16 kHz f32. This is the decisive test: if Silero fires here but the
/// field logs show zero speech, the capture path (48k stereo interleaved /
/// resampling) is the culprit, not the model wrapper.
#[test]
fn diag_real_speech_clean_chunking() {
    let Some(path) = find_model() else {
        eprintln!("SKIP: silero_vad.onnx not present");
        return;
    };
    let Some(audio) = load_speech_fixture() else {
        eprintln!("skipping: assets/test-speech-16k.f32 not present");
        return;
    };
    let cfg = VadConfig::default();
    let mut vad = SileroVad::new(&path, &cfg).unwrap();

    let (mut speech_frames, mut total_frames) = (0usize, 0usize);
    let mut probs = Vec::new();
    for chunk in audio.chunks(512) {
        let fr = vad.process(chunk).unwrap();
        total_frames += 1;
        if fr.is_speech {
            speech_frames += 1;
        }
        if total_frames % 30 == 0 {
            probs.push((fr.speech_probability * 1000.0).round() / 1000.0);
        }
    }
    println!(
        "REAL-SPEECH clean-chunking: speech frames {speech_frames}/{total_frames}"
    );
    println!("prob samples (every ~1 s): {probs:?}");
    assert!(
        speech_frames > total_frames / 2,
        "real speech must be detected: only {speech_frames}/{total_frames}"
    );
}

/// REAL SPEECH through the full endpoint policy — the field failure mode:
/// ~9 s speech then silence; endpoint must count seconds of speech and
/// finalize, never instantly discard.
#[test]
fn diag_real_speech_endpoint() {
    let Some(path) = find_model() else {
        eprintln!("SKIP: silero_vad.onnx not present");
        return;
    };
    let Some(mut audio) = load_speech_fixture() else {
        eprintln!("skipping: assets/test-speech-16k.f32 not present");
        return;
    };
    audio.extend_from_slice(&vec![0.0f32; 16_000 * 2]);

    let cfg = VadConfig::default();
    let mut vad = SileroVad::new(&path, &cfg).unwrap();
    let mut endpoint = Endpoint::new(cfg, 16_000);
    for chunk in audio.chunks(512) {
        let fr = vad.process(chunk).unwrap();
        endpoint.feed(&fr);
    }
    let speech_secs = endpoint.speech_samples() as f64 / 16_000.0;
    println!("REAL-SPEECH endpoint: counted {speech_secs:.2} s speech");
    assert!(
        endpoint.has_enough_speech() && speech_secs > 2.0,
        "endpoint counted only {speech_secs:.2} s of 9.3 s real speech"
    );
}

/// Repro of the OLD poll_vad: buffer grows 640 samples per 40 ms poll; only
/// the leading 512 are fed, so each poll re-feeds 128 samples next tick.
/// Overlapping re-feed must not crash and must still detect real speech.
#[test]
fn diag_overlapping_refeed_pattern() {
    let Some(path) = find_model() else {
        eprintln!("SKIP: silero_vad.onnx not present");
        return;
    };
    let cfg = VadConfig::default();
    let mut vad = SileroVad::new(&path, &cfg).unwrap();

    let Some(audio) = load_speech_fixture() else {
        eprintln!("skipping: assets/test-speech-16k.f32 not present");
        return;
    };
    let (mut speech_frames, mut total_frames) = (0usize, 0usize);
    let mut buffer: Vec<f32> = Vec::new();
    let mut pos = 0usize;
    while pos < audio.len() {
        let end = (pos + 640).min(audio.len());
        buffer.extend_from_slice(&audio[pos..end]);
        pos = end;
        while buffer.len() >= 512 {
            let fr = vad.process(&buffer[..512]).unwrap();
            total_frames += 1;
            if fr.is_speech {
                speech_frames += 1;
            }
            buffer.drain(..512);
        }
    }
    println!("overlap-refeed: speech frames {speech_frames}/{total_frames}");
    assert!(
        speech_frames > total_frames / 4,
        "overlapping re-feed must still detect speech"
    );
}

/// Stale recurrent state: begin_recording resets the endpoint but NOT the
/// Silero state. Does trailing state from prior audio poison the next
/// utterance's detection? Uses real speech + reset() (the new behavior).
#[test]
fn diag_detection_after_long_silence_with_reset() {
    let Some(path) = find_model() else {
        eprintln!("SKIP: silero_vad.onnx not present");
        return;
    };
    let cfg = VadConfig::default();
    let mut vad = SileroVad::new(&path, &cfg).unwrap();

    // ~3 s of silence flowing through the VAD (state accumulates "silence").
    for _ in 0..94 {
        let _ = vad.process(&[0.0f32; 512]).unwrap();
    }
    vad.reset();

    let Some(audio) = load_speech_fixture() else {
        eprintln!("skipping: assets/test-speech-16k.f32 not present");
        return;
    };
    let (mut speech_frames, mut total) = (0usize, 0usize);
    for chunk in audio.chunks(512) {
        let fr = vad.process(chunk).unwrap();
        total += 1;
        if fr.is_speech {
            speech_frames += 1;
        }
    }
    println!("after-silence+reset: speech {speech_frames}/{total}");
    assert!(speech_frames > total / 2, "must detect speech after silence");
}
