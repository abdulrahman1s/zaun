//! Any link a user can paste: a full connection request or a short link (Agent/Protocol.hs `ConnectionLink`, `CreatedConnLink`, AGPL-3.0-only).
use super::conn_req::ConnectionRequestUri;
use super::short_link::ConnShortLink;
use crate::agent::error::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionLink {
    Full(ConnectionRequestUri),
    Short(ConnShortLink),
}

/// A link as created: the full one, and the short one when the server stores link data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedConnLink {
    pub full: ConnectionRequestUri,
    pub short: Option<ConnShortLink>,
}

impl ConnectionLink {
    #[must_use]
    pub fn to_uri(&self) -> String {
        match self {
            Self::Full(l) => l.to_uri(),
            Self::Short(l) => l.to_uri(),
        }
    }

    /// Full links have `/invitation` or `/contact` as the path; short ones a single letter.
    pub fn parse(s: &str) -> Result<Self> {
        match ConnectionRequestUri::parse(s) {
            Ok(full) => Ok(Self::Full(full)),
            Err(_) => ConnShortLink::parse(s).map(Self::Short),
        }
    }
}
