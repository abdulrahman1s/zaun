//! Everything the session thread waits for comes through one channel.
use std::net::SocketAddr;

use easyplay_core::error::ErrorCode;

pub(crate) enum Inbox {
    /// A datagram on the call's UDP socket.
    Udp {
        from: SocketAddr,
        data: Vec<u8>,
    },
    /// A message from TURN server `relay` over TCP or TLS (`None`: the stream closed).
    Stream {
        relay: usize,
        frame: Option<Vec<u8>>,
    },
    /// 20 ms from the microphone: an Opus packet (`None` while muted) and its level.
    Frame {
        packet: Option<Vec<u8>>,
        level: u8,
    },
    /// The friend's answer to our offer.
    Answer(String),
    /// More of the friend's candidates.
    Candidates(Vec<String>),
    Muted(bool),
    Deafened(bool),
    /// A sound thread failed.
    Failed {
        code: ErrorCode,
        detail: String,
    },
    /// Hang up.
    Stop,
    /// A stream's picture and notes (ADR-0036).
    Video(super::video::VideoMsg),
}
