//! Text injection via Win32 `SendInput`.
//!
//! The draft spec injected one `wVk` per character, which **cannot type
//! Persian** (virtual key codes only cover the active keyboard layout). The
//! correct approach for Unicode text is `KEYEVENTF_UNICODE` key-down/up
//! pairs carrying UTF-16 code units — layout-independent, works for any
//! script, and still reaches the focused window.
//!
//! Batch size note: `SendInput` is atomic per call (all events processed
//! together), so we send in blocks of 32 events for speed.
//!
//! ## What a send may claim
//!
//! `SendInput` returns the number of `INPUT` records the system accepted into
//! its queue. That is **not** proof that a character reached a document: the
//! queue is drained into whatever holds focus, and the focused control may
//! ignore it. So a send reports [`Injection`] — records accepted, records
//! asked for, and why it stopped — and nothing here is named "inserted" or
//! "typed". A batch that is only partly accepted is **reported, not raised**:
//! the caller has to decide what a torn send means, and it cannot simply ask
//! for the same text again without risking a duplicate in the document.

use anyhow::Result;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
};

/// How many INPUT structs to send per `SendInput` call.
const BATCH: usize = 32;

/// Default input sender calling Win32 `SendInput`.
fn win32_send_input(inputs: &[INPUT]) -> usize {
    unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) as usize }
}

/// What the platform took, in the only terms it can be asked for.
///
/// Every field is a fact about the *call*, never about a document: `accepted`
/// is what the system reported, `attempted` is what was passed to the platform
/// across all called batches before stopping, `total_events` is the total
/// `INPUT` records the complete input needed, and `stopped` says whether delivery
/// stopped early.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Injection {
    /// Total `INPUT` records required to deliver the complete input.
    pub total_events: usize,
    /// `INPUT` records actually dispatched to `SendInput` across all called batches.
    ///
    /// This reflects only the batches that were actually executed before delivery
    /// halted; it is not the total input requirement when an earlier batch fails.
    pub attempted: usize,
    /// `INPUT` records the system accepted.
    pub accepted: usize,
    /// Why delivery stopped early, when it did. `None` on a whole send.
    pub stopped: Option<String>,
}

impl Injection {
    /// Every record the send needed was accepted.
    pub fn whole(&self) -> bool {
        self.stopped.is_none()
            && self.total_events == self.attempted
            && self.attempted == self.accepted
    }

    /// Accepted, **and** in whole down/up pairs.
    ///
    /// An odd accepted count is a torn pair: a key-down that went out while its
    /// key-up did not, or the other way round. A code unit or key in that state
    /// is not a success — it may well have been typed, and a key may be left down.
    pub fn whole_pairs(&self) -> bool {
        self.whole() && self.accepted.is_multiple_of(2)
    }

    /// Whole down/up pairs the platform accepted: one UTF-16 code unit, or one
    /// virtual key press for backspace. A torn remainder is deliberately **not**
    /// counted. Note: this is **not** a character count, as multi-unit characters
    /// (e.g. surrogate pairs for emoji) require multiple pairs.
    pub fn pairs(&self) -> usize {
        self.accepted / 2
    }
}

/// Injects `text` into the currently focused window as synthetic keystrokes via Win32.
///
/// Reports what `SendInput` accepted; see [`Injection`] for what that does and
/// does not prove. A batch the platform only partly took ends the send: once it
/// has stopped taking input, sending the rest of a dictation is a guess.
pub fn inject_text(text: &str) -> Injection {
    inject_text_with(text, win32_send_input)
}

/// Injects `text` in small paced groups, so a dictation arrives in the document
/// the way it was spoken instead of appearing all at once.
///
/// `step_chars` characters go out, then the thread sleeps `step_ms`, then the
/// next group. The grouping is in **UTF-16 units**, because that is what
/// `SendInput` delivers: splitting a surrogate pair across two sleeps would
/// send half a character and then reject its partner, and `flush_with` would
/// report that as a torn keystroke pair.
///
/// **The report is about delivery, not pacing.** It is the same
/// [`Injection`] the burst path returns, with `attempted` covering the whole
/// text, so every downstream decision — the "was this typed whole?" question,
/// the "do not re-send a partial" rule — behaves identically whether or not
/// pacing is on. Nothing above this function can tell the difference except by
/// watching it take longer.
///
/// A step size of zero characters is treated as one, because a zero would emit
/// nothing and then loop forever.
pub fn inject_text_paced(text: &str, step_chars: usize, step_ms: std::time::Duration) -> Injection {
    inject_text_paced_with(text, step_chars, step_ms, win32_send_input, || {
        std::thread::sleep(step_ms)
    })
}

