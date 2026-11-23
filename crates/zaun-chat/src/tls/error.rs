//! TLS failures, named after the alert each would send (RFC 8446 section 6).
use std::io;

use thiserror::Error;

use super::alert::Alert;
use super::der::CertError;
use crate::crypto::CryptoError;

#[derive(Debug, Error)]
pub enum TlsError {
    #[error("tls i/o: {0}")]
    Io(#[from] io::Error),
    #[error("server alert: {0}")]
    Alert(Alert),
    #[error("decode_error: {0}")]
    Decode(&'static str),
    #[error("unexpected_message: {0}")]
    Protocol(&'static str),
    #[error("illegal_parameter: {0}")]
    Illegal(&'static str),
    #[error("unsupported: {0}")]
    Unsupported(&'static str),
    #[error("bad_record_mac")]
    BadRecordMac,
    #[error("record_overflow")]
    RecordOverflow,
    #[error("decrypt_error: server Finished does not verify")]
    BadFinished,
    #[error("decrypt_error: CertificateVerify does not verify")]
    BadCertificateVerify,
    #[error("bad_certificate: empty certificate chain")]
    EmptyChain,
    #[error("no_application_protocol: the server chose an ALPN we did not offer")]
    Alpn,
    #[error("HelloRetryRequest is not supported")]
    HelloRetryRequest,
    #[error("bad_certificate: {0}")]
    Certificate(#[from] CertError),
    #[error("internal_error: {0}")]
    Internal(&'static str),
    #[error("internal_error: {0}")]
    Crypto(#[from] CryptoError),
}

impl From<TlsError> for io::Error {
    fn from(e: TlsError) -> Self {
        match e {
            TlsError::Io(e) => e,
            TlsError::Alert(_) => io::Error::new(io::ErrorKind::ConnectionAborted, e),
            TlsError::Decode(_)
            | TlsError::Protocol(_)
            | TlsError::Illegal(_)
            | TlsError::Unsupported(_)
            | TlsError::BadRecordMac
            | TlsError::RecordOverflow
            | TlsError::BadFinished
            | TlsError::BadCertificateVerify
            | TlsError::EmptyChain
            | TlsError::Alpn
            | TlsError::HelloRetryRequest
            | TlsError::Certificate(_)
            | TlsError::Internal(_)
            | TlsError::Crypto(_) => io::Error::new(io::ErrorKind::InvalidData, e),
        }
    }
}
