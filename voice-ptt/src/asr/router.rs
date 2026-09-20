//! Priority router over ASR engines with health tracking and cooldowns.
//!
//! The research design calls for a failover chain (whisper primary, cloud
//! fallback later). Engines are tried in registration order; after a failure
//! an engine enters a cooldown and is skipped until it expires. Blocking
//! transcription work runs on Tokio's blocking pool so the async state machine
//! never stalls.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
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
    failed_at_ms: Arc<AtomicU64>, // unix-style ms timestamp of last failure
}

/// Engine-ordered router. Clone-safe; all state shared.
#[derive(Clone)]
pub struct AsrRouter {
    slots: Arc<RwLock<Vec<EngineSlot>>>,
    active_engine: Arc<RwLock<String>>,
    cooldown_ms: Arc<AtomicU64>,
    last_error: Arc<Mutex<Option<String>>>,
}

impl AsrRouter {
    /// Builds a router over engines in priority order (index 0 = preferred in auto mode).
    pub fn new(engines: Vec<Arc<dyn AsrEngine>>) -> Self {
        Self::new_with_active(engines, "auto".to_string())
    }

    /// Builds a router with an initial active engine setting.
    pub fn new_with_active(engines: Vec<Arc<dyn AsrEngine>>, active: String) -> Self {
        Self {
            slots: Arc::new(RwLock::new(
                engines
                    .into_iter()
                    .map(|engine| EngineSlot {
                        engine,
                        failed_at_ms: Arc::new(AtomicU64::new(0)),
                    })
                    .collect(),
            )),
            active_engine: Arc::new(RwLock::new(active)),
            cooldown_ms: Arc::new(AtomicU64::new(COOLDOWN_MS)),
            last_error: Arc::new(Mutex::new(None)),
        }
    }

    /// Returns the currently active engine identifier ("auto" or specific engine id).
    pub fn active_engine(&self) -> String {
        self.active_engine.read().unwrap().clone()
    }

    /// Sets the active engine. Use "auto" for automatic fallback chain,
    /// or pass an engine ID (e.g. "google", "local_whisper", "groq", or custom ID).
    pub fn set_active_engine(&self, id: &str) {
        *self.active_engine.write().unwrap() = id.to_string();
    }

    /// Registers or updates an engine in the router.
    pub fn register_engine(&self, engine: Arc<dyn AsrEngine>) {
        let mut slots = self.slots.write().unwrap();
        if let Some(existing) = slots.iter_mut().find(|s| s.engine.id() == engine.id()) {
            existing.engine = engine;
            existing.failed_at_ms.store(0, Ordering::Relaxed);
        } else {
            slots.push(EngineSlot {
                engine,
                failed_at_ms: Arc::new(AtomicU64::new(0)),
            });
        }
    }

    /// Removes an engine by its ID or name. Returns true if an engine was removed.
    pub fn remove_engine(&self, id: &str) -> bool {
        let mut slots = self.slots.write().unwrap();
        let initial_len = slots.len();
        slots.retain(|s| s.engine.id() != id && s.engine.name() != id);
        slots.len() < initial_len
    }

    /// Returns a list of all registered engines: (id, display_name, kind, health, is_active).
    pub fn list_engines(&self) -> Vec<(String, String, &'static str, AsrHealth, bool)> {
        let slots = self.slots.read().unwrap();
        let active = self.active_engine.read().unwrap().clone();
        slots
            .iter()
            .map(|s| {
                let id = s.engine.id();
                let display = s.engine.display_name();
                let kind = s.engine.kind();
                let health = s.engine.health();
                let is_selected = active == id || active == s.engine.name();
                (id, display, kind, health, is_selected)
            })
            .collect()
    }

    pub fn set_cooldown(&self, ms: u64) {
        self.cooldown_ms.store(ms, Ordering::Relaxed);
    }

    /// Human-readable status of each engine (for UI/logs).
    pub fn status(&self) -> Vec<(&'static str, AsrHealth)> {
        let slots = self.slots.read().unwrap();
        slots.iter().map(|s| (s.engine.name(), s.engine.health())).collect()
    }

