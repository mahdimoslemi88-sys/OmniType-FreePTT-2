#!/usr/bin/env node
/**
 * proto-dump.mjs — schema-free body dumper for the Antigravity language server.
 *
 * We never got a .proto for the Antigravity audio RPCs, but the captures showed
 * two things that matter more (2026-09-26, session 2):
 *   1. the requests are plain JSON wrapped in a gRPC-web frame
 *      (`Content-Type: application/grpc-web+json`, 1 flag byte + 4 length bytes), and
 *   2. an aborted request has *no* body at all — only `bodyKind: fetch-error:…`.
 * So this dumper does three things, in order of usefulness:
 *   - unwrap gRPC-web frames and pretty-print JSON bodies (abbreviating long blobs),
 *   - fall back to protobuf wire parsing for binary bodies,
 *   - and print a line for failed/aborted RPCs instead of silently skipping them.
 *
 * Usage:
 *   node proto-dump.mjs --jsonl logs/cdp-antigravity-*.jsonl --rpc StreamAudioTranscription
 *   node proto-dump.mjs --jsonl logs/cdp-antigravity-*.jsonl --rpc SendAudioChunk --limit 2
 *   node proto-dump.mjs --jsonl logs/cdp-antigravity-*.jsonl --all      # everything, incl. errors
 *   node proto-dump.mjs --hex 0a0461626364
 *   node proto-dump.mjs --b64 "$(cat body.b64)"
 *
 * Flags: --rpc <Name>  --dir req|res  --limit N  --all  --raw  --no-grpcweb
 */

import fs from 'node:fs';

const args = process.argv.slice(2);
const opt = { jsonl: null, rpc: null, dir: null, limit: 3, b64: null, hex: null, all: false, raw: false, noGrpcWeb: false };
for (let i = 0; i < args.length; i++) {
  const a = args[i];
  if (a === '--jsonl') opt.jsonl = args[++i];
  else if (a === '--rpc') opt.rpc = args[++i];
  else if (a === '--dir') opt.dir = args[++i];
  else if (a === '--limit') opt.limit = Number(args[++i]);
  else if (a === '--b64') opt.b64 = args[++i];
  else if (a === '--hex') opt.hex = args[++i];
  else if (a === '--all') opt.all = true;
  else if (a === '--raw') opt.raw = true;
  else if (a === '--no-grpcweb') opt.noGrpcWeb = true;
  else if (a === '-h' || a === '--help') {
    console.log('usage: node proto-dump.mjs [--b64 <s> | --hex <s> | --jsonl <file> [--rpc <Name>] [--dir req|res] [--limit N] [--all] [--raw] [--no-grpcweb]]');
    process.exit(0);
  }
}

// -------------------------------------------------------- gRPC-web framing ----
/**
 * Split a gRPC-web body into frames: `flag(1) len(4, big-endian) payload(len)`.
 * Flag 0x00 = message, 0x80 = trailer (status/headers as text). Returns null when
 * the bytes are not consistently framed, so protobuf bodies fall through untouched.
 */
function unframe(buf) {
  const frames = [];
  let i = 0;
  while (i + 5 <= buf.length) {
    const flag = buf[i];
    const len = buf.readUInt32BE(i + 1);
    if (flag !== 0x00 && flag !== 0x80) return frames.length ? { frames } : null;
    if (i + 5 + len > buf.length) {
      // A frame header that overruns the buffer is almost always a truncated copy
      // (half-pasted log line) — say so instead of blaming the protobuf parser.
      return frames.length
        ? { frames }
        : { truncated: { flag, len, have: buf.length - i - 5 } };
    }
    frames.push({ flag, len, payload: buf.subarray(i + 5, i + 5 + len) });
    i += 5 + len;
  }
  if (i !== buf.length || frames.length === 0) return frames.length ? { frames } : null;
  if (frames[0].flag !== 0x00) return null;
  return { frames };
}

// ------------------------------------------------------------ wire format ----
const WIRE = { 0: 'varint', 1: 'fixed64', 2: 'bytes', 5: 'fixed32' };

function readVarint(buf, start) {
  let shift = 0n, out = 0n, i = start;
  while (i < buf.length) {
    const b = buf[i++];
    out |= BigInt(b & 0x7f) << shift;
    if ((b & 0x80) === 0) return { value: out, next: i };
    shift += 7n;
    if (shift > 63n) throw new Error(`varint too long at ${start}`);
  }
  throw new Error(`truncated varint at ${start}`);
}

