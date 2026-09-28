#!/usr/bin/env node
/**
 * ag-live-probe.mjs — talk to Antigravity's local language server from outside
 * the app, the way the app itself does (measured in Antigravity-Log-Analysis-02.md).
 *
 *   1. find every `language_server.exe` (PID), read its command line to get
 *      `--csrf_token` (the token the bundle sends as `x-codeium-csrf-token`),
 *   2. find the loopback port it actually listens on (`--https_server_port` may be 0),
 *   3. POST `StreamAudioTranscription` as a gRPC-Web framed JSON request, read the
 *      framed response stream (`ready` → `transcription`* → `complete`),
 *   4. push PCM16/16kHz chunks via `SendAudioChunk` and finish with `EndAudioSession`.
 *
 * This is the reference implementation the Rust engine mirrors — and the tool that
 * fills the last hole in the research: the real `SendAudioChunk` body schema.
 *
 * Usage:
 *   node probes/ag-live-probe.mjs --dry                      # discovery only, no requests
 *   node probes/ag-live-probe.mjs --cascade <id>             # id from /c/<id> in the app URL
 *   node probes/ag-live-probe.mjs --cascade <id> --seconds 6 --tone 440
 *   node probes/ag-live-probe.mjs --cascade <id> --submit-key data   # try another field name
 */
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const args = process.argv.slice(2);
const opt = { cascade: null, seconds: 6, tone: 0, dry: false, submitKey: 'data', say: null, wav: null, keepOpenMs: 4000, port: null, http: false };
for (let i = 0; i < args.length; i++) {
  const a = args[i];
  if (a === '--cascade') opt.cascade = args[++i];
  else if (a === '--port') opt.port = Number(args[++i]);
  else if (a === '--http') opt.http = true;
  else if (a === '--seconds') opt.seconds = Number(args[++i]);
  else if (a === '--tone') opt.tone = Number(args[++i]);
  else if (a === '--dry') opt.dry = true;
  else if (a === '--say') opt.say = args[++i];
  else if (a === '--wav') opt.wav = args[++i];
  else if (a === '--keep-open') opt.keepOpenMs = Number(args[++i]);
  else if (a === '--submit-key') opt.submitKey = args[++i];
  else if (a === '-h' || a === '--help') {
    console.log('usage: node ag-live-probe.mjs [--dry] [--cascade <id>] [--seconds N] [--tone Hz] [--say "text"] [--wav file.wav] [--submit-key data]');
    process.exit(0);
  }
}

const log = (...m) => console.log(...m);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ------------------------------------------------------------- discovery ----
function ps(command) {
  return execFileSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', command], {
    encoding: 'utf8',
    maxBuffer: 4 * 1024 * 1024,
  });
}

/** Every live language_server.exe with its command line, token and listening ports. */
function discover() {
  let csv = '';
  try {
    csv = execFileSync('tasklist.exe', ['/FI', 'IMAGENAME eq language_server.exe', '/FO', 'CSV', '/NH'], { encoding: 'utf8' });
  } catch (e) {
    console.error('tasklist failed:', e.message);
  }
  const pids = csv
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l.startsWith('"'))
    .map((l) => l.split('","')[1])
    .filter(Boolean);

  if (!pids.length) return [];

  const cmdlines = ps(
    pids.map((p) => `(Get-CimInstance Win32_Process -Filter "ProcessId=${p}").CommandLine`).join('; '),
  ).split(/\r?\n/);

  return pids.map((pid, i) => {
    const cmdline = cmdlines[i] || '';
    const token = (cmdline.match(/--csrf_token\s+(\S+)/) || [])[1] || null;
    const declared = (cmdline.match(/--https_server_port\s+(\d+)/) || [])[1] || null;
    let ports = [];
    try {
      ports = execFileSync('netstat.exe', ['-ano'], { encoding: 'utf8' })
        .split(/\r?\n/)
        .filter((l) => l.includes('LISTENING') && new RegExp(`\\s${pid}\\s*$`).test(l))
        .map((l) => (l.match(/127\.0\.0\.1:(\d+)/) || [])[1])
        .filter(Boolean);
    } catch {}
    return { pid, token, declaredPort: declared, ports, cmdline: cmdline.slice(0, 120) };
  });
}

// -------------------------------------------------------------- framing ----
/** gRPC-Web message frame: 1 flag byte (0) + 4 byte big-endian length + payload. */
function frame(obj) {
  const payload = Buffer.from(JSON.stringify(obj), 'utf8');
  const head = Buffer.alloc(5);
  head[0] = 0x00;
  head.writeUInt32BE(payload.length, 1);
  return Buffer.concat([head, payload]);
}

