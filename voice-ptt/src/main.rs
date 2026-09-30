#![windows_subsystem = "windows"]

//! voice-ptt — Windows executable entry point.

fn main() {
    // `--doctor` is checked before anything else, and writes a file rather than
    // printing: this binary is built with the `windows` subsystem, so it has no
    // console and no stdout even when launched from one. `voice-ptt.exe
    // --doctor` therefore produces a report next to config.toml and exits
    // without touching the microphone, the models, or the network.
    if voice_ptt::doctor_requested() {
        let report = voice_ptt::run_doctor();
        tracing::info!(report = %report.display(), "diagnostic report written");
        std::process::exit(0);
    }

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
