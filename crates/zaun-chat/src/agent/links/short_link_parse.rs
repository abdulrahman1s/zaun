//! Parsing short links from text (Agent/Protocol.hs `AConnShortLink` `strP`, AGPL-3.0-only).
use super::contact_type::ContactConnType;
use super::short_link::{ConnShortLink, ShortLinkScheme};
use crate::agent::error::{AgentError, Result};
use crate::agent::query;
use crate::encoding::b64_decode;
use crate::smp::SmpServer;

impl ConnShortLink {
    /// `strP`.
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim();
        let (scheme, host, rest) = match s.strip_prefix("simplex:") {
            Some(rest) => (ShortLinkScheme::Simplex, None, rest),
            None => {
                let rest = s
                    .strip_prefix("https://")
                    .ok_or(AgentError::Parse("short link scheme"))?;
                let end = rest.find('/').ok_or(AgentError::Parse("short link path"))?;
                let host = &rest[..end];
                if host.is_empty() || host.contains([':', '#', ',', ';', '[', ']']) {
                    return Err(AgentError::Parse("short link host"));
                }
                (ShortLinkScheme::Server, Some(host), &rest[end..])
            }
        };
        let rest = rest
            .strip_prefix('/')
            .ok_or(AgentError::Parse("short link path"))?;
        let mut chars = rest.chars();
        let kind = chars.next().ok_or(AgentError::Parse("short link type"))?;
        let rest = chars.as_str();
        let rest = rest.strip_prefix('/').unwrap_or(rest);
        let rest = rest
            .strip_prefix('#')
            .ok_or(AgentError::Parse("short link without #"))?;
        let (frag, qs) = rest
            .split_once('?')
            .map_or((rest, None), |(f, q)| (f, Some(q)));
        let q = qs.map(query::parse).unwrap_or_default();
        let mut hosts: Vec<String> = host.map(str::to_owned).into_iter().collect();
        hosts.extend(
            query::get(&q, "h")
                .iter()
                .flat_map(|h| h.split(','))
                .filter(|h| !h.is_empty())
                .map(str::to_owned),
        );
        if hosts.is_empty() {
            return Err(AgentError::Parse("short link without server"));
        }
        let port = query::get(&q, "p")
            .map(|p| p.parse::<u16>())
            .transpose()
            .map_err(|_| AgentError::Parse("short link port"))?;
        let key_hash = query::get(&q, "c")
            .map(b64_decode)
            .transpose()?
            .unwrap_or_default();
        let server = SmpServer {
            hosts,
            port,
            key_hash,
            basic_auth: None,
        };
        if kind == 'i' {
            let (id, key) = frag
                .split_once('/')
                .ok_or(AgentError::Parse("invitation short link"))?;
            return Ok(Self::Invitation {
                scheme,
                server,
                link_id: b64_decode(id)?,
                link_key: b64_decode(key)?,
            });
        }
        let contact_type = ContactConnType::from_char(kind)?;
        Ok(Self::Contact {
            scheme,
            contact_type,
            server,
            link_key: b64_decode(frag)?,
        })
    }
}
