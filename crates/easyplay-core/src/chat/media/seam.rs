//! The media of one call behind a trait (ADR-0035): one peer connection per call, tracks
//! added by kind. The real one is `easyplay-media` (WebRTC through str0m on its own
//! threads); tests use [`super::fake::FakeMedia`].
use std::sync::Arc;
use std::time::Duration;

use crate::error::{ErrorCode, Result};
use crate::model::{AudioDevices, IceServer};

/// Unanswered this long, a call stops ringing and becomes a missed call (chat.html).
pub const RING_TIMEOUT: Duration = Duration::from_secs(20);
/// A ringing call is let go here this much after the caller's own timeout, which normally
/// ends it first (so a caller's line never reads "declined" for a call that merely rang out).
pub const RING_GRACE: Duration = Duration::from_secs(5);
/// Answered but without sound getting through this long, the call failed to connect.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// What a track carries. A call has one audio track; a shared game (R-26) adds video.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TrackKind {
    Audio,
    Video,
}

/// Which side writes the session description. With the chat engine the friend who answers
/// writes the offer and the one who called answers it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Negotiation {
    /// Write an offer ([`MediaEvent::Description`]); the answer comes through
    /// [`CallMedia::answer`].
    Offer,
    /// Answer this offer (plain SDP).
    Answer(String),
}

/// `chat.mic`, `chat.speakers`, `chat.noise_suppression`, `chat.mic_volume`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioSettings {
    /// The microphone (a sound server's source name; empty = the system default).
    pub mic: String,
    /// The speakers (a sink name; empty = the system default).
    pub speakers: String,
    pub noise_suppression: bool,
    /// The gain on what the microphone records, in percent (`0 ..= 100`).
    pub mic_volume: u8,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            mic: String::new(),
            speakers: String::new(),
            noise_suppression: false,
            mic_volume: 100,
        }
    }
}

/// What one call's media is started with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaConfig {
    pub negotiation: Negotiation,
    /// The tracks, sent and received (`[Audio]` for a voice call).
    pub tracks: Vec<TrackKind>,
    /// STUN and TURN servers (`chat.ice_servers`, else [`super::DEFAULT_ICE_SERVERS`]).
    pub ice: Vec<IceServer>,
    /// `chat.relay_calls`: only relay candidates, so the friend never learns the address.
    pub relay_only: bool,
    pub audio: AudioSettings,
}

/// What a call's media reports, on its own thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaEvent {
    /// The local session description (an offer or an answer, by [`Negotiation`]) with every
    /// candidate already in it.
    Description(String),
    /// Sound gets through both ways.
    Connected,
    /// The path was lost; the media keeps trying.
    Interrupted,
    /// Voice levels 0..=100: the user's microphone (after mute) and the friend's sound.
    Levels { me: u8, friend: u8 },
    /// The path's round trip in milliseconds and the packets lost in percent, about once a
    /// second.
    Quality { rtt_ms: u16, loss: u8 },
    /// The media stopped and cannot go on.
    Failed {
        /// Why, for the user (`ChtNoMicrophone`, `ChtMediaFailed`, `ChtCallFailed`).
        code: ErrorCode,
        /// For the log.
        detail: String,
    },
}

/// Where a call's media events go: called on the media's own threads.
pub type MediaSink = Arc<dyn Fn(MediaEvent) + Send + Sync>;

/// One running call's media. Dropping it hangs up: sockets, relay allocations and the sound
/// devices are released.
pub trait CallMedia: Send {
    /// The friend's answer to our offer (plain SDP).
    fn answer(&self, sdp: String);
    /// More of the friend's ICE candidates (`candidate:…` lines).
    fn candidates(&self, candidates: Vec<String>);
    /// Stop (or resume) sending the microphone.
    fn set_muted(&self, on: bool);
    /// Stop (or resume) playing the friend's sound.
    fn set_deafened(&self, on: bool);
}

/// What the microphone test reports, on its own thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MicTestEvent {
    /// The input level, 0..=100 (after the volume and noise suppression), about 20 Hz.
    Level(u8),
    /// The microphone or the speakers failed (`ChtNoMicrophone`, `ChtMediaFailed`).
    Failed { code: ErrorCode, detail: String },
}

/// Where the microphone test's events go.
pub type MicTestSink = Arc<dyn Fn(MicTestEvent) + Send + Sync>;

/// A running microphone test: the microphone is open, its sound plays back about half a
/// second later on the speakers. Dropping it closes both.
pub trait MicTest: Send {}

/// Starts calls' media.
pub trait MediaFactory: Send + Sync {
    /// Start the media of one call; events go to `sink` until it is dropped.
    fn start(&self, config: MediaConfig, sink: MediaSink) -> Result<Box<dyn CallMedia>>;
    /// The microphones and speakers. Blocks for a moment (it asks the sound server): call it
    /// on the job pool. None by default.
    fn devices(&self) -> Result<AudioDevices> {
        Ok(AudioDevices::default())
    }

    /// Open the microphone of `audio` (with its volume and noise suppression), report its
    /// level, and play it back on its speakers half a second late. Unsupported by default.
    fn mic_test(&self, audio: AudioSettings, sink: MicTestSink) -> Result<Box<dyn MicTest>> {
        let _ = (audio, sink);
        Err(crate::error::Error::new(ErrorCode::ChtMediaFailed)
            .with("detail", "no microphone test in this build"))
    }

    /// How long to really wait for a call timeout of `wait` ([`RING_TIMEOUT`],
    /// [`CONNECT_TIMEOUT`]). The wait itself by default; the fake shortens it.
    fn wait(&self, wait: Duration) -> Duration {
        wait
    }
    /// Game streams through the same media (ADR-0036); `None`: this media cannot stream.
    fn streams(&self) -> Option<Arc<dyn super::stream::StreamFactory>> {
        None
    }
}
