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

use std::collections::HashMap;
use std::io::Read;
use std::process::Command;
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

/// Service root on the local server.
const RPC_ROOT: &str = "exa.language_server_pb.LanguageServerService";
/// 40 ms of PCM16 mono @ 16 kHz — the chunk size the app itself uses.
const CHUNK_BYTES: usize = 1_280;
/// Chunks sent back to back before yielding (~1 s of audio per burst).
const CHUNKS_PER_BURST: usize = 25;
/// Pause between bursts (~16× real time overall).
const BURST_PAUSE: Duration = Duration::from_millis(60);
/// How long a discovered port/token pair stays valid before re-discovery.
const ENDPOINT_TTL: Duration = Duration::from_secs(90);
const READ_BUF: usize = 8 * 1024;

// ------------------------------------------------------------- discovery ----

/// A running Antigravity language server and the credentials its renderer uses.
///
/// The server listens twice: HTTPS on one loopback port and **plain HTTP on
/// another** (an HTTP request to the HTTPS port is answered with 400). Plain
/// HTTP is preferred because the native-TLS handshake to the self-signed local
/// certificate costs ~14 s on this machine, while HTTP answers in ~1 ms — the
/// same 14 s stall the Antigravity UI itself shows before its first audio chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub pid: u32,
    pub port: u16,
    pub token: String,
    pub https: bool,
}

impl Endpoint {
    fn base(&self) -> String {
        let scheme = if self.https { "https" } else { "http" };
        format!("{scheme}://127.0.0.1:{}/{}", self.port, RPC_ROOT)
    }

    /// Cheap RPC used to tell a usable candidate from a wrong scheme/port.
    fn probe(&self, client: &Client) -> bool {
        matches!(self.probe_status(client), Some(200))
    }

    fn probe_status(&self, client: &Client) -> Option<u16> {
        // `{}` is not the real request body, but the point is the transport:
        // a working candidate answers 200, the HTTPS port rejects plain HTTP
        // with 400, and a dead port fails the connection.
        let timeout = Duration::from_secs(if self.https { 20 } else { 3 });
        let response = client
            .post(format!("{}/GetMendelFlags", self.base()))
            .headers(request_headers(self.port, &self.token))
            .timeout(timeout)
            .body(encode_frame(&json!({})))
            .send()
            .ok()?;
        let status = response.status().as_u16();
        let _ = response.text();
        Some(status)
    }
}

/// One `language_server.exe` as reported by WMI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerProcess {
    pub pid: u32,
    pub cmdline: String,
}

/// Parses `Get-CimInstance … | Select-Object ProcessId, CommandLine | ConvertTo-Json`
/// output. `ConvertTo-Json` returns a bare object for a single match and an array
/// for several, so both shapes are accepted.
pub fn parse_wmi_processes(json: &str) -> Vec<ServerProcess> {
    let Ok(value) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let items: Vec<&Value> = match &value {
        Value::Array(items) => items.iter().collect(),
        Value::Object(_) => vec![&value],
        _ => Vec::new(),
    };
    items
        .into_iter()
        .filter_map(|item| {
            let pid = item.get("ProcessId")?.as_u64()? as u32;
            let cmdline = item
                .get("CommandLine")
                .and_then(|c| c.as_str())
                .unwrap_or_default()
                .to_string();
            Some(ServerProcess { pid, cmdline })
        })
        .collect()
}

/// Value following `--flag` (also accepts `--flag=value`, strips quotes).
pub fn arg_value(cmdline: &str, flag: &str) -> Option<String> {
    let tokens: Vec<&str> = cmdline.split_whitespace().collect();
    let prefixed = format!("{flag}=");
    for (i, token) in tokens.iter().enumerate() {
        if *token == flag {
            return tokens.get(i + 1).map(|v| v.trim_matches('"').to_string());
        }
        if let Some(rest) = token.strip_prefix(&prefixed) {
            return Some(rest.trim_matches('"').to_string());
        }
    }
    None
}

