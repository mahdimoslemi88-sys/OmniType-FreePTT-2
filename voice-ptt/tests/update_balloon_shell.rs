//! Does the Windows shell actually accept the balloon we build?
//!
//! `tray_balloon`'s own tests cover every decision — when to announce, what to
//! say, how to cut the text — but they are all pure. None of them can catch the
//! failure this file is for: a `NOTIFYICONDATAW` that is *structurally* wrong in
//! a way the compiler accepts. A wrong `cbSize`, a missing `NIM_SETVERSION`, an
//! `HICON` that failed to load — every one of those produces a `Shell_NotifyIconW`
//! that returns `FALSE` and a notification that never appears, with no error
//! anywhere the user or a test would look.
//!
//! So this drives the real thing: spawn the notifier, show a real balloon, and
//! assert the shell did not refuse it. `show()` logs `warn!` on every refusal,
//! so a capturing subscriber turns that silent failure into a failing test.
//!
//! What this still does **not** prove: that a human saw the balloon. Windows
//! can accept a notification and never display it — Focus Assist, a full
//! Action Center, "Do not disturb" — and no API call reports that. See the note
//! in `tray_balloon` about the in-app banner remaining the backstop.
//!
//! Ignored by default because it creates a real icon in the real notification
//! area, which is a visible side effect on the developer's machine. Run with
//! `-- --include-ignored`.

#![cfg(windows)]

use std::sync::{Arc, Mutex};

use voice_ptt::gui::flags::DashboardFlags;
use voice_ptt::gui::tray_balloon::{self, UpdateBalloon};
use voice_ptt::updates::UpdateInfo;

/// The one captured log, shared by every test in this binary.
///
/// `set_global_default` succeeds at most once per process, so installing a
/// subscriber per test is not an option — and a per-test one would silently make
/// every test after the first one assert against an *empty* capture, which
/// passes vacuously. Hence one subscriber, installed once, guarded by `Once`.
static CAPTURE: std::sync::OnceLock<Arc<Mutex<Vec<String>>>> = std::sync::OnceLock::new();

/// Installs the capturing subscriber (once) and returns the shared log.
///
/// If another subscriber already owns the process, this returns `None` rather
/// than panicking — and the caller **skips**, because an empty capture would
/// make every "the shell did not refuse this" assertion true for the wrong
/// reason.
fn capture() -> Option<Arc<Mutex<Vec<String>>>> {
    static ONCE: std::sync::Once = std::sync::Once::new();
    let mut installed = false;
    ONCE.call_once(|| {
        let shared: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        if tracing::subscriber::set_global_default(Capture(shared.clone())).is_ok() {
            CAPTURE.set(shared).ok();
            installed = true;
        }
    });
    if installed {
        return CAPTURE.get().cloned();
    }
    CAPTURE.get().cloned()
}

/// Captures every `tracing` event emitted while the subscriber is installed.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<String>>>);

impl tracing::Subscriber for Capture {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut message = String::new();
        event.record(&mut MessageVisitor(&mut message));
        // A poisoned lock means another test panicked; dropping the event is
        // the right response, since the test that owns the panic already fails.
        if let Ok(mut captured) = self.0.lock() {
            captured.push(message);
        }
    }

    fn enter(&self, _span: &tracing::span::Id) {}
    fn exit(&self, _span: &tracing::span::Id) {}
}

struct MessageVisitor<'a>(&'a mut String);

impl tracing::field::Visit for MessageVisitor<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        let _ = write!(self.0, "{}={value:?} ", field.name());
    }
}

fn release(latest: &str) -> UpdateInfo {
    UpdateInfo {
        current_version: "0.3.0".into(),
        latest_version: latest.into(),
        release_name: format!("v{latest}"),
        release_url: "https://example.invalid/release".into(),
        release_notes: "notes".into(),
        installer_url: Some("https://example.invalid/setup.exe".into()),
        installer_name: Some("setup.exe".into()),
        installer_size_bytes: Some(10_992_053),
        published_at: "2026-10-05T15:23:29Z".into(),
    }
}

