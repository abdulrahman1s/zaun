//! One HTTP/2 request and its response, as the client sees them.
use super::hpack_decode::Headers;

pub struct Request<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub authority: &'a str,
    pub headers: &'a [(&'a str, &'a str)],
    pub body: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub headers: Headers,
    pub trailers: Headers,
    pub body: Vec<u8>,
}

/// Body bytes moved so far, handed to the watcher after each step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Progress {
    pub sent: usize,
    pub received: usize,
}
