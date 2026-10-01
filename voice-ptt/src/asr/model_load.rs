//! What the background model fetch *meant*, as a value.
//!
//! The fetch itself is unavoidable I/O — a multi-hundred-megabyte download and a
//! `whisper.cpp` load. The *decision* around it is not, and it was previously
//! welded into `run()` as a `match` whose arms had three different log lines and
//! one arm with none at all. A reviewer could not tell what the app was claiming
//! in each case, and no test could ask.
//!
//! The decision is: given that the local engine was (or was not) already loaded,
//! and given how the background attempt ended, what should be reported?
//!
//! Four outcomes, and the fourth is the reason this module exists. Today
//! `engine.reload(&path)` returning `false` produces no line here at all: the
//! `if` is simply false, so a model that downloaded successfully but failed to
//! load looks exactly like one that is still downloading. [`ModelLoad::classify`]
//! keeps those apart, so the caller can log something a user can act on.

use std::path::PathBuf;

use crate::logging::Severity;

/// What one background fetch attempt ended up meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelLoad {
    /// The engine already had a model; nothing was fetched.
    NotNeeded,
    /// Downloaded and hot-swapped into the running engine.
    Ready {
        /// Where the model file landed.
        path: PathBuf,
    },
    /// The file arrived but the engine refused it.
    ///
    /// Distinct from "still downloading" on purpose. `WhisperEngine::reload`
    /// logs its own error, but nothing at the call site says *the fetch is over
    /// and failed* — so a reader of the log cannot tell this apart from a slow
    /// network, and the user is told nothing at all.
    ReloadRefused {
        /// The file that would not load.
        path: PathBuf,
    },
    /// The download itself failed; cloud engines still work.
    DownloadFailed {
        /// The downloader's own error, already formatted.
        error: String,
    },
}

impl ModelLoad {
    /// Classifies a finished attempt.
    ///
    /// `reloaded` is ignored when `download` is an `Err`: there is nothing to
    /// reload into. `engine_ready` short-circuits everything — a model that is
    /// already loaded makes a fetch meaningless, and fetching anyway would
    /// overwrite a working model with a re-download.
    pub fn classify(engine_ready: bool, download: Result<PathBuf, String>, reloaded: bool) -> Self {
        if engine_ready {
            return ModelLoad::NotNeeded;
        }
        match download {
            Err(error) => ModelLoad::DownloadFailed { error },
            Ok(path) if reloaded => ModelLoad::Ready { path },
            Ok(path) => ModelLoad::ReloadRefused { path },
        }
    }

    /// The severity this outcome deserves.
    pub fn level(&self) -> Severity {
        match self {
            ModelLoad::NotNeeded | ModelLoad::Ready { .. } => Severity::Info,
            // The file exists, so this is not a network problem and not fatal:
            // cloud engines still transcribe. But it is not "ready" either.
            ModelLoad::ReloadRefused { .. } => Severity::Warn,
            ModelLoad::DownloadFailed { .. } => Severity::Error,
        }
    }

    /// One line saying what happened, without the level decoration.
    pub fn message(&self) -> String {
        match self {
            ModelLoad::NotNeeded => "whisper model already loaded; no download needed".into(),
            ModelLoad::Ready { path } => {
                format!(
                    "whisper model ready (background download): {}",
                    path.display()
                )
            }
            ModelLoad::ReloadRefused { path } => format!(
                "whisper model downloaded to {} but the engine refused it; the local engine \
                 stays offline (cloud engines still work)",
                path.display()
            ),
            ModelLoad::DownloadFailed { error } => format!(
                "whisper model download failed; local engine stays offline (cloud engines \
                 still work): {error}"
            ),
        }
    }

