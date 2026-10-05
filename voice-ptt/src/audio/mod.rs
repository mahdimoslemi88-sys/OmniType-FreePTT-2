//! Audio layer: device discovery, capture and the lock-free ring buffer.

pub mod capture;
pub mod device;
pub mod diagnostics;
// Crate-visible, not public: the gate needs the crate-private status channel,
// and it has no use outside this crate. A public one would be a `pub fn` whose
// argument nobody outside can name.
pub(crate) mod gate;
pub mod ring_buffer;

pub use capture::{AudioCapture, CaptureConfig};
pub use device::{list_input_devices, InputDeviceInfo};
pub use ring_buffer::RingBuffer;
