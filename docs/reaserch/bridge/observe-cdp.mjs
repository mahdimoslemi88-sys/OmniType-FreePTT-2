#!/usr/bin/env node
/**
 * observe-cdp.mjs - zero-dependency Chrome DevTools Protocol observer.
 *
 * Attaches to a running Chromium/Electron app's debug port and records what it
 * does while it turns speech into text:
 *   - every outbound network request (so we can see WHERE audio is sent)
 *   - WebSocket creation + frame sizes (streaming ASR shows up as a live socket)
 *   - console output
 *   - a periodic snapshot of the focused element / visible text (the transcript)
 *
 * No npm install, no dependencies: uses Node's global fetch + WebSocket (Node 21+).
 *
 * Usage:
 *   node observe-cdp.mjs --app antigravity --duration 120
 *   node observe-cdp.mjs --port 9334 --duration 120          # Gemini/ChatGPT relaunched with a flag
 *   node observe-cdp.mjs --app antigravity --once --expr "document.title"
 *
 * Read-only by design: it never injects input and never drives the app.
 */

import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));

// ------------------------------------------------------------------ args -----
function parseArgs(argv) {
  const out = {
    app: 'antigravity', host: '127.0.0.1', port: 0, duration: 0,
    poll: 1000, out: path.join(HERE, 'logs'), once: false, expr: null, frames: true,
    hook: null, binding: '__omniTypeCapture', target: null, list: false,
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const next = () => argv[++i];
    if (a === '--app') out.app = next();
    else if (a === '--host') out.host = next();
    else if (a === '--port') out.port = Number(next());
    else if (a === '--duration') out.duration = Number(next());
    else if (a === '--poll') out.poll = Number(next());
    else if (a === '--out') out.out = next();
    else if (a === '--expr') out.expr = next();
    else if (a === '--expr-file') out.exprFile = next();
    else if (a === '--once') out.once = true;
    else if (a === '--hook') out.hook = next();
    else if (a === '--target') out.target = next();
    else if (a === '--list') out.list = true;
    else if (a === '--binding') out.binding = next();
    else if (a === '--no-frames') out.frames = false;
    else if (a === '-h' || a === '--help') { out.help = true; }
  }
  return out;
}

const args = parseArgs(process.argv.slice(2));
if (args.help) {
  console.log(`observe-cdp.mjs - record how a Chromium app does speech-to-text

  --app <name>     antigravity | gemini   (finds the debug port automatically)
  --port <n>       explicit debug port (overrides --app)
  --host <h>       default 127.0.0.1
  --duration <s>   auto-stop after N seconds (0 = until Ctrl+C)
  --poll <ms>      text-sample period (default 1000)
  --out <dir>      log directory (default ./logs)
  --once           attach, evaluate --expr, print result, exit
  --expr <js>      expression used by --once
  --expr-file <f>  read the --once expression from a file (easier for long JS)
  --no-frames      do not log WebSocket frame metadata
  --hook <file>    inject JS (e.g. probes/asr-fetch-hook.js) into the page first,
                   to capture the EXACT request/response bytes of the
                   language-server audio RPCs (protobuf bodies as base64)
  --binding <n>    binding name for --hook (default __omniTypeCapture)
  --target <text>  attach to the page whose title/url contains <text>
  --list           print the available page targets and exit`);
  process.exit(0);
}

// -------------------------------------------------------------- discovery ----
function userDataDirs(app) {
  const roaming = process.env.APPDATA || path.join(os.homedir(), 'AppData', 'Roaming');
  switch (app) {
    case 'antigravity': return [path.join(roaming, 'Antigravity')];
    case 'gemini':      return [path.join(roaming, 'Gemini')];
    case 'chatgpt':     return [path.join(roaming, 'ChatGPT'), path.join(roaming, 'Codex')];
    default:            return [];
  }
}

function findPort() {
  if (args.port) return args.port;
  for (const dir of userDataDirs(args.app)) {
    const f = path.join(dir, 'DevToolsActivePort');
    try {
      const first = fs.readFileSync(f, 'utf8').split(/\r?\n/)[0].trim();
      const p = Number(first);
      if (p > 0) return p;
    } catch { /* not found */ }
  }
  return 0;
}

async function listTargets(port) {
  const res = await fetch(`http://${args.host}:${port}/json/list`);
  const list = await res.json();
  return list.filter((t) => t.type === 'page' && t.webSocketDebuggerUrl);
}

