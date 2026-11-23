//! Short links: `https://<host>/(i|a|c|g|r)#<linkId>/<linkKey>` and `simplex:/…` (Agent/Protocol.hs `ConnShortLink`, AGPL-3.0-only).
use crate::encoding::b64_encode;

use super::contact_type::ContactConnType;
use crate::agent::query::{self, pair};
use crate::smp::SmpServer;

/// `https://<host>` or `simplex:`; the host is then in the query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortLinkScheme {
    Simplex,
    Server,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnShortLink {
    Invitation {
        scheme: ShortLinkScheme,
        server: SmpServer,
        link_id: Vec<u8>,
        link_key: Vec<u8>,
    },
    Contact {
        scheme: ShortLinkScheme,
        contact_type: ContactConnType,
        server: SmpServer,
        link_key: Vec<u8>,
    },
}

impl ConnShortLink {
    #[must_use]
    pub fn server(&self) -> &SmpServer {
        match self {
            Self::Invitation { server, .. } | Self::Contact { server, .. } => server,
        }
    }

    #[must_use]
    pub fn link_key(&self) -> &[u8] {
        match self {
            Self::Invitation { link_key, .. } | Self::Contact { link_key, .. } => link_key,
        }
    }

    #[must_use]
    pub fn with_server(mut self, new: SmpServer) -> Self {
        match &mut self {
            Self::Invitation { server, .. } | Self::Contact { server, .. } => *server = new,
        }
        self
    }

    /// `simplexShortLink`.
    #[must_use]
    pub fn into_simplex(mut self) -> Self {
        match &mut self {
            Self::Invitation { scheme, .. } | Self::Contact { scheme, .. } => {
                *scheme = ShortLinkScheme::Simplex
            }
        }
        self
    }

    /// `strEncode`.
    #[must_use]
    pub fn to_uri(&self) -> String {
        let (scheme, server, kind, link_id, key) = match self {
            Self::Invitation {
                scheme,
                server,
                link_id,
                link_key,
            } => (scheme, server, 'i', link_id.as_slice(), link_key),
            Self::Contact {
                scheme,
                contact_type,
                server,
                link_key,
            } => (
                scheme,
                server,
                contact_type.char().to_ascii_lowercase(),
                &[][..],
                link_key,
            ),
        };
        let (authority, extra_hosts) = match scheme {
            ShortLinkScheme::Simplex => ("simplex:".to_owned(), &server.hosts[..]),
            ShortLinkScheme::Server => (format!("https://{}", server.hosts[0]), &server.hosts[1..]),
        };
        let mut q = Vec::new();
        if !extra_hosts.is_empty() {
            q.push(pair("h", extra_hosts.join(",")));
        }
        if let Some(p) = server.port {
            q.push(pair("p", p.to_string()));
        }
        if !server.key_hash.is_empty() {
            q.push(pair("c", b64_encode(&server.key_hash, false)));
        }
        let qs = if q.is_empty() {
            String::new()
        } else {
            format!("?{}", query::render(&q))
        };
        let id = if link_id.is_empty() {
            String::new()
        } else {
            format!("{}/", b64_encode(link_id, false))
        };
        format!("{authority}/{kind}#{id}{}{qs}", b64_encode(key, false))
    }
}