    /// Transcribes via the preferred / healthy engine; on total failure returns the
    /// last error seen.
    pub async fn transcribe(&self, audio: &AudioUtterance) -> Result<String> {
        let (candidates, active, cooldown) = {
            let slots = self.slots.read().unwrap();
            let active = self.active_engine.read().unwrap().clone();
            let cooldown = self.cooldown_ms.load(Ordering::Relaxed);
            let list: Vec<(Arc<dyn AsrEngine>, Arc<AtomicU64>)> = slots
                .iter()
                .map(|s| (s.engine.clone(), s.failed_at_ms.clone()))
                .collect();
            (list, active, cooldown)
        };

        let mut candidate_indices: Vec<usize> = (0..candidates.len()).collect();
        // Stable sort: if active engine is specified, prioritize it first; otherwise sort by availability & registration order
        candidate_indices.sort_by_key(|&i| {
            let (engine, failed_at_ms) = &candidates[i];
            let matches_active = active != "auto" && (engine.id() == active || engine.name() == active);
            let cooling = {
                let failed_at = failed_at_ms.load(Ordering::Relaxed);
                failed_at != 0 && now_ms().saturating_sub(failed_at) < cooldown
            };
            let available = engine.health().is_available();
            match (matches_active, available, cooling) {
                (true, true, false) => 0,
                (true, true, true) => 1,
                (false, true, false) => 2,
                (false, true, true) => 3,
                (_, false, _) => 4,
            }
        });

        let mut last_err: Option<anyhow::Error> = None;
        for &i in &candidate_indices {
            let (engine, failed_at_ms) = &candidates[i];
            if !engine.health().is_available() {
                continue;
            }
            let engine_cloned = engine.clone();
            let audio_owned = audio.clone();
            let started = Instant::now();
            let result =
                tokio::task::spawn_blocking(move || engine_cloned.transcribe(&audio_owned)).await;

            match result {
                Ok(Ok(text)) if !text.trim().is_empty() => {
                    failed_at_ms.store(0, Ordering::Relaxed);
                    tracing::info!(
                        engine = engine.name(),
                        elapsed_ms = started.elapsed().as_millis() as u64,
                        chars = text.chars().count(),
                        "asr success"
                    );
                    return Ok(text);
                }
                Ok(Ok(_)) => {
                    // Empty transcription: treat as soft failure and try next.
                    tracing::warn!(engine = engine.name(), "empty transcription");
                    last_err = Some(anyhow::anyhow!("engine returned empty text"));
                }
                Ok(Err(e)) => {
                    tracing::warn!(engine = engine.name(), %e, "asr engine failed");
                    last_err = Some(e);
                }
                Err(e) => {
                    // JoinError (panic in task)
                    last_err = Some(anyhow::anyhow!("asr task panicked: {e}"));
                }
            }
            failed_at_ms.store(now_ms(), Ordering::Relaxed);
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

    #[tokio::test]
    async fn active_engine_routing_prioritizes_selected_engine() {
        let first = Arc::new(FakeEngine {
            name: "engine_a",
            available: true,
            result_text: "output_a".into(),
            calls: AtomicUsize::new(0),
        });
        let second = Arc::new(FakeEngine {
            name: "engine_b",
            available: true,
            result_text: "output_b".into(),
            calls: AtomicUsize::new(0),
        });

        // Default auto: first should be called
        let router = AsrRouter::new(vec![first.clone(), second.clone()]);
        let text = router.transcribe(&utterance()).await.unwrap();
        assert_eq!(text, "output_a");
        assert_eq!(first.calls.load(Ordering::Relaxed), 1);
        assert_eq!(second.calls.load(Ordering::Relaxed), 0);

        // Switch to engine_b explicitly
        router.set_active_engine("engine_b");
        assert_eq!(router.active_engine(), "engine_b");
        let text2 = router.transcribe(&utterance()).await.unwrap();
        assert_eq!(text2, "output_b");
        assert_eq!(second.calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn dynamic_engine_registration_and_removal() {
        let router = AsrRouter::new(vec![]);
        assert!(router.list_engines().is_empty());

        let engine = Arc::new(FakeEngine {
            name: "custom_1",
            available: true,
            result_text: "test".into(),
            calls: AtomicUsize::new(0),
        });

        router.register_engine(engine);
        let list = router.list_engines();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0, "custom_1");

        let removed = router.remove_engine("custom_1");
        assert!(removed);
        assert!(router.list_engines().is_empty());
    }
}
