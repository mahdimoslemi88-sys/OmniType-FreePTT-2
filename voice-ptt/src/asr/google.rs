//! Google Free Speech Recognition engine (Chromium Web Speech v2 endpoint).
//!
//! Privacy & Requirements:
//! - Requires NO API key, registration, or credit card.
//! - Uses the public Chromium project speech API key embedded in open-source clients.
//! - Audio is encoded to FLAC in memory (pure Rust via `flacenc`) and sent over HTTPS.
//! - In case of network interruption or timeout, `AsrRouter` fails over to the local
//!   Whisper engine automatically.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use flacenc::bitsink::ByteSink;
use flacenc::component::BitRepr;
use flacenc::error::Verify;
use reqwest::blocking::Client;
use serde_json::Value;

use super::engine::{AsrEngine, AsrHealth, AudioUtterance};
use crate::config::settings::GoogleConfig;

/// Public Chromium speech API developer key (widely used by open-source clients).
pub const CHROMIUM_SPEECH_KEY: &str = "AIzaSyBOti4mM-6x9WDnZIjIeyEU21OpBXqWBgw";

/// Default endpoint URL for Chromium speech API v2.
pub const GOOGLE_SPEECH_ENDPOINT: &str = "https://www.google.com/speech-api/v2/recognize";

/// Google Free Speech transcription engine.
pub struct GoogleEngine {
    client: Client,
    config: GoogleConfig,
}

impl GoogleEngine {
    /// Creates a new GoogleEngine instance with the given configuration.
    pub fn new(config: GoogleConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs.max(3)))
            .connect_timeout(Duration::from_secs(4))
            .pool_max_idle_per_host(2)
            .tcp_keepalive(Duration::from_secs(60))
            .build()
            .unwrap_or_default();

        Self { client, config }
    }

    /// Maps language codes to standard Google Speech language tags.
    pub fn resolve_language(&self) -> &str {
        let lang = self.config.language.trim();
        if lang.is_empty() || lang == "fa" || lang == "fa-IR" || lang == "auto" {
            "fa-IR"
        } else if lang == "en" || lang == "en-US" {
            "en-US"
        } else {
            lang
        }
    }
}

impl AsrEngine for GoogleEngine {
    fn name(&self) -> &'static str {
        "google"
    }

    fn id(&self) -> String {
        "google".to_string()
    }

    fn display_name(&self) -> String {
        "Google Free Speech".to_string()
    }

    fn kind(&self) -> &'static str {
        "Cloud (Free)"
    }

    fn health(&self) -> AsrHealth {
        if !self.config.enabled {
            AsrHealth::Failed {
                reason: "google engine disabled in config".into(),
            }
        } else {
            AsrHealth::Ready
        }
    }

    fn transcribe(&self, audio: &AudioUtterance) -> Result<String> {
        if audio.samples.is_empty() {
            return Err(anyhow!("cannot transcribe empty audio"));
        }
        let sample_rate = if audio.sample_rate == 0 { 16_000 } else { audio.sample_rate };

        // 1. Encode samples to FLAC in memory
        let flac_bytes = encode_samples_to_flac(&audio.samples, sample_rate)
            .context("failed to encode audio to FLAC")?;

        // 2. Build request URL with parameters
        let lang = self.resolve_language();
        let url = format!(
            "{}?client=chromium&lang={}&key={}&pFilter=0",
            GOOGLE_SPEECH_ENDPOINT, lang, CHROMIUM_SPEECH_KEY
        );

        // 3. Send HTTP POST request
        let res = self
            .client
            .post(&url)
            .header("Content-Type", format!("audio/x-flac; rate={sample_rate}"))
            .body(flac_bytes)
            .send()
            .context("Google Speech HTTP request failed")?;

        let status = res.status();
        let body = res.text().context("failed to read Google Speech response body")?;

        if !status.is_success() {
            return Err(anyhow!(
                "Google Speech API returned HTTP {}: {}",
                status,
                body.chars().take(200).collect::<String>()
            ));
        }

        // 4. Parse streaming chunked JSON response
        let transcript = parse_google_response(&body)
            .context("failed to parse Google Speech response")?;

        if transcript.is_empty() {
            return Err(anyhow!("Google Speech returned empty transcript (silence or unrecognized)"));
        }

        Ok(transcript)
    }
}

/// Encodes mono f32 audio samples into standard 16-bit FLAC format in memory.
pub fn encode_samples_to_flac(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>> {
    if samples.is_empty() {
        return Err(anyhow!("sample buffer is empty"));
    }

    let channels = 1;
    let bits_per_sample = 16;

    // Convert f32 [-1.0, 1.0] to signed 16-bit i32 integers
    let samples_i32: Vec<i32> = samples
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * 32_767.0) as i32)
        .collect();

    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|e| anyhow!("invalid flac encoder config: {e:?}"))?;

    let source = flacenc::source::MemSource::from_samples(
        &samples_i32,
        channels,
        bits_per_sample,
        sample_rate as usize,
    );

    let flac_stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
        .map_err(|e| anyhow!("flac encoding failed: {e:?}"))?;

    let mut sink = ByteSink::new();
    flac_stream
        .write(&mut sink)
        .map_err(|e| anyhow!("flac bitstream write failed: {e:?}"))?;

    Ok(sink.as_slice().to_vec())
}

/// Parses the multi-line streaming JSON response returned by Google's Chromium speech API.
pub fn parse_google_response(response_text: &str) -> Result<String> {
    // Response format is line-delimited JSON chunks:
    // {"result":[]}
    // {"result":[{"alternative":[{"transcript":"سلام خوبی","confidence":0.95}],"final":true}],"result_index":0}
    for line in response_text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Ok(val) = serde_json::from_str::<Value>(line) {
            if let Some(results) = val.get("result").and_then(|r| r.as_array()) {
                for res in results {
                    if let Some(alternatives) = res.get("alternative").and_then(|a| a.as_array()) {
                        for alt in alternatives {
                            if let Some(transcript) = alt.get("transcript").and_then(|t| t.as_str()) {
                                let trimmed = transcript.trim();
                                if !trimmed.is_empty() {
                                    return Ok(trimmed.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_samples_to_flac() {
        let sample_rate = 16_000;
        let samples = vec![0.0f32; 16_000]; // 1 second of silence
        let flac_bytes = encode_samples_to_flac(&samples, sample_rate).expect("FLAC encoding failed");

        assert!(!flac_bytes.is_empty());
        // FLAC header starts with magic bytes "fLaC"
        assert_eq!(&flac_bytes[0..4], b"fLaC");
    }

    #[test]
    fn test_parse_google_response_success() {
        let response = r#"{"result":[]}
{"result":[{"alternative":[{"transcript":"تست تشخیص صدا","confidence":0.98}],"final":true}],"result_index":0}
"#;
        let transcript = parse_google_response(response).expect("Parsing failed");
        assert_eq!(transcript, "تست تشخیص صدا");
    }

    #[test]
    fn test_parse_google_response_empty() {
        let response = r#"{"result":[]}"#;
        let transcript = parse_google_response(response).expect("Parsing failed");
        assert_eq!(transcript, "");
    }

    #[test]
    fn test_google_engine_health() {
        let mut cfg = GoogleConfig::default();
        let engine = GoogleEngine::new(cfg.clone());
        assert_eq!(engine.health(), AsrHealth::Ready);

        cfg.enabled = false;
        let engine_disabled = GoogleEngine::new(cfg);
        assert!(!engine_disabled.health().is_available());
    }
}
