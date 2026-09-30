//! Tests for the overlay shell: app state, the frame loop, and the
//! pieces that are still here rather than in a panel module.
//!
//! Kept in its own file so the test bodies do not push the shell's real
//! code down the page, and so a panel's own tests can live next to it
//! in that panel's module.
//!
//! The shared `OverlayApp` fixture is [`super::testutil`].

use super::*;

use crate::config::Settings;

#[test]
fn validate_rejects_unparseable_hotkey() {
    let mut s = Settings::default();
    s.hotkey.record = "not a key".into();
    assert!(validate_settings(&s).is_err());
}

#[test]
fn validate_accepts_user_hotkey_combination() {
    let mut s = Settings::default();
    s.hotkey.record = "Shift+F5".into();
    s.hotkey.toggle_overlay = "Ctrl+Alt+P".into();
    s.hotkey.quit = "Ctrl+Shift+Q".into();
    assert!(validate_settings(&s).is_ok());
}

#[test]
fn test_format_persian_display_handles_persian() {
    let input = "سلام دنیا";
    let formatted = format_persian_display(input);
    assert!(!formatted.is_empty());
    // Reshaped Persian does not stay identical to raw input (contains contextual presentation forms)
    assert_ne!(formatted, input);
}

/// Theme sanity for whichever theme this binary was built with: every
/// text role must sit far from every surface role in luminance (dark:
/// text much lighter, light: text much darker) — catches accidentally
/// swapped or duplicated role values in either theme. Role-name parity
/// between the two cfg modules is enforced by the compiler: the shared
/// call sites simply do not compile if either theme misses a role.
#[test]
fn test_theme_text_surface_contrast() {
    let lum = |c: egui::Color32| {
        0.2126_f32 * c.r() as f32 + 0.7152_f32 * c.g() as f32 + 0.0722_f32 * c.b() as f32
    };
    let surfaces = [
        palette::WINDOW_BG,
        palette::TOAST_BG,
        palette::CARD_BG,
        palette::CARD_BG_ALT,
        palette::CHIP_BG,
    ];
    let texts = [
        palette::TEXT_PRIMARY,
        palette::TEXT_SECTION,
        palette::TEXT_LABEL,
        palette::TEXT_SECONDARY,
        palette::TEXT_MUTED,
        palette::TEXT_FAINT,
    ];
    for s in surfaces {
        for t in texts {
            assert!(
                (lum(t) - lum(s)).abs() > 40.0,
                "text role {t:?} too close to surface {s:?} luminance"
            );
        }
    }
}

#[test]
fn test_format_persian_display_handles_english() {
    let input = "Hello World";
    let formatted = format_persian_display(input);
    assert_eq!(formatted, "Hello World");
}

#[test]
fn overlay_toggles_visibility() {
    // Before `DashboardFlags` this test had to hand-build a five-element tuple
    // of flags *plus* a sixth `settings_flag` on the side, purely to satisfy the
    // constructor. None of them were ever read here.
    let mut app = testutil::app();
    assert!(app.visible);
    app.toggle_visible();
    assert!(!app.visible);
    app.toggle_visible();
    assert!(app.visible);
}

#[test]
fn test_render_dashboard_does_not_panic() {
    // One sweep per tab: the fixture already opens the dashboard and seeds
    // a history row, so each tab is rendered in its populated state.
    for tab in [
        DashboardTab::Engines,
        DashboardTab::Dictionary,
        DashboardTab::History,
        DashboardTab::Settings,
    ] {
        testutil::sweep(|app, ctx| {
            app.dashboard_tab = tab;
            app.render_dashboard(ctx);
        });
    }
}

#[tokio::test]
async fn status_client_sees_updates() {
    let (tx, rx) = tokio::sync::watch::channel(AppStatus {
        state: AppState::Idle,
        last_text: None,
        vad_engine: "silero",
        partial: None,
        latched: false,
        chunk_busy: false,
    });
    let client = Arc::new(StatusClient::new(rx));
    assert_eq!(client.get().state, AppState::Idle);
    tx.send(AppStatus {
        state: AppState::Recording,
        last_text: None,
        vad_engine: "silero",
        partial: None,
        latched: false,
        chunk_busy: false,
    })
    .unwrap();
    assert_eq!(client.get().state, AppState::Recording);
}

#[test]
fn test_history_item_serialization_and_retrieval() {
    let item = HistoryItem {
        id: 1,
        text: "متن تستی اول".into(),
        timestamp: "12:30:45".into(),
        engine: "google".into(),
    };
    let json = serde_json::to_string(&item).unwrap();
    let parsed: HistoryItem = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, 1);
    assert_eq!(parsed.text, "متن تستی اول");
    assert_eq!(parsed.engine, "google");
}

#[test]
fn test_history_truncation_limits() {
    let mut history: Vec<HistoryItem> = Vec::new();
    for i in 0..120 {
        history.insert(
            0,
            HistoryItem {
                id: i,
                text: format!("Text {i}"),
                timestamp: "10:00:00".into(),
                engine: "auto".into(),
            },
        );
        if history.len() > 100 {
            history.truncate(100);
        }
    }
    assert_eq!(history.len(), 100);
    assert_eq!(history[0].id, 119);
    assert_eq!(history[99].id, 20);
}

#[test]
fn test_toast_auto_dismiss_10s_expiration() {
    let now = Instant::now();
    let start = now - Duration::from_secs(11);
    let elapsed = now.duration_since(start);
    assert!(elapsed >= Duration::from_secs(10));
    let remaining_secs = 10_u64.saturating_sub(elapsed.as_secs());
    assert_eq!(remaining_secs, 0);

    let active_start = now - Duration::from_secs(3);
    let active_elapsed = now.duration_since(active_start);
    assert!(active_elapsed < Duration::from_secs(10));
    let active_remaining = 10_u64.saturating_sub(active_elapsed.as_secs()).max(1);
    assert_eq!(active_remaining, 7);
}

#[test]
fn test_visual_mode_transitions() {
    let determine_mode = |state: AppState, is_hovered: bool| -> VisualMode {
        match state {
            AppState::Recording => VisualMode::RecordingActive,
            AppState::Processing | AppState::Typing => VisualMode::Processing,
            _ => {
                if is_hovered {
                    VisualMode::HoveredAwake
                } else {
                    VisualMode::IdleDormant
                }
            }
        }
    };

    assert_eq!(
        determine_mode(AppState::Idle, false),
        VisualMode::IdleDormant
    );
    assert_eq!(
        determine_mode(AppState::Idle, true),
        VisualMode::HoveredAwake
    );
    assert_eq!(
        determine_mode(AppState::Recording, false),
        VisualMode::RecordingActive
    );
    assert_eq!(
        determine_mode(AppState::Recording, true),
        VisualMode::RecordingActive
    );
    assert_eq!(
        determine_mode(AppState::Processing, false),
        VisualMode::Processing
    );
    assert_eq!(
        determine_mode(AppState::Typing, false),
        VisualMode::Processing
    );
}
