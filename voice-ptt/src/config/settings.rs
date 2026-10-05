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
    pub text: TextSettings,
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
    /// Per-application rules, applied on top of the general settings above.
    ///
    /// Written as `[[profiles]]` entries, bound to an executable. An empty set
    /// is the default and means every window gets the general rules, so a user
    /// who never opens the panel sees exactly the behaviour they had before
    /// this section existed.
    #[serde(default)]
    pub profiles: crate::profiles::ProfileSet,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            audio: AudioSettings::default(),
            asr: AsrSettings::default(),
            vad: VadSettings::default(),
            streaming: StreamingSettings::default(),
            text: TextSettings::default(),
            hotkey: HotkeySettings::default(),
            gui: GuiSettings::default(),
            cloud: CloudConfig::default(),
            google: GoogleConfig::default(),
            antigravity: AntigravityConfig::default(),
            active_engine: "auto".into(),
            custom_providers: Vec::new(),
            updates: UpdateSettings::default(),
            profiles: crate::profiles::ProfileSet::default(),
        }
    }
}

/// What the typed text is allowed to go through on its way out of the
/// recogniser.
///
/// Kept as its own `[text]` section rather than a key inside `[streaming]`,
/// because chunking is about *when* a session is cut and this is about *what
/// survives the cut* — the two were never the same decision and lumping them
/// together is why T0 could not find a switch to turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct TextSettings {
    /// `standard` (default), `conservative`, or `raw`.
    ///
    /// A string rather than the enum itself so an unknown value can fall back
    /// to the historical behaviour instead of failing to load the whole file:
    /// see [`TextSettings::mode`].
    pub mode: String,
}

impl Default for TextSettings {
    fn default() -> Self {
        Self {
            mode: "standard".into(),
        }
    }
}

impl TextSettings {
    /// The mode to run, resolved.
    ///
    /// Anything unrecognised — including a typo — becomes
    /// [`TextMode::Standard`](crate::processing::TextMode::Standard), because
    /// the failure mode matters more than the setting: silently dropping to
    /// `raw` would be discovered by reading the text of a document somebody
    /// else is going to read.
    pub fn mode(&self) -> crate::processing::TextMode {
        crate::processing::TextMode::parse(&self.mode)
    }

    /// The pipeline options this section describes.
    pub fn options(&self) -> crate::processing::ProcessingOptions {
        crate::processing::ProcessingOptions::new(self.mode())
    }
}

