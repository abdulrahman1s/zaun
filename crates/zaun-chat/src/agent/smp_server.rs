//! The binary form of an SMP server and the link helpers of `crate::smp::SmpServer` (Protocol.hs `ProtocolServer`, AGPL-3.0-only).
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, NonEmpty};
use crate::smp::SmpServer;

/// Onion twins of the preset servers (`extraSMPServerHosts`), added to links of old clients.
const EXTRA_HOSTS: [(&str, &str); 6] = [
    (
        "smp4.simplex.im",
        "o5vmywmrnaxalvz6wi3zicyftgio6psuvyniis6gco6bp6ekl4cqj4id.onion",
    ),
    (
        "smp5.simplex.im",
        "jjbyvoemxysm7qxap7m5d5m35jzv5qq6gnlv7s4rsn7tdwwmuqciwpid.onion",
    ),
    (
        "smp6.simplex.im",
        "bylepyau3ty4czmn77q4fglvperknl4bi2eb2fdy2bh4jxtf32kf73yd.onion",
    ),
    (
        "smp8.simplex.im",
        "beccx4yfxxbvyhqypaavemqurytl6hozr47wfc7uuecacjqdvwpw2xid.onion",
    ),
    (
        "smp9.simplex.im",
        "jssqzccmrcws6bhmn77vgmhfjmhwlyr3u7puw4erkyoosywgl67slqqd.onion",
    ),
    (
        "smp10.simplex.im",
        "rb2pbttocvnbrngnwziclp2f4ckjq65kebafws6g4hy22cdaiv5dwjqd.onion",
    ),
];

impl SmpServer {
    #[must_use]
    pub fn only_host(host: &str) -> Self {
        Self {
            hosts: vec![host.to_owned()],
            port: None,
            key_hash: Vec::new(),
            basic_auth: None,
        }
    }

    /// `SMPServerOnlyHost`.
    #[must_use]
    pub fn is_only_host(&self) -> bool {
        self.hosts.len() == 1 && self.port.is_none() && self.key_hash.is_empty()
    }

    /// `updateSMPServerHosts`.
    #[must_use]
    pub fn with_extra_hosts(mut self) -> Self {
        if let [h] = self.hosts.as_slice()
            && let Some((_, onion)) = EXTRA_HOSTS.iter().find(|(p, _)| p == h)
        {
            self.hosts.push((*onion).to_owned());
        }
        self
    }

    /// `sameSrvAddr`.
    #[must_use]
    pub fn same_addr(&self, other: &Self) -> bool {
        self.hosts == other.hosts && self.port == other.port
    }

    /// The pre-hostnames encoding: one host, port, key hash.
    pub(super) fn decode_legacy(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (host, port, key_hash) = <(String, String, Vec<u8>)>::decode(d)?;
        Ok(Self {
            hosts: vec![host],
            port: decode_port(&port)?,
            key_hash,
            basic_auth: None,
        }
        .with_extra_hosts())
    }

    pub(super) fn encode_legacy(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        let host = self.hosts.first().ok_or(EncodeError::EmptyList)?;
        (host, &port_str(self.port), &self.key_hash).encode(out)
    }
}

/// The binary form writes the port as text, empty when the link does not say.
fn port_str(port: Option<u16>) -> String {
    port.map(|p| p.to_string()).unwrap_or_default()
}

fn decode_port(s: &str) -> Result<Option<u16>, DecodeError> {
    if s.is_empty() {
        return Ok(None);
    }
    s.parse()
        .map(Some)
        .map_err(|_| DecodeError::Invalid("server port"))
}

impl Encode for SmpServer {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        let hosts = NonEmpty::new(self.hosts.clone()).ok_or(EncodeError::EmptyList)?;
        (&hosts, &port_str(self.port), &self.key_hash).encode(out)
    }
}

impl Decode for SmpServer {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (hosts, port, key_hash) = <(NonEmpty<String>, String, Vec<u8>)>::decode(d)?;
        Ok(Self {
            hosts: hosts.into_vec(),
            port: decode_port(&port)?,
            key_hash,
            basic_auth: None,
        })
    }
}
