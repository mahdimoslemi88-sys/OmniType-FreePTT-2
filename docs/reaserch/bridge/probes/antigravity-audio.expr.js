/**
 * --once expression for observe-cdp.mjs.
 *
 * Runs inside the Antigravity page, discovers the JS bundles it actually
 * loaded, fetches them and reports the code around anything that looks like
 * audio streaming / speech recognition. This is how we find out *where* the
 * voice audio is sent.
 *
 * Usage:
 *   node observe-cdp.mjs --app antigravity --once --expr-file probes/antigravity-audio.expr.js
 */
(async () => {
  const origin = location.origin;

  // 1. Real script URLs from the performance timeline, plus the obvious root.
  const fromPerf = performance.getEntriesByType('resource')
    .map((e) => e.name)
    .filter((n) => /\.js(\?|$)/.test(n) && n.startsWith(origin));
  const candidates = [...new Set(['/main.js', ...fromPerf.map((n) => new URL(n).pathname + new URL(n).search)])];

  const termRe = /(AudioStreaming|audioStream|AudioService|transcrib\w*|transcription|speech\w*|SpeechRecognition|getUserMedia|MediaRecorder|RealtimeInput|audio\/pcm)/gi;
  const endpointRe = /(https?:\/\/[^\s"'`)]{6,140}|\/v[0-9]+\/[A-Za-z0-9_./{}-]{3,100})/g;

  const bundles = [];
  const byTerm = {};
  const endpoints = new Map();
  let total = 0;

  for (const c of candidates.slice(0, 8)) {
    let text;
    try {
      const r = await fetch(c);
      if (!r.ok) continue;
      text = await r.text();
    } catch { continue; }
    total += text.length;
    bundles.push({ url: c, sizeBytes: text.length });
    if (total > 80_000_000) break;

    let m;
    let guard = 0;
    while ((m = termRe.exec(text)) !== null && guard++ < 300) {
      const ctx = text.slice(Math.max(0, m.index - 200), m.index + 320);
      (byTerm[m[0]] ||= []).push({ bundle: c, ctx });
    }

    let e;
    let eg = 0;
    while ((e = endpointRe.exec(text)) !== null && eg++ < 20000) {
      const around = text.slice(Math.max(0, e.index - 140), e.index + 140).toLowerCase();
      if (/(audio|speech|transcri|voice|stream|microphone|\bmic\b)/.test(around)) {
        if (!endpoints.has(e[0])) endpoints.set(e[0], { bundle: c, ctx: text.slice(Math.max(0, e.index - 140), e.index + 140) });
      }
    }
  }

  const compact = {};
  for (const [k, v] of Object.entries(byTerm)) {
    compact[k] = { hits: v.length, samples: v.slice(0, 2) };
  }

  return {
    bundles,
    terms: compact,
    endpoints: [...endpoints.entries()].slice(0, 30).map(([url, info]) => ({ url, ...info })),
  };
})()
