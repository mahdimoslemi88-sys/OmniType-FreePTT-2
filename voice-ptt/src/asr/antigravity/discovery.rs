//! Finding a running Antigravity language server, and the conversation id to
//! talk to it with.
//!
//! This used to be the first ~320 lines of `antigravity/mod.rs`, where it sat
//! directly above the engine that consumes its output. The split follows the
//! same line as `protocol`: everything here answers "what is the address?"
//! from text and subprocesses, and nothing here touches the audio pipeline.
//! That is why all seven tests can run without Antigravity installed.
//!
//! Two facts drive the whole module, both measured rather than assumed:
//!
//! * the server listens **twice** — HTTPS on one loopback port and plain HTTP
//!   on another (an HTTP request to the HTTPS port is answered with 400), and
//!   the native-TLS handshake to the self-signed local certificate costs ~14 s
//!   on this machine while HTTP answers in ~1 ms;
//! * an empty or invented `cascadeId` is accepted *sometimes*; a session opened
//!   with one can hang without ever sending `ready`, so the renderer's own id
//!   (read out of its page URL) is preferred whenever it can be read.

use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde_json::{json, Value};

use super::protocol::{encode_frame, request_headers};

/// Service root on the local server.
const RPC_ROOT: &str = "exa.language_server_pb.LanguageServerService";

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
    /// The RPC root URL for this candidate. `pub(super)` because the engine
    /// posts its own methods under the same root.
    pub(super) fn base(&self) -> String {
        let scheme = if self.https { "https" } else { "http" };
        format!("{scheme}://127.0.0.1:{}/{RPC_ROOT}", self.port)
    }

    /// Cheap RPC used to tell a usable candidate from a wrong scheme/port.
    fn probe(&self, client: &Client) -> bool {
        matches!(self.probe_status(client), Some(200))
    }

    fn probe_status(&self, client: &Client) -> Option<u16> {
        // `{}` is not the real request body, but the point is the transport:
        // a working candidate answers 200, the HTTPS port rejects plain HTTP
        // with 400, and a dead port fails the connection.
        let response = client
            .post(format!("{}/GetMendelFlags", self.base()))
            .headers(request_headers(self.port, &self.token))
            .timeout(probe_timeout(self.https))
            .body(encode_frame(&json!({})))
            .send()
            .ok()?;
        let status = response.status().as_u16();
        let _ = response.text();
        Some(status)
    }
}

/// How long a candidate may take to answer the probe.
///
/// The asymmetry is the whole point: HTTPS costs a full native-TLS handshake
/// against a self-signed local certificate (~14 s measured), plain HTTP answers
/// in ~1 ms. Probing the HTTPS ports with a 3 s budget would declare a working
/// server dead; probing the HTTP ports with 20 s would stall the whole discovery
/// on the first wrong port.
pub fn probe_timeout(https: bool) -> Duration {
    Duration::from_secs(if https { 20 } else { 3 })
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
            cols[1]
                .rsplit(':')
                .next()
                .and_then(|p| p.parse::<u16>().ok()),
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
        // Port 0 means "not picked yet" in the declared flag and is never a
        // listening port at all, so it must not become a probe target. The
        // declared flag was already guarded above; this guards the discovered
        // side too, so the invariant holds by construction instead of relying
        // on `netstat` never printing such a line.
        if *port == 0 || list.contains(port) {
            continue;
        }
        list.push(*port);
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
    Ok(parse_wmi_processes(&String::from_utf8_lossy(
        &output.stdout,
    )))
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
    let looks_like_an_id =
        id.len() >= 8 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
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
pub fn fallback_cascade_id() -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
            arg_value(cmdline, "--csrf_token").as_deref(),
            Some("abc-123")
        );
        assert_eq!(
            arg_value(cmdline, "--https_server_port").as_deref(),
            Some("0")
        );
        assert_eq!(
            arg_value("--csrf_token=xyz", "--csrf_token").as_deref(),
            Some("xyz")
        );
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

    /// A declared port of `0` means "I have not picked one yet" — it must not
    /// become candidate port 0, which nothing listens on.
    #[test]
    fn a_declared_port_of_zero_is_not_a_port() {
        let process = ServerProcess {
            pid: 3,
            cmdline: "language_server.exe --csrf_token tok --https_server_port 0".into(),
        };
        let mut ports = HashMap::new();
        ports.insert(3u32, vec![0u16, 1061u16]);
        let candidates = candidate_endpoints(&process, &ports);
        assert!(
            candidates.iter().all(|c| c.port != 0),
            "port 0 must never be probed, got {candidates:?}"
        );
    }

    /// The 20 s/3 s split is the measured workaround for the ~14 s
    /// native-TLS handshake against the self-signed local certificate
    /// (`docs/MEASURED-FACTS.md`). Collapsing it either way reintroduces a
    /// 14-second stall or declares a live HTTPS server dead.
    #[test]
    fn the_https_probe_gets_the_long_budget() {
        assert_eq!(probe_timeout(false), Duration::from_secs(3));
        assert_eq!(probe_timeout(true), Duration::from_secs(20));
        assert!(
            probe_timeout(true) > probe_timeout(false) * 4,
            "the asymmetry must stay wide enough to cover the handshake"
        );
    }

    #[test]
    fn cascade_is_read_from_renderer_urls() {
        assert_eq!(
            cascade_from_url(
                "https://127.0.0.1:1061/c/b4c34916-3a7f-4fd8-be8e-6261ad0a20ad?section=abc"
            )
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
}
