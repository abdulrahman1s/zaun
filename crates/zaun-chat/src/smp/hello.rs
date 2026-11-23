//! The two hello messages (Transport.hs `SMPServerHandshake`, `SMPClientHandshake`; nook smp/handshake.ts).
use crate::crypto::PublicKey;
use crate::encoding::{
    Decode, DecodeError, Decoder, Encode, EncodeError, Large, NonEmpty, Version, VersionRange,
};

use super::version::{SERVER_INFO, SERVICE_CERTS};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerHello {
    pub range: VersionRange,
    pub session_id: Vec<u8>,
    /// DER certificates, leaf first.
    pub chain: Vec<Vec<u8>>,
    /// DER of the signed X25519 key.
    pub signed_key: Vec<u8>,
    /// JSON `ServerPublicInfo`, sent by servers at v21 and later.
    pub server_info: Option<Vec<u8>>,
}

impl ServerHello {
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut d = Decoder::new(bytes);
        let range = VersionRange::decode(&mut d)?;
        let session_id = Vec::decode(&mut d)?;
        let chain = NonEmpty::<Large>::decode(&mut d)?
            .into_vec()
            .into_iter()
            .map(|c| c.0)
            .collect();
        let signed_key = Large::decode(&mut d)?.0;
        // Newer servers may append more; only the info block is read, and a malformed one is dropped.
        let server_info = if range.max() >= Version(SERVER_INFO) && d.remaining() > 0 {
            Option::<Large>::decode(&mut d).ok().flatten().map(|l| l.0)
        } else {
            None
        };
        Ok(Self {
            range,
            session_id,
            chain,
            signed_key,
            server_info,
        })
    }

    /// What a server writes; used by tests that play the server.
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        let mut out = Vec::new();
        self.range.encode(&mut out)?;
        self.session_id.encode(&mut out)?;
        let chain = self.chain.iter().cloned().map(Large).collect::<Vec<_>>();
        NonEmpty::new(chain)
            .ok_or(EncodeError::EmptyList)?
            .encode(&mut out)?;
        Large(self.signed_key.clone()).encode(&mut out)?;
        if self.range.max() >= Version(SERVER_INFO) {
            self.server_info.clone().map(Large).encode(&mut out)?;
        }
        Ok(out)
    }
}

/// `smpEncode (v, keyHash) ‖ authPubKey? ‖ proxyServer ‖ clientService?`: the DH key is the bare encoded key,
/// the proxy flag is `F`, and v16+ adds `'0'` for no client service.
pub fn encode_client_hello(
    v: u16,
    key_hash: &[u8],
    auth_key: Option<&PublicKey>,
) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::new();
    v.encode(&mut out)?;
    key_hash.encode(&mut out)?;
    if let Some(k) = auth_key {
        k.encode(&mut out)?;
    }
    false.encode(&mut out)?;
    if v >= SERVICE_CERTS {
        out.push(b'0');
    }
    Ok(out)
}
