//! Sound in and out of a call (ADR-0035): the devices behind a trait (PulseAudio, or a tone
//! and a recorder for tests), the microphone thread (noise suppression, level, Opus) and
//! the speaker thread.

pub mod backend;
pub(crate) mod capture;
pub(crate) mod devices;
pub(crate) mod level;
pub(crate) mod mic_test;
pub(crate) mod playback;
pub mod pulse;
pub mod tone;
