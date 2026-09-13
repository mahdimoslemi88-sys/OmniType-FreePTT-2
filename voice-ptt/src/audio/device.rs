//! Audio input device discovery.

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use serde::{Deserialize, Serialize};

/// A serializable snapshot of an input device's identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputDeviceInfo {
    pub name: String,
    pub default_sample_rate: u32,
    pub max_channels: u16,
}

/// Lists every input (microphone) device exposed by the default host.
pub fn list_input_devices() -> Result<Vec<InputDeviceInfo>> {
    let host = cpal::default_host();
    let mut out = Vec::new();
    for dev in host
        .input_devices()
        .context("failed to enumerate input devices")?
    {
        let name = match dev.name() {
            Ok(n) => n,
            Err(_) => continue, // some devices fail to report a name; skip them
        };
        let (rate, channels) = match dev.default_input_config() {
            Ok(c) => (c.sample_rate().0, c.channels()),
            Err(_) => (0, 0),
        };
        out.push(InputDeviceInfo {
            name,
            default_sample_rate: rate,
            max_channels: channels,
        });
    }
    Ok(out)
}

/// Resolves the capture device: the named device if given and present,
/// otherwise the system default input device.
pub fn resolve_input_device(name: Option<&str>) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if let Some(want) = name {
        for dev in host.input_devices()? {
            if dev.name().ok().as_deref() == Some(want) {
                return Ok(dev);
            }
        }
        anyhow::bail!("input device not found: {want}");
    }
    host.default_input_device()
        .context("no default input device available (is a microphone connected?)")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On CI/VMs without microphones this must still return a clean error,
    /// never panic.
    #[test]
    fn resolve_never_panics_without_device() {
        let r = resolve_input_device(Some("definitely-not-a-real-device-xyz"));
        assert!(r.is_err());
    }
}
