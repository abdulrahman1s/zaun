//! What a request handler fails with: one of core's stable `ZN-CHT-*` codes and the engine's words.

use super::agent_port::AgentFailure;
use crate::chat::ChatError;
use crate::store::StoreError;

/// The codes the engine answers with (core's `ErrorCode` strings, ERROR_HANDLING §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    NoConnection,
    BadLink,
    RequestFailed,
    CallFailed,
    StreamFailed,
}

impl Code {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoConnection => "ZN-CHT-004",
            Self::BadLink => "ZN-CHT-005",
            Self::RequestFailed => "ZN-CHT-006",
            Self::CallFailed => "ZN-CHT-012",
            Self::StreamFailed => "ZN-CHT-017",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineError {
    pub code: Code,
    /// For the log; never message text.
    pub detail: String,
}

pub type Result<T, E = EngineError> = std::result::Result<T, E>;

#[must_use]
pub fn fail(code: Code, detail: impl Into<String>) -> EngineError {
    EngineError {
        code,
        detail: detail.into(),
    }
}

/// A request this engine does not carry yet.
#[must_use]
pub fn not_yet(code: Code) -> EngineError {
    fail(code, "not yet")
}

impl From<StoreError> for EngineError {
    fn from(e: StoreError) -> Self {
        fail(Code::RequestFailed, format!("store: {e}"))
    }
}

impl From<ChatError> for EngineError {
    fn from(e: ChatError) -> Self {
        fail(Code::RequestFailed, format!("chat message: {e}"))
    }
}

impl From<AgentFailure> for EngineError {
    fn from(e: AgentFailure) -> Self {
        let code = match &e {
            AgentFailure::Network(_) => Code::NoConnection,
            AgentFailure::BadLink(_) => Code::BadLink,
            AgentFailure::Other(_) => Code::RequestFailed,
        };
        fail(code, e.detail())
    }
}
