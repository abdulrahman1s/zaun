//! What an XFTP transfer can fail with (FileTransfer/Transport.hs `XFTPErrorType`, Client.hs `XFTPClientError`).
use thiserror::Error;

use crate::crypto::CryptoError;
use crate::encoding::{DecodeError, EncodeError};
use crate::h2::{H2Error, Stop};

use super::error_type::XftpErrorType;

/// The handshake step that failed (Transport.hs `HandshakeError`, `TransportError`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum HandshakeError {
    #[error("no common XFTP version")]
    Version,
    #[error("session id differs from the TLS channel binding")]
    Session,
    #[error("server identity does not match the address")]
    Identity,
    #[error("server certificate or key signature is invalid")]
    BadAuth,
    #[error("malformed handshake")]
    Parse,
    #[error("server refused the handshake with HTTP {0}")]
    Status(u16),
}

#[derive(Debug, Error)]
pub enum XftpError {
    #[error("bad server address: {0}")]
    Address(&'static str),
    #[error("no XFTP server to use")]
    NoServers,
    #[error("connect: {0}")]
    Connect(String),
    #[error("encoding: {0}")]
    Encode(#[from] EncodeError),
    #[error("decoding: {0}")]
    Decode(#[from] DecodeError),
    #[error("crypto: {0}")]
    Crypto(#[from] CryptoError),
    #[error("http/2: {0}")]
    H2(H2Error),
    #[error("handshake: {0}")]
    Handshake(#[from] HandshakeError),
    #[error("server answered ERR {0}")]
    Server(XftpErrorType),
    #[error("server answered HTTP {0}")]
    Status(u16),
    #[error("unexpected {0}")]
    Unexpected(&'static str),
    #[error("file description: {0}")]
    Description(String),
    #[error("file: {0}")]
    File(String),
    #[error("{0} does not match the description")]
    Mismatch(&'static str),
    #[error("transfer cancelled")]
    Cancelled,
    #[error("transfer timed out")]
    TimedOut,
}

impl From<H2Error> for XftpError {
    fn from(e: H2Error) -> Self {
        match e {
            H2Error::Stopped(Stop::Cancelled) => Self::Cancelled,
            H2Error::Stopped(Stop::TimedOut) => Self::TimedOut,
            e @ (H2Error::Io(_)
            | H2Error::Protocol(_)
            | H2Error::FlowControl(_)
            | H2Error::FrameSize(_)
            | H2Error::Compression(_)
            | H2Error::Reset(_)
            | H2Error::GoAway(_)
            | H2Error::Closed
            | H2Error::TooLarge(_)) => Self::H2(e),
        }
    }
}

impl From<std::io::Error> for XftpError {
    fn from(e: std::io::Error) -> Self {
        Self::File(e.to_string())
    }
}

impl XftpError {
    /// A failure another attempt or another server may not repeat (network, timeout, server trouble).
    #[must_use]
    pub fn is_temporary(&self) -> bool {
        match self {
            Self::Connect(_) | Self::H2(_) | Self::TimedOut | Self::Status(_) => true,
            Self::Server(e) => e.is_temporary(),
            Self::Handshake(h) => matches!(h, HandshakeError::Status(_)),
            Self::Address(_)
            | Self::NoServers
            | Self::Encode(_)
            | Self::Decode(_)
            | Self::Crypto(_)
            | Self::Unexpected(_)
            | Self::Description(_)
            | Self::File(_)
            | Self::Mismatch(_)
            | Self::Cancelled => false,
        }
    }
}
