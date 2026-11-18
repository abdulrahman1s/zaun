//! Game streams behind a trait (ADR-0036): the user shares one game's window with a friend,
//! or watches a friend's. Each stream is its own peer connection, signalled through the chat
//! engine as a video call; the real one is `zaun-media` (portal capture, H.264, str0m),
//! tests use [`super::fake_stream::FakeStreams`].
use std::sync::Arc;

use super::frame::FrameSlot;
use crate::error::{ErrorCode, Result};
use crate::model::{IceServer, StreamInfo};

/// What a share sends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShareConfig {
    /// The game's title (the friend's stage names it).
    pub title: String,
    /// Picks which window the portal remembers for this game (its id).
    pub game: String,
    /// The game's processes (to find its sound).
    pub pids: Vec<u32>,
    /// The picture's height at most (`None`: as the game draws it).
    pub max_height: Option<u32>,
    /// Frames a second at most (never more than the game draws).
    pub fps: u32,
    /// Try the GPU's encoder first (`chat.share_hw_encode`).
    pub hardware: bool,
    /// `chat.share_sound`.
    pub sound: bool,
    pub ice: Vec<IceServer>,
    pub relay_only: bool,
}

/// What watching a friend's stream needs.
#[derive(Clone, Debug)]
pub struct WatchConfig {
    pub ice: Vec<IceServer>,
    pub relay_only: bool,
    /// Where decoded frames go (one stream is watched at a time).
    pub frames: FrameSlot,
}

/// What a stream's media reports, on its own threads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StreamEvent {
    /// Sharing: the window is chosen and its frames come.
    Ready,
    /// The local session description, every candidate in it.
    Description(String),
    /// The friend's side is reached.
    Connected,
    /// The path was lost; the media keeps trying.
    Interrupted,
    /// Sharing: the friend started (or stopped) watching.
    Watching(bool),
    /// Sharing: what is sent now (the encoder opened, the size or rate changed).
    Sending(StreamInfo),
    /// The picture stopped coming (the game's window is hidden or minimised) or came back.
    /// Sharing: the capture; watching: the friend says so.
    Paused(bool),
    /// Watching: the game the friend shares.
    Title(String),
    /// Sharing: the game's window went away.
    Ended,
    /// The stream cannot go on.
    Failed {
        /// Why, for the user (`ChtShareRefused`, `ChtCaptureFailed`, `ChtStreamFailed`).
        code: ErrorCode,
        /// For the log.
        detail: String,
    },
}

/// Where a stream's events go: called on the media's threads.
pub type StreamSink = Arc<dyn Fn(StreamEvent) + Send + Sync>;

/// One running stream, shared or watched. Dropping it stops everything: the capture, the
/// encoder or decoder, sockets and relay allocations.
pub trait StreamMedia: Send {
    /// Sharing: the friend's offer arrived; answer it ([`StreamEvent::Description`]).
    fn offer(&self, sdp: String);
    /// Watching: the friend's answer to our offer.
    fn answer(&self, sdp: String);
    /// More of the friend's ICE candidates.
    fn candidates(&self, candidates: Vec<String>);
    /// Watching: start (or stop) receiving the picture.
    fn watch(&self, on: bool);
    /// Watching: the stream's sound off (or on).
    fn mute(&self, on: bool);
    /// Sharing: send at most this height (`None`: as in the game) and frame rate from now
    /// on, without stopping; the next picture is a keyframe.
    fn set_quality(&self, max_height: Option<u32>, fps: u32);
}

/// Starts streams.
pub trait StreamFactory: Send + Sync {
    /// Share a game: the portal asks the user for the game's window ([`StreamEvent::Ready`]);
    /// the connection is made when the friend's offer arrives ([`StreamMedia::offer`]).
    fn share(&self, config: ShareConfig, sink: StreamSink) -> Result<Box<dyn StreamMedia>>;
    /// Take a friend's stream: write the offer ([`StreamEvent::Description`]), receive
    /// nothing until [`StreamMedia::watch`].
    fn watch(&self, config: WatchConfig, sink: StreamSink) -> Result<Box<dyn StreamMedia>>;
    /// Whether windows can be captured here (a build without capture can still watch, but
    /// the page offers streams only when both work).
    fn can_share(&self) -> bool {
        true
    }
}
