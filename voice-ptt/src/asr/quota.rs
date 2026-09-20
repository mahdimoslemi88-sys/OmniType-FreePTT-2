//! Persistent daily-quota tracker for the cloud ASR engine.
//!
//! Groq's free tier allows ~300 requests per day. Hitting the limit is *not*
//! a transient failure: retrying every router-cooldown (30 s) would spam the
//! API until midnight. Instead the engine keeps a date-keyed counter in
//! `cloud_usage.json`; once the daily limit is reached, `health()` reports a
//! `Cooldown` lasting until local midnight, so the router permanently prefers
//! the local whisper engine for the rest of the day — zero network calls.
//!
//! The counter survives restarts (the whole point: closing/reopening the app
//! must not reset the budget) and rolls over automatically when the local
//! date changes.

use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::Context;

/// One day's usage record, persisted as JSON.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct QuotaState {
    /// Local calendar date, e.g. "2026-09-15".
    date: String,
    /// Requests consumed on that date.
    used: u32,
}

/// Daily request budget with on-disk persistence.
pub struct DailyQuota {
    path: PathBuf,
    limit: u32,
    inner: Mutex<QuotaState>,
}

impl DailyQuota {
    /// Loads (or initializes) the quota file. A record from a previous day
    /// is reset automatically.
    pub fn new(path: PathBuf, limit: u32) -> Self {
        let state = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<QuotaState>(&s).ok())
            .map(|s| {
                if s.date == local_date_string() {
                    s
                } else {
                    QuotaState {
                        date: local_date_string(),
                        used: 0,
                    }
                }
            })
            .unwrap_or_else(|| QuotaState {
                date: local_date_string(),
                used: 0,
            });
        Self {
            path,
            limit,
            inner: Mutex::new(state),
        }
    }

    /// Remaining requests today (never negative).
    pub fn remaining(&self) -> u32 {
        let mut state = self.inner.lock().expect("quota mutex poisoned");
        self.rollover_if_new_day(&mut state);
        self.limit.saturating_sub(state.used)
    }

    /// Requests consumed today.
    pub fn used(&self) -> u32 {
        let mut state = self.inner.lock().expect("quota mutex poisoned");
        self.rollover_if_new_day(&mut state);
        state.used
    }

    /// Configured daily limit.
    pub fn limit(&self) -> u32 {
        self.limit
    }

    /// Whether the budget for today is spent.
    pub fn exhausted(&self) -> bool {
        self.remaining() == 0
    }

    /// Records one consumed request (call after the server accepted it).
    pub fn record_request(&self) {
        let mut state = self.inner.lock().expect("quota mutex poisoned");
        self.rollover_if_new_day(&mut state);
        state.used = state.used.saturating_add(1);
        self.persist(&state);
    }

    /// Marks today's budget as spent (server reported a daily-limit 429 even
    /// though our local counter disagrees — the server wins).
    pub fn exhaust_today(&self) {
        let mut state = self.inner.lock().expect("quota mutex poisoned");
        self.rollover_if_new_day(&mut state);
        state.used = self.limit;
        self.persist(&state);
    }

    /// Milliseconds until the local-midnight reset.
    pub fn ms_until_reset(&self) -> u64 {
        ms_until_local_midnight()
    }

    /// If the local date changed since the record was written, reset it.
    fn rollover_if_new_day(&self, state: &mut QuotaState) {
        let today = local_date_string();
        if state.date != today {
            state.date = today;
            state.used = 0;
            self.persist(state);
        }
    }

    fn persist(&self, state: &QuotaState) {
        // Best effort: losing one day's counter only risks a couple of extra
        // 429s, never correctness — the server remains the source of truth.
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let json = serde_json::to_string(state).context("serialize quota state");
        match json {
            Ok(json) => {
                let tmp = self.path.with_extension("json.tmp");
                if std::fs::write(&tmp, json).is_ok() {
                    let _ = std::fs::rename(&tmp, &self.path);
                }
            }
            Err(e) => tracing::warn!(%e, "failed to serialize quota state"),
        }
    }
}

/// Local calendar date as `YYYY-MM-DD`.
#[cfg(windows)]
pub fn local_date_string() -> String {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let st = unsafe { GetLocalTime() }; // returns Foundation::SYSTEMTIME by value
    format!("{:04}-{:02}-{:02}", st.wYear, st.wMonth, st.wDay)
}

/// Local (here: UTC fallback) calendar date as `YYYY-MM-DD`.
#[cfg(not(windows))]
pub fn local_date_string() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(windows)]
fn ms_until_local_midnight() -> u64 {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let st = unsafe { GetLocalTime() };
    let secs_today = u64::from(st.wHour) * 3600
        + u64::from(st.wMinute) * 60
        + u64::from(st.wSecond);
    (86_400 - secs_today) * 1_000
}

#[cfg(not(windows))]
fn ms_until_local_midnight() -> u64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    (86_400 - secs % 86_400) * 1_000
}

/// Howard Hinnant's civil-from-days algorithm (no chrono dependency).
#[cfg(not(windows))]
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_quota(limit: u32) -> (DailyQuota, PathBuf) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        // Unique dir per call: tests run in parallel threads; a shared file
        // made `requests_are_counted_and_persisted` flaky.
        let dir = std::env::temp_dir().join(format!(
            "voice-ptt-quota-test-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cloud_usage.json");
        let _ = std::fs::remove_file(&path);
        (DailyQuota::new(path.clone(), limit), path)
    }

    #[test]
    fn fresh_quota_starts_full() {
        let (q, _) = temp_quota(300);
        assert_eq!(q.remaining(), 300);
        assert!(!q.exhausted());
        assert_eq!(q.used(), 0);
    }

    #[test]
    fn requests_are_counted_and_persisted() {
        let (q, path) = temp_quota(300);
        q.record_request();
        q.record_request();
        assert_eq!(q.used(), 2);
        assert_eq!(q.remaining(), 298);

        // A new instance (app restart) must see the same counter.
        let q2 = DailyQuota::new(path, 300);
        assert_eq!(q2.used(), 2, "counter must survive restart");
    }

    #[test]
    fn previous_day_record_resets() {
        let dir = std::env::temp_dir().join(format!("voice-ptt-quota-roll-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cloud_usage.json");

        // Simulate yesterday's exhausted budget on disk.
        let yesterday = QuotaState {
            date: "2000-01-01".into(),
            used: 300,
        };
        std::fs::write(&path, serde_json::to_string(&yesterday).unwrap()).unwrap();

        let q = DailyQuota::new(path, 300);
        assert_eq!(q.used(), 0, "old records must reset at date change");
        assert!(!q.exhausted());
    }

    #[test]
    fn exhaust_today_marks_budget_spent() {
        let (q, _) = temp_quota(5);
        q.exhaust_today();
        assert!(q.exhausted());
        assert_eq!(q.remaining(), 0);
        assert!(q.ms_until_reset() > 0);
        assert!(q.ms_until_reset() <= 86_400 * 1_000);
    }

    #[test]
    fn saturating_at_limit() {
        let (q, _) = temp_quota(2);
        for _ in 0..5 {
            q.record_request();
        }
        assert_eq!(q.used(), 5, "over-limit requests still counted honestly");
        assert!(q.exhausted());
    }

    #[test]
    fn date_format_is_iso() {
        let d = local_date_string();
        assert_eq!(d.len(), 10);
        assert_eq!(d.as_bytes()[4], b'-');
        assert_eq!(d.as_bytes()[7], b'-');
    }

    #[cfg(not(windows))]
    #[test]
    fn civil_algorithm_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1)); // 2024-01-01
    }
}
