# voice-ptt — Push-to-Talk voice typing for Windows (Persian-first)

High-performance, free, offline-capable Push-to-Talk (PTT) voice typing built in Rust —
the v2 rewrite of [OmniType-FreePTT](../../v-1) (Python), designed around the research
findings in [`../docs`](../docs).

## Pipeline

```
[Hold Caps Lock]
      ↓  (< 50 ms)
WASAPI Shared capture (cpal, 16 kHz mono, 256-sample frames)
      ↓  zero-copy
Lock-free SPSC ring buffer (30 s, overwrite-oldest)
      ↓  512-sample frames (32 ms)
VAD endpointing (Silero v5 ONNX, RMS fallback) — 1500 ms silence timeout
      ↓
whisper.cpp (large-v3-turbo on GPU / small on CPU, beam 5, fa initial prompt)
      ↓
Persian normalizer (Arabic→Farsi codepoints, ZWNJ, punctuation) + Aho-Corasick dictionary
      ↓
SendInput KEYEVENTF_UNICODE injection (types any script, no clipboard touched)
      ↓
[Text appears in the focused app]
```

## Build

Requirements (one-time): Rust (MSVC), VS Build Tools 2022 (C++ + CMake), LLVM (libclang).

```bat
cargo build --release
```

The binary lands at `target/release/voice-ptt.exe` (~28 MB — whisper.cpp and the GUI
are statically linked; no runtime dependencies).

## Run

```bat
target\release\voice-ptt.exe
```

First run downloads the selected whisper model automatically (from Hugging Face) into
`models/` and the Silero VAD model into `assets/`. Model choice ("auto" default):

| Hardware            | Model            | Accuracy (fa) | Speed (5 s audio) |
| ------------------- | ---------------- | ------------- | ----------------- |
| Discrete GPU        | large-v3-turbo   | ~93 %         | ~1.5 s            |
| CPU ≥ 8 threads     | small            | ~82 %         | ~2.5 s            |
| Weaker CPU          | base             | ~75 %         | ~1.2 s            |

The only network access in the entire app is the one-time model download. After that,
everything runs locally.

## Hotkeys

| Keys           | Action                       |
| -------------- | ---------------------------- |
| `Caps Lock`    | Hold to record, release to type |
| `Caps Lock` ×2 | Tap twice quickly for **hands-free**: recording latches and keeps running after the key is released; the next press types it |
| `Ctrl+Alt+S`   | Show/hide the overlay        |
| `Ctrl+Alt+Q`   | Quit                         |

Long dictations are no longer cut at ~30 s: while you speak, the audio is flushed
to the ASR engine in ordered chunks (see `[streaming]` below) and typed as it goes.

## Configuration

`%APPDATA%\voice-ptt\config.toml` is created with spec defaults on first run:

```toml
[audio]
sample_rate = 16000
channels = 1
buffer_frames = 256
ring_seconds = 60
device = "default"

[asr]
model = "auto"        # tiny | base | small | medium | large-v3 | large-v3-turbo | auto
language = "fa"
beam_size = 5
n_threads = 8
# Loading the local model maps ~1.6 GB of RAM, so the `auto` engine chain never
# falls back to it on its own: pick "Local Whisper" explicitly, or set this true.
auto_local_fallback = false

[vad]
threshold = 0.5
silence_timeout_ms = 1500
chunk_size = 512
min_speech_ms = 150

[hotkey]
record = "CapsLock"
toggle_overlay = "Ctrl+Alt+S"
quit = "Ctrl+Alt+Q"
double_tap_latch = true   # tap twice for hands-free recording
tap_max_ms = 350          # press shorter than this counts as a "tap"
double_tap_window_ms = 600

[streaming]
enabled = true            # flush long dictations as ordered chunks while recording
chunk_seconds = 20        # hard cap per chunk (kept well below the free Google limit)
min_chunk_seconds = 4     # never flush a shorter fragment
overlap_ms = 300          # audio repeated into the next chunk so seams don't clip words
silence_ms = 1200         # trailing pause that marks a clean cut (strategy = "silence")
strategy = "silence"      # silence | fixed
max_utterance_seconds = 600  # absolute safety cap for one held session
# Seam repair: the overlap above makes the recogniser repeat the tail of the
# previous chunk, and a cut can land mid-word. These keep one long dictation
# reading as one continuous text.
seam_merge = true         # drop repeated head words at a chunk seam
seam_backspace = true     # also delete a word fragment the cut left behind
seam_max_words = 6        # at most this many repeated head words per chunk
seam_fuzzy = true         # tolerate ZWNJ/diacritics/one recognition slip

# How much the pipeline may change what the recogniser said, and whether a
# spoken phrase may be an instruction rather than words.
[text]
mode = "standard"         # standard | conservative | raw
commands = false          # "خط جدید" / "ویرگول" insert a break or punctuation
# Fires only when the whole sentence IS the phrase, or after the word "دستور" —
# a sentence that merely mentions one is typed exactly as spoken. A newline is
# sent as the Enter key, never as a character. Independent of `mode`: raw text
# plus commands is a supported combination.

[gui]
show_overlay = true
theme = "dark"
show_transcript_bubble = true   # floating transcript card, bottom-center above the taskbar

# Optional: cloud ASR (off by default — opting in sends audio off-machine).
# Any OpenAI-compatible /audio/transcriptions endpoint works (Groq, OpenRouter,
# OpenAI, or a LAN whisper server). When enabled it runs FIRST; the local
# whisper model is the automatic fallback (30 s cooldown after a failure).
[cloud]
enabled = false
provider = "groq"
base_url = "https://api.groq.com/openai/v1"
api_key = ""                       # or set the VOICE_PTT_CLOUD_KEY env var
model = "whisper-large-v3-turbo"
language = "fa"
timeout_secs = 30
daily_limit = 300                 # free-tier budget; stands down until local midnight when spent
```

