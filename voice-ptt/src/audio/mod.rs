//! Audio layer: device discovery, capture and the lock-free ring buffer.

pub mod capture;
pub mod device;
pub mod ring_buffer;

pub use capture::{AudioCapture, CaptureConfig};
pub use device::{list_input_devices, InputDeviceInfo};
pub use ring_buffer::RingBuffer;
