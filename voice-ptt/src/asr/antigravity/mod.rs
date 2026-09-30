//! Antigravity live-dictation engine.
//!
//! Antigravity (the IDE) ships a local Go server, `language_server.exe`, that
//! proxies its cloud speech-to-text as a **gRPC-Web stream over loopback**. Its
//! renderer never holds a Google token — it only sends a CSRF token that the
//! server put on its own command line. That is what makes the pipeline
//! reachable from outside the app, and it is what this engine does.
//!
//! Measured end-to-end on 2026-09-26/27 (`docs/reaserch/bridge/`):
//!
//! ```text
//! POST /exa.language_server_pb.LanguageServerService/StreamAudioTranscription
//!   header x-codeium-csrf-token: <from `--csrf_token` on the process cmdline>
//!   body   gRPC-Web frame: 00 | len(u32 BE) | {"mimeType":"audio/pcm;rate=16000","cascadeId":""}
//!        <- {"ready":{"sessionId":"…"}}                       (~200 ms)
//!   POST …/SendAudioChunk {sessionId, data: base64(PCM16), sequenceNumber}
//!        <- {"transcription":{"text":"Hello"}}                (partial, cumulative)
//!        <- {"transcription":{"text":"Hello world.","isFinal":true}}
//!        <- {"complete":{}} + trailer `grpc-status: 0`
//!   POST …/EndAudioSession {sessionId}
//! ```
//!
//! Findings that shaped this implementation:
//! - `cascadeId` may be empty; the server opens a session either way.
//! - `ready` gates everything — the app itself stays silent until it arrives.
//! - The server tolerates a burst far faster than real time (the app flushes
//!   12.5 s of buffered audio inside its first second), so we drain an utterance
//!   at ~16× real time instead of one chunk per 40 ms. Latency over accuracy of
//!   pacing: the cloud model gets the whole utterance immediately.
//! - Audio leaves this process only towards `127.0.0.1`; the local server (which
//!   the user is already signed in to) owns the cloud hop.

use std::io::Read;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use reqwest::blocking::Client;
use serde_json::{json, Value};

use super::engine::{AsrEngine, AsrHealth, AudioUtterance};
use super::progress;
use crate::config::settings::AntigravityConfig;

// The wire format lives in its own module: it is pure, so the contract can be
// tested without a server running. See `protocol` for why.
mod protocol;
// Endpoint discovery (process spawn + RPC probe) is split out the same way,
// for the same reason: it answers "what address?", never "what did you hear?".
mod discovery;

use discovery::{discover_cascade_id, discover_endpoint_with, fallback_cascade_id};
// Re-exported so `antigravity::discover_endpoint` keeps working for callers
// outside this module (`tests/antigravity_live.rs`) after the split.
pub use discovery::{
    arg_value, candidate_endpoints, cascade_from_url, discover_endpoint,
    parse_cascade_from_targets, parse_netstat_ports, parse_wmi_processes, Endpoint, ServerProcess,
};

use protocol::{
    encode_frame, is_complete, ready_session_id, request_headers, to_pcm16k_mono,
    transcription_text, FrameDecoder,
};
/// 40 ms of PCM16 mono @ 16 kHz — the chunk size the app itself uses.
const CHUNK_BYTES: usize = 1_280;
/// Chunks sent back to back before yielding (~1 s of audio per burst).
const CHUNKS_PER_BURST: usize = 25;
/// Pause between bursts (~16× real time overall).
const BURST_PAUSE: Duration = Duration::from_millis(60);
/// How long a discovered port/token pair stays valid before re-discovery.
const ENDPOINT_TTL: Duration = Duration::from_secs(90);
const READ_BUF: usize = 8 * 1024;

// ---------------------------------------------------------------- engine ----

struct CachedEndpoint {
    endpoint: Endpoint,
    at: Instant,
}

/// Streaming ASR engine backed by a locally running Antigravity.
pub struct AntigravityEngine {
    client: Client,
    cfg: AntigravityConfig,
    endpoint: Mutex<Option<CachedEndpoint>>,
    health: RwLock<AsrHealth>,
    last_error: Mutex<Option<String>>,
}

