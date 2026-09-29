//! Application settings: typed config with TOML persistence.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Custom cloud ASR provider (OpenAI-compatible speech-to-text API).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomProvider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    pub model: String,
    #[serde(default = "default_provider_language")]
    pub language: String,
    #[serde(default = "default_provider_timeout")]
    pub timeout_secs: u64,
}

fn default_provider_language() -> String {
    "fa".into()
}

fn default_provider_timeout() -> u64 {
    15
}

fn default_active_engine() -> String {
    "auto".into()
}

/// Root configuration (mirrors the spec's `config.toml`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub audio: AudioSettings,
    pub asr: AsrSettings,
    pub vad: VadSettings,
    pub streaming: StreamingSettings,
    pub hotkey: HotkeySettings,
    pub gui: GuiSettings,
    pub cloud: CloudConfig,
    pub google: GoogleConfig,
    #[serde(default)]
    pub antigravity: AntigravityConfig,
    #[serde(default = "default_active_engine")]
    pub active_engine: String,
    #[serde(default)]
    pub custom_providers: Vec<CustomProvider>,
    #[serde(default)]
    pub updates: UpdateSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            audio: AudioSettings::default(),
            asr: AsrSettings::default(),
            vad: VadSettings::default(),
            streaming: StreamingSettings::default(),
            hotkey: HotkeySettings::default(),
            gui: GuiSettings::default(),
            cloud: CloudConfig::default(),
            google: GoogleConfig::default(),
            antigravity: AntigravityConfig::default(),
            active_engine: "auto".into(),
            custom_providers: Vec::new(),
            updates: UpdateSettings::default(),
        }
    }
}

/// Auto-update checker configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct UpdateSettings {
    pub check_on_startup: bool,
    pub auto_check_interval_hours: u64,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            check_on_startup: true,
            auto_check_interval_hours: 6,
        }
    }
}

/// Google Free Speech Recognition (Chromium v2 endpoint).
/// Does not require an API key or account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GoogleConfig {
    /// Whether to use Google Free Speech Recognition. Defaults to true.
    pub enabled: bool,
    /// Language code (e.g. "fa-IR" or "en-US").
    pub language: String,
    /// Request timeout in seconds.
    pub timeout_secs: u64,
}

impl Default for GoogleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            language: "fa-IR".into(),
            timeout_secs: 10,
        }
    }
}

/// Chunked ("streaming") dictation.
///
/// Historically the session ended at the ring-buffer safety valve (~30 s of
/// audio) and everything before that was transcribed at once. With streaming on,
/// the session is instead flushed as **ordered chunks while the microphone keeps
/// running**: the audio is cut at a pause (or at `chunk_seconds`, whichever comes
/// first), transcribed, injected, and recording continues — so a dictation can
/// last as long as the user wants, and text appears while they are still talking.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct StreamingSettings {
    pub enabled: bool,
    /// Hard cap for one chunk, in seconds: flushed even mid-phrase at this
    /// length. Kept well below the free Google endpoint's own limit.
    pub chunk_seconds: u64,
    /// Never flush a chunk shorter than this (avoids tiny fragments).
    pub min_chunk_seconds: u64,
    /// Audio repeated at the start of the next chunk so a seam cannot clip a word.
    pub overlap_ms: u64,
    /// Trailing silence that marks a clean cut point in the `silence` strategy.
    ///
    /// phase 3.2: 600 ms cut a chunk at every short breath, which felt like the
    /// app stopping and restarting mid-dictation. 1200 ms is a real pause.
    pub silence_ms: u64,
    /// `silence` (prefer cutting at pauses) or `fixed` (pure time slicing).
    pub strategy: String,
    /// Absolute safety cap for one held session, in seconds.
    pub max_utterance_seconds: u64,
    /// Stitch chunk seams: drop head words the previous chunk already typed
    /// (the deliberate audio overlap makes the recogniser repeat them) and, when
    /// a cut truncated a word, delete that fragment before typing the full word.
    pub seam_merge: bool,
    /// Delete the truncated word fragment at the seam with backspaces. Off ⇒
    /// only duplicated words are removed and the fragment stays in the text.
    pub seam_backspace: bool,
    /// How many head words may be treated as the repeated overlap (per chunk).
    pub seam_max_words: usize,
    /// Treat near-identical words (diacritics, ZWNJ, one recognition slip) as
    /// the same word when matching a seam.
    pub seam_fuzzy: bool,
}