    /// Whether the local engine is usable once this attempt is over.
    ///
    /// The single question a caller actually wants, kept here so nobody has to
    /// re-derive it from the variants (and get `ReloadRefused` wrong).
    pub fn engine_is_usable(&self) -> bool {
        matches!(self, ModelLoad::NotNeeded | ModelLoad::Ready { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> PathBuf {
        PathBuf::from(name)
    }

    /// The gate: a model that is already loaded must never be re-fetched.
    /// Re-fetching would overwrite a working model with a slower copy of it.
    #[test]
    fn a_ready_engine_is_never_fetched() {
        let ok = ModelLoad::classify(true, Ok(file("m.bin")), true);
        assert_eq!(ok, ModelLoad::NotNeeded);
        assert!(ok.engine_is_usable());
        // Even a *failed* fetch cannot make it not-needed disappear into an error.
        let failed = ModelLoad::classify(true, Err("network".into()), false);
        assert_eq!(failed, ModelLoad::NotNeeded);
    }

    #[test]
    fn a_successful_download_and_reload_is_ready() {
        let ok = ModelLoad::classify(false, Ok(file("m.bin")), true);
        assert_eq!(
            ok,
            ModelLoad::Ready {
                path: file("m.bin")
            }
        );
        assert_eq!(ok.level(), Severity::Info);
        assert!(ok.message().contains("m.bin"), "{}", ok.message());
        assert!(ok.engine_is_usable());
    }

    /// The case that had no line of its own: the file arrived and the engine
    /// would not take it. Nothing at the call site used to distinguish this from
    /// a download still in flight.
    #[test]
    fn a_refused_model_is_not_reported_as_ready() {
        let refused = ModelLoad::classify(false, Ok(file("m.bin")), false);
        assert_eq!(
            refused,
            ModelLoad::ReloadRefused {
                path: file("m.bin")
            }
        );
        assert_ne!(refused.level(), Severity::Info, "the user must be told");
        assert_eq!(refused.level(), Severity::Warn);
        assert!(!refused.engine_is_usable());
        let msg = refused.message();
        assert!(msg.contains("refused"), "{msg}");
        assert!(
            msg.contains("cloud engines still work"),
            "the message must say the app is not dead: {msg}"
        );
    }

    #[test]
    fn a_failed_download_is_an_error_and_names_the_cause() {
        let failed = ModelLoad::classify(false, Err("connection reset".into()), false);
        assert_eq!(
            failed,
            ModelLoad::DownloadFailed {
                error: "connection reset".into()
            }
        );
        assert_eq!(failed.level(), Severity::Error);
        assert!(!failed.engine_is_usable());
        assert!(
            failed.message().contains("connection reset"),
            "{}",
            failed.message()
        );
    }

    /// Exactly one outcome is fatal to the local engine, and it is the only one.
    #[test]
    fn only_two_of_the_four_outcomes_leave_a_usable_engine() {
        let outcomes = [
            ModelLoad::NotNeeded,
            ModelLoad::Ready {
                path: file("m.bin"),
            },
            ModelLoad::ReloadRefused {
                path: file("m.bin"),
            },
            ModelLoad::DownloadFailed { error: "x".into() },
        ];
        let usable: Vec<bool> = outcomes.iter().map(ModelLoad::engine_is_usable).collect();
        assert_eq!(usable, vec![true, true, false, false]);
    }

    /// `reloaded` is meaningless without a file, and must not leak in.
    #[test]
    fn a_failed_download_is_not_rescued_by_a_reload_flag() {
        let failed = ModelLoad::classify(false, Err("nope".into()), true);
        assert!(matches!(failed, ModelLoad::DownloadFailed { .. }));
    }

    /// Every outcome says something a user could act on: no empty messages, and
    /// no message that promises a working engine while not delivering one.
    #[test]
    fn no_outcome_is_silent_or_misleading() {
        for outcome in [
            ModelLoad::NotNeeded,
            ModelLoad::Ready {
                path: file("m.bin"),
            },
            ModelLoad::ReloadRefused {
                path: file("m.bin"),
            },
            ModelLoad::DownloadFailed {
                error: "boom".into(),
            },
        ] {
            let msg = outcome.message();
            assert!(msg.len() > 20, "message too short to act on: {msg}");
            if !outcome.engine_is_usable() {
                assert!(
                    msg.contains("cloud engines still work"),
                    "an unusable local engine must say the app still works: {msg}"
                );
            }
        }
    }
}