/** Decode one message into a flat field list; throws if the bytes are not protobuf. */
function decode(buf) {
  const fields = [];
  let i = 0;
  while (i < buf.length) {
    const at = i;
    const key = readVarint(buf, i);
    i = key.next;
    const field = Number(key.value >> 3n);
    const wire = Number(key.value & 7n);
    if (field === 0) throw new Error(`field number 0 at ${at}`);
    if (!(wire in WIRE)) throw new Error(`wire type ${wire} at ${at}`);

    const f = { field, wire, at };
    if (wire === 0) {
      const v = readVarint(buf, i); i = v.next; f.varint = v.value.toString();
    } else if (wire === 1) {
      f.fixed64 = buf.subarray(i, i + 8).toString('hex'); i += 8;
    } else if (wire === 5) {
      f.fixed32 = buf.subarray(i, i + 4).toString('hex'); i += 4;
    } else {
      const len = readVarint(buf, i);
      i = len.next;
      const n = Number(len.value);
      if (i + n > buf.length) throw new Error(`field ${field} overruns buffer (${n} bytes at ${i})`);
      f.bytes = buf.subarray(i, i + n);
      i += n;
    }
    fields.push(f);
  }
  return fields;
}

/** Plain ASCII text (no control bytes) — a real string field, not a nested message. */
const plainText = (b) => {
  for (const byte of b) if (byte < 0x20 || byte > 0x7e) return false;
  return b.length > 0;
};
const base64ish = (b) => b.length >= 8 && b.length % 4 === 0 && /^[A-Za-z0-9+/]+={0,2}$/.test(b.toString('latin1'));
const utf8 = (b) => { try { return new TextDecoder('utf-8', { fatal: true }).decode(b); } catch { return null; } };
function pcmStats(b) {
  if (b.length < 320 || b.length % 2) return null;
  let sum = 0, peak = 0;
  for (let k = 0; k + 1 < b.length; k += 2) {
    const s = b.readInt16LE(k);
    sum += s * s;
    peak = Math.max(peak, Math.abs(s));
  }
  return { rms: Math.sqrt(sum / (b.length / 2)), peak };
}

/** Render a message; descends into nested messages, stops at text and audio. */
function render(buf, indent, depth = 0) {
  const out = [];
  let fields;
  try { fields = decode(buf); } catch (e) { return [`${indent}!! not protobuf: ${e.message}`]; }
  if (buf.length > 0 && fields.length === 0) return [`${indent}!! not protobuf: no fields`];

  for (const f of fields) {
    let line = `${indent}#${f.field} ${WIRE[f.wire]}`;
    if (f.varint !== undefined) line += ` = ${f.varint}`;
    if (f.fixed32 !== undefined) line += ` = 0x${f.fixed32}`;
    if (f.fixed64 !== undefined) line += ` = 0x${f.fixed64}`;
    if (f.bytes !== undefined) line += ` len=${f.bytes.length}`;
    out.push(line);

    if (f.bytes === undefined) continue;
    const b = f.bytes;

    if (opt.raw) out.push(`${indent}  hex: ${b.toString('hex').slice(0, 96)}${b.length > 48 ? '…' : ''}`);
    if (b.length === 0) { out.push(`${indent}  (empty)`); continue; }

    // A real string field is plain ASCII with no control bytes. Anything else is
    // either a nested message or a binary blob (raw PCM / base64 of PCM).
    if (!plainText(b) && depth < 3) {
      const nested = render(b, indent + '  ', depth + 1);
      if (!nested[0] || !nested[0].includes('not protobuf')) {
        out.push(`${indent}  {`);
        out.push(...nested);
        out.push(`${indent}  }`);
        continue;
      }
      const u = utf8(b);
      if (u !== null) { out.push(`${indent}  utf8: ${JSON.stringify(u.slice(0, 300))}`); continue; }
    } else if (plainText(b)) {
      out.push(`${indent}  text: ${JSON.stringify(b.toString('latin1').slice(0, 300))}`);
      if (base64ish(b)) {
        const inner = Buffer.from(b.toString('latin1'), 'base64');
        const pcm = pcmStats(inner);
        out.push(`${indent}  base64 -> ${inner.length}B${pcm ? ` — pcm? rms=${pcm.rms.toFixed(1)} peak=${pcm.peak}` : ''} | hex ${inner.toString('hex').slice(0, 32)}…`);
      }
      continue;
    }

    const pcm = pcmStats(b);
    if (pcm) out.push(`${indent}  binary ${b.length}B — pcm? rms=${pcm.rms.toFixed(1)} peak=${pcm.peak} | hex ${b.toString('hex').slice(0, 48)}…`);
    else out.push(`${indent}  binary ${b.length}B | hex ${b.toString('hex').slice(0, 48)}${b.length > 24 ? '…' : ''}`);
  }
  return out;
}

// -------------------------------------------------------------- JSON bodies ---
/**
 * Long strings are the audio payload, so never print them whole: report length,
 * and if the string is base64, the decoded size plus PCM loudness.
 */
