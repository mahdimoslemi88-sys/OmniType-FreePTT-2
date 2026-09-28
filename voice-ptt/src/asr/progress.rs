//! Process-wide sink for live partial transcripts.
//!
//! Streaming engines (`antigravity`) receive fragments of text *while* the
//! utterance is being transcribed. The ASR trait has no callback channel, so we
//! keep one tiny process-wide sink: `lib.rs` installs it once (pointing at the
//! state machine, which forwards the text to the capsule), engines publish into
//! it, and a missing sink is simply a no-op.

use std::sync::{Arc, OnceLock, RwLock};

/// Receives partial transcripts as they arrive.
pub type PartialSink = Arc<dyn Fn(&str) + Send + Sync>;

static SINK: OnceLock<RwLock<Option<PartialSink>>> = OnceLock::new();

fn cell() -> &'static RwLock<Option<PartialSink>> {
    SINK.get_or_init(|| RwLock::new(None))
}

/// Installs (or replaces) the sink. Called once at startup by `lib.rs`.
pub fn set_sink(sink: PartialSink) {
    match cell().write() {
        Ok(mut slot) => *slot = Some(sink),
        Err(_) => tracing::warn!("partial sink poisoned; live partials disabled"),
    }
}

/// Publishes a partial transcript. No-op when no sink is installed.
pub fn publish(text: &str) {
    let sink = cell().read().ok().and_then(|slot| slot.clone());
    if let Some(sink) = sink {
        sink(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// The sink is process-wide, so this is deliberately a single test: two tests
    /// sharing it would race each other (and the no-op case is covered here too).
    #[test]
    fn sink_receives_partials_and_pre_install_publishes_are_dropped() {
        // Before installation: a silent no-op, never a panic.
        publish("بی‌سینک");

        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = seen.clone();
        set_sink(Arc::new(move |text: &str| {
            if let Ok(mut list) = sink.lock() {
                list.push(text.to_string());
            }
        }));

        publish("خب");
        publish("خب ببین");

        let recorded = seen.lock().map(|list| list.clone()).unwrap_or_default();
        assert!(recorded.iter().any(|text| text == "خب"));
        assert!(recorded.iter().any(|text| text == "خب ببین"));
        assert!(
            !recorded.iter().any(|text| text == "بی‌سینک"),
            "a partial published before the sink existed must not be replayed"
        );
    }
}
