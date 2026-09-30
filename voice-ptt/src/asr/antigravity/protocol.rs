//! The byte-level contract with Antigravity's local `language_server.exe`.
//!
//! Everything here is **pure**: framing, PCM encoding and header construction
//! touch no socket, no process and no clock. That is the reason this is its own
//! module rather than part of the engine — the wire contract can be tested
//! against a real spec (see the `gobs` probe) with no server running, and the
//! engine keeps only what actually does I/O.

use serde_json::Value;

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
            let len =
                u32::from_be_bytes([self.buf[1], self.buf[2], self.buf[3], self.buf[4]]) as usize;
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
        || message
            .get("value")
            .and_then(|v| v.get("complete"))
            .is_some()
}

// ----------------------------------------------------------------- audio ----

/// Mono f32 at `sample_rate` → PCM16 LE at 16 kHz (linear resampling when needed).
pub fn to_pcm16k_mono(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let rate = if sample_rate == 0 {
        16_000
    } else {
        sample_rate
    };
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

/// `pub(super)` because only the engine needs it; nothing outside `antigravity`
/// does, and keeping it narrow stops this module from becoming a grab bag.
pub(super) fn request_headers(port: u16, token: &str) -> reqwest::header::HeaderMap {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        assert!(
            decoder.push(&stream[3..10]).is_empty(),
            "payload still incomplete"
        );
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
            transcription_text(
                &json!({"value": {"transcription": {"text": "سلام", "is_final": true}}})
            ),
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
        let samples: Vec<f32> = (0..8_000).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();
        let pcm = to_pcm16k_mono(&samples, 8_000);
        assert_eq!(pcm.len(), 32_000); // 16 000 samples × 2 bytes
    }

    #[test]
    fn request_headers_carry_the_csrf_token() {
        let headers = request_headers(1061, "tok-1");
        assert_eq!(headers["x-codeium-csrf-token"], "tok-1");
        assert_eq!(headers["content-type"], "application/grpc-web+json");
        assert_eq!(headers["origin"], "https://127.0.0.1:1061");
    }
}
