//! What one piece of audio turned out to be worth: its loudness, whether there
//! is anything to type, and what the visible state should end up as.
//!
//! The middle chunk of a long dictation and its last chunk follow the same
//! path — transcribe, normalise, stitch the seam, erase the cut artefact, type
//! — and differ only in what a *failure* means: a failed mid-session chunk is
//! skipped so the dictation survives, a failed final one is surfaced as an
//! error. That fork used to be written out twice. The shared rules live here as
//! pure functions; the keyboard stays in `state/machine.rs`.

use std::time::Duration;

use crate::output::Injection;
use crate::processing::seam::SeamMerge;

use super::status::AppState;

/// Full scale is 1.0, so the floor keeps silence printable.
const DBFS_FLOOR: f32 = 1e-6;

/// How long a failure stays readable before the machine returns to `Idle`.
///
/// `Error` is a resting state, not a dead end: one failed utterance (offline
/// engine, provider 403) must never freeze push-to-talk until restart.
pub(crate) const ERROR_READABLE: Duration = Duration::from_secs(3);

/// Audio-level diagnostics for one utterance.
///
/// Distinguishes "mic delivered silence" (peak ≈ −∞ dBFS → wrong or muted
/// device) from "audio arrived but the VAD called it non-speech" (healthy
/// levels, discarded). Computed from the samples, so it is testable without a
/// microphone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AudioLevels {
    pub peak: f32,
    pub rms: f32,
}

impl AudioLevels {
    pub fn measure(samples: &[f32]) -> Self {
        let peak = samples.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        let rms = if samples.is_empty() {
            0.0
        } else {
            (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
        };
        Self { peak, rms }
    }

    /// dBFS, floored: silence reports a very low number, never `NaN`/`-inf`.
    pub fn peak_dbfs(&self) -> f32 {
        20.0 * self.peak.max(DBFS_FLOOR).log10()
    }

    pub fn rms_dbfs(&self) -> f32 {
        20.0 * self.rms.max(DBFS_FLOOR).log10()
    }
}

/// Why a transcript produced nothing to type. Kept as a value so the skip is
/// reportable rather than a silent early return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkipReason {
    /// The recogniser returned nothing (or only whitespace).
    EmptyTranscript,
    /// Everything in the chunk was overlap with the previous one.
    EmptyAfterSeam,
}

impl SkipReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EmptyTranscript => "empty transcript",
            Self::EmptyAfterSeam => "pure seam overlap",
        }
    }
}

/// What to do with one transcript, decided before a single key is pressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TypePlan {
    /// Nothing to type, and why.
    Skip(SkipReason),
    /// Erase `backspaces` seam artefacts, then type `text`.
    Type { text: String, backspaces: usize },
}

/// Decides whether a transcript is worth typing.
///
/// The two emptiness checks are separate on purpose: a recogniser that returns
/// nothing and a chunk that the seam stitcher reduced to nothing are different
/// faults, and the logs say which.
pub(crate) fn plan_typing(raw: &str, merge: &SeamMerge) -> TypePlan {
    if raw.trim().is_empty() {
        return TypePlan::Skip(SkipReason::EmptyTranscript);
    }
    if merge.text.is_empty() {
        return TypePlan::Skip(SkipReason::EmptyAfterSeam);
    }
    TypePlan::Type {
        text: merge.text.clone(),
        backspaces: merge.backspaces,
    }
}

/// What one insert achieved, in the only terms that can honestly be known.
///
/// Four cases, and the three that are *not* `Complete` are the point. A send
/// that the platform only partly took is never called a success, and it is
/// never repeated: the accepted half is already in the document, so asking
/// again would type the whole text a second time.
///
/// The count is in **keystroke pairs**, deliberately not in characters that
/// "landed". `SendInput` reports the input records the system accepted, which
/// is the last honest step before the document, and a reader who sees
/// `Complete { accepted_pairs: 12 }` should not conclude that twelve letters
/// are sitting in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InjectOutcome {
    /// Every down/up pair the platform was asked for was accepted.
    Complete { accepted_pairs: usize },
    /// Some pairs went out and delivery then stopped. The torn remainder's fate
    /// is unknown, so this is reported and never re-sent.
    Partial { accepted_pairs: usize },
    /// The platform accepted nothing at all.
    Failed,
    /// Nothing was asked: the destination refused, or there was no text.
    NotAttempted,
}

