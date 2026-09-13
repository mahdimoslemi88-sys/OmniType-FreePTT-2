//! Model downloader with progress reporting and resume support.
//!
//! The whole application is offline except for this module: models are pulled
//! from Hugging Face on first run into `models/`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use futures_util::StreamExt;

/// Known whisper.cpp ggml models hosted on Hugging Face.
pub const WHISPER_MODELS: &[(&str, &str)] = &[
    (
        "tiny",
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
    ),
    (
        "base",
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
    ),
    (
        "small",
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
    ),
    (
        "medium",
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin",
    ),
    (
        "large-v3",
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin",
    ),
    (
        "large-v3-turbo",
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
    ),
];

/// Silero VAD v5 ONNX model mirrors, tried in order. The Hugging Face
/// `onnx-community/silero-vad` paths are dead (HTTP 404/"Entry not found"),
/// so the official snakers4/silero-vad repo is the source of truth.
pub const SILERO_VAD_URLS: &[&str] = &[
    "https://github.com/snakers4/silero-vad/raw/master/src/silero_vad/data/silero_vad.onnx",
    "https://raw.githubusercontent.com/snakers4/silero-vad/master/src/silero_vad/data/silero_vad.onnx",
];

/// Primary Silero VAD URL (first mirror; kept for backwards compatibility).
pub const SILERO_VAD_URL: &str = SILERO_VAD_URLS[0];

/// Progress callback receives (downloaded_bytes, total_bytes_or_None).
pub type ProgressFn<'a> = &'a (dyn Fn(u64, Option<u64>) + Send + Sync);

/// Resolves the local path of a named model, downloading it when missing.
pub async fn ensure_model(
    models_dir: &Path,
    name: &str,
    progress: Option<ProgressFn<'_>>,
) -> Result<PathBuf> {
    let url = WHISPER_MODELS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, u)| *u)
        .with_context(|| {
            format!(
                "unknown model '{name}'; available: {}",
                WHISPER_MODELS
                    .iter()
                    .map(|(n, _)| *n)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;

    let dest = models_dir.join(format!("ggml-{name}.bin"));
    download_file(url, &dest, progress).await?;
    Ok(dest)
}

/// Resolves the Silero VAD model path, downloading when missing.
/// Tries every mirror in [`SILERO_VAD_URLS`] before giving up.
pub async fn ensure_vad_model(assets_dir: &Path, progress: Option<ProgressFn<'_>>) -> Result<PathBuf> {
    let dest = assets_dir.join("silero_vad.onnx");
    let mut last_err: Option<anyhow::Error> = None;
    for url in SILERO_VAD_URLS {
        match download_file(url, &dest, progress).await {
            Ok(()) => return Ok(dest),
            Err(e) => {
                tracing::warn!(url, error = %e, "silero mirror failed; trying next");
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no silero VAD mirror configured")))
}

/// Downloads `url` to `dest` with partial-file resume and progress.
pub async fn download_file(url: &str, dest: &Path, progress: Option<ProgressFn<'_>>) -> Result<()> {
    if dest.exists() {
        tracing::debug!(path = %dest.display(), "model already present");
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let part = dest.with_extension("part");
    let mut start: u64 = match tokio::fs::metadata(&part).await {
        Ok(m) => m.len(),
        Err(_) => 0,
    };

    tracing::info!(url, dest = %dest.display(), "downloading model");
    let client = reqwest::Client::builder()
        // A dead IPv6 path must not hang the whole download: fail that
        // address quickly so the resolver moves on to IPv4.
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .context("failed to build download client")?;
    let mut request = client.get(url);
    if start > 0 {
        request = request.header("Range", format!("bytes={start}-"));
    }
    let response = request.send().await.context("model download failed")?;

    let total = response.content_length().map(|l| l + start);
    if !response.status().is_success() {
        anyhow::bail!("download failed: HTTP {}", response.status());
    }

    // Server ignored the Range header → restart from scratch.
    if start > 0 && response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        start = 0;
    }

    let mut file = if start > 0 {
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(&part)
            .await
            .context("failed to open partial download")?
    } else {
        tokio::fs::File::create(&part)
            .await
            .context("failed to create download target")?
    };

    let mut stream = response.bytes_stream();
    let mut downloaded = start;
    let mut last_report = std::time::Instant::now();
    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.context("connection lost during download")?;
        file.write_all(&bytes).await.context("failed to write download")?;
        downloaded += bytes.len() as u64;
        if let Some(cb) = progress {
            // Throttle progress callbacks to ~10 Hz for UI sanity.
            if last_report.elapsed() >= std::time::Duration::from_millis(100) {
                cb(downloaded, total);
                last_report = std::time::Instant::now();
            }
        }
    }
    file.flush().await.context("failed to flush download")?;
    drop(file);

    tokio::fs::rename(&part, dest)
        .await
        .with_context(|| format!("failed to finalize {}", dest.display()))?;
    tracing::info!(dest = %dest.display(), bytes = downloaded, "model download complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silero_vad_url_serves_a_valid_onnx() {
        // Regression: the previous URL returned HTTP 200-ish error bodies
        // ("Entry not found"), so the file existed but was not a model.
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap();
        let resp = match client.get(SILERO_VAD_URL).send() {
            Ok(r) => r,
            Err(e) => {
                // Offline machine: nothing to assert, but never a false failure.
                eprintln!("skipping (network unavailable): {e}");
                return;
            }
        };
        assert!(resp.status().is_success(), "status: {}", resp.status());
        let bytes = resp.bytes().unwrap();
        assert!(bytes.len() > 1_000_000, "suspiciously small: {} bytes", bytes.len());
        // ONNX files are protobuf — they start with a field-1 (ir_version) tag.
        assert_eq!(&bytes[..2], &[0x08, 0x08], "not a valid ONNX header");
    }

    #[test]
    fn unknown_model_gives_clean_error() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(ensure_model(Path::new("models"), "gpt-5", None))
            .unwrap_err();
        assert!(err.to_string().contains("unknown model"));
    }
}
