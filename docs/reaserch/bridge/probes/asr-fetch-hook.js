/*
 * asr-fetch-hook.js — injected into the Antigravity renderer by
 *     node observe-cdp.mjs --app antigravity --hook probes/asr-fetch-hook.js
 *
 * The CDP Network domain only gives us URLs, never the protobuf bodies. This
 * hook wraps window.fetch *inside the page* and ships the exact bytes out
 * through a Runtime binding, so we can learn the real request/response framing
 * of the language-server audio RPCs:
 *
 *   StreamAudioTranscription  (start)   -> ready{sessionId} + transcription{text,isFinal}
 *   SendAudioChunk            (~25/s)   -> the audio itself
 *   EndAudioSession           (stop)
 *
 * Everything is base64 so binary data survives the JSON transport.
 * Read-only: it only observes; it never modifies the request or the response.
 */
(() => {
  const BINDING = '__omniTypeCapture';
  if (window.__otAsrHook) return 'already-hooked';
  window.__otAsrHook = true;

  const send = (obj) => { try { window[BINDING](JSON.stringify(obj)); } catch { /* observer gone */ } };

  const rpcOf = (url) => {
    const m = /LanguageServerService\/([A-Za-z0-9_]+)/.exec(url || '');
    return m ? m[1] : null;
  };

  const b64 = (u8) => {
    let s = '';
    for (let i = 0; i < u8.length; i += 8192) s += String.fromCharCode.apply(null, u8.subarray(i, i + 8192));
    return btoa(s);
  };

  const headerOf = (headers, want) => {
    try {
      if (!headers) return null;
      if (typeof headers.get === 'function') return headers.get(want);
      if (Array.isArray(headers)) {
        const hit = headers.find((h) => String(h[0]).toLowerCase() === want);
        return hit ? hit[1] : null;
      }
      const k = Object.keys(headers).find((x) => x.toLowerCase() === want);
      return k ? headers[k] : null;
    } catch { return null; }
  };

  const describe = (body) => {
    if (body == null) return { bodyKind: 'no-body', body: null, len: null };
    try {
      if (body instanceof Uint8Array) return { bodyKind: 'bytes', body: b64(body), len: body.byteLength };
      if (body instanceof ArrayBuffer) return { bodyKind: 'bytes', body: b64(new Uint8Array(body)), len: body.byteLength };
      if (typeof body === 'string') return { bodyKind: 'text', body: body, len: body.length };
      if (typeof body.getReader === 'function') return { bodyKind: 'stream', body: null, len: null };
      if (body instanceof Blob) return { bodyKind: 'blob', body: null, len: body.size };
      return { bodyKind: Object.prototype.toString.call(body), body: null, len: null };
    } catch (e) {
      return { bodyKind: 'error:' + e.message, body: null, len: null };
    }
  };

  const orig = window.fetch;
  window.fetch = function (input, init) {
    let url = '';
    try { url = typeof input === 'string' ? input : (input && input.url) || ''; } catch { /* ignore */ }
    const rpc = rpcOf(url);

    if (rpc) {
      const d = describe(init && init.body);
      send({
        dir: 'req', rpc, url,
        mime: headerOf(init && init.headers, 'content-type'),
        bodyKind: d.kind, len: d.len, body: d.body,
      });
    }

    const p = orig.apply(this, arguments);

    // The start RPC's response is the whole stream: ready -> transcription* -> complete.
    if (rpc === 'StreamAudioTranscription' || rpc === 'EndAudioSession') {
      p.then((resp) => {
        try {
          const copy = resp.clone();
          copy.arrayBuffer()
            .then((ab) => {
              const u8 = new Uint8Array(ab);
              send({
                dir: 'res', rpc, status: resp.status,
                mime: resp.headers.get('content-type'),
                bodyKind: 'bytes', len: u8.byteLength, body: b64(u8),
              });
            })
            .catch((e) => send({ dir: 'res', rpc, bodyKind: 'read-error:' + e.message }));
        } catch (e) {
          send({ dir: 'res', rpc, bodyKind: 'clone-error:' + e.message });
        }
      }).catch((e) => send({ dir: 'res', rpc, bodyKind: 'fetch-error:' + e.message }));
    }

    return p;
  };

  return 'hooked:' + (typeof orig === 'function' ? 'ok' : 'no-fetch');
})()