impl AntigravityEngine {
    pub fn new(cfg: AntigravityConfig) -> Self {
        let budget = cfg.ready_timeout_secs + cfg.finalize_timeout_secs + 45;
        let client = Client::builder()
            .timeout(Duration::from_secs(budget))
            .connect_timeout(Duration::from_secs(4))
            // The local server presents its own self-signed certificate; the app
            // trusts it, and nothing here ever leaves loopback.
            .danger_accept_invalid_certs(true)
            // The renderer speaks gRPC-Web over HTTP/1.1 — mirror that exactly.
            .http1_only()
            .pool_max_idle_per_host(4)
            .build()
            .unwrap_or_default();

        let initial = if cfg.enabled {
            AsrHealth::Cooldown {
                reason: "not probed yet (waiting for Antigravity)".into(),
                retry_after_ms: 0,
            }
        } else {
            AsrHealth::Failed {
                reason: "antigravity engine disabled in config".into(),
            }
        };

        Self {
            client,
            cfg,
            endpoint: Mutex::new(None),
            health: RwLock::new(initial),
            last_error: Mutex::new(None),
        }
    }

    /// Probes for a running server and publishes the result as health.
    /// Spawns PowerShell + netstat, so it runs on a background thread.
    pub fn refresh_health(&self) {
        if !self.cfg.enabled {
            return;
        }
        let found = discover_endpoint_with(&self.client);
        self.publish_probe(&found);
    }

    /// Background maintenance loop body: re-probe when the cached endpoint got
    /// stale. Antigravity picks new ports on every restart, so a cached one has
    /// a limited lifetime even while everything is healthy.
    pub fn maintain(&self) {
        if !self.cfg.enabled {
            return;
        }
        let fresh = self
            .endpoint
            .lock()
            .map(|slot| {
                slot.as_ref()
                    .map(|cached| cached.at.elapsed() < ENDPOINT_TTL)
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if !fresh {
            let found = discover_endpoint_with(&self.client);
            self.publish_probe(&found);
        }
    }

    fn publish_probe(&self, found: &Result<Endpoint>) {
        match found {
            Ok(endpoint) => {
                if let Ok(mut slot) = self.endpoint.lock() {
                    *slot = Some(CachedEndpoint {
                        endpoint: endpoint.clone(),
                        at: Instant::now(),
                    });
                }
                if let Ok(mut slot) = self.health.write() {
                    *slot = AsrHealth::Ready;
                }
            }
            Err(e) => {
                tracing::debug!(error = %e, "antigravity language server unavailable");
                if let Ok(mut slot) = self.health.write() {
                    *slot = AsrHealth::Failed {
                        reason: e.to_string(),
                    };
                }
            }
        }
    }

    /// Cached endpoint, re-discovered when missing or stale.
    /// Cascade id to send with a session: explicit config → the open window →
    /// a random UUID. Never empty, because the server can silently ignore
    /// sessions opened with an empty id.
    fn cascade_id(&self) -> String {
        let configured = self.cfg.cascade_id.trim();
        if !configured.is_empty() && configured != "auto" {
            return configured.to_string();
        }
        match discover_cascade_id() {
            Some(id) => {
                tracing::info!(cascade_id = %id, "using the cascade id of the open Antigravity window");
                id
            }
            None => {
                let id = fallback_cascade_id();
                tracing::warn!(
                    cascade_id = %id,
                    "no Antigravity window found; using a generated cascade id"
                );
                id
            }
        }
    }

    fn endpoint(&self) -> Result<Endpoint> {
        if !self.cfg.enabled {
            bail!("antigravity engine disabled in config");
        }
        if let Ok(slot) = self.endpoint.lock() {
            if let Some(cached) = slot.as_ref() {
                if cached.at.elapsed() < ENDPOINT_TTL {
                    return Ok(cached.endpoint.clone());
                }
            }
        }
        let found = discover_endpoint_with(&self.client);
        self.publish_probe(&found);
        found
    }

    fn record_error(&self, message: &str) {
        if let Ok(mut slot) = self.last_error.lock() {
            *slot = Some(message.to_string());
        }
    }

    /// Last failure seen by this engine (for logs; the router keeps its own).
    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().ok().and_then(|slot| slot.clone())
    }

    fn post_rpc(&self, endpoint: &Endpoint, rpc: &str, value: &Value) -> Result<u16> {
        let response = self
            .client
            .post(format!("{}/{rpc}", endpoint.base()))
            .headers(request_headers(endpoint.port, &endpoint.token))
            .body(encode_frame(value))
            .send()
            .with_context(|| format!("{rpc} request failed"))?;
        let status = response.status().as_u16();
        // Drain the (tiny) body so the pooled connection stays reusable.
        let _ = response.text();
        Ok(status)
    }
}

impl AsrEngine for AntigravityEngine {
    fn name(&self) -> &'static str {
        "antigravity"
    }

