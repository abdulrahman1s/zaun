//! Encoding and decoding failures (Encoding.hs parser errors).
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DecodeError {
    #[error("unexpected end of input")]
    Eof,
    #[error("{0} bytes left after the value")]
    Trailing(usize),
    #[error("invalid {0} tag {1:#04x}")]
    BadTag(&'static str, u8),
    #[error("empty list")]
    EmptyList,
    #[error("invalid UTF-8")]
    Utf8,
    #[error("invalid base64url")]
    Base64,
    #[error("invalid {0}")]
    Invalid(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EncodeError {
    #[error("{what} of {len} exceeds the limit of {max}")]
    TooLong {
        what: &'static str,
        len: usize,
        max: usize,
    },
    #[error("empty list")]
    EmptyList,
}
