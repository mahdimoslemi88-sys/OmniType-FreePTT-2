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

/// The plain-text crash/error sidecar log, next to the daily files.
const CRASH_LOG_NAME: &str = "crash.log";

/// App data directory resolved at [`init`]; used by the panic hook and
/// [`record_fatal_error`] so crash reports land next to the daily logs
/// even if the failure happens before/without tracing.
static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

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
        let backtrace = std::backtrace::Backtrace::force_capture();

        // 1) Main daily log (formatted like every other event).
        tracing::error!("PANIC at {location}: {payload}\n{backtrace}");

        // 2) Dedicated crash.log — plain synchronous append, independent of
        //    the tracing machinery (survives a wedged logger/aborted unwind).
        append_to_crash_log(
            "PANIC",
            &format!("at {location}\n{payload}\nbacktrace:\n{backtrace}"),
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
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        use std::io::Write;
        let _ = writeln!(f, "----- {} | {} -----", unix_now(), title);
        let _ = writeln!(f, "{body}");
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

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Deletes daily log files older than the retention window (best effort).
fn prune_old_logs(logs_dir: &Path) {
    let cutoff = unix_now().saturating_sub(LOG_RETENTION_DAYS * 24 * 3600);
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
}
