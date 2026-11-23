//! Chat message layer errors (Protocol.hs `parseChatMessages` strings, Compression.hs).
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatError {
    /// A message or batch that does not parse; the text names the rule.
    Parse(String),
    /// A size, count or depth limit was exceeded.
    Limit(&'static str),
    /// The compressed body does not fit after compression (`large compressed body`).
    TooLarge,
    Compression(String),
    Random,
}

pub type Result<T, E = ChatError> = std::result::Result<T, E>;

impl fmt::Display for ChatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(s) => write!(f, "bad chat message: {s}"),
            Self::Limit(s) => write!(f, "limit exceeded: {s}"),
            Self::TooLarge => f.write_str("large compressed body"),
            Self::Compression(s) => write!(f, "compression: {s}"),
            Self::Random => f.write_str("the system random source failed"),
        }
    }
}

impl std::error::Error for ChatError {}

impl From<serde_json::Error> for ChatError {
    fn from(e: serde_json::Error) -> Self {
        Self::Parse(e.to_string())
    }
}

pub(crate) fn parse_err<T>(s: impl Into<String>) -> Result<T> {
    Err(ChatError::Parse(s.into()))
}