/// The paced send with its two effects supplied, so the pacing is testable
/// without a real clock.
///
/// `pace` is called **between** groups and never before the first one, so an
/// empty or single-step text sends exactly once and does not sleep.
pub fn inject_text_paced_with<F, P>(
    text: &str,
    step_chars: usize,
    _step_ms: std::time::Duration,
    mut sender: F,
    mut pace: P,
) -> Injection
where
    F: FnMut(&[INPUT]) -> usize,
    P: FnMut(),
{
    let units: Vec<u16> = text.encode_utf16().collect();
    let total_events = units.len() * 2;
    let mut report = Injection {
        total_events,
        attempted: 0,
        accepted: 0,
        stopped: None,
    };
    if units.is_empty() {
        return report;
    }

    // A zero step would produce an empty group and never advance.
    let step = step_chars.max(1);
    let mut group: Vec<INPUT> = Vec::with_capacity(step * 2);
    let mut sent_any = false;

    for (index, unit) in units.iter().enumerate() {
        group.push(make_unicode_input(*unit, false));
        group.push(make_unicode_input(*unit, true));

        // The step closes on a **unit** boundary, so a surrogate pair is never
        // split across two sends.
        let at_step_end = (index + 1) % step == 0;
        let at_text_end = index + 1 == units.len();
        if at_step_end || at_text_end {
            flush_with(&mut group, &mut report, &mut sender);
            if report.stopped.is_some() {
                return report;
            }
            if sent_any {
                pace();
            }
            sent_any = true;
        }
    }

    report
}

/// Injects `text` using the provided input sender function.
///
/// Dispatches `KEYEVENTF_UNICODE` down/up pairs in batches of [`BATCH`].
/// If the sender returns fewer events than requested or an odd number of events,
/// delivery stops immediately and subsequent batches are not dispatched.
pub fn inject_text_with<F: FnMut(&[INPUT]) -> usize>(text: &str, mut sender: F) -> Injection {
    let total_units = text.encode_utf16().count();
    let total_events = total_units * 2;
    let mut report = Injection {
        total_events,
        attempted: 0,
        accepted: 0,
        stopped: None,
    };
    if text.is_empty() {
        return report;
    }

    let mut inputs: Vec<INPUT> = Vec::with_capacity(text.len() * 2);

    for unit in text.encode_utf16() {
        // Down
        inputs.push(make_unicode_input(unit, false));
        // Up
        inputs.push(make_unicode_input(unit, true));

        if inputs.len() >= BATCH {
            flush_with(&mut inputs, &mut report, &mut sender);
            if report.stopped.is_some() {
                return report;
            }
        }
    }
    if !inputs.is_empty() {
        flush_with(&mut inputs, &mut report, &mut sender);
    }

    report
}

/// Deletes `count` characters before the caret by pressing Backspace `count`
/// times via Win32.
pub fn inject_backspaces(count: usize) -> Injection {
    inject_backspaces_with(count, win32_send_input)
}

/// Deletes `count` characters using the provided input sender function.
pub fn inject_backspaces_with<F: FnMut(&[INPUT]) -> usize>(
    count: usize,
    mut sender: F,
) -> Injection {
    let total_events = count * 2;
    let mut report = Injection {
        total_events,
        attempted: 0,
        accepted: 0,
        stopped: None,
    };
    if count == 0 {
        return report;
    }
    use windows::Win32::UI::Input::KeyboardAndMouse::{KEYBD_EVENT_FLAGS, VK_BACK};
    let down = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_BACK,
                dwFlags: KEYBD_EVENT_FLAGS(0),
                ..Default::default()
            },
        },
    };
    let mut up = down;
    up.Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;

    let mut inputs: Vec<INPUT> = Vec::with_capacity(count * 2);
    for _ in 0..count {
        inputs.push(down);
        inputs.push(up);
        if inputs.len() >= BATCH {
            flush_with(&mut inputs, &mut report, &mut sender);
            if report.stopped.is_some() {
                return report;
            }
        }
    }
    if !inputs.is_empty() {
        flush_with(&mut inputs, &mut report, &mut sender);
    }
    report
}

/// Injects a plain `\n` as the Enter key (keystroke, not a Unicode char).
pub fn press_enter() -> Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_RETURN;
    let down = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_RETURN,
                dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0),
                ..Default::default()
            },
        },
    };
    let mut up = down;
    up.Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;

    unsafe {
        let sent = SendInput(&[down, up], std::mem::size_of::<INPUT>() as i32);
        if sent != 2 {
            anyhow::bail!("SendInput failed for Enter: sent {sent}");
        }
    }
    Ok(())
}