    fn id(&self) -> String {
        "antigravity".to_string()
    }

    fn display_name(&self) -> String {
        "Antigravity Live Dictation".to_string()
    }

    fn kind(&self) -> &'static str {
        "Cloud (Local Bridge)"
    }

    fn health(&self) -> AsrHealth {
        self.health
            .read()
            .map(|slot| slot.clone())
            .unwrap_or_else(|_| AsrHealth::Failed {
                reason: "health lock poisoned".into(),
            })
    }

    fn transcribe(&self, audio: &AudioUtterance) -> Result<String> {
        if audio.samples.is_empty() {
            bail!("cannot transcribe empty audio");
        }
        let endpoint = match self.endpoint() {
            Ok(endpoint) => endpoint,
            Err(e) => {
                self.record_error(&e.to_string());
                return Err(e);
            }
        };
        let started = Instant::now();

        // 1. Open the stream. `ready` arrives on this response body.
        let stream_budget =
            Duration::from_secs(self.cfg.ready_timeout_secs + self.cfg.finalize_timeout_secs + 10);
        let connect_started = Instant::now();
        let response = self
            .client
            .post(format!("{}/StreamAudioTranscription", endpoint.base()))
            .headers(request_headers(endpoint.port, &endpoint.token))
            // A session that never opens must not hold a socket forever.
            .timeout(stream_budget)
            .body(encode_frame(&json!({
                "mimeType": "audio/pcm;rate=16000",
                "cascadeId": self.cascade_id(),
            })))
            .send()
            .context("StreamAudioTranscription request failed")?;
        let headers_ms = connect_started.elapsed().as_millis() as u64;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            let message = format!(
                "StreamAudioTranscription returned HTTP {}: {}",
                status.as_u16(),
                body.chars().take(200).collect::<String>()
            );
            self.record_error(&message);
            bail!("{message}");
        }

        // 2. Decode the response on its own thread; chunks are unary calls on
        //    this thread, so neither blocks the other.
        let (tx, rx) = mpsc::channel::<Value>();
        spawn_reader(response, tx);

        let session_id = match wait_for_ready(&rx, Duration::from_secs(self.cfg.ready_timeout_secs))
        {
            Ok(id) => id,
            Err(e) => {
                self.record_error(&e.to_string());
                return Err(e);
            }
        };
        let ready_ms = started.elapsed().as_millis() as u64;

        // 3. Drain the utterance as fast as the server accepts it.
        let pcm = to_pcm16k_mono(&audio.samples, audio.sample_rate);
        let mut sequence = 0u32;
        let mut in_burst = 0usize;
        let total_chunks = pcm.len().div_ceil(CHUNK_BYTES);
        for chunk in pcm.chunks(CHUNK_BYTES) {
            let value = json!({
                "sessionId": session_id,
                "data": B64.encode(chunk),
                "sequenceNumber": sequence,
            });
            if let Err(e) = self.post_rpc(&endpoint, "SendAudioChunk", &value) {
                self.record_error(&e.to_string());
                return Err(e);
            }
            sequence += 1;
            in_burst += 1;
            if in_burst == CHUNKS_PER_BURST && (sequence as usize) < total_chunks {
                in_burst = 0;
                std::thread::sleep(BURST_PAUSE);
            }
        }
        let sent_ms = started.elapsed().as_millis() as u64;

        // 4. Close the session first: the server emits the *final* transcript in
        //    response to `EndAudioSession` (~1 s), so waiting before ending the
        //    session would burn the whole finalize budget for nothing. This also
        //    ends the reader thread's socket.
        if let Err(e) = self.post_rpc(
            &endpoint,
            "EndAudioSession",
            &json!({ "sessionId": session_id }),
        ) {
            tracing::warn!(error = %e, "EndAudioSession failed; the server will time the session out");
        }

        // 5. Collect the final transcript (partials were published live).
        let outcome = collect_final(
            &rx,
            Duration::from_secs(self.cfg.finalize_timeout_secs),
            started,
        );

        let text = outcome.transcript.unwrap_or_default().trim().to_string();
        if text.is_empty() {
            let message = format!(
                "antigravity returned no transcript ({sequence} chunks sent, complete={})",
                outcome.complete
            );
            self.record_error(&message);
            bail!("{message}");
        }

        tracing::info!(
            headers_ms,
            ready_ms,
            sent_ms,
            total_ms = started.elapsed().as_millis() as u64,
            chunks = sequence,
            chars = text.chars().count(),
            "antigravity transcription complete"
        );
        if let Ok(mut slot) = self.health.write() {
            *slot = AsrHealth::Ready;
        }
        Ok(text)
    }
}

