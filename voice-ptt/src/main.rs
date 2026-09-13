//! voice-ptt — Windows executable entry point.

fn main() {
    if let Err(e) = voice_ptt::run() {
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
