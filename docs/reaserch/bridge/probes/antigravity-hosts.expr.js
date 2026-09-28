/**
 * --once expression for observe-cdp.mjs.
 *
 * Lists the distinct backend hostnames referenced by Antigravity's own bundle,
 * with special attention to anything near speech / cloud-code terminology.
 * This is what tells us which service the live transcription actually hits.
 *
 * Usage:
 *   node observe-cdp.mjs --app antigravity --once --expr-file probes/antigravity-hosts.expr.js
 */
(async () => {
  const r = await fetch('/main.js');
  const text = await r.text();

  const hosts = new Map();
  const hostRe = /https?:\/\/([a-z0-9.-]+\.[a-z]{2,})(?::(\d+))?/gi;
  let m;
  while ((m = hostRe.exec(text)) !== null) {
    const host = m[1].toLowerCase();
    hosts.set(host, (hosts.get(host) || 0) + 1);
  }

  const interesting = /(cloudcode|cloud-code|codeassist|generativelanguage|speech|aiplatform|googleapis|gemini|antigravity|daily)/i;

  const contexts = [];
  const termRe = /(cloudcode[a-z0-9.\-]*|codeassist[a-z0-9.\-]*|generativelanguage[a-z0-9.\-]*|speech[a-z0-9.\-/]*|cloud-code[a-z0-9.\-]*)/gi;
  let t;
  let guard = 0;
  const seen = new Set();
  while ((t = termRe.exec(text)) !== null && guard++ < 400) {
    const key = t[0].toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    contexts.push({ term: t[0], ctx: text.slice(Math.max(0, t.index - 220), t.index + 260) });
  }

  return {
    hosts: [...hosts.entries()]
      .filter(([h]) => interesting.test(h))
      .sort((a, b) => b[1] - a[1])
      .map(([host, count]) => ({ host, count })),
    contexts: contexts.slice(0, 30),
  };
})()
