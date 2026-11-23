//! An SMP server in link text: `smp://<keyhash>@host[,host2][:port]`, never with the password (Protocol.hs `ProtocolServer` `StrEncoding`, AGPL-3.0-only).
use super::error::{AgentError, Result};
use crate::encoding::{b64_decode, b64_encode};
use crate::smp::SmpServer;

impl SmpServer {
    /// `strEncode`.
    #[must_use]
    pub fn to_uri(&self) -> String {
        self.uri_with(&self.hosts)
    }

    /// `legacyStrEncodeServer`: the first host only.
    #[must_use]
    pub fn to_legacy_uri(&self) -> String {
        self.uri_with(&self.hosts[..1])
    }

    fn uri_with(&self, hosts: &[String]) -> String {
        let port = self.port.map(|p| format!(":{p}")).unwrap_or_default();
        format!(
            "smp://{}@{}{port}",
            b64_encode(&self.key_hash, true),
            hosts.join(",")
        )
    }

    /// Parses the server at the start of `s` and returns what follows it (from the first `/`).
    pub fn parse_prefix(s: &str) -> Result<(Self, &str)> {
        let rest = s
            .strip_prefix("smp://")
            .ok_or(AgentError::Parse("server scheme"))?;
        let (hash, rest) = rest
            .split_once('@')
            .ok_or(AgentError::Parse("server without @"))?;
        if hash.contains(':') {
            return Err(AgentError::Parse("server basic auth"));
        }
        let end = rest.find('/').unwrap_or(rest.len());
        let (auth, tail) = rest.split_at(end);
        let (hosts, port) = match auth.rsplit_once(':') {
            Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => {
                let port = p.parse().map_err(|_| AgentError::Parse("server port"))?;
                (h, Some(port))
            }
            _ => (auth, None),
        };
        let hosts: Vec<String> = hosts.split(',').map(str::to_owned).collect();
        if hosts.iter().any(String::is_empty) {
            return Err(AgentError::Parse("server hosts"));
        }
        let key_hash = b64_decode(hash)?;
        Ok((
            Self {
                hosts,
                port,
                key_hash,
                basic_auth: None,
            },
            tail,
        ))
    }

    pub fn parse(s: &str) -> Result<Self> {
        match Self::parse_prefix(s)? {
            (srv, "") => Ok(srv),
            _ => Err(AgentError::Parse("server")),
        }
    }
}
