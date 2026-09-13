//! Cloud ASR engine — any OpenAI-compatible `/audio/transcriptions` endpoint
//! (Groq, OpenRouter, OpenAI, or a self-hosted whisper server on the LAN).
//!
//! Privacy: **opt-in.** When `[cloud] enabled = true` and a key is present,
//! utterances are encoded to 16-bit PCM WAV in memory and uploaded. The
//! router treats this engine as *higher priority* than the local whisper and
//! falls back to it automatically on network failure (30 s cooldown).
//!
//! Quota: a persistent daily counter (`cloud_usage.json`) enforces the free
//! tier's ~300 requests/day budget. When exhausted — locally or by a server
//! daily-limit 429 — the engine stands down until **local midnight** instead
//! of burning cooldown retries all day; the router then routes to local
//! whisper for the rest of the day with zero cloud calls.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use reqwest::blocking::{multipart, Client};

use super::engine::{AsrEngine, AsrHealth, AudioUtterance};
use super::quota::DailyQuota;
use crate::config::settings::CloudConfig;

/// Cloud transcription engine (blocking client; the router offloads it to
/// the blocking pool, same as the local whisper engine).
pub struct CloudEngine {
    client: Client,
    config: CloudConfig,
    resolved_key: String,
    quota: Arc<DailyQuota>,
}

impl CloudEngine {
    /// Builds the engine. The `VOICE_PTT_CLOUD_KEY` env var takes precedence
    /// over `config.api_key` so the key never has to touch disk.
    /// `usage_path` persists the daily request counter across restarts.
    pub fn new(mut config: CloudConfig, usage_path: PathBuf) -> Self {
        if let Ok(key) = std::env::var("VOICE_PTT_CLOUD_KEY") {
            if !key.trim().is_empty() {
                config.api_key = key;
            }
        }
        let resolved_key = config.api_key.trim().to_string();

        let client = Client::builder()
            // Total request timeout: caps the spawn_blocking thread so a hung
            // server can never stall the router's fallback path.
            .timeout(Duration::from_secs(config.timeout_secs.max(5)))
            // Connect timeout: fails fast when the network is down.
            .connect_timeout(Duration::from_secs(5))
            .pool_max_idle_per_host(2)
            .tcp_keepalive(Duration::from_secs(60))
            .build()
            .unwrap_or_default(); // builder only fails on TLS backend init; default client still works

        let quota = Arc::new(DailyQuota::new(usage_path, config.daily_limit));

        Self {
            client,
            config,
            resolved_key,
            quota,
        }
    }

    /// Requests remaining in today's cloud budget (for the UI).
    pub fn remaining_today(&self) -> u32 {
        self.quota.remaining()
    }

    fn is_daily_limit_response(status: reqwest::StatusCode, body: &str) -> bool {
        if status != reqwest::StatusCode::TOO_MANY_REQUESTS {
            return false;
        }
        let lower = body.to_ascii_lowercase();
        // Per-minute limits ("Rate limit reached for RPM: 30") must NOT stand
        // the engine down for the day — check them first and bail out.
        if lower.contains("rpm") || lower.contains("per minute") {
            return false;
        }
        // Daily markers: "daily requests", "per day", tokens-per-day (TPD).
        lower.contains("daily") || lower.contains("per day") || lower.contains("tpd")
    }
}