/// Auto-update checker configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct UpdateSettings {
    pub check_on_startup: bool,
    pub auto_check_interval_hours: u64,
    /// Say so out loud when a newer release exists, rather than waiting for
    /// the user to open the settings tab and notice the banner.
    ///
    /// On by default, and this is the fix for the gap that motivated the whole
    /// balloon: detection worked and nothing interrupted anybody. Turning it
    /// off restores the old silence; the check itself keeps running, so this
    /// governs the *notification* and not the checking.
    #[serde(default = "default_true")]
    pub notify_on_available: bool,
    /// The newest version already announced to this user.
    ///
    /// Written by the app, never meant to be edited — but it lives in
    /// `config.toml` like everything else, which is why
    /// [`crate::gui::tray_balloon::decide`] normalises it before comparing. A
    /// hand-typed `v0.4.0` must not read as a different release from `0.4.0`,
    /// because that would re-announce on every single start.
    #[serde(default)]
    pub last_notified_version: String,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            check_on_startup: true,
            auto_check_interval_hours: 6,
            notify_on_available: true,
            last_notified_version: String::new(),
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
    /// Whether the orb walks back to its corner after a spell of inactivity.
    ///
    /// Defaults to **on**, because the feature exists to solve a complaint
    /// (the orb parking itself wherever the last dictation left it). An existing
    /// `config.toml` has no key for it, and `serde(default)` gives it the same
    /// on-state a fresh install gets rather than silently opting every current
    /// user out of the thing they were missing.
    #[serde(default = "default_true")]
    pub orb_return_enabled: bool,
    /// Seconds of inactivity before the orb returns. Zero means "use the
    /// product default" rather than "return immediately", because a `0` here
    /// would otherwise be a valid config file that returns the orb the moment
    /// the user lets go of the mouse.
    #[serde(default)]
    pub orb_return_after_idle_secs: u64,
    /// Pin the orb where it was put: suppress the automatic return. An explicit
    /// "take it back to my spot" is still honoured.
    #[serde(default)]
    pub orb_pinned: bool,
    /// Which corner the orb returns to. A **string**, not an enum, because this
    /// is a user-facing config value and a typo must degrade to the default
    /// rather than fail the whole file's load.
    ///
    /// The corner used to be hardcoded to top-right, which is why the orb always
    /// jumped *up* regardless of where the user had put it: on a screen whose
    /// taskbar is at the bottom, the bottom-right corner is the one out of the
    /// way, and there was no way to say so.
    #[serde(default = "default_return_corner")]
    pub orb_return_corner: String,
    /// How large the orb is drawn and how large a click it answers, as a
    /// percentage of the built-in size. `100` is the size this build ships.
    ///
    /// Clamped on read by [`Self::orb_scale_percent`]: a hand-edited `0` or
    /// `5000` would otherwise produce an orb that cannot be clicked at all, or
    /// one that covers the screen.
    #[serde(default = "default_orb_scale")]
    pub orb_scale_percent: u32,
    /// Type the result progressively, in small groups, instead of delivering a
    /// whole dictation in one burst.
    ///
    /// Off by default, and that is a decision rather than an omission: the
    /// burst form has a real advantage, which is that a dictation that the
    /// platform refuses part-way has failed *visibly and early*. Pacing trades
    /// that for looking nicer, so it is the user's call.
    #[serde(default)]
    pub type_progressively: bool,
    /// Characters typed per step when `type_progressively` is on, and the gap
    /// between steps in milliseconds. Both are clamped on read.
    #[serde(default = "default_type_step_chars")]
    pub type_step_chars: u32,
    #[serde(default = "default_type_step_ms")]
    pub type_step_ms: u32,
    /// Show the finished text before it is typed, and let the user edit, insert,
    /// copy or drop it.
    ///
    /// Off by default. Direct typing is what this app has always done and it is
    /// what makes a dictation feel like talking rather than filling in a form;
    /// review is for the cases where a typo is cheaper to catch than to undo.
    ///
    /// Note this does **not** govern recovery: text that failed to insert is
    /// always offered, because there the text is in nobody's document and this
    /// setting cannot make losing it acceptable.
    #[serde(default)]
    pub review_before_insert: bool,
    /// Seconds a held text stays on offer. Zero means "use the product
    /// default" rather than "expire immediately".
    #[serde(default)]
    pub draft_ttl_secs: u64,
}

impl GuiSettings {
    /// The return corner as a value, defaulting on an unrecognised string.
    ///
    /// `default_return_corner` keeps the happy path total; this keeps the
    /// unhappy path total too. A config written by hand, or by a future version
    /// that renames a corner, must not make the orb unreturnable.
    pub fn orb_return_corner_value(&self) -> crate::gui::orb_idle_policy::Corner {
        use crate::gui::orb_idle_policy::Corner;
        match self.orb_return_corner.trim().to_ascii_lowercase().as_str() {
            "top_left" | "topleft" | "top-left" => Corner::TopLeft,
            "bottom_left" | "bottomleft" | "bottom-left" => Corner::BottomLeft,
            "bottom_right" | "bottomright" | "bottom-right" => Corner::BottomRight,
            _ => Corner::TopRight,
        }
    }

    /// The configured orb scale, as a multiplier, clamped to a usable range.
    ///
    /// The floor is not cosmetic: the click region is derived from the painted
    /// circle, so too small an orb means too small a target, and a target below
    /// a usable click size is a feature that cannot be operated. The ceiling is
    /// the window this orb can be drawn into.
    pub fn orb_scale(&self) -> f32 {
        const MIN_PERCENT: u32 = 60;
        const MAX_PERCENT: u32 = 200;
        let pct = self.orb_scale_percent.clamp(MIN_PERCENT, MAX_PERCENT);
        pct as f32 / 100.0
    }

    /// Characters per paced step, clamped so a hand-edited `0` cannot stall
    /// typing forever and a huge value cannot turn pacing back into a burst.
    pub fn type_step(&self) -> (usize, std::time::Duration) {
        const MIN_CHARS: u32 = 1;
        const MAX_CHARS: u32 = 32;
        const MIN_MS: u32 = 0;
        const MAX_MS: u32 = 250;
        (
            self.type_step_chars.clamp(MIN_CHARS, MAX_CHARS) as usize,
            std::time::Duration::from_millis(self.type_step_ms.clamp(MIN_MS, MAX_MS) as u64),
        )
    }

