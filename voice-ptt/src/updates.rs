//! Automated update detection and notification system.
//!
//! Checks the official GitHub repository releases API (`api.github.com/repos/.../releases/latest`)
//! asynchronously, parses the latest version, checks if it is strictly newer than the current
//! running version, and publishes state for the GUI overlay and system tray to notify the user.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// GitHub Releases API URL for OmniType-FreePTT-2.
pub const RELEASES_API_URL: &str =
    "https://api.github.com/repos/mahdimoslemi88-sys/OmniType-FreePTT-2/releases/latest";

/// Fallback browser URL when no specific release is resolved.
pub const RELEASES_WEB_URL: &str =
    "https://github.com/mahdimoslemi88-sys/OmniType-FreePTT-2/releases";

/// GitHub Release Asset JSON model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: Option<u64>,
    pub content_type: Option<String>,
}

/// GitHub Release JSON model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
    pub name: Option<String>,
    pub html_url: String,
    pub body: Option<String>,
    pub published_at: Option<String>,
    #[serde(default)]
    pub assets: Vec<GitHubAsset>,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub draft: bool,
}

/// Information about an available update.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub release_name: String,
    pub release_url: String,
    pub release_notes: String,
    pub installer_url: Option<String>,
    pub installer_name: Option<String>,
    pub installer_size_bytes: Option<u64>,
    pub published_at: String,
}

/// Current state of the update checker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateState {
    /// Not checked yet.
    Idle,
    /// Currently performing a network request.
    Checking,
    /// Up to date with the latest release.
    UpToDate { checked_at: String },
    /// A newer release is available.
    Available(Box<UpdateInfo>),
    /// An error occurred during checking (e.g. offline, rate limit, timeout).
    Error(String),
}

/// Thread-safe shared handle to the update state.
pub type SharedUpdateState = Arc<RwLock<UpdateState>>;

/// Creates a new idle shared update state.
pub fn new_shared_state() -> SharedUpdateState {
    Arc::new(RwLock::new(UpdateState::Idle))
}

/// Compares `current` and `remote` versions (e.g. `"0.1.0"` and `"v0.2.0"`).
///
/// Returns `true` if and only if `remote` is strictly newer than `current`.
pub fn is_newer_version(current: &str, remote: &str) -> bool {
    let curr_triplet = parse_semver_triplet(current);
    let remote_triplet = parse_semver_triplet(remote);

    match (curr_triplet, remote_triplet) {
        (Some((c_maj, c_min, c_pat)), Some((r_maj, r_min, r_pat))) => {
            (r_maj, r_min, r_pat) > (c_maj, c_min, c_pat)
        }
        _ => false,
    }
}

/// Parses standard SemVer triplet `(major, minor, patch)`, stripping leading `v`/`V`.
fn parse_semver_triplet(v: &str) -> Option<(u64, u64, u64)> {
    let trimmed = v.trim().trim_start_matches(['v', 'V']);
    let base = trimmed.split('-').next()?.split('+').next()?;
    let mut parts = base.split('.');

    let major: u64 = parts.next()?.parse().ok()?;
    let minor: u64 = parts.next().unwrap_or("0").parse().ok()?;
    let patch: u64 = parts.next().unwrap_or("0").parse().ok()?;

    Some((major, minor, patch))
}

/// Checks GitHub Releases API for the latest release.
///
/// Returns `Ok(Some(UpdateInfo))` if a strictly newer version is available.
pub async fn check_for_updates(current_version: &str) -> Result<Option<UpdateInfo>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent(format!("OmniType-FreePTT/{current_version}"))
        .build()
        .context("failed to build reqwest client for update check")?;

    let response = client
        .get(RELEASES_API_URL)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .context("network error checking for updates from GitHub")?;

    if !response.status().is_success() {
        anyhow::bail!("GitHub API returned HTTP status {}", response.status());
    }

    let release: GitHubRelease = response
        .json()
        .await
        .context("failed to parse GitHub release JSON")?;

    if release.draft {
        return Ok(None);
    }

    let remote_tag = release.tag_name.trim();
    let remote_clean = remote_tag.trim_start_matches(['v', 'V']);

    if !is_newer_version(current_version, remote_clean) {
        return Ok(None);
    }

    // Find the installer asset if present (e.g. .exe setup)
    let installer_asset = release.assets.iter().find(|a| {
        let name_lower = a.name.to_lowercase();
        name_lower.ends_with(".exe") || name_lower.contains("setup")
    });

    let installer_url = installer_asset.map(|a| a.browser_download_url.clone());
    let installer_name = installer_asset.map(|a| a.name.clone());
    let installer_size_bytes = installer_asset.and_then(|a| a.size);

    let release_name = release
        .name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| format!("OmniType FreePTT {remote_tag}"));

    let release_notes = release.body.unwrap_or_default();
    let published_at = release.published_at.unwrap_or_default();

    Ok(Some(UpdateInfo {
        current_version: current_version.to_string(),
        latest_version: remote_clean.to_string(),
        release_name,
        release_url: release.html_url,
        release_notes,
        installer_url,
        installer_name,
        installer_size_bytes,
        published_at,
    }))
}

