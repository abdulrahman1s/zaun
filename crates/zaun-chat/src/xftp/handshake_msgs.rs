//! The two handshake blocks (Transport.hs `XFTPServerHandshake`, `XFTPClientHandshake`; nook xftp/handshake.ts).
use crate::crypto::{pad, unpad};
use crate::encoding::{Decode, Decoder, Encode, Large, NonEmpty, Version, VersionRange};

use super::error::{HandshakeError, XftpError};
use super::error_type::XftpErrorType;
use super::version::{BLOCK_SIZE, FILE_STORAGE_TIME};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerHandshake {
    pub range: VersionRange,
    pub session_id: Vec<u8>,
    /// DER certificates, leaf first.
    pub chain: Vec<Vec<u8>>,
    /// DER of the signed X25519 key.
    pub signed_key: Vec<u8>,
    /// The web handshake's proof; a native client does not ask for it, so it is read and ignored.
    pub web_proof: Option<Vec<u8>>,
    /// JSON `ServerPublicInfo`, sent when the server's top version is 4 or more.
    pub info: Option<Vec<u8>>,
}

impl ServerHandshake {
    /// Anything after the known fields is ignored, as Haskell's `Tail` does.
    pub fn decode_block(block: &[u8]) -> Result<Self, XftpError> {
        let raw = unpad(block)?;
        if raw.len() < 20
            && raw.iter().all(|b| b.is_ascii_uppercase() || *b == b'_')
            && let Some(e) = XftpErrorType::parse(raw)
        {
            return Err(XftpError::Server(e));
        }
        Self::decode(raw).map_err(|_| HandshakeError::Parse.into())
    }

    fn decode(raw: &[u8]) -> Result<Self, crate::encoding::DecodeError> {
        let mut d = Decoder::new(raw);
        let range = VersionRange::decode(&mut d)?;
        let session_id = Vec::<u8>::decode(&mut d)?;
        let chain = NonEmpty::<Large>::decode(&mut d)?
            .into_vec()
            .into_iter()
            .map(|c| c.0)
            .collect();
        let signed_key = Large::decode(&mut d)?.0;
        let web_proof = if d.remaining() > 0 {
            Some(Vec::<u8>::decode(&mut d)?).filter(|p| !p.is_empty())
        } else {
            None
        };
        let info = if range.max() >= Version(FILE_STORAGE_TIME) && d.remaining() > 0 {
            Option::<Large>::decode(&mut d)?.map(|l| l.0)
        } else {
            None
        };
        Ok(Self {
            range,
            session_id,
            chain,
            signed_key,
            web_proof,
            info,
        })
    }

    /// What a server writes; used by tests that play the server.
    pub fn encode_block(&self) -> Result<Vec<u8>, XftpError> {
        let mut out = Vec::new();
        self.range.encode(&mut out)?;
        self.session_id.encode(&mut out)?;
        let chain: Vec<Large> = self.chain.iter().cloned().map(Large).collect();
        NonEmpty::new(chain)
            .ok_or(crate::encoding::EncodeError::EmptyList)?
            .encode(&mut out)?;
        Large(self.signed_key.clone()).encode(&mut out)?;
        self.web_proof
            .clone()
            .unwrap_or_default()
            .encode(&mut out)?;
        if self.range.max() >= Version(FILE_STORAGE_TIME) {
            self.info.clone().map(Large).encode(&mut out)?;
        }
        Ok(pad(&out, BLOCK_SIZE)?)
    }
}

/// `(version, keyHash)`, and from v4 the entitlement proof, which Zaun never has (`'0'`); padded.
pub fn encode_client_handshake(version: u16, key_hash: &[u8]) -> Result<Vec<u8>, XftpError> {
    let mut out = Vec::new();
    version.encode(&mut out)?;
    key_hash.encode(&mut out)?;
    if version >= FILE_STORAGE_TIME {
        out.push(b'0');
    }
    Ok(pad(&out, BLOCK_SIZE)?)
}