impl Default for StreamingSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            // 20 s sits comfortably inside what the free Chromium speech endpoint
            // handles reliably, and short enough that text shows up as you speak.
            chunk_seconds: 20,
            min_chunk_seconds: 4,
            overlap_ms: 300,
            silence_ms: 1200,
            strategy: "silence".into(),
            max_utterance_seconds: 600,
            // Seam repair is on: it is the difference between a long dictation
            // reading as one text and reading as a stutter of repeated words.
            seam_merge: true,
            // Backspacing deletes ~1 word we typed ourselves at the seam; if a
            // fragment ever gets eaten wrongly, turn this off first.
            seam_backspace: true,
            // 300 ms of overlap is a handful of words at any speaking rate.
            seam_max_words: 6,
            seam_fuzzy: true,
        }
    }
}

/// Antigravity live-dictation bridge settings.
///
/// Talks to the language server of a *locally running* Antigravity app
/// (`language_server.exe`) and borrows its cloud speech-to-text stream.
/// No API key is involved: the token the renderer uses is read from the
/// process command line (`--csrf_token`), and audio stays on loopback between
/// this app and the local server. Leave `enabled = true` and select
/// «Antigravity Live» in the engine list to use it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AntigravityConfig {
    /// Whether the engine is registered at all.
    pub enabled: bool,
    /// Conversation id sent as `cascadeId`. Empty is accepted by the server.
    pub cascade_id: String,
    /// Seconds to wait for the stream to open (`ready`).
    pub ready_timeout_secs: u64,
    /// Seconds to wait for the final transcript after the last chunk.
    pub finalize_timeout_secs: u64,
}

impl Default for AntigravityConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cascade_id: String::new(),
            // Measured on this machine: the language server takes ~13 s before
            // it even sends response headers for a session (it does the same in
            // its own UI — the app shows a 12.6 s stall before its first chunk),
            // and answers within ~0.2 s once it does. Budget generously above
            // that, otherwise every dictation fails on a healthy server.
            ready_timeout_secs: 30,
            finalize_timeout_secs: 12,
        }
    }
}

/// Optional cloud ASR engine (Groq or any OpenAI-compatible endpoint).
/// Disabled by default: opting in means audio leaves the machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CloudConfig {
    pub enabled: bool,
    pub provider: String,
    pub base_url: String,
    /// Prefer the `VOICE_PTT_CLOUD_KEY` env var over a key on disk.
    pub api_key: String,
    pub model: String,
    pub language: String,
    /// Domain hint prepended to guide recognition of technical terms.
    pub initial_prompt: Option<String>,
    pub timeout_secs: u64,
    /// Requests per local calendar day before the engine stands down until
    /// midnight (Groq free tier ≈ 300). Server 429s also trigger the stand-down.
    pub daily_limit: u32,
}

impl Default for CloudConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: "groq".into(),
            base_url: "https://api.groq.com/openai/v1".into(),
            api_key: String::new(),
            model: "whisper-large-v3-turbo".into(),
            language: "fa".into(),
            initial_prompt: Some(
                "متن فارسی با اصطلاحات فنی مانند Python، JavaScript، Docker، Kubernetes، API، Database."
                    .into(),
            ),
            timeout_secs: 30,
            daily_limit: 300,
        }
    }
}

