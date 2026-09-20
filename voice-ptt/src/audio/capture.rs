//! Microphone capture on WASAPI Shared Mode via `cpal`.
//!
//! Threading model: `cpal::Stream` is `!Send` on Windows (raw COM pointers)
//! and must be created *and* dropped on the same thread. A dedicated audio
//! thread therefore owns the stream; the rest of the app communicates via a
//! command channel. Recording is gated by an atomic flag copied into the
//! callback, so pressing the hotkey starts pulling samples within one buffer
//! callback (~16 ms) with zero device re-open cost.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{BufferSize, SampleRate, Stream, StreamConfig};

use super::device::resolve_input_device;
use super::ring_buffer::RingBuffer;

/// Capture parameters (spec: 16 kHz mono, 256-sample frames).
#[derive(Debug, Clone)]
pub struct CaptureConfig {
    pub sample_rate: u32,
    pub channels: u16,
    pub buffer_frames: u32,
    /// Ring buffer duration in seconds (spec: 30 s).
    pub ring_seconds: u32,
    pub device_name: Option<String>,
    pub gain_db: f32,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16_000,
            channels: 1,
            buffer_frames: 256,
            ring_seconds: 30,
            device_name: None,
            gain_db: 0.0,
        }
    }
}

/// Commands sent to the audio thread that owns the stream.
enum StreamCommand {
    Play,
    Pause,
    Exit,
}

/// Owns the capture session; the stream itself lives on a private thread.
pub struct AudioCapture {
    ring_buffer: Arc<RingBuffer>,
    is_recording: Arc<AtomicBool>,
    cmd_tx: std::sync::mpsc::Sender<StreamCommand>,
    worker: Option<std::thread::JoinHandle<()>>,
    sample_rate: u32,
    channels: u16,
    /// The device's native sample rate (for upstream resampling decisions).
    native_sample_rate: u32,
    gain_factor: f32,
}

impl AudioCapture {
    /// Opens the device and starts the (gated) stream on its worker thread.
    pub fn new(config: &CaptureConfig) -> Result<Self> {
        let ring_buffer = Arc::new(RingBuffer::new(
            config.sample_rate as usize * config.ring_seconds as usize,
        ));
        let is_recording = Arc::new(AtomicBool::new(false));

        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<StreamCommand>();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<u32, String>>();

        let ring_clone = ring_buffer.clone();
        let rec_clone = is_recording.clone();
        let cfg = config.clone();

        let worker = std::thread::Builder::new()
            .name("audio-capture".into())
            .spawn(move || {
                let built = build_stream(&cfg, &ring_clone, &rec_clone);
                match built {
                    Ok((stream, native_rate)) => {
                        if stream.play().is_err() {
                            let _ = ready_tx.send(Err("failed to start capture stream".into()));
                            return;
                        }
                        let _ = ready_tx.send(Ok(native_rate));

                        // Command loop: the stream lives (and dies) here.
                        for cmd in cmd_rx {
                            match cmd {
                                StreamCommand::Play => {
                                    let _ = stream.play();
                                }
                                StreamCommand::Pause => {
                                    let _ = stream.pause();
                                }
                                StreamCommand::Exit => break,
                            }
                        }
                        drop(stream); // dropped on the creating thread ✓
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(format!("{e:#}")));
                    }
                }
            })
            .context("failed to spawn audio thread")?;

        let native_sample_rate = ready_rx
            .recv()
            .context("audio thread died before reporting status")?
            .map_err(|e| anyhow::anyhow!(e))?;

        tracing::info!(
            native_rate = native_sample_rate,
            requested_rate = config.sample_rate,
            frames = config.buffer_frames,
            "capture stream running on audio thread"
        );

        let gain_factor = if config.gain_db.abs() < 1e-4 {
            1.0
        } else {
            10.0f32.powf(config.gain_db / 20.0)
        };

