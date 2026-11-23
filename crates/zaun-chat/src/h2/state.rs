//! What a connection keeps: its options, a header block in progress, and the request in flight.
use super::hpack_decode::Headers;

#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Receive window announced per stream.
    pub stream_window: u32,
    /// Receive window announced for the whole connection.
    pub connection_window: u32,
    pub max_header_bytes: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            stream_window: 8 << 20,
            connection_window: 16 << 20,
            max_header_bytes: 1 << 20,
        }
    }
}

/// A header block waiting for its CONTINUATION frames.
pub(super) struct HeaderBlock {
    pub(super) stream: u32,
    pub(super) bytes: Vec<u8>,
    pub(super) end_stream: bool,
}

/// The state of the request in flight.
pub(super) struct Exchange {
    pub(super) id: u32,
    pub(super) send_window: i64,
    pub(super) recv_window: i64,
    pub(super) headers: Option<Headers>,
    pub(super) trailers: Headers,
    pub(super) body: Vec<u8>,
    pub(super) max_body: usize,
    pub(super) sent: usize,
    pub(super) ended: bool,
}
