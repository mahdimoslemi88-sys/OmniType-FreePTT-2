//! Priority router over ASR engines with health tracking and cooldowns.
//!
//! The research design calls for a failover chain (whisper primary, cloud
//! fallback later). Engines are tried in registration order; after a failure
//! an engine enters a cooldown and is skipped until it expires. Blocking
//! transcription work runs on Tokio's blocking pool so the async state machine
//! never stalls.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use anyhow::Result;

use super::engine::{AsrEngine, AsrHealth, AudioUtterance};

const COOLDOWN_MS: u64 = 30_000;

/// Milliseconds since the process started (monotonic clock).
fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    let start = START.get_or_init(Instant::now);
    start.elapsed().as_millis() as u64
}

struct EngineSlot {
    engine: Arc<dyn AsrEngine>,
    failed_at_ms: AtomicU64, // unix-style ms timestamp of last failure
}

/// Engine-ordered router. Clone-safe; all state shared.
#[derive(Clone)]
pub struct AsrRouter {
    slots: Arc<Vec<EngineSlot>>,
    cooldown_ms: Arc<AtomicU64>,
    last_error: Arc<Mutex<Option<String>>>,
}

impl AsrRouter {
    /// Builds a router over engines in priority order (index 0 = preferred).
    pub fn new(engines: Vec<Arc<dyn AsrEngine>>) -> Self {
        Self {
            slots: Arc::new(engines.into_iter().map(|engine| EngineSlot {
                engine,
                failed_at_ms: AtomicU64::new(0),
            }).collect()),
            cooldown_ms: Arc::new(AtomicU64::new(COOLDOWN_MS)),
            last_error: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_cooldown(&self, ms: u64) {
        self.cooldown_ms.store(ms, Ordering::Relaxed);
    }

    /// Human-readable status of each engine (for UI/logs).
    pub fn status(&self) -> Vec<(&'static str, AsrHealth)> {
        self.slots.iter().map(|s| (s.engine.name(), s.engine.health())).collect()
    }

    /// Transcribes via the first healthy engine; on total failure returns the
    /// last error seen.
    pub async fn transcribe(&self, audio: &AudioUtterance) -> Result<String> {
        let mut candidate_indices: Vec<usize> = (0..self.slots.len()).collect();
        // Stable sort: available engines first, then by priority (index).
        candidate_indices.sort_by_key(|&i| {
            let slot = &self.slots[i];
            let cooling = {
                let failed_at = slot.failed_at_ms.load(Ordering::Relaxed);
                failed_at != 0
                    && now_ms().saturating_sub(failed_at) < self.cooldown_ms.load(Ordering::Relaxed)
            };
            let available = slot.engine.health().is_available();
            match (available, cooling) {
                (false, _) => 2,             // not available at all
                (true, true) => 1,           // available but cooling down
                (true, false) => 0,          // ready now
            }
        });

        let mut last_err: Option<anyhow::Error> = None;
        for &i in &candidate_indices {
            let slot = &self.slots[i];
            if !slot.engine.health().is_available() {
                continue;
            }
            let engine = slot.engine.clone();
            let audio_owned = audio.clone();
            let started = Instant::now();
            let result =
                tokio::task::spawn_blocking(move || engine.transcribe(&audio_owned)).await;

            match result {
                Ok(Ok(text)) if !text.trim().is_empty() => {
                    slot.failed_at_ms.store(0, Ordering::Relaxed);
                    tracing::info!(
                        engine = slot.engine.name(),
                        elapsed_ms = started.elapsed().as_millis() as u64,
                        chars = text.chars().count(),
                        "asr success"
                    );
                    return Ok(text);
                }
                Ok(Ok(_)) => {
                    // Empty transcription: treat as soft failure and try next.
                    tracing::warn!(engine = slot.engine.name(), "empty transcription");
                    last_err = Some(anyhow::anyhow!("engine returned empty text"));
                }
                Ok(Err(e)) => {
                    tracing::warn!(engine = slot.engine.name(), %e, "asr engine failed");
                    last_err = Some(e);
                }
                Err(e) => {
                    // JoinError (panic in task)
                    last_err = Some(anyhow::anyhow!("asr task panicked: {e}"));
                }
            }
            slot.failed_at_ms.store(now_ms(), Ordering::Relaxed);
        }

        if let Some(err) = &last_err {
            *self.last_error.lock().unwrap() = Some(err.to_string());
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no ASR engine available")))
    }

    /// Last recorded router-level error (for the UI).
    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct FakeEngine {
        name: &'static str,
        available: bool,
        result_text: String,
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
        fn transcribe(&self, _audio: &AudioUtterance) -> Result<String> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if self.result_text.is_empty() {
                anyhow::bail!("synthetic failure");
            }
            Ok(self.result_text.clone())
        }
    }

    fn utterance() -> AudioUtterance {
        AudioUtterance {
            samples: vec![0.0; 16_000],
            sample_rate: 16_000,
        }
    }

    #[tokio::test]
    async fn routes_to_first_healthy_engine() {
        let bad = Arc::new(FakeEngine {
            name: "bad",
            available: true,
            result_text: String::new(), // always fails
            calls: AtomicUsize::new(0),
        });
        let good = Arc::new(FakeEngine {
            name: "good",
            available: true,
            result_text: "سلام دنیا".into(),
            calls: AtomicUsize::new(0),
        });

        let router = AsrRouter::new(vec![bad.clone(), good.clone()]);
        let text = router.transcribe(&utterance()).await.unwrap();
        assert_eq!(text, "سلام دنیا");
        assert_eq!(bad.calls.load(Ordering::Relaxed), 1);
        assert_eq!(good.calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn skips_unavailable_engines_entirely() {
        let dead = Arc::new(FakeEngine {
            name: "dead",
            available: false,
            result_text: "nope".into(),
            calls: AtomicUsize::new(0),
        });
        let good = Arc::new(FakeEngine {
            name: "good",
            available: true,
            result_text: "ok".into(),
            calls: AtomicUsize::new(0),
        });

        let router = AsrRouter::new(vec![dead.clone(), good.clone()]);
        let text = router.transcribe(&utterance()).await.unwrap();
        assert_eq!(text, "ok");
        assert_eq!(dead.calls.load(Ordering::Relaxed), 0, "unavailable engine must not be called");
    }

    #[tokio::test]
    async fn all_engines_failing_returns_error() {
        let bad = Arc::new(FakeEngine {
            name: "bad",
            available: true,
            result_text: String::new(),
            calls: AtomicUsize::new(0),
        });
        let router = AsrRouter::new(vec![bad.clone()]);
        assert!(router.transcribe(&utterance()).await.is_err());
        assert!(router.last_error().is_some());
        assert_eq!(bad.calls.load(Ordering::Relaxed), 1, "single engine, single attempt per call");
    }

    #[test]
    fn duration_secs_handles_zero_rate() {
        let u = AudioUtterance {
            samples: vec![0.0; 100],
            sample_rate: 0,
        };
        assert_eq!(u.duration_secs(), 0.0);
    }
}