/// Loopback listening ports per PID, from `netstat -ano` output.
pub fn parse_netstat_ports(text: &str) -> HashMap<u32, Vec<u16>> {
    let mut map: HashMap<u32, Vec<u16>> = HashMap::new();
    for line in text.lines() {
        if !line.contains("LISTENING") {
            continue;
        }
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 5 || !cols[1].starts_with("127.0.0.1:") {
            continue;
        }
        let (Some(port), Ok(pid)) = (
            cols[1].rsplit(':').next().and_then(|p| p.parse::<u16>().ok()),
            cols[cols.len() - 1].parse::<u32>(),
        ) else {
            continue;
        };
        let entry = map.entry(pid).or_default();
        if !entry.contains(&port) {
            entry.push(port);
        }
    }
    map
}

/// Every candidate worth probing for one server process, cheapest first:
/// plain HTTP on each listening port (declared `--https_server_port` first),
/// then the same ports over HTTPS.
pub fn candidate_endpoints(
    process: &ServerProcess,
    ports: &HashMap<u32, Vec<u16>>,
) -> Vec<Endpoint> {
    let Some(token) = arg_value(&process.cmdline, "--csrf_token") else {
        return Vec::new();
    };
    if token.is_empty() {
        return Vec::new();
    }

    let declared = arg_value(&process.cmdline, "--https_server_port")
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(0);
    let mut list: Vec<u16> = Vec::new();
    if declared != 0 {
        list.push(declared);
    }
    for port in ports.get(&process.pid).into_iter().flatten() {
        if !list.contains(port) {
            list.push(*port);
        }
    }

    let plain = list.iter().map(|port| Endpoint {
        pid: process.pid,
        port: *port,
        token: token.clone(),
        https: false,
    });
    let secure = list.iter().map(|port| Endpoint {
        pid: process.pid,
        port: *port,
        token: token.clone(),
        https: true,
    });
    plain.chain(secure).collect()
}

/// Spawns an OS command without flashing a console window.
fn quiet_command(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Every running `language_server.exe` (one PowerShell round-trip).
fn list_language_servers() -> Result<Vec<ServerProcess>> {
    // The name is assembled from two literals so the script needs no quotes at
    // all — quoting rules of `powershell.exe -Command` are not worth the risk.
    const SCRIPT: &str = "Get-CimInstance Win32_Process | Where-Object { $_.Name -eq ('language_' + 'server.exe') } | Select-Object ProcessId, CommandLine | ConvertTo-Json -Compress";
    let output = quiet_command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .output()
        .context("failed to run powershell.exe")?;
    if !output.status.success() {
        bail!("powershell.exe exited with {}", output.status);
    }
    Ok(parse_wmi_processes(&String::from_utf8_lossy(&output.stdout)))
}

fn netstat_listening_ports() -> HashMap<u32, Vec<u16>> {
    match quiet_command("netstat.exe").arg("-ano").output() {
        Ok(output) => parse_netstat_ports(&String::from_utf8_lossy(&output.stdout)),
        Err(e) => {
            tracing::debug!(%e, "netstat failed; falling back to the declared port");
            HashMap::new()
        }
    }
}

/// Cascade (conversation) id from an Antigravity renderer URL (`…/c/<uuid>…`).
pub fn cascade_from_url(url: &str) -> Option<String> {
    let rest = url.split("/c/").nth(1)?;
    let id: String = rest
        .chars()
        .take_while(|c| *c != '/' && *c != '?' && *c != '#')
        .collect();
    let looks_like_an_id = id.len() >= 8 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    looks_like_an_id.then_some(id)
}

/// First cascade id in a CDP `/json/list` payload (the open project window).
pub fn parse_cascade_from_targets(json: &str) -> Option<String> {
    let targets: Value = serde_json::from_str(json).ok()?;
    targets.as_array()?.iter().find_map(|target| {
        target
            .get("url")
            .and_then(Value::as_str)
            .and_then(cascade_from_url)
    })
}

/// CDP port of the running app, from `%APPDATA%\Antigravity\DevToolsActivePort`.
fn devtools_port() -> Option<u16> {
    let appdata = std::env::var_os("APPDATA")?;
    let path = std::path::PathBuf::from(appdata)
        .join("Antigravity")
        .join("DevToolsActivePort");
    let text = std::fs::read_to_string(path).ok()?;
    text.lines().next()?.trim().parse::<u16>().ok()
}

/// Cascade id of the conversation currently open in Antigravity.
///
/// The server accepts an empty or invented id *sometimes*; measured on
/// 2026-09-27, a session opened with an empty id can hang without ever sending
/// `ready`. The renderer's own id — visible in the page URL — always worked, so
/// the engine prefers it whenever it can read it.
pub fn discover_cascade_id() -> Option<String> {
    let port = devtools_port()?;
    let url = format!("http://127.0.0.1:{port}/json/list");
    let client = Client::builder()
        .timeout(Duration::from_millis(1_500))
        .build()
        .ok()?;
    let body = client.get(&url).send().ok()?.text().ok()?;
    parse_cascade_from_targets(&body)
}

/// Last-resort id: a well-formed random UUID (never an empty `cascadeId`).
fn fallback_cascade_id() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed) as u64;
    let a = (nanos >> 32) as u32 ^ (n as u32).rotate_left(7);
    let b = (nanos & 0xffff_ffff) as u32;
    let c = (std::process::id() as u64 * 0x9e37_79b9) as u32 ^ n as u32;
    let d = a ^ b.rotate_left(13);
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        a,
        (b >> 16) as u16,
        (b & 0x0fff) as u16,
        (0x8000 | (c & 0x3fff)) as u16,
        d as u64 & 0xffff_ffff_ffff
    )
}

