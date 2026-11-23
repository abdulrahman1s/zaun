//! What an SMP client call can fail with (Protocol.hs `ErrorType`/`BrokerErrorType`, Transport.hs `TransportError`).
use crate::crypto::CryptoError;
use crate::encoding::{DecodeError, EncodeError};

use super::server_error::ErrorType;

/// The handshake step that failed (Transport.hs `HandshakeError`, `TransportError`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HandshakeError {
    #[error("no common SMP version")]
    Version,
    #[error("session id differs from the TLS channel binding")]
    Session,
    #[error("server identity does not match the address")]
    Identity,
    #[error("server certificate or key signature is invalid")]
    BadAuth,
    #[error("malformed handshake")]
    Parse,
    #[error("bad block")]
    Block,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SmpError {
    #[error("encoding: {0}")]
    Encode(#[from] EncodeError),
    #[error("decoding: {0}")]
    Decode(#[from] DecodeError),
    #[error("crypto: {0}")]
    Crypto(#[from] CryptoError),
    #[error("io: {0}")]
    Io(String),
    #[error("handshake: {0}")]
    Handshake(#[from] HandshakeError),
    #[error("server answered ERR {0}")]
    Server(ErrorType),
    #[error("command timed out")]
    Timeout,
    #[error("not connected")]
    NotConnected,
    #[error("server stopped answering")]
    Unresponsive,
    #[error("connection closed")]
    Closed,
    #[error("send queue is full")]
    Busy,
    #[error("too many commands waiting for an answer")]
    TooManyPending,
    #[error("unexpected {0}")]
    Unexpected(&'static str),
    #[error("transmission does not fit in a block")]
    LargeMsg,
    #[error("bad server address: {0}")]
    Address(&'static str),
}

impl From<std::io::Error> for SmpError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}
