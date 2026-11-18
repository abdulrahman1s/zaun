//! Sound devices behind a trait: 48 kHz mono, 16-bit, 20 ms frames.
use std::io;

pub const RATE: u32 = 48_000;
/// Samples in one 20 ms frame (one Opus packet).
pub const FRAME: usize = 960;

/// A microphone: each read blocks until a whole frame was recorded.
pub trait AudioIn: Send {
    fn read(&mut self, frame: &mut [i16; FRAME]) -> io::Result<()>;
}

/// Speakers: a write blocks while the device's buffer is full.
pub trait AudioOut: Send {
    fn write(&mut self, frame: &[i16]) -> io::Result<()>;
}

/// Opens devices by name (empty = the system default). Called on the thread that will use
/// the device.
pub trait AudioBackend: Send + Sync {
    fn open_input(&self, device: &str) -> io::Result<Box<dyn AudioIn>>;
    fn open_output(&self, device: &str) -> io::Result<Box<dyn AudioOut>>;
    /// The microphones and speakers (ADR-0035); none by default. May block for a moment.
    fn devices(&self) -> io::Result<zaun_core::model::AudioDevices> {
        Ok(zaun_core::model::AudioDevices::default())
    }
}
