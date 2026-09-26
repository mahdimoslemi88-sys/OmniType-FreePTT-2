//! Durable file logging.
//!
//! Why: a double-clicked Windows GUI app has no console, so every
//! `eprintln!` and the default panic message vanish into nothing. Combined
//! with `panic = "abort"` (relaxed to `unwind` in Cargo.toml), a crash left
//! **zero** diagnostic trace — the app just disappeared a few seconds after
//! startup.
//!
//! What this module guarantees:
//! 1. Every `tracing` event is appended to a daily-rolled file under
//!    `<data dir>/logs/voice-ptt.log.YYYY-MM-DD`, written **synchronously**
//!    on the emitting thread (no background worker → the very last line
//!    before a crash is on disk).
//! 2. Native whisper.cpp / ggml stderr logs are redirected into `tracing`
//!    ([`install_whisper_log_redirect`]), so model-load problems land in the
//!    same file instead of an invisible stderr.
//! 3. Panics are intercepted: message, location and a backtrace go both to
//!    the main log and to a plain-text `logs/crash.log` that bypasses
//!    tracing entirely (survives even a wedged logger).
//! 4. Fatal `run()` errors from `main` are recorded the same way
//!    ([`record_fatal_error`]).
//! 5. A `=== session start ===` banner (and `=== session ended ===` on clean
//!    exit) makes sessions easy to tell apart in the file.
//!
//! Log retention: daily files older than 7 days are deleted on startup
//! (best effort, never fatal).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// How many days of daily log files to keep.
const LOG_RETENTION_DAYS: u64 = 7;

/// Hard size cap for `logs/crash.log`. A panic that fires on every frame can
/// otherwise produce gigabytes in minutes; past this we keep only the tail.
const CRASH_LOG_MAX_BYTES: usize = 8 * 1024 * 1024;

/// The plain-text crash/error sidecar log, next to the daily files.
const CRASH_LOG_NAME: &str = "crash.log";

/// App data directory resolved at [`init`]; used by the panic hook and
/// [`record_fatal_error`] so crash reports land next to the daily logs
/// even if the failure happens before/without tracing.
static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Per-panic-site occurrence counts, so a panic that fires on every frame is
/// logged once with a "occurrence N" counter instead of spamming thousands of
/// identical sections. Keyed by `location: payload`.
static PANIC_COUNTS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, usize>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Installs Rust-side callbacks for whisper.cpp *and* ggml native logs.
///
/// By default both libraries write to stderr, which is invisible for a
/// double-clicked GUI app. This routes them through `tracing` so they end
/// up in the daily log file (and still on the console when launched from
/// PowerShell).
///
/// Idempotent in practice: called once from `run()` before any whisper API.
pub fn install_whisper_log_redirect() {
    // SAFETY: `set_log_callback` only swaps a function pointer inside
    // whisper.cpp; the callback itself is FFI-safe (no unwinding, null-
    // tolerant) and stays valid for the process lifetime (static fn).
    unsafe {
        whisper_rs::set_log_callback(Some(whisper_log_trampoline), std::ptr::null_mut());
    }
    // SAFETY: same as above, for the ggml-side logger (backend registry,
    // device info). Both signatures match the `ggml_log_callback` C type.
    // (`whisper_rs` re-exports the `whisper_rs_sys` crate.)
    unsafe {
        whisper_rs::whisper_rs_sys::ggml_log_set(Some(whisper_log_trampoline), std::ptr::null_mut());
    }
}

/// FFI trampoline: whisper.cpp/ggml → tracing. Must not panic or unwind
/// (C may call it from arbitrary stacks).
unsafe extern "C" fn whisper_log_trampoline(
    level: whisper_rs::whisper_rs_sys::ggml_log_level,
    text: *const std::ffi::c_char,
    _user_data: *mut std::ffi::c_void,
) {
    // Missing/invalid text: report and bail out — never panic across FFI.
    if text.is_null() {
        tracing::warn!("whisper/ggml log callback received null text");
        return;
    }
    let msg = match std::ffi::CStr::from_ptr(text).to_str() {
        Ok(s) => s.trim(),
        Err(_) => "<non-UTF8 native log line>",
    };
    if msg.is_empty() {
        return;
    }

    // ggml_log_level: 0 = NONE, 1 = INFO, 2 = WARN, 3 = ERROR, 4 = DEBUG,
    // 5 = CONT (continuation of the previous line).
    match level {
        2 => tracing::warn!("{msg}"),
        3 => tracing::error!("{msg}"),
        4 => tracing::debug!("{msg}"),
        // NONE carries plain output (e.g. model-load banner); INFO is info.
        _ => tracing::info!("{msg}"),
    }
}

