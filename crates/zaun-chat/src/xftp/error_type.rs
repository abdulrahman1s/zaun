//! The error a server answers with (Transport.hs `XFTPErrorType`, `CommandError`); nook xftp/commands.ts `decodeXFTPError`.
use std::fmt;

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

    fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "UNKNOWN",
            Self::Syntax => "SYNTAX",
            Self::Prohibited => "PROHIBITED",
            Self::NoAuth => "NO_AUTH",
            Self::HasAuth => "HAS_AUTH",
            Self::NoEntity => "NO_ENTITY",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XftpErrorType {
    Block,
    Session,
    Handshake,
    Cmd(CommandError),
    Auth,
    Blocked(String),
    Size,
    Quota,
    Digest,
    Crypto,
    NoFile,
    HasFile,
    FileIo,
    Timeout,
    Internal,
    Duplicate,
}

impl XftpErrorType {
    /// Parses `ERR`'s argument: a tag, then for `CMD` and `BLOCKED` a space and the rest.
    #[must_use]
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(bytes).ok()?;
        let (tag, rest) = text.split_once(' ').unwrap_or((text, ""));
        Some(match tag {
            "BLOCK" => Self::Block,
            "SESSION" => Self::Session,
            "HANDSHAKE" => Self::Handshake,
            "CMD" => Self::Cmd(CommandError::parse(rest.split(' ').next()?)?),
            "AUTH" => Self::Auth,
            "BLOCKED" => Self::Blocked(rest.to_owned()),
            "SIZE" => Self::Size,
            "QUOTA" => Self::Quota,
            "DIGEST" => Self::Digest,
            "CRYPTO" => Self::Crypto,
            "NO_FILE" => Self::NoFile,
            "HAS_FILE" => Self::HasFile,
            "FILE_IO" => Self::FileIo,
            "TIMEOUT" => Self::Timeout,
            "INTERNAL" => Self::Internal,
            "DUPLICATE_" => Self::Duplicate,
            _ => return None,
        })
    }

    /// Worth retrying later or on another server; a missing, blocked or refused file is not.
    #[must_use]
    pub fn is_temporary(&self) -> bool {
        matches!(
            self,
            Self::Session
                | Self::Handshake
                | Self::Timeout
                | Self::Internal
                | Self::FileIo
                | Self::Quota
        )
    }
}

impl fmt::Display for XftpErrorType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Block => f.write_str("BLOCK"),
            Self::Session => f.write_str("SESSION"),
            Self::Handshake => f.write_str("HANDSHAKE"),
            Self::Cmd(e) => write!(f, "CMD {}", e.as_str()),
            Self::Auth => f.write_str("AUTH"),
            Self::Blocked(info) => write!(f, "BLOCKED {info}"),
            Self::Size => f.write_str("SIZE"),
            Self::Quota => f.write_str("QUOTA"),
            Self::Digest => f.write_str("DIGEST"),
            Self::Crypto => f.write_str("CRYPTO"),
            Self::NoFile => f.write_str("NO_FILE"),
            Self::HasFile => f.write_str("HAS_FILE"),
            Self::FileIo => f.write_str("FILE_IO"),
            Self::Timeout => f.write_str("TIMEOUT"),
            Self::Internal => f.write_str("INTERNAL"),
            Self::Duplicate => f.write_str("DUPLICATE_"),
        }
    }
}