/** Incremental frame splitter: feed it buffers, get back complete JSON messages. */
function makeFrameReader(onMessage, onTrailer) {
  let buf = Buffer.alloc(0);
  return (chunk) => {
    buf = Buffer.concat([buf, chunk]);
    while (buf.length >= 5) {
      const flag = buf[0];
      const len = buf.readUInt32BE(1);
      if (buf.length < 5 + len) return;
      const payload = buf.subarray(5, 5 + len);
      buf = buf.subarray(5 + len);
      const text = payload.toString('utf8');
      if (flag === 0x80) onTrailer(text);
      else onMessage(text);
    }
  };
}

// ---------------------------------------------------------------- audio ----
// Windows' built-in TTS gives us real speech without a microphone — the only way
// to prove the whole chain (our chunks → cloud ASR → text) from outside the app.
function synthesize(text, outFile) {
  const script = [
    'Add-Type -AssemblyName System.Speech',
    '$s = New-Object System.Speech.Synthesis.SpeechSynthesizer',
    '$fmt = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(16000, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono)',
    `$s.SetOutputToWaveFile('${outFile.replace(/'/g, "''")}', $fmt)`,
    `$s.Speak('${text.replace(/'/g, "''")}')`,
    '$s.Dispose()',
  ].join('; ');
  ps(script);
  return outFile;
}

/** Parse a PCM WAV and resample to mono PCM16 @ 16 kHz (linear interpolation). */
function wavToPcm16k(file) {
  const buf = readFileSync(file);
  if (buf.toString('ascii', 0, 4) !== 'RIFF') throw new Error('not a RIFF/WAV file');
  let pos = 12, fmt = null, data = null;
  while (pos + 8 <= buf.length) {
    const id = buf.toString('ascii', pos, pos + 4);
    const size = buf.readUInt32LE(pos + 4);
    const body = buf.subarray(pos + 8, pos + 8 + size);
    if (id === 'fmt ') {
      fmt = { format: body.readUInt16LE(0), channels: body.readUInt16LE(2), rate: body.readUInt32LE(4), bits: body.readUInt16LE(14) };
    } else if (id === 'data') data = body;
    pos += 8 + size + (size % 2);
  }
  if (!fmt || !data) throw new Error('WAV without fmt/data chunk');
  if (fmt.format !== 1 || fmt.bits !== 16) throw new Error(`unsupported WAV (format=${fmt.format} bits=${fmt.bits})`);

  const frames = Math.floor(data.length / (2 * fmt.channels));
  const mono = new Float32Array(frames);
  for (let i = 0; i < frames; i++) {
    let acc = 0;
    for (let c = 0; c < fmt.channels; c++) acc += data.readInt16LE((i * fmt.channels + c) * 2);
    mono[i] = acc / fmt.channels;
  }
  const ratio = fmt.rate / 16000;
  const outFrames = Math.floor(frames / ratio);
  const out = Buffer.alloc(outFrames * 2);
  for (let i = 0; i < outFrames; i++) {
    const src = i * ratio;
    const i0 = Math.floor(src);
    const i1 = Math.min(i0 + 1, frames - 1);
    const v = mono[i0] + (mono[i1] - mono[i0]) * (src - i0);
    out.writeInt16LE(Math.max(-32768, Math.min(32767, Math.round(v))), i * 2);
  }
  return { pcm: out, rate: fmt.rate, seconds: outFrames / 16000 };
}

/** PCM16 mono 16 kHz, `ms` milliseconds — a tone (to prove `data` is real audio) or silence. */
function pcmChunk(ms, rate = 16000, toneHz = 0, phaseRef = { v: 0 }) {
  const n = Math.round((rate * ms) / 1000);
  const out = Buffer.alloc(n * 2);
  for (let i = 0; i < n; i++) {
    const s = toneHz ? Math.round(0.25 * 32767 * Math.sin(phaseRef.v)) : 0;
    if (toneHz) phaseRef.v += (2 * Math.PI * toneHz) / rate;
    out.writeInt16LE(s, i * 2);
  }
  return out;
}

// ----------------------------------------------------------------- main ----
const targets = discover();
log(`language_server.exe instances: ${targets.length}`);
for (const t of targets) {
  log(`  pid=${t.pid} declaredPort=${t.declaredPort} listening=[${t.ports.join(', ')}] token=${t.token ? t.token.slice(0, 8) + '…' : 'MISSING'}`);
}
if (opt.dry) process.exit(0);

const target = targets.find((t) => t.token && t.ports.length) || targets.find((t) => t.ports.length);
if (!target) {
  console.error('no usable language server found (running Antigravity with a project open is required)');
  process.exit(2);
}
// The same service listens twice: HTTPS on one port and plain HTTP on the other
// (plain HTTP on the HTTPS port answers 400). HTTP is ~40x faster to open here
// because the native-TLS handshake costs ~14 s on this box.
const scheme = opt.http ? 'http' : 'https';
const port = opt.port || target.ports[0];
const base = `${scheme}://127.0.0.1:${port}/exa.language_server_pb.LanguageServerService`;
log(`using pid=${target.pid} ${scheme} port=${port} (listening: [${target.ports.join(', ')}])`);