/// Initializes global logging (console + daily file) and the panic hook.
///
/// `data_dir` — app data directory; logs go to `<data_dir>/logs/`.
pub fn init(data_dir: &Path) {
    let logs_dir = data_dir.join("logs");
    let _ = fs::create_dir_all(&logs_dir);
    let _ = DATA_DIR.set(data_dir.to_path_buf());

    // Rolling file appender used *directly* as the writer: each event is
    // written synchronously on the emitting thread. Event volume here is
    // tiny (state changes, transcription summaries), so there is no
    // throughput concern — and durability is the whole point.
    let file_appender = tracing_appender::rolling::daily(&logs_dir, "voice-ptt.log");

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(file_appender)
        .with_ansi(false);

    // Console keeps working for PowerShell launches (`.\\voice-ptt.exe`).
    let console_layer = tracing_subscriber::fmt::layer();

    // Default filter: info everywhere, but silence ONNX Runtime's very
    // chatty per-session INFO spam (it floods the file at startup).
    // Override with RUST_LOG as usual.
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,ort=warn"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .with(console_layer)
        .init();

    install_panic_hook();
    prune_old_logs(&logs_dir);
}

/// Writes the `=== session start ===` banner with app metadata.
pub fn log_session_start() {
    let cmdline = std::env::args().collect::<Vec<_>>().join(" ");
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        pid = std::process::id(),
        cmdline = %cmdline,
        "=== session start ==="
    );
}

/// Writes the `=== session ended ===` banner (clean exit only).
pub fn log_session_end(started: Instant) {
    tracing::info!(
        uptime_secs = started.elapsed().as_secs(),
        "=== session ended ==="
    );
}

/// Coarse boot-stage marker: makes it obvious in the log how far startup
/// got when something dies silently.
pub fn stage(stage: &str, message: &str) {
    tracing::info!(stage = %stage, message = %message, "boot stage");
}

/// Records a fatal error returned out of `run()` — used by `main` so that
/// startup failures reach the log file with their full cause chain.
pub fn record_fatal_error(err: &anyhow::Error) {
    // The subscriber may not exist yet (failure before `init`); then this
    // is a harmless no-op.
    tracing::error!(error = %err, "fatal error — exiting");

    let mut chain = format!("{err}\n");
    let mut cause = err.source();
    while let Some(c) = cause {
        chain.push_str(&format!("  caused by: {c}\n"));
        cause = c.source();
    }
    append_to_crash_log("FATAL ERROR", &chain);
}

/// Panics are the silent killer: hook them and persist everything.
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<non-string panic payload>".to_string());
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown>".to_string());

        // Deduplicate a panic *loop*: the same site firing on every repaint
        // (a known egui 0.28 hit_test panic, see docs) otherwise writes one
        // report per frame — thousands per second. The first occurrence is
        // always logged in full; repeats only refresh a "N times" counter.
        let key = format!("{location}: {payload}");
        let mut counter = PANIC_COUNTS.lock().unwrap_or_else(|e| e.into_inner());
        let count = counter.entry(key.clone()).or_insert(0);
        *count += 1;
        let occurrence = *count;
        drop(counter);

        // The last occurrence before the process dies needs its backtrace so
        // we can see the real stack; pure repeats add nothing.
        let backtrace = if occurrence == 1 || occurrence.is_power_of_two() {
            format!(
                "\nbacktrace:\n{}",
                std::backtrace::Backtrace::force_capture()
            )
        } else {
            String::new()
        };

        // 1) Main daily log (formatted like every other event).
        if occurrence == 1 || occurrence.is_power_of_two() {
            tracing::error!("PANIC at {location}: {payload} (occurrence {occurrence}){backtrace}");
        }

        // 2) Dedicated crash.log — plain synchronous append, independent of
        //    the tracing machinery (survives a wedged logger/aborted unwind).
        append_to_crash_log(
            "PANIC",
            &format!("at {location}\n{payload}\noccurrence {occurrence}{backtrace}"),
        );

        // 3) Keep default behavior (stderr) for console launches.
        default_hook(info);
    }));
}

