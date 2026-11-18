//! STUN and TURN servers for calls (ADR-0035): `chat.ice_servers`, else the SimpleX apps'
//! own public ones. A value type here so `config` can check the setting.
//!
//! A server is written as a URI (RFC 7064/7065) with the TURN credentials before the host:
//! `stun:host[:port]`, `turn:user:password@host[:port][?transport=udp|tcp]`,
//! `turns:user:password@host[:port]` (TLS over TCP). Ports default to 3478 (5349 for TLS).

/// The SimpleX apps' defaults (`defaultIceServers` in `packages/simplex-chat-webrtc/src/
/// call.ts`, simplex-chat v7.0.3): the same operator as the preset message relays. Their
/// `stuns:` entry is left out (a mapping learned over TLS is a TCP one, of no use to media
/// sent over UDP); their TURN server is reached over TLS on port 443 only.
pub const DEFAULT_ICE_SERVERS: &[&str] = &[
    "stun:stun.simplex.im:443",
    "turns:private2:Hxuq2QxUjnhj96Zq2r4HjqHRj@turn.simplex.im:443?transport=tcp",
];

/// What a server does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IceKind {
    /// Tells this computer its address as seen from outside (direct calls only).
    Stun,
    /// Relays the media (`chat.relay_calls`).
    Turn,
}

/// How this computer reaches the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IceTransport {
    Udp,
    Tcp,
    /// TLS over TCP (`turns:`).
    Tls,
}

/// One STUN or TURN server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IceServer {
    pub kind: IceKind,
    pub transport: IceTransport,
    /// A name or an address (IPv6 without brackets).
    pub host: String,
    pub port: u16,
    pub username: String,
    pub credential: String,
}

impl IceServer {
    /// Read one server; `None` when it is not one of the forms above.
    pub fn parse(uri: &str) -> Option<Self> {
        let (scheme, rest) = uri.trim().split_once(':')?;
        let (kind, tls) = match scheme.to_ascii_lowercase().as_str() {
            "stun" => (IceKind::Stun, false),
            "turn" => (IceKind::Turn, false),
            "turns" => (IceKind::Turn, true),
            _ => return None,
        };
        let (rest, query) = rest.split_once('?').unwrap_or((rest, ""));
        let (creds, hostport) = rest.rsplit_once('@').unwrap_or(("", rest));
        let (username, credential) = creds.split_once(':').unwrap_or((creds, ""));
        let transport = match (tls, query.to_ascii_lowercase().as_str()) {
            (true, "" | "transport=tcp") => IceTransport::Tls,
            (false, "" | "transport=udp") => IceTransport::Udp,
            (false, "transport=tcp") if kind == IceKind::Turn => IceTransport::Tcp,
            _ => return None,
        };
        let default_port = if tls { 5349 } else { 3478 };
        let (host, port) = split_host_port(hostport, default_port)?;
        let creds_ok = match kind {
            IceKind::Turn => !username.is_empty(),
            IceKind::Stun => creds.is_empty(),
        };
        if host.is_empty() || !creds_ok || host.contains('/') {
            return None;
        }
        Some(Self {
            kind,
            transport,
            host,
            port,
            username: username.to_owned(),
            credential: credential.to_owned(),
        })
    }
}

fn split_host_port(s: &str, default_port: u16) -> Option<(String, u16)> {
    if let Some(v6) = s.strip_prefix('[') {
        let (host, rest) = v6.split_once(']')?;
        let port = match rest.strip_prefix(':') {
            Some(p) => p.parse().ok()?,
            None if rest.is_empty() => default_port,
            None => return None,
        };
        return Some((host.to_owned(), port));
    }
    match s.split_once(':') {
        Some((host, port)) => Some((host.to_owned(), port.parse().ok()?)),
        None => Some((s.to_owned(), default_port)),
    }
}

/// The servers a call uses: `setting` (`chat.ice_servers`), or the defaults when it is
/// empty. Entries that do not parse are left out (the setting's reader already drops them).
pub fn ice_servers(setting: &[String]) -> Vec<IceServer> {
    if setting.is_empty() {
        DEFAULT_ICE_SERVERS
            .iter()
            .filter_map(|s| IceServer::parse(s))
            .collect()
    } else {
        setting.iter().filter_map(|s| IceServer::parse(s)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_a_stun_server_and_a_tls_turn_server() {
        let s = ice_servers(&[]);
        assert_eq!(s.len(), 2);
        assert_eq!(
            (s[0].kind, s[0].transport),
            (IceKind::Stun, IceTransport::Udp)
        );
        assert_eq!((s[0].host.as_str(), s[0].port), ("stun.simplex.im", 443));
        assert_eq!(
            (s[1].kind, s[1].transport),
            (IceKind::Turn, IceTransport::Tls)
        );
        assert_eq!((s[1].username.as_str(), s[1].port), ("private2", 443));
    }

    #[test]
    fn a_turn_server_needs_credentials_and_a_stun_server_takes_none() {
        assert!(IceServer::parse("turn:relay.example:3478").is_none());
        assert!(IceServer::parse("stun:u:p@stun.example").is_none());
        let t = IceServer::parse("turn:u:p@[::1]:3479?transport=tcp").unwrap();
        assert_eq!(
            (t.host.as_str(), t.port, t.transport),
            ("::1", 3479, IceTransport::Tcp)
        );
        assert_eq!(IceServer::parse("turns:u:p@h").unwrap().port, 5349);
        assert_eq!(IceServer::parse("stun:h").unwrap().port, 3478);
    }

    #[test]
    fn unknown_schemes_and_transports_are_refused() {
        for bad in [
            "http://h",
            "turns:u:p@h?transport=udp",
            "stun:h?transport=tcp",
            "turn:u:p@h:port",
            "stun:",
        ] {
            assert!(IceServer::parse(bad).is_none(), "{bad}");
        }
    }
}
