//! The ERR payload: `ErrorType` and `CommandError` (Protocol.hs `instance Encoding ErrorType`).
use std::fmt;

use crate::encoding::DecodeError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandError {
    Unknown,
    Syntax,
    Prohibited,
    NoAuth,
    HasAuth,
    NoEntity,
}

impl CommandError {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "UNKNOWN",
            Self::Syntax => "SYNTAX",
            Self::Prohibited => "PROHIBITED",
            Self::NoAuth => "NO_AUTH",
            Self::HasAuth => "HAS_AUTH",
            Self::NoEntity => "NO_ENTITY",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "UNKNOWN" => Self::Unknown,
            "SYNTAX" => Self::Syntax,
            "PROHIBITED" => Self::Prohibited,
            "NO_AUTH" => Self::NoAuth,
            "HAS_AUTH" => Self::HasAuth,
            "NO_ENTITY" | "NO_QUEUE" => Self::NoEntity,
            _ => return None,
        })
    }
}

/// Free-text payloads (`PROXY`, `BLOCKED`, `STORE`, `NAME`) are kept as the server wrote them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorType {
    Block,
    Session,
    Cmd(CommandError),
    Proxy(String),
    Auth,
    Blocked(String),
    Service,
    Crypto,
    Quota,
    Store(String),
    Expired,
    NoMsg,
    LargeMsg,
    Internal,
    Name(String),
    Duplicate,
}

impl ErrorType {
    pub fn parse(bytes: &[u8]) -> Result<Self, DecodeError> {
        let text = std::str::from_utf8(bytes).map_err(|_| DecodeError::Utf8)?;
        let (tag, rest) = text.split_once(' ').unwrap_or((text, ""));
        let bad = DecodeError::Invalid("ERR type");
        Ok(match tag {
            "BLOCK" => Self::Block,
            "SESSION" => Self::Session,
            "CMD" => Self::Cmd(CommandError::parse(rest).ok_or(bad)?),
            "PROXY" => Self::Proxy(rest.to_owned()),
            "AUTH" => Self::Auth,
            "BLOCKED" => Self::Blocked(rest.to_owned()),
            "SERVICE" => Self::Service,
            "CRYPTO" => Self::Crypto,
            "QUOTA" => Self::Quota,
            "STORE" => Self::Store(rest.to_owned()),
            "EXPIRED" => Self::Expired,
            "NO_MSG" => Self::NoMsg,
            "LARGE_MSG" => Self::LargeMsg,
            "INTERNAL" => Self::Internal,
            "NAME" => Self::Name(rest.to_owned()),
            "DUPLICATE_" => Self::Duplicate,
            _ => return Err(bad),
        })
    }
}

impl fmt::Display for ErrorType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Block => f.write_str("BLOCK"),
            Self::Session => f.write_str("SESSION"),
            Self::Cmd(e) => write!(f, "CMD {}", e.as_str()),
            Self::Proxy(s) => write!(f, "PROXY {s}"),
            Self::Auth => f.write_str("AUTH"),
            Self::Blocked(s) => write!(f, "BLOCKED {s}"),
            Self::Service => f.write_str("SERVICE"),
            Self::Crypto => f.write_str("CRYPTO"),
            Self::Quota => f.write_str("QUOTA"),
            Self::Store(s) => write!(f, "STORE {s}"),
            Self::Expired => f.write_str("EXPIRED"),
            Self::NoMsg => f.write_str("NO_MSG"),
            Self::LargeMsg => f.write_str("LARGE_MSG"),
            Self::Internal => f.write_str("INTERNAL"),
            Self::Name(s) => write!(f, "NAME {s}"),
            Self::Duplicate => f.write_str("DUPLICATE_"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_type_round_trips_through_its_wire_text() {
        for text in [
            "AUTH",
            "QUOTA",
            "CMD SYNTAX",
            "CMD NO_AUTH",
            "NO_MSG",
            "LARGE_MSG",
            "BLOCKED reason=spam",
            "STORE db is down",
        ] {
            assert_eq!(ErrorType::parse(text.as_bytes()).unwrap().to_string(), text);
        }
    }

    #[test]
    fn old_no_queue_command_error_reads_as_no_entity() {
        assert_eq!(
            ErrorType::parse(b"CMD NO_QUEUE").unwrap(),
            ErrorType::Cmd(CommandError::NoEntity)
        );
    }

    #[test]
    fn unknown_error_tag_is_rejected() {
        assert!(ErrorType::parse(b"BOGUS").is_err());
        assert!(ErrorType::parse(b"CMD NOPE").is_err());
    }
}
