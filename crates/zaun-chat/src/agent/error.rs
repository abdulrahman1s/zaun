//! Failures of the agent: codecs, link data, and the connection layer (Agent/Protocol.hs `AgentErrorType`, simplexmq 7.1.4, AGPL-3.0-only).
use crate::crypto::CryptoError;
use crate::encoding::{DecodeError, EncodeError};
use crate::ratchet::RatchetError;
use crate::smp::SmpError;
use crate::store::StoreError;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Error)]
pub enum AgentError {
    #[error("bad link or message: {0}")]
    Parse(&'static str),
    #[error("missing query parameter {0}")]
    MissingParam(&'static str),
    #[error("link data: {0}")]
    Link(String),
    #[error(transparent)]
    Decode(#[from] DecodeError),
    #[error(transparent)]
    Encode(#[from] EncodeError),
    #[error(transparent)]
    Crypto(#[from] CryptoError),
    #[error("store: {0}")]
    Store(String),
    #[error("smp: {0}")]
    Smp(#[from] SmpError),
    #[error("ratchet: {0}")]
    Ratchet(#[from] RatchetError),
    /// `A_VERSION`.
    #[error("no common version with the peer")]
    Version,
    /// `CMD PROHIBITED`: the request or message does not fit the connection's state.
    #[error("prohibited: {0}")]
    Prohibited(&'static str),
    #[error("no such connection")]
    ConnNotFound,
    #[error("no such message")]
    MsgNotFound,
    /// `NOT_AVAILABLE`: the peer's queue is full while it is being joined.
    #[error("the peer's queue is not available")]
    NotAvailable,
    /// The message could not be delivered before its deadline.
    #[error("message expired")]
    Expired,
    #[error("not supported yet: {0}")]
    Unsupported(&'static str),
}

/// `StoreError` holds driver errors that are neither `Clone` nor comparable, so it is kept as text.
impl From<StoreError> for AgentError {
    fn from(e: StoreError) -> Self {
        Self::Store(e.to_string())
    }
}

pub type Result<T, E = AgentError> = std::result::Result<T, E>;
