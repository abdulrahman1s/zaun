//! SMP server addresses `smp://<keyHash>[:<basicAuth>]@host[,host2][:port]` (Protocol.hs `ProtocolServer` strP/strEncode; nook smp/address.ts).
use std::fmt;
use std::str::FromStr;

use crate::encoding::{b64_decode, b64_encode};

use super::error::SmpError;

pub const DEFAULT_PORT: u16 = 5223;

/// One type for configured servers and servers named in links (agent `ProtocolServer`).
#[derive(Debug, Clone)]
pub struct SmpServer {
    /// SHA-256 of the server's identity (CA) certificate. Empty for a preset server a short link names by host only;
    /// such a server cannot be connected to, since the handshake compares this hash.
    pub key_hash: Vec<u8>,
    /// Never empty; the first is the primary, the rest are alternatives such as onion hosts.
    pub hosts: Vec<String>,
    pub port: Option<u16>,
    /// Configuration only: never written into links, and not part of the server's identity.
    pub basic_auth: Option<String>,
}

impl PartialEq for SmpServer {
    fn eq(&self, other: &Self) -> bool {
        (&self.key_hash, &self.hosts, self.port) == (&other.key_hash, &other.hosts, other.port)
    }
}

impl Eq for SmpServer {}

impl std::hash::Hash for SmpServer {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        (&self.key_hash, &self.hosts, self.port).hash(h);
    }
}

impl SmpServer {
    #[must_use]
    pub fn port_or_default(&self) -> u16 {
        self.port.unwrap_or(DEFAULT_PORT)
    }
}

fn bad(why: &'static str) -> SmpError {
    SmpError::Address(why)
}

impl FromStr for SmpServer {
    type Err = SmpError;

    fn from_str(s: &str) -> Result<Self, SmpError> {
        let rest = s
            .trim()
            .strip_prefix("smp://")
            .ok_or_else(|| bad("missing smp://"))?;
        let (id, hosts) = rest.split_once('@').ok_or_else(|| bad("missing @"))?;
        let (hash, basic_auth) = match id.split_once(':') {
            Some((h, a)) => (h, Some(a.to_owned()).filter(|a| !a.is_empty())),
            None => (id, None),
        };
        let key_hash = b64_decode(hash).map_err(|_| bad("key hash is not base64"))?;
        if key_hash.len() != 32 {
            return Err(bad("key hash must be 32 bytes"));
        }
        // Hosts never contain ':' except inside brackets (IPv6), so a trailing ":digits" outside them is the port.
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
impl fmt::Display for SmpServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "smp://{}", b64_encode(&self.key_hash, true))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    const FP: &str = "hpq7_4gGJiilmz5Rf-CswuU5kZGkm_zOIooSw6yALRg";

    #[test]
    fn address_keeps_hosts_port_and_key_hash() {
        let s: SmpServer = format!("smp://{FP}@smp5.simplex.im,example.onion:5224")
            .parse()
            .unwrap();
        assert_eq!(s.hosts, ["smp5.simplex.im", "example.onion"]);
        assert_eq!(s.port, Some(5224));
        assert_eq!(s.key_hash, b64_decode(FP).unwrap());
        assert_eq!(
            s.to_string(),
            format!("smp://{FP}=@smp5.simplex.im,example.onion:5224")
        );
    }

    #[test]
    fn address_without_port_uses_the_default() {
        let s: SmpServer = format!("smp://{FP}=@h").parse().unwrap();
        assert_eq!((s.port, s.port_or_default()), (None, 5223));
    }

    #[test]
    fn address_reads_basic_auth_and_ipv6_hosts() {
        let s: SmpServer = format!("smp://{FP}:secret@[::1]:443").parse().unwrap();
        assert_eq!(
            (s.basic_auth.as_deref(), s.hosts.as_slice(), s.port),
            (Some("secret"), ["::1".to_owned()].as_slice(), Some(443))
        );
        assert_eq!(s.to_string(), format!("smp://{FP}=:secret@[::1]:443"));
        let v6: SmpServer = format!("smp://{FP}@[2001:db8::1]").parse().unwrap();
        assert_eq!((v6.hosts[0].as_str(), v6.port), ("2001:db8::1", None));
    }

    #[test]
    fn the_password_is_not_part_of_the_server_identity() {
        let a: SmpServer = format!("smp://{FP}:secret@h").parse().unwrap();
        let b: SmpServer = format!("smp://{FP}@h").parse().unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn address_rejects_malformed_input() {
        assert!("smp://abc@h".parse::<SmpServer>().is_err());
        assert!(format!("xftp://{FP}@h").parse::<SmpServer>().is_err());
        assert!(format!("smp://{FP}").parse::<SmpServer>().is_err());
        assert!(format!("smp://{FP}@a,,b").parse::<SmpServer>().is_err());
    }
}