## Cloud engine (optional)

Groq's free tier (~300 requests/day) transcribes 5 s of audio in well under a second
at ~95 % Persian accuracy — better than small local models, and it uploads ~160 KB
per utterance instead of needing a 1.6 GB local model:

1. Create a free key at `console.groq.com` (no card required).
2. Set `enabled = true` and paste the key into `[cloud] api_key` **or** set the
   `VOICE_PTT_CLOUD_KEY` environment variable (preferred — keeps the key off disk).
3. Restart. `Ctrl+Alt+Q` → relaunch.

No key configured? The cloud engine simply never loads. No internet at runtime?
By default the failed request is simply reported — the 1.6 GB local whisper model is
**not** loaded behind your back (`asr.auto_local_fallback = false`). Select «Local
Whisper» explicitly, or set `auto_local_fallback = true`, to let a cloud failure fall
back to the local model.

**Daily quota:** usage is tracked in `%APPDATA%\voice-ptt\cloud_usage.json`. After
`daily_limit` requests (or a server-reported daily-limit 429), the engine stands down
until **local midnight** — no cooldown retries spamming the API — and the local whisper
model handles everything until the counter resets. Per-minute rate-limit 429s are
treated as transient and only trigger the normal 30 s cooldown.

Custom technical-term corrections can be added to `%APPDATA%\voice-ptt\dictionary.toml`:

```toml
[[corrections]]
from = "پاتون"
to   = "پایتون"
```

## Long dictation (chunked streaming)

A session is no longer ended by the ring-buffer safety valve, so you can hold the key
(indeed, latch it with a double tap) for as long as you like. With `[streaming]
enabled = true`, the audio is instead cut into ordered chunks while the microphone keeps
running: each chunk is cut at a pause (`silence` strategy, ≥ `silence_ms`) or at
`chunk_seconds`, transcribed, typed, and recording continues — text appears while you
are still talking, and the free cloud endpoint never sees a long upload.

While a session is running the app **looks like it is recording the whole time**: a
chunk boundary does not change the state or the orb (only the text box updates), and
nothing ends the session on its own — it finishes when you release the key, click the
orb, or press the cancel key. Only `streaming.max_utterance_seconds` can stop it
without you, as an absolute safety cap.

Chunks are transcribed sequentially, so the order is always the order you spoke. Two
seam artefacts are repaired for you by `[streaming] seam_*`:

* the deliberate `overlap_ms` of audio repeated into the next chunk makes the
  recogniser re-emit the previous chunk's tail — those repeated head words are dropped;
* if the cut landed mid-word, the fragment at the end of the previous chunk
  (`… می‌خوا`) is deleted with backspaces and the complete word from the new chunk
  takes its place.

Matching tolerates ZWNJ/Arabic codepoints and a single recognition slip, but never
guesses: it will not delete a word that is also a common word on its own, and it leaves
repeated words in place rather than half-eating a longer run. Set
`seam_backspace = false` (or `seam_merge = false`) if you ever prefer the raw chunks.

## Tests & benchmarks

```bat
cargo test            # 183 unit + integration tests
cargo clippy --all-targets   # zero warnings
cargo bench           # ring buffer / VAD / text-processing benchmarks (criterion)
```

## Architecture notes

- **`cpal::Stream` is `!Send` on Windows** — it is created, driven and dropped on a
  dedicated audio thread (`src/audio/capture.rs`); the rest of the app talks to it via
  a command channel. Recording is gated by an atomic flag, so starting capture costs
  one buffer callback (~16 ms), never a device re-open.
- **Ring buffer** uses monotonic counters (no wrap-around ambiguity), `Release`/
  `Acquire` publication, and an overwrite-oldest policy so the newest audio always wins.
- **Text injection** uses `KEYEVENTF_UNICODE` (UTF-16 code units), not virtual key
  codes — the only correct way to type Persian regardless of the active keyboard layout.
- **VAD endpointing** requires ≥ 150 ms of speech before an utterance is accepted and
  finalizes after 1500 ms of trailing silence; sub-threshold utterances (clicks, taps)
  are discarded without calling ASR.
- **Engines** sit behind the `AsrEngine` trait with a priority router (health checks,
  30 s cooldown after failure); cloud engines (Groq/Web Speech) can be added later
  without touching the state machine.

## Status vs. the v2 proposal

| Metric                    | v1 (Python) | v2 target | v2 now            |
| ------------------------- | ----------- | --------- | ----------------- |
| Hotkey → recording        | ~105 ms     | < 50 ms   | ~16–27 ms         |
| Recognition (5 s audio)   | 2–5 s       | < 1.5 s   | model-dependent   |
| Typing latency            | 550 ms      | < 100 ms  | < 5 ms            |
| exe size                  | 180 MB      | < 10 MB   | 27.8 MB (whisper static) |
| RAM (idle)                | 120 MB      | < 10 MB   | measured at runtime |
| Offline                   | partial     | yes       | yes (after model download) |

## License

MIT