impl InjectOutcome {
    /// Whether the document can be believed to hold what was sent.
    ///
    /// Only `Complete` can. Everything else leaves the tail of the text unknown,
    /// and anything built on that tail — the seam memory above all — has to be
    /// thrown away rather than believed.
    pub(crate) fn is_whole(self) -> bool {
        matches!(self, Self::Complete { .. })
    }
}

/// Folds the sends of one insert — the seam erase, then the text — into the
/// single verdict the loop may report.
///
/// `steps` is what the platform actually said, in the order it was asked, so an
/// empty list is a destination that refused before the first key. The fold is
/// cumulative on purpose: an erase that went out whole followed by a text that
/// did not is `Partial`, not `Failed`, because "a word was erased" and "nothing
/// happened" are different states of the user's document and only the second one
/// is safe to treat as a clean slate.
pub(crate) fn judge_insert(steps: &[Injection]) -> InjectOutcome {
    if steps.is_empty() {
        return InjectOutcome::NotAttempted;
    }
    let accepted_pairs: usize = steps.iter().map(|step| step.pairs()).sum();
    if steps.iter().all(|step| step.whole_pairs()) {
        return InjectOutcome::Complete { accepted_pairs };
    }
    if accepted_pairs == 0 {
        return InjectOutcome::Failed;
    }
    InjectOutcome::Partial { accepted_pairs }
}