function abbreviate(_key, value) {
  if (typeof value !== 'string' || value.length <= 240) return value;
  const inner = value.length >= 512 && value.length % 4 === 0 && /^[A-Za-z0-9+/]+={0,2}$/.test(value)
    ? Buffer.from(value, 'base64')
    : null;
  const pcm = inner ? pcmStats(inner) : null;
  return `«${value.length} chars»`
    + (inner ? ` -> ${inner.length}B${pcm ? ` — pcm? rms=${pcm.rms.toFixed(1)} peak=${pcm.peak}` : ''} | hex ${inner.toString('hex').slice(0, 32)}…` : ` | head ${JSON.stringify(value.slice(0, 40))}`);
}

/** Body text of a JSON payload, or null when it is not JSON. */
function jsonText(payload) {
  const s = payload.toString('utf8').trimStart();
  return s.startsWith('{') || s.startsWith('[') ? s : null;
}

function renderPayload(indent, payload) {
  const text = jsonText(payload);
  if (text !== null) {
    try {
      const obj = JSON.parse(text);
      if (opt.raw) console.log(`${indent}raw: ${text.slice(0, 200)}${text.length > 200 ? '…' : ''}`);
      console.log(JSON.stringify(obj, abbreviate, 2).split('\n').map((l) => indent + l).join('\n'));
      return;
    } catch (e) {
      console.log(`${indent}!! invalid JSON (${e.message}): ${text.slice(0, 200)}`);
      return;
    }
  }
  console.log(render(payload, indent).join('\n'));
}

function show(title, buf, note = '') {
  console.log('');
  console.log(`== ${title}  (${buf.length} bytes)${note}`);
  if (!opt.noGrpcWeb) {
    const framed = unframe(buf);
    if (framed && framed.truncated) {
      const t = framed.truncated;
      console.log(`  !! truncated gRPC-web frame: header claims ${t.len} payload bytes, only ${t.have} present (flag=0x${t.flag.toString(16)}) — the capture/line is cut short`);
      return;
    }
    if (framed) {
      framed.frames.forEach((f, idx) => {
        console.log(`  [${f.flag === 0x80 ? 'trailer' : 'message'} frame ${idx + 1}] flag=0x${f.flag.toString(16)} len=${f.len}`);
        renderPayload('    ', f.payload);
      });
      return;
    }
  }
  renderPayload('  ', buf);
}

/** One line for an RPC we only saw fail — no body, but the reason is the finding. */
function showFailure(rec) {
  console.log('');
  console.log(`== ${rec.dir} ${rec.rpc}  (NO BODY CAPTURED) status=${rec.status ?? '-'} bodyKind=${rec.bodyKind || '-'} ts=${rec.ts || '-'}`);
  console.log('  ^ an aborted/never-answered request: the fetch rejected before any response byte arrived.');
}

// ------------------------------------------------------------------ inputs ---
if (opt.b64 !== null) show('base64 input', Buffer.from(opt.b64, 'base64'));
if (opt.hex !== null) show('hex input', Buffer.from(opt.hex, 'hex'));

if (opt.jsonl) {
  const lines = fs.readFileSync(opt.jsonl, 'utf8').split(/\r?\n/).filter(Boolean);
  let shown = 0, seen = new Map(), bodyless = 0;
  for (const line of lines) {
    let rec;
    try { rec = JSON.parse(line); } catch { continue; }
    if (rec.kind !== 'rpc') continue;
    if (opt.rpc && rec.rpc !== opt.rpc) continue;
    if (opt.dir && rec.dir !== opt.dir) continue;
    seen.set(`${rec.dir} ${rec.rpc}`, (seen.get(`${rec.dir} ${rec.rpc}`) || 0) + 1);
    // SendAudioChunk fires ~25x/s; skip it unless asked for it explicitly.
    if (!opt.all && rec.rpc === 'SendAudioChunk' && opt.rpc !== 'SendAudioChunk') continue;
    if (shown >= opt.limit) continue;
    shown++;
    if (typeof rec.body === 'string' && rec.body.length > 0) {
      const buf = Buffer.from(rec.body, 'base64');
      show(`${rec.dir} ${rec.rpc}`, buf, `  mime=${rec.mime || '-'} bodyKind=${rec.bodyKind || '-'} status=${rec.status ?? '-'} ts=${rec.ts || '-'}`);
    } else {
      bodyless++;
      showFailure(rec);
    }
  }
  console.log('');
  console.log('captured in this file:');
  for (const [k, v] of seen) console.log(`  ${k}  x${v}`);
  if (seen.size === 0) console.log('  (no `rpc` records — this recording predates the fetch hook; re-record with --hook)');
  console.log(`(${shown} message(s) shown of ${lines.length} log lines${bodyless ? `, ${bodyless} with no body captured` : ''})`);
}

if (!opt.jsonl && opt.b64 === null && opt.hex === null) {
  console.error('nothing to do: pass --jsonl <file>, or --b64 / --hex');
  process.exit(1);
}