/**
 * Pick the window that actually has the editor + mic button. Antigravity keeps
 * several page targets around (onboarding, login, devtools) and the RPCs only
 * ever come from a conversation window, whose URL looks like /c/<uuid>.
 */
async function pickTarget(port) {
  const pages = await listTargets(port);
  if (!pages.length) throw new Error('no page target found');

  if (args.target) {
    const hit = pages.find((t) => `${t.title || ''} ${t.url || ''}`.toLowerCase().includes(args.target.toLowerCase()));
    if (!hit) {
      throw new Error(`no page target matching "${args.target}". Available:\n` +
        pages.map((t) => `  - ${t.title}  [${t.url}]`).join('\n'));
    }
    return hit;
  }

  const score = (t) => {
    let s = 0;
    if (/\/c\//.test(t.url || '')) s += 10;                 // conversation window: has the mic
    if (/onboarding|login|signin/i.test(t.url || '')) s -= 10;
    if (/^devtools:|^chrome-extension:/i.test(t.url || '')) s -= 100;
    return s;
  };
  return pages.slice().sort((a, b) => score(b) - score(a))[0];
}

// ---------------------------------------------------------------- --list -----
// Handled before any log file is created, so listing never leaves stray logs.
if (args.list) {
  const port = findPort();
  if (port) {
    const pages = await listTargets(port);
    console.log(`port ${port} — ${pages.length} page target(s):`);
    for (const t of pages) console.log(`  ${t.title}  [${t.url}]`);
  } else {
    console.log('no debug port found');
  }
  process.exit(0);
}

// --------------------------------------------------------------- log io ------
fs.mkdirSync(args.out, { recursive: true });
const stamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
const logPath = path.join(args.out, `cdp-${args.app}-${stamp}.jsonl`);
const stream = fs.createWriteStream(logPath, { flags: 'a' });
const summary = { requests: new Map(), websockets: new Map(), console: [], texts: [], rpcs: new Map(), started: Date.now() };

function log(obj) {
  stream.write(JSON.stringify({ ts: new Date().toISOString(), ...obj }) + '\n');
}

function hr(ms) { return new Date(ms).toISOString().slice(11, 23); }

function record(obj) {
  log(obj);
  switch (obj.kind) {
    case 'target':   console.log(`${hr(Date.now())}  attached -> ${obj.title}  [${obj.url}]`); break;
    case 'request': {
      const key = obj.host || '(data)';
      const bucket = summary.requests.get(key) || { count: 0, paths: new Set(), types: new Set() };
      bucket.count++; bucket.paths.add(obj.path); bucket.types.add(obj.type);
      summary.requests.set(key, bucket);
      if (obj.type !== 'data' && !/\.(png|jpg|svg|woff2?|css|ico)$/i.test(obj.path)) {
        console.log(`${hr(Date.now())}  NET  ${obj.method} ${obj.host}${obj.path}  [${obj.type}]`);
      }
      break;
    }
    case 'ws_created': {
      summary.websockets.set(obj.url, { frames: 0, inBytes: 0, outBytes: 0 });
      console.log(`${hr(Date.now())}  WS   + ${obj.url}`);
      break;
    }
    case 'ws_closed': console.log(`${hr(Date.now())}  WS   - ${obj.url}`); break;
    case 'console':   summary.console.push(obj.text); console.log(`${hr(Date.now())}  LOG  ${obj.text}`); break;
    case 'text':
      summary.texts.push(obj.text);
      console.log(`${hr(Date.now())}  TEXT ${JSON.stringify(obj.text).slice(0, 300)}`);
      break;
    case 'rpc': {
      const label = `${obj.dir} ${obj.rpc}`;
      const b = summary.rpcs.get(label) || { count: 0, bytes: 0 };
      b.count++; b.bytes += obj.len || 0;
      summary.rpcs.set(label, b);
      // SendAudioChunk arrives ~25x/s: log it to the JSONL but keep stdout readable.
      if (obj.dir === 'res' || obj.rpc !== 'SendAudioChunk') {
        const head = obj.body ? String(obj.body).slice(0, 120) : '';
        console.log(`${hr(Date.now())}  RPC  ${obj.dir === 'res' ? '<-' : '->'} ${obj.rpc}  ${obj.mime || ''} ${obj.len ?? ''}B  ${head}`);
      }
      break;
    }
    case 'error':     console.log(`${hr(Date.now())}  ERR  ${obj.message}`); break;
    case 'hook':      console.log(`${hr(Date.now())}  HOOK ${obj.result}`); break;
  }
}

// ------------------------------------------------------------------ CDP ------
const ws = await (async () => {
  const port = findPort();
  if (!port) {
    console.error(`Could not find a debug port for "${args.app}".`);
    console.error(`If the app is not already listening, restart it with --remote-debugging-port=<port> and pass --port.`);
    process.exit(1);
  }
  const target = await pickTarget(port);
  const sock = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    sock.addEventListener('open', resolve, { once: true });
    sock.addEventListener('error', (e) => reject(new Error('websocket error: ' + (e.message || 'failed'))), { once: true });
  });
  sock.target = target;
  sock.port = port;
  return sock;
})();

