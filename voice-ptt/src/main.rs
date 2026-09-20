#![windows_subsystem = "windows"]

//! voice-ptt — Windows executable entry point.

fn main() {
    if let Err(e) = voice_ptt::run() {
        // Persist the failure (with its full cause chain) to the log file —
        // stderr is invisible for double-clicked GUI launches.
        voice_ptt::logging::record_fatal_error(&e);

        eprintln!("error: {e:#}");
        // Surface the full cause chain to stderr for diagnostics.
        let mut cause = e.source();
        while let Some(c) = cause {
            eprintln!("  caused by: {c}");
            cause = c.source();
        }
        std::process::exit(1);
    }
}
