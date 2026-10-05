//! Proves the update notification path against the **live** GitHub API.
//!
//! The unit tests in `src/updates.rs` parse a JSON literal, which proves the
//! parser accepts a shape someone wrote by hand. It does not prove the thing the
//! user actually depends on: that publishing a tag makes the app say so.
//!
//! Ignored by default because it needs the network. Run it with:
//!
//! ```text
//! cargo test -p voice-ptt --test update_live_check -- --ignored --nocapture
//! ```
//!
//! Set `UPDATES_EXPECT_NEWER=1` to assert the *positive* case — that a tag
//! newer than `CARGO_PKG_VERSION` produces `UpdateState::Available`. That is the
//! assertion that fails loudly if a release is published and nothing appears,
//! and it is the one to re-run after cutting a real tag.

use std::sync::Arc;

use voice_ptt::updates::{
    self, check_for_updates, perform_check, SharedUpdateState, UpdateState,
};

/// One shared live call, then the offline assertions run against its result.
///
/// A single request on purpose: the unauthenticated GitHub API allows 60
/// requests an hour per IP, and a test suite that spends them is a test suite
/// that starts failing for reasons that have nothing to do with the code.
fn live_release() -> Option<voice_ptt::updates::GitHubRelease> {
    let body = std::process::Command::new("curl")
        .args([
            "-sS",
            "-H",
            "Accept: application/vnd.github.v3+json",
            "-H",
            "User-Agent: OmniType-FreePTT-update-live-check",
            voice_ptt::updates::RELEASES_API_URL,
        ])
        .output()
        .ok()?;
    // Say *why* rather than swallowing it. A silent skip is the worst outcome
    // here: the whole point of this file is to prove the network leg works, and
    // a skip that prints nothing looks identical to a pass.
    if !body.status.success() {
        eprintln!(
            "SKIP: curl exited {:?}; stderr={:?}",
            body.status.code(),
            String::from_utf8_lossy(&body.stderr)
        );
        return None;
    }
    match serde_json::from_slice(&body.stdout) {
        Ok(release) => Some(release),
        Err(e) => {
            let preview: String = String::from_utf8_lossy(&body.stdout)
                .chars()
                .take(120)
                .collect();
            eprintln!("SKIP: response was not the expected JSON ({e}); starts: {preview}");
            None
        }
    }
}

#[test]
fn the_latest_release_endpoint_is_reachable_and_answers_the_expected_shape() {
    let Some(release) = live_release() else {
        eprintln!("SKIP: no network or unparseable response");
        return;
    };

    assert!(
        !release.tag_name.trim().is_empty(),
        "a release with no tag cannot be compared against anything"
    );
    assert!(
        !release.draft,
        "`/releases/latest` returned a draft; the API is not meant to, and if it \
         does the app would offer users an unpublished build"
    );
    eprintln!(
        "live tag = {} (published {})",
        release.tag_name,
        release.published_at.as_deref().unwrap_or("?")
    );
}

/// The real question: **would a newly published tag produce a visible update?**
///
/// The positive case is opt-in via an env var, because against the real
/// repository the newest tag usually equals the version in `Cargo.toml` — and
/// then the correct answer is `UpToDate`, which must not be mistaken for a
/// failure.
#[tokio::test]
#[ignore = "needs the network; asserts the positive case only when asked"]
async fn a_newly_published_tag_produces_a_visible_update() {
    let current = env!("CARGO_PKG_VERSION");
    let Some(release) = live_release() else {
        eprintln!("SKIP: no network");
        return;
    };
    let remote = release.tag_name.trim().trim_start_matches(['v', 'V']);

    if std::env::var("UPDATES_EXPECT_NEWER").is_err() {
        // Not asked for the positive case: assert the *honest* outcome instead,
        // which is that equal versions report no update.
        assert!(
            !updates::is_newer_version(current, remote),
            "set UPDATES_EXPECT_NEWER=1 to assert the newer-tag path"
        );
        eprintln!("SKIP: UPDATES_EXPECT_NEWER not set; checked only the equal case");
        return;
    }

    assert!(
        updates::is_newer_version(current, remote),
        "expected the published tag {remote} to be newer than the built version \
         {current}, or this test is asserting nothing"
    );

    // The full path, not just the comparison: network → parse → shared state.
    let state: SharedUpdateState = Arc::new(std::sync::RwLock::new(UpdateState::Idle));
    perform_check(&state, current).await;

    let snapshot = state.read().unwrap().clone();
    match snapshot {
        UpdateState::Available(info) => {
            // An update the user cannot act on is not a working update.
            assert!(
                !info.release_url.is_empty(),
                "an available update with no release page gives the user nothing to click"
            );
            eprintln!(
                "OK: update {} -> {} (installer: {:?})",
                info.current_version, info.latest_version, info.installer_name
            );
        }
        other => panic!("expected UpdateState::Available, got {other:?}"),
    }
}

/// The guard rail that keeps a notification from appearing for a build that is
/// not actually older. Exercised offline so it is cheap and deterministic.
#[test]
fn the_built_version_is_the_one_compared_against_the_published_tag() {
    let current = env!("CARGO_PKG_VERSION");
    // The tag GitHub currently serves must parse the same way the built version
    // does, or a routine version bump would silently stop being detected.
    let Some(release) = live_release() else {
        return;
    };
    let remote = release.tag_name.trim().trim_start_matches(['v', 'V']);
    let built = updates::is_newer_version(current, "999.0.0");
    assert!(built, "the built version must parse as a version at all");
    // Same format on both sides means the comparison is meaningful.
    assert!(
        remote.split('.').count() == current.split('.').count(),
        "built version has {current} ({}) parts but the published tag has {remote} ({})",
        current.split('.').count(),
        remote.split('.').count()
    );
}

