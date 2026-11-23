//! XFTP server addresses `xftp://<keyHash>[:<basicAuth>]@host[,host2][:port]` (Protocol.hs `ProtocolServer`; nook xftp/address.ts).
use std::cmp::Ordering;
use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

use crate::encoding::{b64_decode, b64_encode};

use super::error::XftpError;
use super::version::DEFAULT_PORT;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct XftpServer {
    /// SHA-256 of the server's identity (CA) certificate; 32 bytes for a real server.
    pub key_hash: Vec<u8>,
    pub hosts: Vec<String>,
    pub port: Option<u16>,
    pub basic_auth: Option<String>,
}

fn bad(why: &'static str) -> XftpError {
    XftpError::Address(why)
}

impl XftpServer {
    #[must_use]
    pub fn port_or_default(&self) -> u16 {
        self.port.unwrap_or(DEFAULT_PORT)
    }

    /// The address as a description stores it: a server's password is for creating files, not for the people who download them.
    #[must_use]
    pub fn without_auth(&self) -> Self {
        Self {
            basic_auth: None,
            ..self.clone()
        }
    }

    /// The hosts to dial: Tor hosts are not supported.
    pub fn dialable_hosts(&self) -> impl Iterator<Item = &str> {
        self.hosts
            .iter()
            .map(String::as_str)
            .filter(|h| !h.ends_with(".onion"))
    }

    /// The order Haskell's derived `Ord` gives: hosts (IPv4, IPv6, onion, domain), port as text, key hash.
    #[must_use]
    pub fn cmp_haskell(&self, other: &Self) -> Ordering {
        let hosts = |s: &Self| s.hosts.iter().map(|h| host_key(h)).collect::<Vec<_>>();
        let port = |s: &Self| s.port.map(|p| p.to_string()).unwrap_or_default();
        hosts(self)
            .cmp(&hosts(other))
            .then_with(|| port(self).cmp(&port(other)))
            .then_with(|| self.key_hash.cmp(&other.key_hash))
    }
}

fn host_key(host: &str) -> (u8, Vec<u8>) {
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        (0, ip.octets().to_vec())
    } else if let Ok(ip) = host.parse::<Ipv6Addr>() {
        (1, ip.octets().to_vec())
    } else if host.ends_with(".onion") {
        (2, host.as_bytes().to_vec())
    } else {
        (3, host.as_bytes().to_vec())
    }
}

impl FromStr for XftpServer {
    type Err = XftpError;

    fn from_str(s: &str) -> Result<Self, XftpError> {
        let rest = s
            .trim()
            .strip_prefix("xftp://")
            .ok_or_else(|| bad("missing xftp://"))?;
        let (id, hosts) = rest.split_once('@').ok_or_else(|| bad("missing @"))?;
        let (hash, basic_auth) = match id.split_once(':') {
            Some((h, a)) => (h, Some(a.to_owned()).filter(|a| !a.is_empty())),
            None => (id, None),
        };
        let key_hash = b64_decode(hash).map_err(|_| bad("key hash is not base64"))?;
        let (hosts, port) = match hosts.rsplit_once(':') {
            Some((h, p))
                if !p.contains(']') && !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) =>
            {
                (h, Some(p.parse::<u16>().map_err(|_| bad("bad port"))?))
            }
            _ => (hosts, None),
        };
        let hosts: Vec<String> = hosts
            .split(',')
            .map(|h| h.trim_start_matches('[').trim_end_matches(']').to_owned())
            .collect();
        if hosts.iter().any(String::is_empty) {
            return Err(bad("empty host"));
        }
        Ok(Self {
            key_hash,
            hosts,
            port,
            basic_auth,
        })
    }
}

/// `strEncode`: padded base64url hash, every host, the port when set.
impl fmt::Display for XftpServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "xftp://{}", b64_encode(&self.key_hash, true))?;
        if let Some(a) = &self.basic_auth {
            write!(f, ":{a}")?;
        }
        let hosts: Vec<String> = self
            .hosts
            .iter()
            .map(|h| {
                if h.contains(':') {
                    format!("[{h}]")
                } else {
                    h.clone()
                }
            })
            .collect();
        write!(f, "@{}", hosts.join(","))?;
        match self.port {
            Some(p) => write!(f, ":{p}"),
            None => Ok(()),
        }
    }
}