/// Locates the language server of a running Antigravity (process spawn + RPC probe).
pub fn discover_endpoint() -> Result<Endpoint> {
    let client = Client::builder()
        .danger_accept_invalid_certs(true)
        .http1_only()
        .build()
        .unwrap_or_default();
    discover_endpoint_with(&client)
}

/// Same, reusing a caller-provided client (the engine's pooled one).
pub fn discover_endpoint_with(client: &Client) -> Result<Endpoint> {
    let processes = list_language_servers()?;
    if processes.is_empty() {
        bail!("Antigravity is not running (no language_server.exe found)");
    }
    let ports = netstat_listening_ports();

    let mut candidates: Vec<Endpoint> = Vec::new();
    for process in &processes {
        candidates.extend(candidate_endpoints(process, &ports));
    }
    if candidates.is_empty() {
        bail!(
            "{} language_server.exe process(es) found, but no CSRF token + listening port could be read",
            processes.len()
        );
    }

    for candidate in &candidates {
        if candidate.probe(client) {
            tracing::info!(
                pid = candidate.pid,
                port = candidate.port,
                scheme = if candidate.https { "https" } else { "http" },
                "antigravity language server ready"
            );
            return Ok(candidate.clone());
        }
    }
    bail!(
        "{} language server candidate(s) found, but none answered on loopback",
        candidates.len()
    )
}

// --------------------------------------------------------------- framing ----

/// Wraps a JSON value in a gRPC-Web message frame (`flag(1) | len(4 BE) | payload`).
pub fn encode_frame(value: &Value) -> Vec<u8> {
    let payload = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
    let mut out = Vec::with_capacity(payload.len() + 5);
    out.push(0x00);
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(&payload);
    out
}

/// Incremental gRPC-Web frame decoder: transport bytes in, JSON messages out.
#[derive(Default)]
pub struct FrameDecoder {
    buf: Vec<u8>,
}

impl FrameDecoder {
    /// Feeds one socket chunk and returns every message that is now complete.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<Value> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        loop {
            if self.buf.len() < 5 {
                break;
            }
            let flag = self.buf[0];
            let len = u32::from_be_bytes([self.buf[1], self.buf[2], self.buf[3], self.buf[4]]) as usize;
            if self.buf.len() < 5 + len {
                break;
            }
            let payload: Vec<u8> = self.buf[5..5 + len].to_vec();
            self.buf.drain(..5 + len);
            // Flag 0x80 is the trailer (grpc-status as text) — not JSON, not needed.
            if flag == 0x00 {
                if let Ok(message) = serde_json::from_slice::<Value>(&payload) {
                    out.push(message);
                }
            }
        }
        out
    }
}

/// `sessionId` of a `ready` message (tolerates a `{"value":{…}}` wrapper).
pub fn ready_session_id(message: &Value) -> Option<String> {
    let ready = message
        .get("ready")
        .or_else(|| message.get("value")?.get("ready"))?;
    ready
        .get("sessionId")
        .or_else(|| ready.get("session_id"))?
        .as_str()
        .map(str::to_string)
}