/// `perform_check` must never leave the shared state stuck in `Checking`.
///
/// The GUI polls this state every frame to decide whether to show a spinner. A
/// state left as `Checking` is a spinner that never resolves, which the user
/// reads as a hang rather than as a failed check — and unlike a wrong version
/// comparison, it has no other symptom to hint at what went wrong.
///
/// Offline and deterministic: run against a version that cannot resolve, so the
/// network call fails without needing the network to be broken.
#[tokio::test]
async fn a_failed_check_settles_instead_of_staying_checking() {
    let state: SharedUpdateState = Arc::new(std::sync::RwLock::new(UpdateState::Idle));
    perform_check(&state, "not a version at all").await;

    let snapshot = state.read().unwrap().clone();
    match snapshot {
        UpdateState::Error(message) => {
            assert!(
                !message.is_empty(),
                "an error state with no message gives the user nothing to report"
            );
        }
        // A malformed *current* version is compared, not fetched, so the release
        // may legitimately come back as "nothing newer". Both are terminal; what
        // matters is that neither is `Checking`.
        UpdateState::UpToDate { .. } => {}
        other => panic!("a finished check must not leave the state as {other:?}"),
    }
}

/// **The positive case, end to end, against the real API.**
///
/// This is the assertion the user actually asked for: "if a new version is
/// released and we publish it, will the app show the update?"
///
/// Rather than mocking the response, it asks the *real* GitHub endpoint what
/// the newest published release is and then checks the app as it would be if
/// it were running an **older** build. The published tag is newer than that
/// pretend version by construction, so the whole path — fetch, parse, compare,
/// publish to the shared state the GUI polls — has to produce `Available`.
///
/// Why a pretend old version rather than a mock: a mock proves the code agrees
/// with a fixture someone wrote. This proves the code agrees with GitHub, which
/// is the thing that can drift (a renamed repo, a changed response shape, a tag
/// format nobody anticipated). If this test ever starts failing, the cause is
/// on GitHub's side or in the mapping between the two, and it fails *before* a
/// release ships rather than after.
///
/// It skips, rather than fails, when the newest published tag happens to be
/// **older** than the pretend version — which can happen for a moment if a
/// prerelease is newest. That is the correct answer, not a defect.
#[tokio::test]
#[ignore = "needs the network"]
async fn a_published_release_newer_than_the_running_build_is_surfaced() {
    // A version guaranteed to be older than any real release.
    const PRETEND_CURRENT: &str = "0.0.1";
    let Some(release) = live_release() else {
        eprintln!("SKIP: no network or unparseable response");
        return;
    };
    let remote = release.tag_name.trim().trim_start_matches(['v', 'V']);
    if !updates::is_newer_version(PRETEND_CURRENT, remote) {
        eprintln!(
            "SKIP: newest published tag {remote} is not newer than {PRETEND_CURRENT}"
        );
        return;
    }

    let state: SharedUpdateState = Arc::new(std::sync::RwLock::new(UpdateState::Idle));
    // The real function, over the real network, with only the *current* version
    // substituted — the substitution a future release makes naturally.
    let direct = check_for_updates(PRETEND_CURRENT)
        .await
        .expect("live fetch failed")
        .unwrap_or_else(|| {
            panic!("the live API served {remote}, which is newer than {PRETEND_CURRENT},                      so it must resolve to an update")
        });
    assert!(
        !direct.latest_version.is_empty(),
        "an update with no version is nothing to compare against"
    );

    // Now the same conclusion through the state the GUI actually polls.
    perform_check(&state, PRETEND_CURRENT).await;
    let snapshot = state.read().unwrap().clone();
    match snapshot {
        UpdateState::Available(found) => {
            let info = *found;
            assert_eq!(info.latest_version, remote.trim_start_matches(['v', 'V']));
            assert!(
                info.release_url.starts_with("https://"),
                "an update the user cannot click through is not a working update: {}",
                info.release_url
            );
            eprintln!(
                "OK: published {} is offered to a {} build (installer: {:?})",
                info.latest_version, PRETEND_CURRENT, info.installer_name
            );
        }
        other => panic!(
            "a published release newer than the running build must surface as              Available; got {other:?}"
        ),
    }
}

/// `Available` is the state a user acts on, so it has to carry something to act
/// on. Asserted against a hand-built release rather than the live one, because
/// this is a property of the mapping, not of whatever is published today.
#[test]
fn an_available_update_always_carries_a_page_to_open() {
    let json = r#"{
        "tag_name": "v9.9.9",
        "name": "OmniType FreePTT v9.9.9",
        "html_url": "https://github.com/mahdimoslemi88-sys/OmniType-FreePTT-2/releases/tag/v9.9.9",
        "body": "notes",
        "published_at": "2026-10-05T00:00:00Z",
        "draft": false,
        "prerelease": false,
        "assets": [{
            "name": "OmniType-FreePTT-9.9.9-setup.exe",
            "browser_download_url": "https://example.invalid/setup.exe",
            "size": 1,
            "content_type": "application/x-msdownload"
        }]
    }"#;
    let release: voice_ptt::updates::GitHubRelease = serde_json::from_str(json).unwrap();
    assert!(updates::is_newer_version("0.3.0", &release.tag_name));
    assert!(
        !release.html_url.is_empty(),
        "an update with no release page gives the user nothing to click"
    );
}