/// Performs a check and updates `state` accordingly. Safe to call concurrently.
pub async fn perform_check(state: &SharedUpdateState, current_version: &str) {
    {
        let mut w = state.write().expect("update_state lock poisoned");
        *w = UpdateState::Checking;
    }

    let result = check_for_updates(current_version).await;
    let mut w = state.write().expect("update_state lock poisoned");
    match result {
        Ok(Some(info)) => {
            tracing::info!(
                latest = %info.latest_version,
                current = %info.current_version,
                "new OmniType update is available"
            );
            *w = UpdateState::Available(Box::new(info));
        }
        Ok(None) => {
            let checked_at = chrono_time_str();
            tracing::debug!("OmniType is up to date");
            *w = UpdateState::UpToDate { checked_at };
        }
        Err(e) => {
            let err_msg = format!("{e:#}");
            tracing::warn!(error = %err_msg, "failed to check for updates");
            *w = UpdateState::Error(err_msg);
        }
    }
}

/// Spawns a background task that executes an initial delayed check (5s)
/// and subsequent periodic checks according to user settings.
///
/// Must be spawned onto an existing runtime via `rt.spawn(...)`, because
/// `tokio::spawn` requires a runtime context and this function runs on the
/// plain main thread during startup.
pub fn spawn_background_checker(
    rt: &tokio::runtime::Runtime,
    state: SharedUpdateState,
    settings: Arc<RwLock<crate::config::settings::Settings>>,
    current_version: &'static str,
) {
    rt.spawn(async move {
        // Initial delay to never compete with startup, audio, or tray initialization
        tokio::time::sleep(Duration::from_secs(5)).await;

        let should_check = {
            let s = settings.read().expect("settings lock poisoned");
            s.updates.check_on_startup
        };

        if should_check {
            tracing::info!("starting background update check");
            perform_check(&state, current_version).await;
        }

        // Periodic loop
        loop {
            let interval_hours = {
                let s = settings.read().expect("settings lock poisoned");
                s.updates.auto_check_interval_hours.max(1)
            };
            tokio::time::sleep(Duration::from_secs(interval_hours * 3600)).await;

            let check_enabled = {
                let s = settings.read().expect("settings lock poisoned");
                s.updates.check_on_startup
            };

            if check_enabled {
                perform_check(&state, current_version).await;
            }
        }
    });
}

/// Opens a web URL in the user's default browser on Windows.
pub fn open_url_in_browser(url: &str) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open")
            .arg(url)
            .spawn();
    }
}

fn chrono_time_str() -> String {
    // Produce simple HH:MM timestamp without pulling heavy chrono formatting dependencies
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let hours = (now / 3600) % 24;
    let minutes = (now / 60) % 60;
    format!("{hours:02}:{minutes:02} UTC")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_comparison_newer() {
        assert!(is_newer_version("0.1.0", "0.2.0"));
        assert!(is_newer_version("0.1.0", "v0.2.0"));
        assert!(is_newer_version("0.1.0", "1.0.0"));
        assert!(is_newer_version("0.1.0", "0.1.1"));
        assert!(is_newer_version("0.1.0-beta", "0.1.1"));
        assert!(is_newer_version("v0.1.0", "v0.1.1"));
    }

    #[test]
    fn test_version_comparison_older_or_equal() {
        assert!(!is_newer_version("0.1.0", "0.1.0"));
        assert!(!is_newer_version("0.2.0", "0.1.0"));
        assert!(!is_newer_version("1.0.0", "0.9.9"));
        assert!(!is_newer_version("v0.1.0", "v0.1.0"));
        assert!(!is_newer_version("invalid", "0.1.0"));
        assert!(!is_newer_version("0.1.0", "invalid"));
    }

    #[test]
    fn test_parse_github_release_json() {
        let json = r#"{
            "tag_name": "v0.2.0",
            "name": "OmniType FreePTT v0.2.0",
            "html_url": "https://github.com/mahdimoslemi88-sys/OmniType-FreePTT-2/releases/tag/v0.2.0",
            "body": "Bug fixes and improvements",
            "published_at": "2026-09-22T14:00:00Z",
            "draft": false,
            "prerelease": false,
            "assets": [
                {
                    "name": "OmniType-FreePTT-0.2.0-setup.exe",
                    "browser_download_url": "https://github.com/mahdimoslemi88-sys/OmniType-FreePTT-2/releases/download/v0.2.0/OmniType-FreePTT-0.2.0-setup.exe",
                    "size": 21972000,
                    "content_type": "application/x-msdownload"
                }
            ]
        }"#;

        let release: GitHubRelease = serde_json::from_str(json).unwrap();
        assert_eq!(release.tag_name, "v0.2.0");
        assert_eq!(release.assets.len(), 1);
        assert_eq!(release.assets[0].name, "OmniType-FreePTT-0.2.0-setup.exe");

        let is_newer = is_newer_version("0.1.0", &release.tag_name);
        assert!(is_newer);
    }
}
