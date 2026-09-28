//! Live end-to-end check of the Antigravity bridge (ignored by default).
//!
//! Needs (a) a running Antigravity signed in to an account and (b) Windows'
//! built-in speech synthesizer, so it never runs in CI. It synthesizes a
//! sentence, pushes it through `AntigravityEngine` exactly the way the state
//! machine does, and prints the transcript the cloud model returned:
//!
//! ```text
//! cargo test --test antigravity_live -- --ignored --nocapture
//! ```

#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::Command;

use voice_ptt::asr::antigravity::{AntigravityEngine, Endpoint};
use voice_ptt::asr::engine::{AsrEngine, AudioUtterance};
use voice_ptt::config::settings::AntigravityConfig;

const SENTENCE: &str = "hello world, this is a test of the dictation bridge";

/// Renders `text` to a 16 kHz mono PCM16 WAV with SAPI (no microphone needed).
fn synthesize(text: &str) -> PathBuf {
    let out = std::env::temp_dir().join("voice-ptt-antigravity-live.wav");
    let script = format!(
        "Add-Type -AssemblyName System.Speech; \
         $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
         $fmt = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(16000, \
         [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, \
         [System.Speech.AudioFormat.AudioChannel]::Mono); \
         $s.SetOutputToWaveFile('{}', $fmt); $s.Speak('{}'); $s.Dispose()",
        out.display(),
        text
    );
    let status = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status()
        .expect("failed to run powershell.exe");
    assert!(status.success(), "SAPI synthesis failed");
    out
}

/// Minimal PCM16 mono WAV reader (SAPI writes none of the exotic chunks).
fn read_pcm16_mono_wav(path: &Path) -> (Vec<f32>, u32) {
    let bytes = std::fs::read(path).expect("read synthesized wav");
    assert_eq!(&bytes[0..4], b"RIFF", "not a RIFF file");

    let mut pos = 12usize;
    let mut format: Option<(u16, u16, u32, u16)> = None;
    let mut data: Option<&[u8]> = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = &bytes[pos + 8..(pos + 8 + size).min(bytes.len())];
        if id == b"fmt " && body.len() >= 16 {
            format = Some((
                u16::from_le_bytes([body[0], body[1]]),
                u16::from_le_bytes([body[2], body[3]]),
                u32::from_le_bytes([body[4], body[5], body[6], body[7]]),
                u16::from_le_bytes([body[14], body[15]]),
            ));
        } else if id == b"data" {
            data = Some(body);
        }
        pos += 8 + size + (size % 2);
    }

    let (encoding, channels, rate, bits) = format.expect("no fmt chunk");
    assert_eq!((encoding, channels, bits), (1, 1, 16), "expected PCM16 mono");
    let data = data.expect("no data chunk");
    let samples = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| i16::from_le_bytes(*pair) as f32 / 32_768.0)
        .collect();
    (samples, rate)
}

fn endpoint_or_panic() -> Endpoint {
    match voice_ptt::asr::antigravity::discover_endpoint() {
        Ok(endpoint) => endpoint,
        Err(e) => panic!("no Antigravity language server available: {e}"),
    }
}

/// Engine logs (`ready_ms`, `sent_ms`, partial timings) need a subscriber.
fn init_logs() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}

#[test]
#[ignore = "requires a running Antigravity plus Windows TTS"]
fn discovers_the_running_language_server() {
    init_logs();
    let endpoint = endpoint_or_panic();
    println!(
        "language_server.exe pid={} port={} token={}…",
        endpoint.pid,
        endpoint.port,
        endpoint.token.chars().take(8).collect::<String>()
    );
    assert!(endpoint.port > 0);
}

#[test]
#[ignore = "requires a running Antigravity plus Windows TTS"]
fn transcribes_synthesized_speech() {
    init_logs();
    endpoint_or_panic(); // fail fast when Antigravity is not up

    let engine = AntigravityEngine::new(AntigravityConfig::default());
    engine.refresh_health();
    assert!(
        engine.health().is_available(),
        "engine should be ready once the server is found: {:?}",
        engine.health()
    );

    let (samples, rate) = read_pcm16_mono_wav(&synthesize(SENTENCE));
    println!(
        "sending {:.2}s @ {rate} Hz through Antigravity…",
        samples.len() as f32 / rate as f32
    );

    let utterance = AudioUtterance {
        samples,
        sample_rate: rate,
    };
    let started = std::time::Instant::now();
    let text = engine
        .transcribe(&utterance)
        .expect("transcription through the Antigravity language server failed");
    println!(
        "antigravity transcript ({} ms): {text}",
        started.elapsed().as_millis()
    );

    let lowered = text.to_lowercase();
    assert!(
        lowered.contains("hello") || lowered.contains("dictation"),
        "unexpected transcript: {text}"
    );
}