impl AsrEngine for CloudEngine {
    fn name(&self) -> &'static str {
        "cloud"
    }

    fn health(&self) -> AsrHealth {
        if !self.config.enabled {
            AsrHealth::Failed {
                reason: "cloud engine disabled in config".into(),
            }
        } else if self.resolved_key.is_empty() {
            AsrHealth::Failed {
                reason: "no API key (set [cloud] api_key or VOICE_PTT_CLOUD_KEY)".into(),
            }
        } else if self.quota.exhausted() {
            AsrHealth::Cooldown {
                reason: format!(
                    "daily quota of {} exhausted; resets at local midnight",
                    self.quota.limit()
                ),
                retry_after_ms: self.quota.ms_until_reset(),
            }
        } else {
            AsrHealth::Ready
        }
    }

    fn transcribe(&self, audio: &AudioUtterance) -> Result<String> {
        // Pre-flight: never spend a network round-trip on a spent budget.
        if self.quota.exhausted() {
            return Err(anyhow!(
                "daily cloud quota exhausted (resets in {} min)",
                self.quota.ms_until_reset() / 60_000
            ));
        }
        if audio.samples.len() < 1_600 {
            return Err(anyhow!("audio too short (<100 ms)"));
        }

        let wav = f32_to_wav_le16(&audio.samples, audio.sample_rate.max(1));
        let file_part = multipart::Part::bytes(wav)
            .file_name("audio.wav")
            .mime_str("audio/wav")?;

        let mut form = multipart::Form::new()
            .part("file", file_part)
            .text("model", self.config.model.clone())
            .text("language", self.config.language.clone())
            .text("temperature", "0")
            .text("response_format", "json");

        if let Some(prompt) = self
            .config
            .initial_prompt
            .as_deref()
            .filter(|p| !p.trim().is_empty())
        {
            form = form.text("prompt", prompt.to_string());
        }

        let url = format!(
            "{}/audio/transcriptions",
            self.config.base_url.trim_end_matches('/')
        );

        let response = self
            .client
            .post(&url)
            .bearer_auth(&self.resolved_key)
            .multipart(form)
            .send()
            .map_err(|e| anyhow!("cloud request failed (network): {e}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            if Self::is_daily_limit_response(status, &body) {
                // Server disagrees with our counter → server wins: stand down
                // until local midnight (router skips this engine; the next
                // health() check reports Cooldown, not a retryable failure).
                self.quota.exhaust_today();
                tracing::warn!(
                    status = %status,
                    "cloud daily limit reported by server; standing down until midnight"
                );
                return Err(anyhow!(
                    "cloud daily quota exhausted (server-confirmed); resets at local midnight"
                ));
            }
            // Transient errors (per-minute 429, 5xx, auth, …): return Err so
            // the router's 30 s cooldown applies and local whisper takes over.
            return Err(anyhow!("cloud ASR HTTP {status}: {}", truncate(&body, 240)));
        }

        #[derive(serde::Deserialize)]
        struct TranscriptResp {
            text: String,
        }

        let parsed: TranscriptResp = response
            .json()
            .map_err(|e| anyhow!("bad JSON from cloud ASR: {e}"))?;

        let text = parsed.text.trim().to_string();
        if text.is_empty() {
            return Err(anyhow!("cloud ASR returned empty transcript"));
        }

        // Count only successful, accepted requests against the budget.
        self.quota.record_request();
        tracing::debug!(
            remaining = self.quota.remaining(),
            "cloud quota consumed"
        );
        Ok(text)
    }
}

/// Encodes mono f32 samples as a 16-bit PCM WAV file in memory
/// (44-byte canonical header, no temp file).
pub fn f32_to_wav_le16(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32_767.0) as i16;
        data.extend_from_slice(&v.to_le_bytes());
    }

    let data_len = data.len() as u32;
    let byte_rate = sample_rate * 2; // mono × 16-bit

    let mut out = Vec::with_capacity(44 + data.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(&data);
    out
}