#[test]
#[ignore = "shows a real tray balloon; run with --include-ignored"]
fn the_shell_accepts_the_balloon_we_build() {
    let Some(capture) = capture() else {
        eprintln!("SKIP: another subscriber owns this process, so the capture would be empty");
        return;
    };

    let handle = tray_balloon::spawn_notifier(DashboardFlags::new())
        .expect("the notifier thread should start");

    // Positive control. On a healthy run `show()` logs *nothing*, so an empty
    // capture is exactly what success looks like — and an empty capture is also
    // what "the subscriber was never installed" looks like. Without this
    // sentinel every assertion below would pass on a broken harness.
    tracing::warn!("balloon-capture-sentinel");

    let balloon = tray_balloon::decide(true, "", &release("0.4.0"))
        .expect("a never-announced update should produce a balloon");
    // `show` waits for the shell's verdict, so this asserts the thing the
    // feature depends on — that a balloon really appeared — and not merely that
    // a channel was open. It is also exactly the value `announce` reads before
    // it writes `last_notified_version`.
    assert!(
        handle.show(balloon),
        "the shell refused the balloon, so nothing would appear and the release \
         would stay unannounced"
    );

    // One poll interval of slack for the notifier's own loop bookkeeping.
    std::thread::sleep(std::time::Duration::from_millis(1_500));

    let lines = capture.lock().expect("capture lock").clone();
    let joined = lines.join("\n");

    assert!(
        joined.contains("balloon-capture-sentinel"),
        "the capture never saw the sentinel, so every assertion below is vacuous"
    );
    assert!(
        !joined.contains("shell refused the update balloon's icon"),
        "the shell refused NIM_ADD — the NOTIFYICONDATAW is wrong:\n{joined}"
    );
    assert!(
        !joined.contains("shell refused the update balloon itself"),
        "the shell refused NIM_MODIFY with NIF_INFO — nothing would ever appear:\n{joined}"
    );
    assert!(
        !joined.contains("could not create the balloon window"),
        "the hidden window was never created, so no balloon is possible:\n{joined}"
    );
}

#[test]
#[ignore = "shows a real tray balloon; run with --include-ignored"]
fn the_balloon_leaves_the_notification_area_when_it_expires() {
    let Some(capture) = capture() else {
        eprintln!("SKIP: another subscriber owns this process, so the capture would be empty");
        return;
    };

    let handle = tray_balloon::spawn_notifier(DashboardFlags::new())
        .expect("the notifier thread should start");

    handle.show(
        tray_balloon::decide(true, "", &release("0.4.0")).expect("a balloon should be produced"),
    );
    std::thread::sleep(std::time::Duration::from_millis(1_500));

    // Dropping the last handle makes the thread exit through its own cleanup,
    // which is the path that removes the temporary icon and destroys the
    // window. If that path were skipped, a tray icon would outlive the process.
    drop(handle);
    std::thread::sleep(std::time::Duration::from_millis(1_500));

    let lines = capture.lock().expect("capture lock").clone();
    let joined = lines.join("\n");
    assert!(
        !joined.contains("could not destroy the balloon window"),
        "the notifier's window was leaked:\n{joined}"
    );
}

/// The one guarantee that needs no shell and no window: the text handed to
/// `szInfo` fits the array it is copied into, with the NUL accounted for.
#[test]
fn the_balloon_text_fits_the_arrays_it_is_copied_into() {
    let mut longest = UpdateBalloon {
        title: String::new(),
        body: String::new(),
    };
    for latest in [
        "0.4.0",
        &"v".repeat(500),
        "۰٫۴٫۰",
        &"🦀".repeat(300),
        &"ن".repeat(400),
    ] {
        let b = tray_balloon::decide(true, "", &release(latest)).expect("announced");
        assert!(b.title.encode_utf16().count() <= tray_balloon::TITLE_CAPACITY);
        assert!(b.body.encode_utf16().count() <= tray_balloon::INFO_CAPACITY);
        // The copy helper reserves the last unit for the NUL, so a full-length
        // string is still a valid, terminated one.
        assert!(b.title.encode_utf16().count() < 256);
        assert!(b.body.encode_utf16().count() < 256);
        if longest.title.len() < b.title.len() {
            longest = b;
        }
    }
    assert!(
        !longest.title.is_empty(),
        "the loop never produced a title"
    );
}