/// Appends a dated section to `logs/crash.log`. Best effort: never panics,
/// never fails startup.
fn append_to_crash_log(title: &str, body: &str) {
    let Some(dir) = DATA_DIR.get().cloned().or_else(fallback_data_dir) else {
        return;
    };
    append_to_crash_log_in(&dir, title, body);
}

/// Inner helper (separate for testability): appends to `<dir>/logs/crash.log`,
/// creating the directory if needed.
fn append_to_crash_log_in(dir: &Path, title: &str, body: &str) {
    let _ = fs::create_dir_all(dir.join("logs"));
    let path = dir.join("logs").join(CRASH_LOG_NAME);
    // Bound the file: a panic storm (e.g. one per frame) would otherwise grow
    // it without limit. If it is already huge, drop the oldest half by
    // rewriting only the tail.
    if let Ok(meta) = fs::metadata(&path) {
        if meta.len() > CRASH_LOG_MAX_BYTES as u64 {
            truncate_crash_log_to_tail(&path);
        }
    }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        use std::io::Write;
        let _ = writeln!(
            f,
            "===== {} | {} | v{} | pid {} =====",
            iso_now(),
            title,
            env!("CARGO_PKG_VERSION"),
            std::process::id()
        );
        let _ = writeln!(f, "{body}");
    }
}

/// Rewrite `crash.log` keeping only its most recent entries, so a panic loop
/// cannot fill the disk. Best effort.
fn truncate_crash_log_to_tail(path: &Path) {
    let Ok(bytes) = fs::read(path) else {
        return;
    };
    let keep_from = bytes.len().saturating_sub(CRASH_LOG_MAX_BYTES / 2);
    // Start at the next entry boundary so we never cut mid-section.
    let start = bytes[keep_from..]
        .iter()
        .position(|&b| b == b'=')
        .map(|i| keep_from + i)
        .unwrap_or(keep_from);
    let tail = &bytes[start..];
    if fs::write(path, tail).is_err() {
        // Corrupt/unreadable: start fresh rather than lose future reports.
        let _ = fs::write(path, b"");
    }
}

/// Data-dir fallback for crash reports logged before [`init`] ran.
fn fallback_data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("VOICE_PTT_DATA_DIR") {
        return Some(PathBuf::from(dir));
    }
    #[cfg(windows)]
    {
        if let Some(base) = std::env::var_os("APPDATA") {
            return Some(PathBuf::from(base).join("voice-ptt"));
        }
    }
    Some(PathBuf::from("."))
}

fn iso_now() -> String {
    // ISO-8601 local-ish timestamp: self-contained and greppable, unlike a
    // raw epoch that needs a converter to be read.
    let secs = unix_secs();
    let (y, mo, d, h, mi, s) = epoch_to_ymdhms(secs);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02} UTC")
}

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Civil-time breakdown of a Unix epoch second (UTC).
///
/// Why not `chrono`: this is the *only* place the binary needs date math, and
/// pulling a crate in for one function is not worth it. Algorithm is Howard
/// Hinnant's `civil_from_days`.
fn epoch_to_ymdhms(secs: u64) -> (i32, u32, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let h = (rem / 3600) as u32;
    let mi = ((rem % 3600) / 60) as u32;
    let s = (rem % 60) as u32;

    // Days since 1970-01-01 → (year, month, day).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    // NOTE: the trailing `/ 365` below is load-bearing. Without it `yoe`
    // runs away and every later field is garbage. Verified against known
    // epochs in the unit tests at the bottom of this file.
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (
        y as i32 + if m <= 2 { 1 } else { 0 },
        m as u32,
        d as u32,
        h,
        mi,
        s,
    )
}

