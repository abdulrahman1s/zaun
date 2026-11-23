//! A queue address from text, in the versioned, key-after-`#` and unversioned forms (Agent/Protocol.hs `StrEncoding SMPQueueUri`, AGPL-3.0-only).
use crate::crypto::Alg;
use crate::encoding::{VersionRange, b64_decode};

use super::error::{AgentError, Result};
use super::keys::parse_key_str;
use super::query;
use super::queue_mode::QueueMode;
use super::queue_uri::{SmpQueueAddress, SmpQueueUri};
use super::version::{INITIAL_CLIENT, SRV_HOSTNAMES_CLIENT, parse_range};
use crate::smp::SmpServer;

impl SmpQueueUri {
    /// `strP`: the versioned form, the older form with the key after `#`, or the unversioned one.
    pub fn parse(s: &str) -> Result<Self> {
        let (server, rest) = SmpServer::parse_prefix(s)?;
        let rest = rest
            .strip_prefix('/')
            .ok_or(AgentError::Parse("queue URI"))?;
        let (sid, after) = rest
            .split_once('#')
            .ok_or(AgentError::Parse("queue URI without #"))?;
        let sender_id = b64_decode(sid.strip_suffix('/').unwrap_or(sid))?;
        let key_len = after
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'='))
            .count();
        let (key, tail) = after.split_at(key_len);
        let tail = tail.strip_prefix('/').unwrap_or(tail);
        let Some(q) = tail.strip_prefix('?') else {
            if !tail.is_empty() || key.is_empty() {
                return Err(AgentError::Parse("queue URI"));
            }
            let address = SmpQueueAddress {
                server: server.with_extra_hosts(),
                sender_id,
                dh_public_key: parse_key_str(key, Alg::X25519)?,
                queue_mode: None,
            };
            return Ok(Self {
                client_v_range: VersionRange::single(INITIAL_CLIENT),
                address,
            });
        };
        let q = query::parse(q);
        let v = query::get(&q, "v").ok_or(AgentError::MissingParam("v"))?;
        let client_v_range = parse_range(v)?;
        let dh = if key.is_empty() {
            query::get(&q, "dh")
        } else {
            Some(key)
        };
        let dh = dh.ok_or(AgentError::MissingParam("dh"))?;
        let mut server = server;
        server.hosts.extend(
            query::get(&q, "srv")
                .into_iter()
                .flat_map(|h| h.split(',').filter(|h| !h.is_empty()).map(str::to_owned)),
        );
        if client_v_range.max() < SRV_HOSTNAMES_CLIENT {
            server = server.with_extra_hosts();
        }
        let queue_mode = match query::get(&q, "q") {
            Some("m") => Some(QueueMode::Messaging),
            Some("c") => Some(QueueMode::Contact),
            _ if query::get(&q, "k") == Some("s") => Some(QueueMode::Messaging),
            _ => None,
        };
        let address = SmpQueueAddress {
            server,
            sender_id,
            dh_public_key: parse_key_str(dh, Alg::X25519)?,
            queue_mode,
        };
        Ok(Self {
            client_v_range,
            address,
        })
    }
}