impl CloudConfig {
    /// Whether the engine can actually run (enabled + some key available,
    /// counting the `VOICE_PTT_CLOUD_KEY` env var).
    pub fn is_configured(&self) -> bool {
        let env_key = std::env::var("VOICE_PTT_CLOUD_KEY")
            .map(|k| !k.trim().is_empty())
            .unwrap_or(false);
        self.enabled && (!self.api_key.trim().is_empty() || env_key)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    pub sample_rate: u32,
    pub channels: u16,
    pub buffer_frames: u32,
    /// Ring-buffer length in seconds. phase 3: raised from 30 s to 60 s so a slow
    /// chunk transcription (the consumer is not draining meanwhile) cannot make
    /// the buffer overwrite the oldest audio of a long session.
    pub ring_seconds: u32,
    /// "default" or a specific device name.
    pub device: String,
    /// Software audio gain in dB (0.0 = unity gain, 6.0 = 2x, etc.).
    pub gain_db: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AsrSettings {
    /// Model name: tiny | base | small | medium | large-v3 | large-v3-turbo,
    /// or "auto" for hardware-based selection.
    pub model: String,
    pub language: String,
    pub beam_size: i32,
    pub n_threads: i32,
    pub initial_prompt: Option<String>,
    /// Allow the `auto` engine chain to fall through to the **local** whisper
    /// model. Off by default: loading large-v3-turbo maps ~1.6 GB into the
    /// process, so it must be a deliberate choice (pick «Local Whisper» in the
    /// dashboard) instead of a silent fallback when a cloud engine hiccups.
    pub auto_local_fallback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VadSettings {
    pub threshold: f32,
    pub silence_timeout_ms: u64,
    pub chunk_size: usize,
    pub min_speech_ms: u64,
    /// Whether silence timeout stops recording even when the hotkey is held down.
    /// Default false: holding hotkey continues recording until release or 30s cap.
    pub cutoff_on_hold: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeySettings {
    pub record: String,
    pub toggle_overlay: String,
    pub quit: String,
    /// Hands-free "latch": two quick presses of the record key keep the
    /// recording running after the key is released; the next press ends it.
    /// Hold-to-talk is unchanged (a hold ≥ `tap_max_ms` finalises on release).
    pub double_tap_latch: bool,
    /// A press shorter than this counts as a "tap" (milliseconds).
    pub tap_max_ms: u64,
    /// Maximum gap allowed between the two taps of a double-tap (milliseconds).
    pub double_tap_window_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GuiSettings {
    pub show_overlay: bool,
    pub theme: String,
    /// Show the floating transcript bubble. It is anchored bottom-center above
    /// the taskbar and is completely independent of the orb; set it to `false`
    /// to keep the desktop clean (no transcript window at all).
    pub show_transcript_bubble: bool,
    /// Orb center, physical screen pixels. None = first run (center of primary screen).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orb_position_x: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orb_position_y: Option<i32>,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            sample_rate: 16_000,
            channels: 1,
            buffer_frames: 256,
            ring_seconds: 60,
            device: "default".into(),
            gain_db: 0.0,
        }
    }
}

impl Default for AsrSettings {
    fn default() -> Self {
        Self {
            model: "auto".into(),
            language: "fa".into(),
            beam_size: 5,
            n_threads: 8,
            initial_prompt: None,
            auto_local_fallback: false,
        }
    }
}

impl Default for VadSettings {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            silence_timeout_ms: 1_500,
            chunk_size: 512,
            min_speech_ms: 150,
            cutoff_on_hold: false,
        }
    }
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self {
            record: "CapsLock".into(),
            toggle_overlay: "Ctrl+Alt+S".into(),
            quit: "Ctrl+Alt+Q".into(),
            double_tap_latch: true,
            tap_max_ms: 350,
            double_tap_window_ms: 600,
        }
    }
}

impl Default for GuiSettings {
    fn default() -> Self {
        Self {
            show_overlay: true,
            theme: "dark".into(),
            show_transcript_bubble: true,
            orb_position_x: None,
            orb_position_y: None,
        }
    }
}

impl Settings {
    /// Loads settings from `path`, creating a default file on first run.
    pub fn load_or_create(path: &Path) -> Result<Self> {
        if path.exists() {
            let content = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", path.display()))?;
            let settings: Settings = toml::from_str(&content)
                .map_err(|e| anyhow::anyhow!("invalid {}: {e}", path.display()))?;
            Ok(settings)
        } else {
            let settings = Settings::default();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let serialized = toml::to_string_pretty(&settings)?;
            std::fs::write(path, serialized)?;
            Ok(settings)
        }
    }

    /// Saves current settings back to disk.
    pub fn save(&self, path: &Path) -> Result<()> {
        let serialized = toml::to_string_pretty(self)?;
        std::fs::write(path, serialized)?;
        Ok(())
    }

    /// Resolves the actual whisper model to use, applying the "auto" policy:
    /// GPU → large-v3-turbo, strong CPU → small, otherwise base.
    pub fn resolve_model_name(&self, gpu: Option<&str>, cpu_cores: usize) -> String {
        if self.asr.model != "auto" {
            return self.asr.model.clone();
        }
        if gpu.is_some() {
            "large-v3-turbo".into()
        } else if cpu_cores >= 8 {
            "small".into()
        } else {
            "base".into()
        }
    }

