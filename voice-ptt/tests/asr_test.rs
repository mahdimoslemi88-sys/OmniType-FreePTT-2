//! Integration tests for the ASR layer (router policy, downloader map,
//! post-processing pipeline). These do NOT download models or run whisper.

use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use voice_ptt::asr::engine::{AsrEngine, AsrHealth, AudioUtterance};
use voice_ptt::asr::router::AsrRouter;
use voice_ptt::processing::dictionary::Dictionary;
use voice_ptt::processing::normalizer::Normalizer;
use voice_ptt::processing::process_text;

struct FakeEngine {
    name: &'static str,
    available: bool,
    result: String,
    calls: AtomicUsize,
}

impl AsrEngine for FakeEngine {
    fn name(&self) -> &'static str {
        self.name
    }
    fn health(&self) -> AsrHealth {
        if self.available {
            AsrHealth::Ready
        } else {
            AsrHealth::NoModel
        }
    }
    fn transcribe(&self, _audio: &AudioUtterance) -> anyhow::Result<String> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if self.result.is_empty() {
            anyhow::bail!("synthetic failure");
        }
        Ok(self.result.clone())
    }
}

fn utt() -> AudioUtterance {
    AudioUtterance {
        samples: vec![0.0; 16_000],
        sample_rate: 16_000,
    }
}

#[tokio::test]
async fn router_failover_returns_first_success() {
    let primary = Arc::new(FakeEngine {
        name: "primary",
        available: true,
        result: String::new(), // fails
        calls: AtomicUsize::new(0),
    });
    let secondary = Arc::new(FakeEngine {
        name: "secondary",
        available: true,
        result: "سلام".into(),
        calls: AtomicUsize::new(0),
    });

    let router = AsrRouter::new(vec![primary.clone(), secondary.clone()]);
    let text = router.transcribe(&utt()).await.unwrap();
    assert_eq!(text, "سلام");
    assert_eq!(primary.calls.load(std::sync::atomic::Ordering::Relaxed), 1);
    assert_eq!(secondary.calls.load(std::sync::atomic::Ordering::Relaxed), 1);
}

#[tokio::test]
async fn router_total_failure_is_reported() {
    let primary = Arc::new(FakeEngine {
        name: "primary",
        available: true,
        result: String::new(),
        calls: AtomicUsize::new(0),
    });
    let router = AsrRouter::new(vec![primary]);
    assert!(router.transcribe(&utt()).await.is_err());
    assert!(router.last_error().is_some());
}

#[test]
fn whisper_missing_model_reports_failed_health() {
    use voice_ptt::asr::whisper::{WhisperEngine, WhisperOptions};
    let engine = WhisperEngine::load(
        std::path::Path::new("models/definitely-missing.bin"),
        WhisperOptions::default(),
    );
    assert!(!AsrEngine::health(&engine).is_available());
}

#[test]
fn post_processing_full_pipeline() {
    let n = Normalizer::new();
    let d = Dictionary::with_defaults();

    // Arabic kaf/yeh, misheard tech term, punctuation spacing, no half-space.
    let out = process_text("من با پاتون کار میکنم ، خیلی خوشحالم", &n, &d);
    assert_eq!(out, "من با پایتون کار می‌کنم، خیلی خوشحالم");
}

#[test]
fn downloader_lists_known_models() {
    use voice_ptt::asr::downloader::WHISPER_MODELS;
    let names: Vec<&str> = WHISPER_MODELS.iter().map(|(n, _)| *n).collect();
    for expected in ["tiny", "base", "small", "medium", "large-v3", "large-v3-turbo"] {
        assert!(names.contains(&expected), "missing model: {expected}");
    }
}