    /// How long held text stays on offer, with `0` meaning the product default.
    ///
    /// Clamped at both ends. A `0` would otherwise be a valid config that expires
    /// every draft before the window can even open, and an enormous one would be
    /// text that follows the user around for hours.
    pub fn draft_ttl_secs(&self) -> u64 {
        const MIN_SECS: u64 = 15;
        const MAX_SECS: u64 = 3_600;
        if self.draft_ttl_secs == 0 {
            return crate::state::review::DEFAULT_DRAFT_TTL.as_secs();
        }
        self.draft_ttl_secs.clamp(MIN_SECS, MAX_SECS)
    }
}

/// `true`, for `#[serde(default = ...)]` on an opt-out boolean.
///
/// A named function because `default = "true"` is a stringly-typed hook into
/// the standard library's private impl, and a typo in it is a compile error
/// only if the path is checked — it is, but the message is unhelpful.
const fn default_true() -> bool {
    true
}

/// The corner the orb returns to when the config does not say.
///
/// Top-right, because that is what every existing install has been getting
/// without asking. Changing it would move the orb under people who never chose
/// a corner, so the default is the *observed* behaviour, not the preferred one.
fn default_return_corner() -> String {
    "top_right".to_string()
}

/// The shipped orb size, as a percentage.
fn default_orb_scale() -> u32 {
    100
}

/// Characters per step when typing progressively.
fn default_type_step_chars() -> u32 {
    2
}