    /// Directory that holds application data (models, config, dictionaries).
    pub fn data_dir(&self) -> PathBuf {
        dirs_or_cwd()
    }

    /// Adds or updates a custom provider in settings.
    pub fn add_or_update_provider(&mut self, provider: CustomProvider) {
        if let Some(existing) = self.custom_providers.iter_mut().find(|p| p.id == provider.id) {
            *existing = provider;
        } else {
            self.custom_providers.push(provider);
        }
    }

    /// Removes a custom provider by id.
    pub fn remove_provider(&mut self, id: &str) -> bool {
        if let Some(pos) = self.custom_providers.iter().position(|p| p.id == id) {
            self.custom_providers.remove(pos);
            true
        } else {
            false
        }
    }
}

/// App data directory: `%APPDATA%\voice-ptt` on Windows, cwd elsewhere.
pub fn dirs_or_cwd() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(base) = std::env::var_os("APPDATA") {
            return PathBuf::from(base).join("voice-ptt");
        }
    }
    PathBuf::from(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let s = Settings::default();
        assert_eq!(s.audio.sample_rate, 16_000);
        assert_eq!(s.audio.buffer_frames, 256);
        assert_eq!(s.audio.ring_seconds, 60);
        assert_eq!(s.asr.beam_size, 5);
        assert_eq!(s.asr.n_threads, 8);
        assert_eq!(s.vad.threshold, 0.5);
        assert_eq!(s.vad.silence_timeout_ms, 1_500);
        assert_eq!(s.vad.chunk_size, 512);
        assert!(!s.vad.cutoff_on_hold);
        assert_eq!(s.audio.gain_db, 0.0);
        assert_eq!(s.hotkey.record, "CapsLock");
        assert!(s.hotkey.double_tap_latch);
        assert_eq!(s.hotkey.tap_max_ms, 350);
        assert_eq!(s.hotkey.double_tap_window_ms, 600);
        assert!(s.gui.show_transcript_bubble);
        assert!(s.streaming.enabled);
        assert_eq!(s.streaming.chunk_seconds, 20);
        assert_eq!(s.streaming.min_chunk_seconds, 4);
        assert_eq!(s.streaming.overlap_ms, 300);
        assert_eq!(s.streaming.silence_ms, 1200);
        assert_eq!(s.streaming.strategy, "silence");
        assert_eq!(s.streaming.max_utterance_seconds, 600);
        assert!(s.google.enabled);
        assert_eq!(s.google.language, "fa-IR");
        assert!(s.updates.check_on_startup);
        assert_eq!(s.updates.auto_check_interval_hours, 6);
    }

    #[test]
    fn round_trips_through_toml() {
        let dir = std::env::temp_dir().join("voice-ptt-cfg-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");

        let s1 = Settings::load_or_create(&path).unwrap();
        let s2 = Settings::load_or_create(&path).unwrap();
        assert_eq!(s1.audio.sample_rate, s2.audio.sample_rate);
        assert_eq!(s1.vad.silence_timeout_ms, s2.vad.silence_timeout_ms);

        // Custom value survives a save/load cycle.
        let mut s3 = s1.clone();
        s3.vad.threshold = 0.7;
        s3.save(&path).unwrap();
        let s4 = Settings::load_or_create(&path).unwrap();
        assert!((s4.vad.threshold - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn auto_model_policy_selects_by_hardware() {
        let s = Settings::default();
        assert_eq!(s.resolve_model_name(Some("nvidia"), 4), "large-v3-turbo");
        assert_eq!(s.resolve_model_name(None, 16), "small");
        assert_eq!(s.resolve_model_name(None, 4), "base");
        // Explicit model is respected.
        let mut s2 = Settings::default();
        s2.asr.model = "tiny".into();
        assert_eq!(s2.resolve_model_name(Some("nvidia"), 16), "tiny");
    }

    #[test]
    fn invalid_toml_is_a_clean_error() {
        let dir = std::env::temp_dir().join("voice-ptt-cfg-bad");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "[audio\nbroken").unwrap();
        assert!(Settings::load_or_create(&path).is_err());
    }
}
