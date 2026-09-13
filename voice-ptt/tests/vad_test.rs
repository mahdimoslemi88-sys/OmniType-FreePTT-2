//! Integration tests for the VAD layer (endpoint policy + RMS engine).

use voice_ptt::vad::{AnyVad, Endpoint, FrameResult, VadConfig};

fn cfg() -> VadConfig {
    VadConfig {
        threshold: 0.5,
        silence_timeout_ms: 500,
        min_speech_ms: 100,
        speech_start_probability: 0.5,
    }
}

#[test]
fn endpoint_full_utterance_lifecycle() {
    let mut ep = Endpoint::new(cfg(), 16_000);

    // 300 ms speech.
    for _ in 0..15 {
        ep.feed(&FrameResult::from_bool(true, 320));
    }
    assert!(ep.has_speech());
    assert!(ep.has_enough_speech());
    assert!(!ep.should_finalize());

    // 600 ms silence → finalize (> 500 ms timeout).
    for _ in 0..30 {
        ep.feed(&FrameResult::from_bool(false, 320));
    }
    assert!(ep.should_finalize());

    // Reset for next utterance.
    ep.reset();
    assert!(!ep.has_speech());
    assert!(!ep.should_finalize());
}

#[test]
fn endpoint_ignores_pure_silence_and_brief_noise() {
    let mut ep = Endpoint::new(cfg(), 16_000);
    for _ in 0..100 {
        ep.feed(&FrameResult::from_bool(false, 320));
    }
    assert!(!ep.should_finalize());
    assert!(!ep.has_enough_speech());
}

/// The auto-dispatcher must always return a usable engine (RMS fallback when
/// the Silero model is missing).
#[test]
fn any_vad_auto_falls_back_without_model() {
    let (vad, kind) = AnyVad::auto(
        std::path::Path::new("assets/definitely-missing.onnx"),
        &VadConfig::default(),
    );
    let _ = vad;
    assert!(
        matches!(kind, voice_ptt::vad::VadEngine::Rms),
        "without the model file, RMS fallback is expected"
    );
}

#[test]
fn rms_vad_discriminates_silence_from_loud() {
    let (mut vad, _) = AnyVad::auto(std::path::Path::new("missing.onnx"), &VadConfig::default());

    let silence = vec![0.0f32; 512];
    assert!(!vad.process(&silence).is_speech);

    let loud: Vec<f32> = (0..512).map(|i| (i as f32 * 0.2).sin() * 0.5).collect();
    assert!(vad.process(&loud).is_speech);
}