        Ok(Self {
            ring_buffer,
            is_recording,
            cmd_tx,
            worker: Some(worker),
            sample_rate: config.sample_rate,
            channels: config.channels,
            native_sample_rate,
            gain_factor,
        })
    }

    /// Starts gated capture into the ring buffer.
    pub fn start(&self) -> Result<()> {
        self.ring_buffer.clear();
        self.is_recording.store(true, Ordering::Relaxed);
        self.cmd_tx
            .send(StreamCommand::Play)
            .context("audio thread unavailable")?;
        tracing::debug!("capture started");
        Ok(())
    }

    /// Stops capture (samples stop flowing; the stream stays alive).
    pub fn stop(&self) -> Result<()> {
        self.is_recording.store(false, Ordering::Relaxed);
        self.cmd_tx
            .send(StreamCommand::Pause)
            .context("audio thread unavailable")?;
        tracing::debug!("capture stopped");
        Ok(())
    }

    /// Removes and returns everything currently buffered, resampled to the
    /// pipeline sample rate when the device runs at its native rate, with
    /// software gain applied if configured.
    ///
    /// Why: the mic rejected our 16 kHz request and runs at 48 kHz, but the
    /// whole downstream pipeline (Silero VAD chunking, whisper, duration
    /// math) assumes 16 kHz. Resample here, at the boundary.
    pub fn take_audio(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.ring_buffer.available());
        self.ring_buffer.drain(&mut out);
        let mut res = if self.native_sample_rate == self.sample_rate || out.is_empty() {
            out
        } else {
            resample_linear(&out, self.native_sample_rate, self.sample_rate)
        };
        if (self.gain_factor - 1.0).abs() > 1e-4 && !res.is_empty() {
            for s in &mut res {
                *s = (*s * self.gain_factor).clamp(-1.0, 1.0);
            }
        }
        res
    }

    /// Sample rate the pipeline receives from [`take_audio`](Self::take_audio).
    pub fn pipeline_sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Samples currently waiting in the ring buffer.
    pub fn available_samples(&self) -> usize {
        self.ring_buffer.available()
    }

    /// Total ring buffer capacity in samples (endpointing safety valve).
    pub fn capacity(&self) -> usize {
        self.ring_buffer.capacity()
    }

    /// Buffered audio duration at the configured sample rate.
    pub fn available_duration(&self) -> Duration {
        Duration::from_secs_f64(self.available_samples() as f64 / self.sample_rate as f64)
    }

    pub fn is_recording(&self) -> bool {
        self.is_recording.load(Ordering::Relaxed)
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn native_sample_rate(&self) -> u32 {
        self.native_sample_rate
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(StreamCommand::Exit);
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}

/// Resamples `input` from `from_hz` to `to_hz` with linear interpolation.
///
/// Good enough for speech (whisper is robust to minor interpolation error);
/// avoids pulling in a heavy DSP crate for one narrow purpose.
/// Duration is preserved: `len_out = len_in * to_hz / from_hz` (rounded).
pub fn resample_linear(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
    if input.is_empty() || from_hz == to_hz || from_hz == 0 {
        return input.to_vec();
    }
    let ratio = f64::from(from_hz) / f64::from(to_hz);
    let out_len = ((input.len() as f64) / ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    // Step in input-sample units; clamp the last interpolation window.
    let step = ratio;
    for i in 0..out_len {
        let pos = i as f64 * step;
        let i0 = (pos.floor() as usize).min(input.len() - 1);
        let i1 = (i0 + 1).min(input.len() - 1);
        let frac = (pos - i0 as f64).clamp(0.0, 1.0);
        let sample = input[i0] * (1.0 - frac as f32) + input[i1] * frac as f32;
        out.push(sample);
    }
    out
}

/// Builds the input stream (runs on the audio thread).
fn build_stream(
    config: &CaptureConfig,
    ring_buffer: &Arc<RingBuffer>,
    is_recording: &Arc<AtomicBool>,
) -> Result<(Stream, u32)> {
    let device = resolve_input_device(config.device_name.as_deref())?;
    let native = device
        .default_input_config()
        .context("failed to read default input config")?;
    let native_sample_rate = native.sample_rate().0;

    let requested = StreamConfig {
        channels: config.channels,
        sample_rate: SampleRate(config.sample_rate),
        buffer_size: BufferSize::Fixed(config.buffer_frames),
    };

    let ring = ring_buffer.clone();
    let rec = is_recording.clone();
    let err_cb = |err| tracing::error!(%err, "audio stream error");

    let make = |cfg: StreamConfig| -> Result<Stream> {
        let ring = ring.clone();
        let rec = rec.clone();
        let channels = cfg.channels.max(1) as usize;
        // Interleaved multi-channel (e.g. stereo fallback) → average each
        // frame's channels into one mono sample. Without this, VAD/whisper
        // receive half-rate interleaved garbage when the fallback config is
        // stereo.
        let downmix = move |data: &[f32]| -> Vec<f32> {
            if channels == 1 {
                data.to_vec()
            } else {
                data.chunks_exact(channels)
                    .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                    .collect()
            }
        };
        let downmix_i16 = move |data: &[i16]| -> Vec<f32> {
            let conv = |s: &i16| *s as f32 / 32_768.0;
            if channels == 1 {
                data.iter().map(conv).collect()
            } else {
                data.chunks_exact(channels)
                    .map(|frame| frame.iter().map(conv).sum::<f32>() / channels as f32)
                    .collect()
            }
        };
        let downmix_u16 = move |data: &[u16]| -> Vec<f32> {
            let conv = |s: &u16| (*s as f32 - 32_768.0) / 32_768.0;
            if channels == 1 {
                data.iter().map(conv).collect()
            } else {
                data.chunks_exact(channels)
                    .map(|frame| frame.iter().map(conv).sum::<f32>() / channels as f32)
                    .collect()
            }
        };
        match native.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &cfg,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if rec.load(Ordering::Relaxed) {
                        ring.write(&downmix(data));
                    }
                },
                err_cb,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                &cfg,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if rec.load(Ordering::Relaxed) {
                        ring.write(&downmix_i16(data));
                    }
                },
                err_cb,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream(
                &cfg,
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    if rec.load(Ordering::Relaxed) {
                        ring.write(&downmix_u16(data));
                    }
                },
                err_cb,
                None,
            ),
            other => anyhow::bail!("unsupported sample format: {other:?}"),
        }
        .context("failed to build input stream")
    };

    // Try the requested config first; fall back to the device's native config
    // (some devices reject arbitrary rates). Upstream handles rate mismatch.
    let stream = match make(requested) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                %e,
                native_rate = native_sample_rate,
                native_channels = native.channels(),
                "requested capture config rejected; retrying with native config (downmix + resample on)"
            );
            make(StreamConfig::from(native.clone()))?
        }
    };

    Ok((stream, native_sample_rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_48k_to_16k_preserves_duration_and_shape() {
        // 4800 samples at 48 kHz = 100 ms → 1600 samples at 16 kHz.
        let input: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.01).sin()).collect();
        let out = resample_linear(&input, 48_000, 16_000);
        assert_eq!(out.len(), 1600);
        // Every third input sample maps onto consecutive output samples.
        assert!((out[100] - input[300]).abs() < 1e-4);
    }

    #[test]
    fn resample_same_rate_is_noop() {
        let input = vec![0.25f32; 1000];
        assert_eq!(resample_linear(&input, 16_000, 16_000), input);
    }

    #[test]
    fn resample_empty_is_noop() {
        assert!(resample_linear(&[], 48_000, 16_000).is_empty());
    }

    #[test]
    fn resample_dc_signal_stays_constant() {
        let input = vec![0.5f32; 9600]; // 200 ms @ 48 kHz
        let out = resample_linear(&input, 48_000, 16_000);
        assert_eq!(out.len(), 3200);
        assert!(out.iter().all(|&s| (s - 0.5).abs() < 1e-6));
    }
}