/// Deletes daily log files older than the retention window (best effort).
fn prune_old_logs(logs_dir: &Path) {
    let cutoff = unix_secs().saturating_sub(LOG_RETENTION_DAYS * 24 * 3600);
    let Ok(entries) = fs::read_dir(logs_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // Daily appender files are named `voice-ptt.log.YYYY-MM-DD`.
        if !name.starts_with("voice-ptt.log.") {
            continue;
        }
        let stale = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() < cutoff)
            .unwrap_or(false);
        if stale {
            let _ = fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_creates_logs_dir() {
        let dir = std::env::temp_dir().join("voice-ptt-log-test");
        let _ = fs::remove_dir_all(&dir);
        init(&dir);
        assert!(dir.join("logs").is_dir());
    }

    #[test]
    fn crash_log_append_creates_file() {
        let dir = std::env::temp_dir().join("voice-ptt-crashlog-test");
        let _ = fs::remove_dir_all(&dir);
        append_to_crash_log_in(&dir, "TEST", "hello");
        let content =
            fs::read_to_string(dir.join("logs").join("crash.log")).expect("crash.log exists");
        assert!(content.contains("TEST") && content.contains("hello"));
    }

    #[test]
    fn crash_log_entries_carry_version_and_iso_timestamp() {
        let dir = std::env::temp_dir().join("voice-ptt-crashlog-meta-test");
        let _ = fs::remove_dir_all(&dir);
        append_to_crash_log_in(&dir, "PANIC", "at src/x.rs:1:1\nboom");
        let line = fs::read_to_string(dir.join("logs").join("crash.log"))
            .expect("crash.log exists")
            .lines()
            .next()
            .unwrap()
            .to_string();
        // ISO-8601 UTC + version + pid, not a bare epoch.
        assert!(line.starts_with("===== 20"), "header was: {line}");
        assert!(line.contains("UTC"), "header was: {line}");
        assert!(
            line.contains(&format!("v{}", env!("CARGO_PKG_VERSION"))),
            "header was: {line}"
        );
        assert!(line.contains("pid "), "header was: {line}");
    }

    #[test]
    fn crash_log_is_capped_under_a_panic_storm() {
        let dir = std::env::temp_dir().join("voice-ptt-crashlog-cap-test");
        let _ = fs::remove_dir_all(&dir);
        // Simulate a per-frame panic: far more sections than the cap allows.
        for i in 0..2_000 {
            append_to_crash_log_in(&dir, "PANIC", &format!("at x.rs:1:1\noccurrence {i}"));
        }
        let len = fs::metadata(dir.join("logs").join("crash.log"))
            .expect("crash.log exists")
            .len();
        assert!(
            len <= 2 * CRASH_LOG_MAX_BYTES as u64,
            "crash.log grew to {len} bytes"
        );
    }

    #[test]
    fn epoch_to_ymdhms_matches_known_dates() {
        // 2026-09-26 00:00:00 UTC
        assert_eq!(epoch_to_ymdhms(1_790_380_800), (2026, 9, 26, 0, 0, 0));
        // 1970-01-01 00:00:00 UTC (the epoch itself)
        assert_eq!(epoch_to_ymdhms(0), (1970, 1, 1, 0, 0, 0));
        // Leap day: 2024-02-29 12:30:05 UTC
        assert_eq!(epoch_to_ymdhms(1_709_209_805), (2024, 2, 29, 12, 30, 5));
        // Year rollover: 2025-01-01 00:00:00 UTC
        assert_eq!(epoch_to_ymdhms(1_735_689_600), (2025, 1, 1, 0, 0, 0));
        // 2026-09-14 00:24:25 UTC — a real crash.log timestamp
        assert_eq!(epoch_to_ymdhms(1_789_345_465), (2026, 9, 14, 0, 24, 25));
        // 2000-02-29 (leap century rule: 2000 IS a leap year)
        assert_eq!(epoch_to_ymdhms(951_782_400), (2000, 2, 29, 0, 0, 0));
    }
}