let nextId = 1;
const pending = new Map();

function send(method, params = {}) {
  const id = nextId++;
  ws.send(JSON.stringify({ id, method, params }));
  return new Promise((resolve) => pending.set(id, resolve));
}

function hostOf(url) { try { return new URL(url).host; } catch { return '(unknown)'; } }
function pathOf(url) { try { return new URL(url).pathname + (new URL(url).search || ''); } catch { return url; } }

ws.addEventListener('message', (ev) => {
  let msg;
  try { msg = JSON.parse(typeof ev.data === 'string' ? ev.data : ev.data.toString()); } catch { return; }
  if (msg.id && pending.has(msg.id)) { pending.get(msg.id)(msg.result); pending.delete(msg.id); return; }
  if (!msg.method) return;

  const p = msg.params || {};
  switch (msg.method) {
    case 'Network.requestWillBeSent': {
      const r = p.request || {};
      record({ kind: 'request', method: r.method, url: r.url, host: hostOf(r.url), path: pathOf(r.url), type: p.type || 'Other' });
      break;
    }
    case 'Network.webSocketCreated':
      record({ kind: 'ws_created', url: p.url });
      break;
    case 'Network.webSocketClosed': {
      const b = summary.websockets.get(p.url);
      if (b) delete summary.websockets[p.url];
      record({ kind: 'ws_closed', url: p.url });
      break;
    }
    case 'Network.webSocketFrameReceived':
    case 'Network.webSocketFrameSent': {
      if (!args.frames) break;
      const dir = msg.method.endsWith('Received') ? 'in' : 'out';
      const len = (p.response && p.response.payloadData || '').length;
      const b = summary.websockets.get(p.url);
      if (b) { if (dir === 'in') b.inBytes += len; else b.outBytes += len; b.frames++; }
      record({ kind: 'ws_frame', url: p.url, dir, bytes: len });
      break;
    }
    case 'Runtime.bindingCalled': {
      if (p.name !== args.binding) break;
      let payload;
      try { payload = JSON.parse(p.payload); } catch { payload = { hookError: String(p.payload).slice(0, 500) }; }
      // `kind: 'rpc'` must win over any field the hook itself sends.
      record({ ...payload, kind: 'rpc' });
      break;
    }
    case 'Runtime.consoleAPICalled': {
      const text = (p.args || []).map((a) => a.value ?? a.description ?? a.type).join(' ');
      record({ kind: 'console', level: p.type, text });
      break;
    }
    case 'Log.entryAdded':
      record({ kind: 'console', level: p.entry.level, text: p.entry.text });
      break;
  }
});

await send('Runtime.enable');
await send('Network.enable');
await send('Log.enable');

if (args.hook) {
  await send('Runtime.addBinding', { name: args.binding });
  const src = fs.existsSync(args.hook) ? fs.readFileSync(args.hook, 'utf8') : args.hook;
  const r = await send('Runtime.evaluate', { expression: src, returnByValue: true, awaitPromise: true });
  const v = r?.result?.value ?? r?.exceptionDetails?.text ?? JSON.stringify(r);
  record({ kind: 'hook', binding: args.binding, result: v });
}

record({
  kind: 'target',
  app: args.app,
  port: ws.port,
  title: ws.target.title,
  url: ws.target.url,
});

// --------------------------------------------------------------- --once ------
if (args.once) {
  let expr = args.expr || 'document.title';
  if (args.exprFile) expr = fs.readFileSync(args.exprFile, 'utf8');
  const r = await send('Runtime.evaluate', { expression: expr, returnByValue: true, awaitPromise: true });
  console.log(JSON.stringify(r?.result?.value ?? r, null, 2));
  ws.close();
  process.exit(0);
}