/// The whole chain, end to end, with no network.
///
/// The unit tests prove `decide` is right and the shell test proves the Windows
/// call is accepted. Neither proves they are **connected**: that the watcher
/// actually observes the state, shows the balloon, and records the version. A
/// wiring bug here — a clone of the wrong state, a settings handle that is not
/// the one the GUI reads — would pass every other test in this file.
///
/// Driven by writing `Available` into the shared state directly rather than by
/// publishing a release, so it needs no network and always finds something newer
/// than the test binary.
#[test]
#[ignore = "shows a real tray balloon; run with --include-ignored"]
fn an_available_update_is_announced_and_recorded_once() {
    let dir = std::env::temp_dir().join(format!("omnitype-balloon-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let config = dir.join("config.toml");
    std::fs::write(
        &config,
        "[updates]\ncheck_on_startup = true\nnotify_on_available = true\n\
         last_notified_version = \"\"\n",
    )
    .expect("write config");

    let settings = voice_ptt::config::settings::Settings::load_or_create(&config)
        .expect("config loads");
    let settings = std::sync::Arc::new(std::sync::RwLock::new(settings));

    let state = voice_ptt::updates::new_shared_state();
    let handle = tray_balloon::spawn_notifier(DashboardFlags::new())
        .expect("the notifier thread should start");
    tray_balloon::spawn_watcher(state.clone(), settings.clone(), config.clone(), Some(handle));

    // Nothing available yet: the watcher must stay silent.
    std::thread::sleep(std::time::Duration::from_millis(6_000));
    assert!(
        settings.read().unwrap().updates.last_notified_version.is_empty(),
        "recorded a release that was never available"
    );

    // Now make one available, as the checker would.
    let info = release("0.4.0");
    {
        let mut guard = state.write().expect("state lock");
        *guard = voice_ptt::updates::UpdateState::Available(Box::new(info));
    }

    // One watch interval plus slack for the notifier thread.
    std::thread::sleep(std::time::Duration::from_millis(8_000));

    let recorded = settings.read().unwrap().updates.last_notified_version.clone();
    assert_eq!(
        recorded, "0.4.0",
        "the balloon was shown but never recorded, so every start would repeat it"
    );

    // And it must have reached the file, not just memory — otherwise a restart
    // re-announces.
    let reloaded = voice_ptt::config::settings::Settings::load_or_create(&config)
        .expect("config reloads");
    assert_eq!(
        reloaded.updates.last_notified_version, "0.4.0",
        "recorded in memory but not on disk"
    );

    // The same release again must be ignored, and must not re-record.
    std::thread::sleep(std::time::Duration::from_millis(8_000));
    assert_eq!(
        settings.read().unwrap().updates.last_notified_version,
        "0.4.0",
        "a repeated Available for the same version changed the record"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The opt-out has to work through the real watcher too, not just `decide`.
#[test]
#[ignore = "needs the watcher; run with --include-ignored"]
fn switching_the_setting_off_silences_the_announcement() {
    let dir = std::env::temp_dir().join(format!("omnitype-balloon-off-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let config = dir.join("config.toml");
    std::fs::write(&config, "[updates]\nnotify_on_available = false\n").expect("write config");

    let settings = voice_ptt::config::settings::Settings::load_or_create(&config)
        .expect("config loads");
    assert!(
        !settings.updates.notify_on_available,
        "the opt-out did not survive the file"
    );
    let settings = std::sync::Arc::new(std::sync::RwLock::new(settings));

    let state = voice_ptt::updates::new_shared_state();
    let handle = tray_balloon::spawn_notifier(DashboardFlags::new())
        .expect("the notifier thread should start");
    tray_balloon::spawn_watcher(state.clone(), settings.clone(), config.clone(), Some(handle));

    {
        let mut guard = state.write().expect("state lock");
        *guard = voice_ptt::updates::UpdateState::Available(Box::new(release("0.4.0")));
    }
    std::thread::sleep(std::time::Duration::from_millis(8_000));

    assert!(
        settings.read().unwrap().updates.last_notified_version.is_empty(),
        "announced an update after the user turned notifications off"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