const headers = {
  'content-type': 'application/grpc-web+json',
  'x-codeium-csrf-token': target.token || '',
  origin: `https://127.0.0.1:${target.ports[0]}`,
};

// Local TLS certificate is self-signed; the app trusts it because it is the app.
process.env.NODE_TLS_REJECT_UNAUTHORIZED = '0';

async function post(name, body) {
  const res = await fetch(`${base}/${name}`, { method: 'POST', headers, body });
  const text = await res.text().catch(() => '');
  return { status: res.status, text: text.slice(0, 300) };
}

// ---------------------------------------------------------------- stream ----
let sessionId = null;
const transcript = [];
const frames = [];
const controller = new AbortController();

const streamRes = await fetch(`${base}/StreamAudioTranscription`, {
  method: 'POST',
  headers,
  body: frame({ mimeType: 'audio/pcm;rate=16000', cascadeId: opt.cascade || '' }),
  signal: controller.signal,
});
log(`StreamAudioTranscription -> HTTP ${streamRes.status}`);
if (!streamRes.ok) {
  log('body:', (await streamRes.text()).slice(0, 400));
  process.exit(3);
}

const reader = streamRes.body.getReader();
const feed = makeFrameReader(
  (text) => {
    frames.push(text);
    log(`  frame: ${text.slice(0, 300)}`);
    try {
      const obj = JSON.parse(text);
      const ready = obj.ready || obj.value?.ready;
      const sid = ready?.sessionId || ready?.session_id;
      if (sid) sessionId = sid;
      const tr = obj.transcription || obj.value?.transcription;
      if (tr && (tr.text || tr.isFinal !== undefined)) transcript.push(tr);
    } catch {}
  },
  (text) => log(`  trailer: ${text.slice(0, 200)}`),
);

const pump = (async () => {
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      feed(Buffer.from(value));
    }
  } catch (e) {
    log(`  stream ended: ${e.name}: ${e.message}`);
  }
})();

// Wait for `ready` (this is the gate the app itself waits on: ~12-14 s on this box).
const waitStart = Date.now();
while (!sessionId && Date.now() - waitStart < 30_000) await sleep(200);
log(sessionId ? `ready after ${Date.now() - waitStart} ms — sessionId=${sessionId}` : 'no ready within 30 s');

if (sessionId) {
  // Source of the 40 ms chunks: synthesized speech (proves the chain end-to-end),
  // a sine tone, or silence.
  let speech = null;
  if (opt.say || opt.wav) {
    const wav = opt.wav || synthesize(opt.say, join(tmpdir(), `ag-probe-${Date.now()}.wav`));
    speech = wavToPcm16k(wav);
    log(`audio source: ${wav} — ${speech.seconds.toFixed(1)}s @ ${speech.rate}Hz -> 16kHz PCM16`);
  }
  const phase = { v: 0 };
  const pending = [];
  const statuses = new Map();
  const total = speech ? Math.ceil(speech.pcm.length / 1280) : Math.round((opt.seconds * 1000) / 40);
  for (let seq = 0; seq < total; seq++) {
    const payload16 = speech
      ? speech.pcm.subarray(seq * 1280, seq * 1280 + 1280)
      : pcmChunk(40, 16000, opt.tone, phase);
    const body = frame({
      sessionId,
      [opt.submitKey]: payload16.toString('base64'),
      sequenceNumber: seq,
    });
    pending.push(
      post('SendAudioChunk', body).then((r) => {
        statuses.set(r.status, (statuses.get(r.status) || 0) + 1);
        if (r.status !== 200) log(`  chunk #${seq} -> HTTP ${r.status}: ${r.text}`);
        return r;
      }),
    );
    await sleep(40);
  }
  await Promise.all(pending);
  log(`sent ${total} chunks (40 ms each, field="${opt.submitKey}") — status tally: ${[...statuses].map(([s, n]) => `${s}x${n}`).join(', ')}`);
  await sleep(1200);
  log('EndAudioSession ->', JSON.stringify(await post('EndAudioSession', frame({ sessionId }))));
  await sleep(opt.keepOpenMs);
}

controller.abort();
await pump;

log('');
log(`frames received: ${frames.length}`);
for (const t of transcript.slice(-5)) log(`  transcript: ${JSON.stringify(t).slice(0, 200)}`);
const finalText = transcript.filter((t) => t.isFinal).map((t) => t.text).filter(Boolean).join(' ');
log(`final text: ${finalText ? JSON.stringify(finalText) : '(none)'}`);
