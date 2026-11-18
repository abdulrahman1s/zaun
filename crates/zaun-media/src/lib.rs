//! Call media for Zaun's Chat (ADR-0035): one WebRTC peer connection per call through
//! `str0m` (sans-IO, driven by our own threads and UDP socket, no async runtime), Opus voice
//! through PulseAudio (or PipeWire's PulseAudio server), noise suppression, and a TURN
//! client so friends never learn the user's address. Implements
//! `zaun_core::chat::media::MediaFactory`; the `zaun` binary wires it in.

pub mod audio;
pub mod capture;
mod clock;
mod factory;
mod net;
mod session;
mod stream;
pub mod video;

pub use audio::backend::{AudioBackend, AudioIn, AudioOut};
pub use audio::pulse::PulseAudio;
pub use audio::tone::{Recording, ToneAudio, tone_strength};
pub use factory::RtcMedia;
pub use video::source::{
    Capture, CaptureError, CaptureRequest, Next, NoCapture, PatternCapture, VideoSource,
};