/// Reads the response body on a dedicated thread and forwards decoded messages.
fn spawn_reader(mut response: reqwest::blocking::Response, tx: Sender<Value>) {
    std::thread::spawn(move || {
        let mut decoder = FrameDecoder::default();
        let mut buf = vec![0u8; READ_BUF];
        loop {
            match response.read(&mut buf) {
                Ok(0) => break,
                Ok(read) => {
                    for message in decoder.push(&buf[..read]) {
                        if tx.send(message).is_err() {
                            return; // session finished; receiver dropped
                        }
                    }
                }
                Err(e) => {
                    tracing::debug!(error = %e, "antigravity stream read ended");
                    break;
                }
            }
        }
    });
}

/// Blocks until the server hands us a `sessionId`.
fn wait_for_ready(rx: &Receiver<Value>, timeout: Duration) -> Result<String> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            bail!("timed out waiting for `ready` from the Antigravity language server");
        }
        match rx.recv_timeout(remaining) {
            Ok(message) => {
                if let Some(id) = ready_session_id(&message) {
                    return Ok(id);
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                bail!("timed out waiting for `ready` from the Antigravity language server")
            }
            Err(RecvTimeoutError::Disconnected) => bail!(
                "the transcription stream closed before `ready` — is the Antigravity window closed or signed out?"
            ),
        }
    }
}

struct FinalOutcome {
    transcript: Option<String>,
    complete: bool,
}

/// Consumes stream messages until the final transcript, `complete`, or the deadline.
fn collect_final(rx: &Receiver<Value>, timeout: Duration, started: Instant) -> FinalOutcome {
    let deadline = Instant::now() + timeout;
    let mut latest: Option<String> = None;
    let mut complete = false;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match rx.recv_timeout(remaining) {
            Ok(message) => {
                if let Some((text, is_final)) = transcription_text(&message) {
                    if !text.is_empty() {
                        tracing::debug!(
                            since_start_ms = started.elapsed().as_millis() as u64,
                            is_final,
                            "antigravity partial"
                        );
                        if is_final {
                            return FinalOutcome {
                                transcript: Some(text),
                                complete,
                            };
                        }
                        progress::publish(&text);
                        latest = Some(text);
                    }
                }
                if is_complete(&message) {
                    complete = true;
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    FinalOutcome {
        transcript: latest,
        complete,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_size_is_forty_milliseconds() {
        assert_eq!(CHUNK_BYTES, 16_000 * 2 * 40 / 1_000);
        let pcm = vec![0u8; CHUNK_BYTES * 2 + 100];
        let chunks: Vec<usize> = pcm.chunks(CHUNK_BYTES).map(<[u8]>::len).collect();
        assert_eq!(chunks, vec![CHUNK_BYTES, CHUNK_BYTES, 100]);
    }

    #[test]
    fn engine_health_starts_unprobed_then_follows_config() {
        let engine = AntigravityEngine::new(AntigravityConfig::default());
        assert!(!engine.health().is_available(), "not probed yet");
        assert!(engine
            .transcribe(&AudioUtterance {
                samples: Vec::new(),
                sample_rate: 16_000
            })
            .is_err());

        let disabled = AntigravityEngine::new(AntigravityConfig {
            enabled: false,
            ..AntigravityConfig::default()
        });
        assert!(matches!(disabled.health(), AsrHealth::Failed { .. }));
    }
}