// ------------------------------------------------------------- text poll -----
const TEXT_EXPR = `(() => {
  const ae = document.activeElement;
  const pick = (el) => el ? String(el.value ?? el.innerText ?? el.textContent ?? '').trim() : '';
  const focused = pick(ae);
  let best = focused;
  if (!best) {
    for (const el of document.querySelectorAll('[contenteditable="true"],textarea,input[type="text"]')) {
      const t = pick(el);
      if (t && t.length > best.length) best = t;
    }
  }
  return {
    active: ae ? ae.tagName + (ae.getAttribute('aria-label') ? '[' + ae.getAttribute('aria-label') + ']' : '') : null,
    text: best.slice(0, 2000),
  };
})()`;

let lastText = null;
const pollTimer = setInterval(async () => {
  try {
    const r = await send('Runtime.evaluate', { expression: TEXT_EXPR, returnByValue: true });
    const v = r?.result?.value;
    if (v && v.text && v.text !== lastText) {
      lastText = v.text;
      record({ kind: 'text', active: v.active, text: v.text });
    }
  } catch { /* page busy */ }
}, args.poll);

// --------------------------------------------------------------- shutdown ----
function finish() {
  clearInterval(pollTimer);
  const elapsed = ((Date.now() - summary.started) / 1000).toFixed(1);
  const lines = [];
  lines.push(`# CDP observation summary (${args.app})`);
  lines.push('');
  lines.push(`Duration: ${elapsed}s | port: ${ws.port} | target: ${ws.target.title}`);
  lines.push('');
  lines.push('## Network hosts contacted');
  lines.push('| host | calls | types | examples |');
  lines.push('| --- | --- | --- | --- |');
  for (const [h, b] of [...summary.requests.entries()].sort((a, b) => b[1].count - a[1].count)) {
    const ex = [...b.paths].slice(0, 3).join(', ');
    lines.push(`| ${h} | ${b.count} | ${[...b.types].join('/')} | ${ex} |`);
  }
  lines.push('');
  lines.push('## Language-server RPCs captured');
  if (summary.rpcs.size === 0) lines.push('_none observed_');
  for (const [k, b] of [...summary.rpcs.entries()].sort((a, b) => b[1].count - a[1].count)) {
    lines.push(`- ${k} — ${b.count} call(s), ${(b.bytes / 1024).toFixed(1)} KiB of body`);
  }
  lines.push('');
  lines.push('## WebSockets');
  if (summary.websockets.size === 0) lines.push('_none observed_');
  for (const [u, b] of summary.websockets) lines.push(`- ${u} — frames: ${b.frames}, in: ${b.inBytes}B, out: ${b.outBytes}B`);
  lines.push('');
  lines.push('## Transcript samples (last 20)');
  for (const t of summary.texts.slice(-20)) lines.push(`- ${JSON.stringify(t).slice(0, 400)}`);
  lines.push('');
  lines.push(`JSONL: ${logPath}`);
  const mdPath = logPath.replace(/\.jsonl$/, '-summary.md');
  fs.writeFileSync(mdPath, lines.join('\n'), 'utf8');
  record({ kind: 'summary', jsonl: logPath, summary: mdPath });
  stream.end();
  console.log('');
  console.log(`Done. from port ${ws.port}, ${summary.requests.size} hosts, ${summary.websockets.size} websockets`);
  console.log(`  JSONL   : ${logPath}`);
  console.log(`  Summary : ${mdPath}`);
}

process.on('SIGINT', () => { try { ws.close(); } catch {} finish(); process.exit(0); });
ws.addEventListener('close', () => { finish(); process.exit(0); });

if (args.duration > 0) setTimeout(() => { try { ws.close(); } catch {} finish(); process.exit(0); }, args.duration * 1000);

console.log('');
console.log('  ==============================================================');
console.log(`   CDP OBSERVER attached to "${ws.target.title}" (port ${ws.port})`);
console.log('  ==============================================================');
console.log('');
console.log('   Now go use the app\'s voice input. Everything it does will be');
console.log('   printed here and written to:');
console.log(`     ${logPath}`);
console.log('');
console.log(`   Auto-stops after ${args.duration || '∞'} s. Ctrl+C to stop.`);
console.log('');