/// Serializes tests that mutate process-global environment variables —
/// cargo runs tests in parallel and `remove_var`/`set_var` would otherwise
/// race each other.
#[cfg(test)]
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn truncate(s: &str, max: usize) -> String {
    let mut t: String = s.chars().take(max).collect();
    if s.chars().count() > max {
        t.push('…');
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_usage_path(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("voice-ptt-cloud-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("cloud_usage.json")
    }

    fn configured_engine(tag: &str, daily_limit: u32) -> CloudEngine {
        let _env = ENV_LOCK.lock().unwrap();
        std::env::remove_var("VOICE_PTT_CLOUD_KEY");
        let cfg = CloudConfig {
            enabled: true,
            api_key: "test-key".into(),
            daily_limit,
            ..Default::default()
        };
        CloudEngine::new(cfg, temp_usage_path(tag))
    }

    #[test]
    fn wav_header_is_valid() {
        let wav = f32_to_wav_le16(&[0.0; 16_000], 16_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
        let data_len = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]);
        assert_eq!(data_len, 32_000); // 1 s mono 16-bit
        assert_eq!(wav.len(), 44 + 32_000);
        let riff = u32::from_le_bytes([wav[4], wav[5], wav[6], wav[7]]);
        assert_eq!(riff, 36 + 32_000);
    }

    #[test]
    fn wav_clips_out_of_range_samples() {
        let wav = f32_to_wav_le16(&[2.0, -2.0], 16_000);
        let s0 = i16::from_le_bytes([wav[44], wav[45]]);
        let s1 = i16::from_le_bytes([wav[46], wav[47]]);
        assert_eq!(s0, 32_767);
        assert_eq!(s1, -32_767);
    }

    #[test]
    fn wav_preserves_round_values() {
        let wav = f32_to_wav_le16(&[0.5, -0.5], 16_000);
        let s0 = i16::from_le_bytes([wav[44], wav[45]]);
        let s1 = i16::from_le_bytes([wav[46], wav[47]]);
        assert_eq!(s0, 16_383);
        assert_eq!(s1, -16_383);
    }

    #[test]
    fn truncate_shortens_long_strings() {
        assert_eq!(truncate("hello world", 5), "hello…");
        assert_eq!(truncate("hi", 10), "hi");
    }

    #[test]
    fn health_requires_enabled_and_key() {
        let _env = ENV_LOCK.lock().unwrap();
        std::env::remove_var("VOICE_PTT_CLOUD_KEY");
        let cfg = CloudConfig::default(); // enabled = false
        let engine = CloudEngine::new(cfg, temp_usage_path("h1"));
        assert!(!AsrEngine::health(&engine).is_available(), "disabled by default");

        let cfg = CloudConfig {
            enabled: true,
            ..Default::default() // no key
        };
        let engine = CloudEngine::new(cfg, temp_usage_path("h2"));
        assert!(
            !AsrEngine::health(&engine).is_available(),
            "enabled but no key (and CI has no env var)"
        );
    }

    #[test]
    fn exhausted_quota_reports_cooldown_until_midnight() {
        let engine = configured_engine("q1", 1);
        engine.quota.record_request();
        assert!(engine.quota.exhausted());

        match AsrEngine::health(&engine) {
            AsrHealth::Cooldown { retry_after_ms, .. } => {
                assert!(retry_after_ms > 0);
                assert!(retry_after_ms <= 86_400 * 1_000);
            }
            other => panic!("expected Cooldown, got {other:?}"),
        }
        assert!(!AsrEngine::health(&engine).is_available());
    }

    #[test]
    fn quota_exhaustion_blocks_before_network() {
        let engine = configured_engine("q2", 0); // limit 0 → born exhausted
        let utt = AudioUtterance {
            samples: vec![0.0; 16_000],
            sample_rate: 16_000,
        };
        let err = AsrEngine::transcribe(&engine, &utt).unwrap_err();
        assert!(
            err.to_string().contains("quota exhausted"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn short_audio_rejected_before_network() {
        let engine = configured_engine("q3", 300);
        let utt = AudioUtterance {
            samples: vec![0.0; 100],
            sample_rate: 16_000,
        };
        let err = AsrEngine::transcribe(&engine, &utt).unwrap_err();
        assert!(err.to_string().contains("too short"));
    }

    #[test]
    fn daily_limit_detection_matches_groq_bodies() {
        let s = reqwest::StatusCode::TOO_MANY_REQUESTS;
        assert!(CloudEngine::is_daily_limit_response(
            s,
            r#"{"error":{"code":"rate_limit_exceeded","message":"Rate limit reached for daily requests: 300"}}"#
        ));
        assert!(CloudEngine::is_daily_limit_response(s, "daily quota exceeded"));
        assert!(!CloudEngine::is_daily_limit_response(
            s,
            r#"{"error":{"message":"Rate limit reached for RPM"}}"#
        ));
        assert!(!CloudEngine::is_daily_limit_response(
            reqwest::StatusCode::UNAUTHORIZED,
            "daily limit"
        ));
    }

    #[test]
    fn env_key_overrides_config_key() {
        let _env = ENV_LOCK.lock().unwrap();
        std::env::set_var("VOICE_PTT_CLOUD_KEY", "env-key-123");
        let cfg = CloudConfig {
            enabled: true,
            api_key: "disk-key".into(),
            ..Default::default()
        };
        let engine = CloudEngine::new(cfg, temp_usage_path("env"));
        std::env::remove_var("VOICE_PTT_CLOUD_KEY");
        assert_eq!(engine.resolved_key, "env-key-123");
    }
}