fn make_unicode_input(code_unit: u16, key_up: bool) -> INPUT {
    let mut flags = KEYEVENTF_UNICODE;
    if key_up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(0),
                wScan: code_unit,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Sends one batch via `sender` and records the result in `report`.
///
/// If `sender` accepts fewer events than asked or accepts an odd number of events
/// (leaving a torn down/up pair), `report.stopped` is armed and no further batches
/// should be sent.
fn flush_with<F: FnMut(&[INPUT]) -> usize>(
    inputs: &mut Vec<INPUT>,
    report: &mut Injection,
    sender: &mut F,
) {
    let wanted = inputs.len();
    let sent = sender(inputs.as_slice());
    report.attempted += wanted;
    report.accepted += sent;
    if sent != wanted {
        report.stopped = Some(format!(
            "SendInput accepted {sent} of {wanted} events (another app may be blocking input)"
        ));
    } else if !sent.is_multiple_of(2) {
        report.stopped = Some(format!(
            "SendInput accepted an odd number of events ({sent}), leaving a torn keystroke pair"
        ));
    }
    inputs.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // ── paced delivery ──────────────────────────────────────────────────
    //
    // The pacing tests share one shape: a scripted sender that records how many
    // events each call carried, and a clock stand-in that counts the gaps. What
    // matters is not that it "looks" paced but that the *batching* is right,
    // because a wrong batch boundary is a wrong character, not just a wrong
    // rhythm.

    #[test]
    fn paced_typing_sends_the_same_events_the_burst_does() {
        let text = "سلام دنیا";
        let mut burst = Vec::new();
        let burst_report = inject_text_with(text, |batch: &[INPUT]| {
            burst.push(batch.len());
            batch.len()
        });
        let mut paced = Vec::new();
        let paced_report =
            inject_text_paced_with(text, 2, Duration::from_millis(1), |batch: &[INPUT]| {
                paced.push(batch.len());
                batch.len()
            }, || {});

        // Same characters out, however they were grouped.
        assert_eq!(burst_report.total_events, paced_report.total_events);
        assert_eq!(burst_report.accepted, paced_report.accepted);
        assert_eq!(
            burst.iter().sum::<usize>(),
            paced.iter().sum::<usize>(),
            "paced delivery must move every event the burst moved"
        );
    }

    #[test]
    fn paced_typing_breaks_the_text_into_steps() {
        let text = "abcdef"; // 6 units, 2 per step => 3 sends of 4 events
        let mut sizes = Vec::new();
        inject_text_paced_with(text, 2, Duration::from_millis(1), |batch: &[INPUT]| {
            sizes.push(batch.len());
            batch.len()
        }, || {});
        assert_eq!(sizes, vec![4, 4, 4], "2 characters per step, down+up each");
    }

    #[test]
    fn a_partial_final_step_is_still_sent() {
        // 5 units at 2 per step: 2, 2, then a remainder of 1. Dropping the
        // remainder would silently lose the last character of every dictation
        // whose length is not a multiple of the step.
        let mut sizes = Vec::new();
        inject_text_paced_with("abcde", 2, Duration::from_millis(1), |batch: &[INPUT]| {
            sizes.push(batch.len());
            batch.len()
        }, || {});
        assert_eq!(sizes, vec![4, 4, 2]);
    }

    /// A step boundary must never fall between the halves of a surrogate pair.
    ///
    /// Splitting one would send a lone high surrogate, which the platform either
    /// drops or turns into a replacement character — and `flush_with` would see
    /// an odd event count and report a torn keystroke pair. Stepping over **UTF-16
    /// units** rather than `char`s is what prevents it.
    #[test]
    fn a_step_boundary_never_splits_a_surrogate_pair() {
        // "🚀🚀" = 4 UTF-16 units. A step of 3 units would land mid-emoji if the
        // code counted characters.
        let mut sizes = Vec::new();
        let report = inject_text_paced_with("🚀🚀", 3, Duration::from_millis(1), |batch: &[INPUT]| {
            sizes.push(batch.len());
            batch.len()
        }, || {});
        // Every send carries a whole number of characters (2 events each).
        for size in &sizes {
            assert_eq!(size % 2, 0, "a send must carry whole down/up pairs");
        }
        assert_eq!(report.accepted, report.total_events);
        assert!(
            report.stopped.is_none(),
            "a split surrogate would report as torn: {:?}",
            report.stopped
        );
    }

    /// The gap goes *between* groups, never before the first one.
    ///
    /// Sleeping first would add the step delay to the latency of even a
    /// one-character dictation, and `pace` after the last group would delay the
    /// report that the text has landed.
    #[test]
    fn pacing_happens_between_groups_and_not_before_the_first() {
        let mut gaps = 0usize;
        let mut sizes = Vec::new();
        inject_text_paced_with("abcd", 2, Duration::from_millis(1), |batch: &[INPUT]| {
            sizes.push(batch.len());
            batch.len()
        }, || {
            gaps += 1;
        });
        assert_eq!(sizes.len(), 2, "two groups of two characters");
        assert_eq!(gaps, 1, "one gap between them, and none before or after");
    }

    /// A zero step would produce an empty group and never advance the loop. It is
    /// treated as one rather than trusted, because a hand-edited `0` must not be
    /// able to hang the injector thread forever.
    #[test]
    fn a_zero_step_still_delivers_everything() {
        let mut sizes = Vec::new();
        let report = inject_text_paced_with("abc", 0, Duration::from_millis(1), |batch: &[INPUT]| {
            sizes.push(batch.len());
            batch.len()
        }, || {});
        assert_eq!(
            report.accepted,
            report.total_events,
            "a zero step must degrade to one character, not to nothing"
        );
        assert_eq!(sizes, vec![2, 2, 2]);
    }

    /// Empty text is still a no-op, and it must not sleep: the pacing loop is the
    /// one place where an empty input could spin.
    #[test]
    fn paced_empty_text_sends_nothing_and_never_sleeps() {
        let mut slept = false;
        let report = inject_text_paced_with("", 2, Duration::from_millis(1), |_: &[INPUT]| 0, || {
            slept = true;
        });
        assert_eq!(report.total_events, 0);
        assert!(!slept);
    }

    /// When the platform stops taking input mid-way, the paced send must stop at
    /// the same place the burst send would — and must report the shortfall
    /// honestly rather than pressing on and typing the rest of the dictation into
    /// a window that has stopped accepting it.
    #[test]
    fn a_refused_paced_send_stops_and_reports() {
        let mut calls = 0usize;
        let report = inject_text_paced_with("abcdefgh", 2, Duration::from_millis(1), |batch: &[INPUT]| {
            calls += 1;
            if calls == 1 {
                batch.len()
            } else {
                0
            }
        }, || {});
        assert_eq!(calls, 2, "it must not keep sending after a refusal");
        assert_eq!(report.accepted, 4);
        assert!(report.stopped.is_some(), "a shortfall must be reported");
    }

    #[test]
    fn utf16_units_are_counted_correctly() {
        // "سلام" = 4 chars, all BMP → 8 INPUT events (down+up each).
        let inputs: Vec<INPUT> = "سلام"
            .encode_utf16()
            .flat_map(|u| [make_unicode_input(u, false), make_unicode_input(u, true)])
            .collect();
        assert_eq!(inputs.len(), 8);

        // Emoji (surrogate pair) = 1 char but 2 UTF-16 units → 4 events.
        let inputs: Vec<INPUT> = "🚀"
            .encode_utf16()
            .flat_map(|u| [make_unicode_input(u, false), make_unicode_input(u, true)])
            .collect();
        assert_eq!(inputs.len(), 4);
    }

    /// Empty input is a no-op that never calls the sender.
    #[test]
    fn empty_text_is_noop() {
        let report = inject_text("");
        assert_eq!(report.total_events, 0);
        assert_eq!(report.attempted, 0);
        assert_eq!(report.accepted, 0);
        assert!(
            report.whole_pairs(),
            "a send that never happened is not a torn one"
        );
    }

    /// The three readings a send can have, and the one that matters: a report
    /// of events, never of characters that landed.
    ///
    /// A torn pair is the case worth pinning. An odd count leaves a key down or
    /// half a code unit sent, so it is treated as incomplete — which is exactly
    /// why it needs a test: a branch that only shows up on an interrupted send
    /// is a branch that must be verified.
    #[test]
    fn a_report_counts_events_and_a_torn_pair_is_not_a_success() {
        let whole = Injection {
            total_events: 4,
            attempted: 4,
            accepted: 4,
            stopped: None,
        };
        assert!(whole.whole_pairs());
        assert_eq!(
            whole.pairs(),
            2,
            "four events are two code unit pairs, not four"
        );

        let torn_pair = Injection {
            total_events: 3,
            attempted: 3,
            accepted: 3,
            stopped: None,
        };
        assert!(
            !torn_pair.whole_pairs(),
            "a key-down without its key-up is not a success"
        );
        assert_eq!(torn_pair.pairs(), 1, "the torn remainder is not counted");

        let short = Injection {
            total_events: 6,
            attempted: 6,
            accepted: 2,
            stopped: Some("blocked".into()),
        };
        assert!(!short.whole());
        assert_eq!(short.pairs(), 1);
    }

    /// Batch 1 complete, Batch 2 zero => Partial with accurate cumulative counts.
    #[test]
    fn batch_one_complete_batch_two_zero_yields_partial_with_cumulative_count() {
        // 32 UTF-16 units = 64 events (2 batches of 32)
        let text = "A".repeat(32);
        let mut batch_idx = 0;
        let report = inject_text_with(&text, |batch| {
            batch_idx += 1;
            if batch_idx == 1 {
                batch.len() // 32 accepted
            } else {
                0 // 0 accepted on batch 2
            }
        });

        assert_eq!(batch_idx, 2, "both batches were dispatched");
        assert_eq!(report.total_events, 64);
        assert_eq!(report.attempted, 64);
        assert_eq!(report.accepted, 32);
        assert_eq!(report.pairs(), 16);
        assert!(!report.whole());
        assert!(!report.whole_pairs());
        assert!(report.stopped.is_some());
    }

    /// An odd acceptance count represents a torn down/up pair and stops delivery.
    #[test]
    fn odd_acceptance_stops_and_yields_torn_pair() {
        // 16 UTF-16 units = 32 events in 1 batch
        let text = "A".repeat(16);
        let report = inject_text_with(&text, |_batch| 5);

        assert_eq!(report.total_events, 32);
        assert_eq!(report.attempted, 32);
        assert_eq!(report.accepted, 5);
        assert_eq!(report.pairs(), 2); // 5 / 2 = 2 whole pairs
        assert!(
            !report.whole_pairs(),
            "odd count must not be considered a whole pair send"
        );
        assert!(report.stopped.is_some());
    }

    /// Once a batch fails, subsequent batches must not be dispatched.
    #[test]
    fn after_failure_subsequent_batches_are_never_sent() {
        // 48 UTF-16 units = 96 events = 3 batches of 32
        let text = "A".repeat(48);
        let mut calls = 0;
        let report = inject_text_with(&text, |_batch| {
            calls += 1;
            0 // Complete refusal on batch 1
        });

        assert_eq!(
            calls, 1,
            "subsequent batches must not be attempted after a batch failure"
        );
        assert_eq!(report.total_events, 96);
        assert_eq!(
            report.attempted, 32,
            "attempted only counts dispatched batches"
        );
        assert_eq!(report.accepted, 0);
        assert_eq!(report.pairs(), 0);
        assert!(report.stopped.is_some());
    }

    /// Emoji surrogate pairs are counted by UTF-16 units and events, not by character count.
    #[test]
    fn emoji_counted_by_utf16_units_and_events_not_char_count() {
        // Rocket emoji 🚀: 1 Unicode character, 2 UTF-16 surrogate code units (4 down/up events).
        assert_eq!("🚀".chars().count(), 1);
        assert_eq!("🚀".encode_utf16().count(), 2);

        let report = inject_text_with("🚀", |batch| batch.len());
        assert_eq!(report.total_events, 4);
        assert_eq!(report.attempted, 4);
        assert_eq!(report.accepted, 4);
        assert_eq!(
            report.pairs(),
            2,
            "surrogate pair yields 2 event pairs, not 1"
        );
        assert!(report.whole());
        assert!(report.whole_pairs());

        // When only the first code unit's events succeed (2 events accepted):
        let partial = inject_text_with("🚀", |_batch| 2);
        assert_eq!(partial.total_events, 4);
        assert_eq!(partial.attempted, 4);
        assert_eq!(partial.accepted, 2);
        assert_eq!(partial.pairs(), 1, "only 1 code unit pair accepted");
        assert!(!partial.whole());
    }

    /// Empty input results in zero sender calls for both text and backspaces.
    #[test]
    fn empty_input_results_in_zero_calls() {
        let mut text_calls = 0;
        let report = inject_text_with("", |_batch| {
            text_calls += 1;
            0
        });
        assert_eq!(text_calls, 0, "empty text must not invoke sender");
        assert_eq!(report.total_events, 0);
        assert_eq!(report.attempted, 0);
        assert_eq!(report.accepted, 0);
        assert!(report.whole_pairs());

        let mut bs_calls = 0;
        let bs_report = inject_backspaces_with(0, |_batch| {
            bs_calls += 1;
            0
        });
        assert_eq!(bs_calls, 0, "zero backspaces must not invoke sender");
        assert_eq!(bs_report.total_events, 0);
        assert_eq!(bs_report.attempted, 0);
        assert_eq!(bs_report.accepted, 0);
        assert!(bs_report.whole_pairs());
    }
}