/// `(text, is_final)` of a `transcription` message.
pub fn transcription_text(message: &Value) -> Option<(String, bool)> {
    let transcription = message
        .get("transcription")
        .or_else(|| message.get("value")?.get("transcription"))?;
    let text = transcription.get("text")?.as_str()?.to_string();
    let is_final = transcription
        .get("isFinal")
        .or_else(|| transcription.get("is_final"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Some((text, is_final))
}

/// True when the server signalled the end of the session.
pub fn is_complete(message: &Value) -> bool {
    message.get("complete").is_some()
        || message.get("value").and_then(|v| v.get("complete")).is_some()
}

// ----------------------------------------------------------------- audio ----

/// Mono f32 at `sample_rate` → PCM16 LE at 16 kHz (linear resampling when needed).
pub fn to_pcm16k_mono(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let rate = if sample_rate == 0 { 16_000 } else { sample_rate };
    let mut out = Vec::with_capacity(samples.len() * 2);
    if samples.is_empty() {
        return out;
    }
    if rate == 16_000 {
        for sample in samples {
            push_sample(&mut out, *sample);
        }
        return out;
    }

    let target = ((samples.len() as f64) * 16_000.0 / rate as f64).round() as usize;
    let step = rate as f64 / 16_000.0;
    for i in 0..target {
        let src = i as f64 * step;
        let i0 = src.floor() as usize;
        let frac = (src - i0 as f64) as f32;
        let a = samples[i0.min(samples.len() - 1)];
        let b = samples[(i0 + 1).min(samples.len() - 1)];
        push_sample(&mut out, a + (b - a) * frac);
    }
    out
}

fn push_sample(out: &mut Vec<u8>, sample: f32) {
    let value = (sample.clamp(-1.0, 1.0) * 32_767.0).round() as i32;
    out.extend_from_slice(&(value as i16).to_le_bytes());
}

fn request_headers(port: u16, token: &str) -> reqwest::header::HeaderMap {
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
    let mut headers = HeaderMap::new();
    headers.insert(
        reqwest::header::CONTENT_TYPE,
        HeaderValue::from_static("application/grpc-web+json"),
    );
    if let Ok(value) = HeaderValue::from_str(token) {
        headers.insert(HeaderName::from_static("x-codeium-csrf-token"), value);
    }
    if let Ok(value) = HeaderValue::from_str(&format!("https://127.0.0.1:{port}")) {
        headers.insert(reqwest::header::ORIGIN, value);
    }
    headers
}

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
        let stream_budget = Duration::from_secs(
            self.cfg.ready_timeout_secs + self.cfg.finalize_timeout_secs + 10,
        );
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
        if let Err(e) = self.post_rpc(&endpoint, "EndAudioSession", &json!({ "sessionId": session_id }))
        {
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
            let message =
                format!("antigravity returned no transcript ({sequence} chunks sent, complete={})", outcome.complete);
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
    fn frame_layout_matches_gobs_spec() {
        let frame = encode_frame(&json!({"a": 1}));
        assert_eq!(frame[0], 0x00);
        assert_eq!(&frame[1..5], &[0, 0, 0, 7]); // `{"a":1}` is 7 bytes
        assert_eq!(&frame[5..], b"{\"a\":1}");
    }

    #[test]
    fn decoder_reassembles_split_and_multiple_frames() {
        let first = encode_frame(&json!({"ready": {"sessionId": "abc"}}));
        let second = encode_frame(&json!({"complete": {}}));
        let mut stream = first.clone();
        stream.extend_from_slice(&second);

        // Split mid-header, mid-payload, and after the first frame.
        let mut decoder = FrameDecoder::default();
        assert!(decoder.push(&stream[..3]).is_empty());
        assert!(decoder.push(&stream[3..10]).is_empty(), "payload still incomplete");
        let messages = decoder.push(&stream[10..first.len()]);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["ready"]["sessionId"], "abc");
        let rest = decoder.push(&stream[first.len()..]);
        assert_eq!(rest.len(), 1);
        assert!(is_complete(&rest[0]));
    }

    #[test]
    fn decoder_ignores_trailer_frames() {
        let mut decoder = FrameDecoder::default();
        let mut trailer = vec![0x80];
        let text = b"grpc-status: 0\r\n";
        trailer.extend_from_slice(&(text.len() as u32).to_be_bytes());
        trailer.extend_from_slice(text);
        assert!(decoder.push(&trailer).is_empty());
    }

    #[test]
    fn message_parsing_accepts_wrapped_values_and_snake_case() {
        assert_eq!(
            ready_session_id(&json!({"value": {"ready": {"session_id": "s-1"}}})).as_deref(),
            Some("s-1")
        );
        assert_eq!(
            transcription_text(&json!({"value": {"transcription": {"text": "سلام", "is_final": true}}})),
            Some(("سلام".to_string(), true))
        );
        assert_eq!(
            transcription_text(&json!({"transcription": {"text": "خب", "isFinal": false}})),
            Some(("خب".to_string(), false))
        );
        assert!(is_complete(&json!({"value": {"complete": {}}})));
    }

    #[test]
    fn pcm_is_two_bytes_per_sample_at_16k() {
        let pcm = to_pcm16k_mono(&[0.0, 1.0, -1.0], 16_000);
        assert_eq!(pcm.len(), 6);
        assert_eq!(i16::from_le_bytes([pcm[2], pcm[3]]), 32_767);
        assert_eq!(i16::from_le_bytes([pcm[4], pcm[5]]), -32_767);
    }

    #[test]
    fn pcm_resamples_8k_to_16k() {
        let samples: Vec<f32> = (0..8_000)
            .map(|i| (i as f32 * 0.01).sin() * 0.5)
            .collect();
        let pcm = to_pcm16k_mono(&samples, 8_000);
        assert_eq!(pcm.len(), 32_000); // 16 000 samples × 2 bytes
    }

    #[test]
    fn chunk_size_is_forty_milliseconds() {
        assert_eq!(CHUNK_BYTES, 16_000 * 2 * 40 / 1_000);
        let pcm = vec![0u8; CHUNK_BYTES * 2 + 100];
        let chunks: Vec<usize> = pcm.chunks(CHUNK_BYTES).map(<[u8]>::len).collect();
        assert_eq!(chunks, vec![CHUNK_BYTES, CHUNK_BYTES, 100]);
    }

    #[test]
    fn wmi_output_parses_both_shapes() {
        let single = r#"{"ProcessId":42,"CommandLine":"C:\\x\\language_server.exe --csrf_token tok --https_server_port 0"}"#;
        let many = r#"[{"ProcessId":1,"CommandLine":"a"},{"ProcessId":2,"CommandLine":null}]"#;
        assert_eq!(parse_wmi_processes(single).len(), 1);
        let parsed = parse_wmi_processes(many);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].cmdline, "");
        assert!(parse_wmi_processes("not json").is_empty());
    }

    #[test]
    fn arg_value_handles_spaces_equals_and_quotes() {
        let cmdline = r#""C:\A B\language_server.exe" --standalone --csrf_token "abc-123" --https_server_port 0"#;
        assert_eq!(arg_value(cmdline, "--csrf_token").as_deref(), Some("abc-123"));
        assert_eq!(arg_value(cmdline, "--https_server_port").as_deref(), Some("0"));
        assert_eq!(arg_value("--csrf_token=xyz", "--csrf_token").as_deref(), Some("xyz"));
        assert_eq!(arg_value(cmdline, "--missing"), None);
    }

    #[test]
    fn netstat_output_maps_ports_to_pids() {
        let text = "\r\n  TCP    127.0.0.1:1061         0.0.0.0:0              LISTENING       22068\r\n  TCP    127.0.0.1:1062         0.0.0.0:0              LISTENING       22068\r\n  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1088\r\n  TCP    127.0.0.1:1061         127.0.0.1:10994        ESTABLISHED     22068\r\n";
        let ports = parse_netstat_ports(text);
        assert_eq!(ports.get(&22068), Some(&vec![1061, 1062]));
        assert_eq!(ports.get(&1088), None, "non-loopback listeners are ignored");
    }

    #[test]
    fn candidates_prefer_plain_http_and_the_declared_port() {
        let declared = ServerProcess {
            pid: 7,
            cmdline: "language_server.exe --csrf_token tok --https_server_port 2182".into(),
        };
        let dynamic = ServerProcess {
            pid: 8,
            cmdline: "language_server.exe --csrf_token tok2 --https_server_port 0".into(),
        };
        let mut ports = HashMap::new();
        ports.insert(7u32, vec![1061u16, 1062u16]);
        ports.insert(8u32, vec![1061u16]);

        let candidates = candidate_endpoints(&declared, &ports);
        let shape: Vec<(u16, &str)> = candidates
            .iter()
            .map(|c| (c.port, if c.https { "https" } else { "http" }))
            .collect();
        assert_eq!(
            shape,
            vec![
                (2182, "http"),
                (1061, "http"),
                (1062, "http"),
                (2182, "https"),
                (1061, "https"),
                (1062, "https"),
            ],
            "plain HTTP first (no TLS stall), declared port before discovered ones"
        );
        assert_eq!(
            candidates[0].base(),
            format!("http://127.0.0.1:2182/{RPC_ROOT}")
        );
        assert_eq!(
            candidates[3].base(),
            format!("https://127.0.0.1:2182/{RPC_ROOT}")
        );

        let dynamic_candidates = candidate_endpoints(&dynamic, &ports);
        assert_eq!(dynamic_candidates.first().map(|c| c.port), Some(1061));

        let tokenless = ServerProcess {
            pid: 9,
            cmdline: "language_server.exe --standalone".into(),
        };
        assert!(candidate_endpoints(&tokenless, &ports).is_empty());
        assert!(candidate_endpoints(&dynamic, &HashMap::new()).is_empty());
    }

    #[test]
    fn cascade_is_read_from_renderer_urls() {
        assert_eq!(
            cascade_from_url("https://127.0.0.1:1061/c/b4c34916-3a7f-4fd8-be8e-6261ad0a20ad?section=abc")
                .as_deref(),
            Some("b4c34916-3a7f-4fd8-be8e-6261ad0a20ad")
        );
        assert_eq!(cascade_from_url("https://127.0.0.1:1061/onboarding"), None);
        assert_eq!(cascade_from_url("https://127.0.0.1:1061/c/short"), None);
        assert_eq!(cascade_from_url("https://127.0.0.1:1061/c/"), None);
    }

    #[test]
    fn cascade_is_picked_from_a_cdp_target_list() {
        let payload = r#"[{"url":"https://127.0.0.1:1061/onboarding"},{"url":"https://127.0.0.1:1061/c/0c135ef3-67fe-446a-893f-c644ef800649?section=x"}]"#;
        assert_eq!(
            parse_cascade_from_targets(payload).as_deref(),
            Some("0c135ef3-67fe-446a-893f-c644ef800649")
        );
        assert_eq!(parse_cascade_from_targets("[]"), None);
        assert_eq!(parse_cascade_from_targets("not json"), None);
    }

    #[test]
    fn fallback_cascade_ids_are_unique_and_uuid_shaped() {
        let first = fallback_cascade_id();
        let second = fallback_cascade_id();
        assert_ne!(first, second, "two sessions must not share an id");
        let groups: Vec<usize> = first.split('-').map(str::len).collect();
        assert_eq!(groups, vec![8, 4, 4, 4, 12]);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
        assert_eq!(&first[14..15], "4", "version nibble");
    }

    #[test]
    fn request_headers_carry_the_csrf_token() {
        let headers = request_headers(1061, "tok-1");
        assert_eq!(headers["x-codeium-csrf-token"], "tok-1");
        assert_eq!(headers["content-type"], "application/grpc-web+json");
        assert_eq!(headers["origin"], "https://127.0.0.1:1061");
    }

    #[test]
    fn engine_health_starts_unprobed_then_follows_config() {
        let engine = AntigravityEngine::new(AntigravityConfig::default());
        assert!(!engine.health().is_available(), "not probed yet");
        assert!(engine.transcribe(&AudioUtterance { samples: Vec::new(), sample_rate: 16_000 }).is_err());

        let disabled = AntigravityEngine::new(AntigravityConfig {
            enabled: false,
            ..AntigravityConfig::default()
        });
        assert!(matches!(disabled.health(), AsrHealth::Failed { .. }));
    }
}