/// Gap between steps, in milliseconds. Long enough to read as typing, short
/// enough that a paragraph does not take a minute to arrive.
fn default_type_step_ms() -> u32 {
    18
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
            orb_return_enabled: default_true(),
            orb_return_after_idle_secs: 0,
            orb_pinned: false,
            orb_return_corner: default_return_corner(),
            orb_scale_percent: default_orb_scale(),
            type_progressively: false,
            type_step_chars: default_type_step_chars(),
            type_step_ms: default_type_step_ms(),
            review_before_insert: false,
            draft_ttl_secs: 0,
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
        if let Some(existing) = self
            .custom_providers
            .iter_mut()
            .find(|p| p.id == provider.id)
        {
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

    // ── the orb preferences added with the user-facing controls ──────────

    /// An existing `config.toml` has none of these keys, and it must come out
    /// of the load with the shipped values rather than failing to parse or
    /// starting from something arbitrary.
    #[test]
    fn a_config_without_the_new_orb_keys_still_loads_with_sane_values() {
        let dir = std::env::temp_dir().join(format!("voiceptt-orb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "[gui]\nshow_overlay = true\n").unwrap();

        let loaded = Settings::load_or_create(&path).expect("an older config must still load");
        assert_eq!(loaded.gui.orb_return_corner, "top_right");
        assert_eq!(loaded.gui.orb_scale_percent, 100);
        assert!(
            !loaded.gui.type_progressively,
            "progressive typing is opt-in, so an old config must not turn it on"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_return_corner_round_trips_and_an_unknown_one_falls_back() {
        use crate::gui::orb_idle_policy::Corner;
        let mut s = Settings::default();
        for (text, expected) in [
            ("top_left", Corner::TopLeft),
            ("top_right", Corner::TopRight),
            ("bottom_left", Corner::BottomLeft),
            ("bottom_right", Corner::BottomRight),
        ] {
            s.gui.orb_return_corner = text.to_string();
            assert_eq!(
                s.gui.orb_return_corner_value(),
                expected,
                "{text} must select its own corner"
            );
        }
        // A hand-edited or future value must not make the orb unreturnable.
        s.gui.orb_return_corner = "diagonal".to_string();
        assert_eq!(s.gui.orb_return_corner_value(), Corner::TopRight);
    }

    /// The bottom corners exist because the sensible corner depends on where the
    /// taskbar is. Before this was a setting the orb always went up, which on a
    /// bottom-taskbar machine is the corner that is *not* out of the way.
    #[test]
    fn the_bottom_corners_are_selectable() {
        use crate::gui::orb_idle_policy::Corner;
        let mut s = Settings::default();
        s.gui.orb_return_corner = "bottom_right".into();
        assert_eq!(s.gui.orb_return_corner_value(), Corner::BottomRight);
        s.gui.orb_return_corner = "bottom_left".into();
        assert_eq!(s.gui.orb_return_corner_value(), Corner::BottomLeft);
    }

    /// A nonsense size must be clamped rather than obeyed: `0` would make the
    /// orb unclickable and `5000` would cover the screen.
    #[test]
    fn the_orb_size_is_clamped_to_a_usable_range() {
        let mut s = Settings::default();
        s.gui.orb_scale_percent = 100;
        assert!((s.gui.orb_scale() - 1.0).abs() < 0.001);
        s.gui.orb_scale_percent = 0;
        assert!(s.gui.orb_scale() >= 0.6, "too small to click must be raised");
        s.gui.orb_scale_percent = 5000;
        assert!(s.gui.orb_scale() <= 2.0, "a screen-filling orb must be refused");
    }

    /// The typing step is likewise clamped: a `0` characters per step would make
    /// the paced path emit nothing, and a huge gap would be indistinguishable
    /// from a hang.
    #[test]
    fn the_typing_step_is_clamped() {
        let mut s = Settings::default();
        s.gui.type_step_chars = 0;
        s.gui.type_step_ms = 100_000;
        let (chars, gap) = s.gui.type_step();
        assert!(chars >= 1, "a zero step would emit nothing at all");
        assert!(gap.as_millis() <= 250, "a huge gap reads as a hang");
    }

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
        // Announcing an available update is the point of the feature: an
        // install that checks for updates and then says nothing is the gap
        // this setting was added to close.
        assert!(s.updates.notify_on_available);
        assert_eq!(
            s.updates.last_notified_version, "",
            "a fresh install must not believe it has announced anything"
        );
        // The idle return is on by default: the feature exists to fix the orb
        // parking itself wherever the last dictation left it, so a fresh install
        // that did not get it would be the install most likely to want it.
        assert!(s.gui.orb_return_enabled);
        assert!(!s.gui.orb_pinned);
        // Zero is the "never chosen" storage value, not a zero timeout. The
        // policy turns it into its proposed default; storing the default
        // itself would make an untouched file indistinguishable from a chosen
        // one and would silently overwrite the default when it changes.
        assert_eq!(s.gui.orb_return_after_idle_secs, 0);
    }

    /// An existing `config.toml` written before these keys existed must still
    /// load, and must get the same idle-return state a fresh install gets.
    ///
    /// This is the migration that matters: the fields are opt-out, so an old
    /// file without the key has to land on `true` or every current user would
    /// be quietly opted out of a feature they were missing.
    #[test]
    fn an_older_config_without_the_orb_keys_still_gets_them() {
        let dir = std::env::temp_dir().join("voice-ptt-cfg-legacy-gui");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        // A `[gui]` section with only the keys that existed before, plus one
        // unrelated section, so an unknown-key failure would have something to
        // trip over if `deny_unknown_fields` were ever added.
        std::fs::write(
            &path,
            "[gui]\nshow_overlay = true\ntheme = \"dark\"\n\n[vad]\nthreshold = 0.5\n",
        )
        .unwrap();

        let loaded = Settings::load_or_create(&path).unwrap();
        assert!(loaded.gui.orb_return_enabled, "legacy file must not opt out");
        assert!(!loaded.gui.orb_pinned);
        assert_eq!(loaded.gui.orb_return_after_idle_secs, 0);
        assert_eq!(loaded.vad.threshold, 0.5, "the rest still loads");
    }

    /// The orb keys must survive a save/load cycle, or a user's choice to pin
    /// the orb would quietly reset on the next restart.
    #[test]
    fn the_orb_keys_round_trip_through_toml() {
        let dir = std::env::temp_dir().join("voice-ptt-cfg-orb-roundtrip");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");

        let mut s = Settings::load_or_create(&path).unwrap();
        s.gui.orb_return_enabled = false;
        s.gui.orb_pinned = true;
        s.gui.orb_return_after_idle_secs = 45;
        s.save(&path).unwrap();

        let loaded = Settings::load_or_create(&path).unwrap();
        assert!(!loaded.gui.orb_return_enabled);
        assert!(loaded.gui.orb_pinned);
        assert_eq!(loaded.gui.orb_return_after_idle_secs, 45);
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

    /// Profiles are written by the panel's save button and read back by the
    /// coordinator on the next dictation, through the **same** `save` /
    /// `load_or_create` pair every other setting uses.
    ///
    /// Tested here rather than only in `crate::profiles`, because the thing
    /// that can break is the *file*: a section that round-trips through
    /// `toml::to_string` in isolation can still be dropped, renamed or nested
    /// wrongly once `Settings` writes it.
    #[test]
    fn profiles_survive_a_save_and_load_cycle() {
        let dir = std::env::temp_dir().join("voice-ptt-cfg-profiles");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let _ = std::fs::remove_file(&path);

        let s = Settings {
            profiles: crate::profiles::ProfileSet::new(vec![
                crate::profiles::AppProfile::new(
                    "Editor",
                    "code.exe",
                    crate::profiles::Overrides {
                        text_mode: Some("raw".into()),
                        ..Default::default()
                    },
                ),
                crate::profiles::AppProfile::new(
                    "Chat",
                    "slack.exe",
                    crate::profiles::Overrides {
                        review_before_insert: Some(true),
                        corrections: vec![crate::processing::dictionary::Correction {
                            from: "پاتون".into(),
                            to: "پایتون".into(),
                            category: Some("profile".into()),
                        }],
                        ..Default::default()
                    },
                ),
            ]),
            ..Settings::default()
        };
        s.save(&path).unwrap();

        let loaded = Settings::load_or_create(&path).unwrap();
        assert_eq!(loaded.profiles.len(), 2, "a profile did not survive the file");

        // What the coordinator asks, against the file that was just read: the
        // whole point is that the *loaded* set resolves, not the one in memory.
        let general = crate::profiles::GeneralRules::from(&loaded);
        let editor = crate::profiles::effective(
            &loaded.profiles,
            Some(std::path::Path::new("D:/portable/VSCode/code.exe")),
            general,
        );
        assert_eq!(editor.mode, crate::processing::TextMode::Raw);
        assert!(editor.is_profiled());

        let chat = crate::profiles::effective(
            &loaded.profiles,
            Some(std::path::Path::new("C:/Apps/slack.exe")),
            general,
        );
        assert!(chat.review_before_insert);
        assert_eq!(chat.corrections.len(), 1);
        assert_eq!(chat.corrections[0].to, "پایتون");

        // …and an application with no entry is untouched by either of them.
        let other = crate::profiles::effective(
            &loaded.profiles,
            Some(std::path::Path::new("C:/Apps/notepad.exe")),
            general,
        );
        assert!(!other.is_profiled());

        // The file carries the array-of-tables spelling, so a user editing it by
        // hand has something to copy.
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("[[profiles]]"), "{text}");
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

    /// The default has to be the behaviour that shipped, or a config file with
    /// no `[text]` section quietly becomes a policy change nobody chose.
    #[test]
    fn the_text_mode_defaults_to_the_historical_behaviour() {
        let s = Settings::default();
        assert_eq!(s.text.mode, "standard");
        assert_eq!(s.text.options().mode, crate::processing::TextMode::Standard);
    }

    /// End to end through the file format: this is the only path a user's
    /// setting actually travels, so a rename of the section or the key would
    /// otherwise be invisible here and visible only as "my option does
    /// nothing".
    #[test]
    fn the_text_mode_survives_a_round_trip_through_the_config_file() {
        let dir = std::env::temp_dir().join("omnitype-text-mode-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");

        for (written, expected) in [
            ("raw", crate::processing::TextMode::Raw),
            ("conservative", crate::processing::TextMode::Conservative),
            ("standard", crate::processing::TextMode::Standard),
            // A typo must not silently become "type everything verbatim".
            ("nonsense", crate::processing::TextMode::Standard),
        ] {
            std::fs::write(&path, format!("[text]\nmode = \"{written}\"\n")).unwrap();
            let s = Settings::load_or_create(&path).expect("a [text] section loads");
            assert_eq!(s.text.mode(), expected, "config.toml said {written:?}");
        }

        // No section at all is the same as saying nothing.
        std::fs::write(&path, "[audio]\nsample_rate = 16000\n").unwrap();
        let s = Settings::load_or_create(&path).expect("loads");
        assert_eq!(s.text.mode(), crate::processing::TextMode::Standard);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
