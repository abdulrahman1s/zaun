//! HTTP/2 client failures (RFC 9113 section 7); any of them leaves the connection unusable.
use std::io;

use thiserror::Error;

use super::hpack_int::HpackError;

/// Why a caller's watcher stopped a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Cancelled,
    TimedOut,
}

#[derive(Debug, Error)]
pub enum H2Error {
    #[error("h2 i/o: {0}")]
    Io(#[from] io::Error),
    #[error("h2 protocol error: {0}")]
    Protocol(&'static str),
    #[error("h2 flow control error: {0}")]
    FlowControl(&'static str),
    #[error("h2 frame size error: {0}")]
    FrameSize(&'static str),
    #[error("h2 {0}")]
    Compression(#[from] HpackError),
    #[error("stream reset by the server (code {0})")]
    Reset(u32),
    #[error("server closed the connection (GOAWAY code {0})")]
    GoAway(u32),
    #[error("the connection is closed")]
    Closed,
    #[error("response body exceeds {0} bytes")]
    TooLarge(usize),
    #[error("request stopped: {0:?}")]
    Stopped(Stop),
}
