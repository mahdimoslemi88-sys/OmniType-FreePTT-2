# OmniType-FreePTT-2 — v2

نسخهٔ دوم **OmniType-FreePTT**: تایپ صوتی Push-to-Talk برای ویندوز، فارسی‌محور،
کاملاً آفلاین و رایگان — بازنویسی‌شده از Python (v1) به **Rust**.

The v2 rewrite of [OmniType-FreePTT](https://github.com/mahdimoslemi88-sys/OmniType-FreePTT)
(Python) as a native Windows Push-to-Talk voice-typing tool built in Rust.

## Goals

| Goal | Target |
|---|---|
| Latency | < 1 s for a 5 s utterance |
| Persian accuracy | WER < 10 % |
| Cost | free — no mandatory API key |
| Footprint | RAM < 50 MB, small binary (whisper.cpp + egui statically linked) |
| Network | 100 % offline core; optional OpenAI-compatible cloud engine with auto-fallback |

## Repository layout

```
v-2/
├── voice-ptt/    # The Rust application (cpal → ring buffer → VAD → whisper → injection)
├── docs/         # Research notes, proposal, testing guide
└── .gitignore    # Keeps weights, build artifacts, and secrets out of git
```

The v1 Python implementation is intentionally **not** part of this repository.

## Build & test

```powershell
cd voice-ptt
cargo build --release      # produces target/release/voice-ptt.exe
cargo test                 # 82 tests
cargo clippy --all-targets # zero warnings expected
```

## Quick start

See **[`voice-ptt/README.md`](voice-ptt/README.md)** for the full pipeline
description, configuration reference (`%APPDATA%\voice-ptt\config.toml`), model
download notes, and the optional cloud (Groq) setup with daily-quota tracking.

For hands-on manual testing (tray, hotkey, VAD auto-stop, cloud failover), see
**[`docs/TESTING-guide.md`](docs/TESTING-guide.md)**.

## Docs

- [`docs/proposal-0.1.md`](docs/proposal-0.1.md) — product proposal
- [`docs/Comprehensive-research-program-developing-roadmap.md`](docs/Comprehensive-research-program-developing-roadmap.md) — roadmap
- [`docs/reaserch/WASAPI Audio Pipeline.md`](docs/reaserch/WASAPI%20Audio%20Pipeline.md) — capture research
- [`docs/reaserch/whisper.cpp Performance Benchmark.md`](docs/reaserch/whisper.cpp%20Performance%20Benchmark.md) — engine benchmarks
- [`docs/reaserch/Web Speech API.md`](docs/reaserch/Web%20Speech%20API.md) — browser-engine research