/// Whether the visible state is a failure that must clear itself.
///
/// Pinned because getting it wrong is silent: a stale `Error` swallows the
/// next press-to-talk and the app looks dead until it is restarted.
pub(crate) fn is_transient(state: &AppState) -> bool {
    matches!(state, AppState::Error(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn merge(text: &str, backspaces: usize) -> SeamMerge {
        SeamMerge {
            text: text.to_string(),
            dropped_words: 0,
            backspaces,
        }
    }

    #[test]
    fn a_whitespace_only_transcript_is_skipped_as_empty() {
        for raw in ["", "   ", "\n\t "] {
            assert_eq!(
                plan_typing(raw, &merge("some text", 0)),
                TypePlan::Skip(SkipReason::EmptyTranscript),
                "raw={raw:?}"
            );
        }
    }

    /// A transcript that is entirely overlap with the previous chunk is not a
    /// recogniser failure — it is the seam repair working.
    #[test]
    fn text_the_seam_ate_is_skipped_for_its_own_reason() {
        assert_eq!(
            plan_typing("قسمت قبلی", &merge("", 0)),
            TypePlan::Skip(SkipReason::EmptyAfterSeam)
        );
    }

    #[test]
    fn real_text_is_typed_with_its_backspaces() {
        assert_eq!(
            plan_typing("سلام دنیا", &merge("دنیا", 3)),
            TypePlan::Type {
                text: "دنیا".into(),
                backspaces: 3
            }
        );
    }

    /// The plan is a copy: later mutation of the merge must not rewrite what
    /// was decided to type.
    #[test]
    fn the_planned_text_is_a_snapshot() {
        let m = merge("سلام", 0);
        let plan = plan_typing("سلام", &m);
        assert_eq!(
            plan,
            TypePlan::Type {
                text: "سلام".into(),
                backspaces: 0
            }
        );
    }

    #[test]
    fn every_skip_reason_names_itself() {
        assert_eq!(SkipReason::EmptyTranscript.as_str(), "empty transcript");
        assert_eq!(SkipReason::EmptyAfterSeam.as_str(), "pure seam overlap");
    }

    #[test]
    fn only_an_error_state_clears_itself() {
        assert!(is_transient(&AppState::Error("ASR failed".into())));
        assert!(!is_transient(&AppState::Idle));
        assert!(!is_transient(&AppState::Recording));
        assert!(!is_transient(&AppState::Processing));
        assert!(!is_transient(&AppState::Typing));
    }

    #[test]
    fn a_failure_stays_readable_for_three_seconds() {
        assert_eq!(ERROR_READABLE, Duration::from_secs(3));
    }

    // ── loudness ─────────────────────────────────────────────────────────

    #[test]
    fn silence_is_reported_as_a_very_low_level_not_a_nan() {
        let l = AudioLevels::measure(&[0.0; 16_000]);
        assert_eq!(
            l,
            AudioLevels {
                peak: 0.0,
                rms: 0.0
            }
        );
        assert!(l.peak_dbfs().is_finite() && l.rms_dbfs().is_finite());
        assert!(l.peak_dbfs() <= -119.0, "{}", l.peak_dbfs());
    }

    #[test]
    fn no_samples_at_all_is_still_a_number() {
        let l = AudioLevels::measure(&[]);
        assert_eq!(
            l,
            AudioLevels {
                peak: 0.0,
                rms: 0.0
            }
        );
        assert!(l.peak_dbfs().is_finite() && l.rms_dbfs().is_finite());
    }

    /// A full-scale signal reads 0 dBFS. The peak/RMS *gap* is what separates a
    /// healthy level from a muted one, so both ends are pinned: a square wave
    /// has rms == peak, a sine sits 3 dB below it.
    #[test]
    fn a_full_scale_signal_reads_zero_dbfs() {
        let square: Vec<f32> = (0..4_000)
            .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let l = AudioLevels::measure(&square);
        assert!((l.peak_dbfs() - 0.0).abs() < 0.01, "peak {}", l.peak_dbfs());
        assert!((l.rms_dbfs() - 0.0).abs() < 0.01, "rms {}", l.rms_dbfs());

        let sine: Vec<f32> = (0..4_000)
            .map(|i| (std::f32::consts::TAU * 400.0 * i as f32 / 16_000.0).sin())
            .collect();
        let l = AudioLevels::measure(&sine);
        assert!((l.peak_dbfs() - 0.0).abs() < 0.1, "peak {}", l.peak_dbfs());
        assert!((l.rms_dbfs() - -3.01).abs() < 0.1, "rms {}", l.rms_dbfs());
    }

    #[test]
    fn a_quiet_mic_reads_about_minus_forty_dbfs() {
        let l = AudioLevels::measure(&[0.01; 1_000]);
        assert!((l.peak_dbfs() - -40.0).abs() < 0.1, "{}", l.peak_dbfs());
        assert!((l.rms_dbfs() - -40.0).abs() < 0.1, "{}", l.rms_dbfs());
    }

    #[test]
    fn a_single_spike_dominates_the_peak_but_not_the_average() {
        let mut samples = vec![0.1f32; 1_000];
        samples[0] = 1.0;
        let l = AudioLevels::measure(&samples);
        assert!((l.peak - 1.0).abs() < 1e-6);
        assert!(l.rms < 0.2, "rms {}", l.rms);
    }

    // ── the insert verdict ────────────────────────────────────────────────

    /// A send the platform took whole: `pairs`, never "characters typed".
    fn whole(pairs: usize) -> Injection {
        Injection {
            total_events: pairs * 2,
            attempted: pairs * 2,
            accepted: pairs * 2,
            stopped: None,
        }
    }

    /// A send that stopped after `took` events out of the `wanted` it needed.
    fn short(took: usize, wanted: usize) -> Injection {
        Injection {
            total_events: wanted,
            attempted: wanted,
            accepted: took,
            stopped: Some(format!("took {took} of {wanted}")),
        }
    }

    /// The whole table, as one test on purpose: the four verdicts differ only
    /// in the shape of what the platform reported, and a table that is read in
    /// four places is a table where two rows can start disagreeing.
    #[test]
    fn the_insert_verdict_follows_the_reports_and_nothing_else() {
        // Nothing was asked, so nothing can be claimed.
        assert_eq!(judge_insert(&[]), InjectOutcome::NotAttempted);

        // Both sends whole.
        assert_eq!(
            judge_insert(&[whole(5), whole(9)]),
            InjectOutcome::Complete { accepted_pairs: 14 }
        );

        // The erase went out, the text did not: partial, and the accepted half
        // is still named so the caller does not have to guess what is in there.
        assert_eq!(
            judge_insert(&[whole(5), short(0, 18)]),
            InjectOutcome::Partial { accepted_pairs: 5 }
        );

        // Neither went out.
        assert_eq!(
            judge_insert(&[short(0, 10), short(0, 18)]),
            InjectOutcome::Failed
        );

        // A torn pair is not a success, and its remainder is not counted.
        assert_eq!(
            judge_insert(&[Injection {
                total_events: 3,
                attempted: 3,
                accepted: 3,
                stopped: None
            }]),
            InjectOutcome::Partial { accepted_pairs: 1 }
        );

        // An erase alone, stopped halfway.
        assert_eq!(
            judge_insert(&[short(3, 10)]),
            InjectOutcome::Partial { accepted_pairs: 1 }
        );
    }

    /// The default of an unused option must not read as a success. `None` in the
    /// seam's words is the neutral value, and mapping it onto "delivered" is the
    /// same mistake as mapping an uncaptured destination onto "allowed".
    #[test]
    fn a_default_report_is_a_whole_send_of_nothing() {
        let report = Injection::default();
        assert!(report.whole_pairs());
        assert_eq!(report.pairs(), 0);
    }
}
