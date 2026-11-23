//! Ratchet failures (Crypto.hs `CryptoError`: `CERatchet*`, `CryptoHeaderError`, `CryptoLargeMsgError`). The variants stay apart because the resync state machine branches on them (Agent/Protocol.hs).
use thiserror::Error;

use crate::crypto::CryptoError;
use crate::encoding::{DecodeError, EncodeError};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RatchetError {
    #[error("message too large to pad")]
    LargeMsg,
    #[error("bad ratchet header: {0}")]
    Header(&'static str),
    #[error("no sending ratchet")]
    State,
    #[error("bad ratchet KEM state")]
    KemState,
    #[error("ratchet header was not decrypted by any key")]
    HeaderKey,
    #[error("too many skipped messages ({0})")]
    TooManySkipped(u32),
    #[error("message is {0} earlier than expected")]
    EarlierMessage(u32),
    #[error("duplicate message")]
    DuplicateMessage,
    #[error("message authentication failed")]
    AesDecrypt,
    #[error("post-quantum KEM: {0}")]
    Kem(&'static str),
    #[error("unsupported ratchet version")]
    Version,
    #[error("bad ratchet state: {0}")]
    BadState(&'static str),
    #[error(transparent)]
    Crypto(CryptoError),
    #[error(transparent)]
    Decode(#[from] DecodeError),
    #[error(transparent)]
    Encode(#[from] EncodeError),
}

impl From<CryptoError> for RatchetError {
    fn from(e: CryptoError) -> Self {
        match e {
            CryptoError::LargeMsg => Self::LargeMsg,
            CryptoError::AesDecrypt | CryptoError::Aead => Self::AesDecrypt,
            e @ (CryptoError::InvalidMsg
            | CryptoError::CbDecrypt
            | CryptoError::BadKey(_)
            | CryptoError::Dh
            | CryptoError::Random
            | CryptoError::WrongAlg(_)) => Self::Crypto(e),
        }
    }
}

pub type Result<T, E = RatchetError> = std::result::Result<T, E>;
