//! The per-queue layer under every agent envelope (Protocol.hs `ClientMsgEnvelope`, `PubHeader`, `ClientMessage`, `PrivHeader`, AGPL-3.0-only).
use crate::crypto::{CbNonce, PublicKey};
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Tail, Version};

/// `ClientMsgEnvelope`: `(PubHeader, nonce, Tail encBody)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientMsgEnvelope {
    /// `phVersion`, the agreed SMP client version.
    pub version: Version,
    /// `phE2ePubDhKey`: the sender's e2e key, only in a confirmation.
    pub e2e_pub: Option<PublicKey>,
    pub nonce: CbNonce,
    pub enc_body: Vec<u8>,
}

/// `PrivHeader`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivHeader {
    /// `K`: the sender's key in a legacy confirmation, for the recipient's KEY.
    Confirmation(PublicKey),
    /// `_`.
    Empty,
}

/// `ClientMessage`: what the per-queue box holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientMessage {
    pub header: PrivHeader,
    pub body: Vec<u8>,
}

impl Encode for ClientMsgEnvelope {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (self.version, &self.e2e_pub).encode(out)?;
        out.extend_from_slice(self.nonce.as_bytes());
        Tail(self.enc_body.clone()).encode(out)
    }
}

impl Decode for ClientMsgEnvelope {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (version, e2e_pub) = <(Version, Option<PublicKey>)>::decode(d)?;
        let nonce = CbNonce::new(d.take_array()?);
        Ok(Self {
            version,
            e2e_pub,
            nonce,
            enc_body: Tail::decode(d)?.0,
        })
    }
}

impl Encode for ClientMessage {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        match &self.header {
            PrivHeader::Confirmation(k) => (b'K', k).encode(out)?,
            PrivHeader::Empty => out.push(b'_'),
        }
        Tail(self.body.clone()).encode(out)
    }
}

impl Decode for ClientMessage {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let header = match d.byte()? {
            b'K' => PrivHeader::Confirmation(PublicKey::decode(d)?),
            b'_' => PrivHeader::Empty,
            t => return Err(DecodeError::BadTag("PrivHeader", t)),
        };
        Ok(Self {
            header,
            body: Tail::decode(d)?.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{Alg, KeyPair};

    #[test]
    fn client_envelope_is_header_nonce_then_the_rest() {
        let key = KeyPair::generate(Alg::X25519).unwrap().public;
        let env = ClientMsgEnvelope {
            version: Version(4),
            e2e_pub: Some(key.clone()),
            nonce: CbNonce::new([9; 24]),
            enc_body: vec![1, 2, 3],
        };
        let bytes = env.to_bytes().unwrap();
        let mut expected = vec![0, 4, b'1'];
        key.encode(&mut expected).unwrap();
        expected.extend([9; 24]);
        expected.extend([1, 2, 3]);
        assert_eq!(bytes, expected);
        assert_eq!(ClientMsgEnvelope::from_bytes(&bytes).unwrap(), env);
    }

    #[test]
    fn empty_private_header_is_an_underscore() {
        let m = ClientMessage {
            header: PrivHeader::Empty,
            body: b"hi".to_vec(),
        };
        assert_eq!(m.to_bytes().unwrap(), b"_hi");
        assert_eq!(ClientMessage::from_bytes(b"_hi").unwrap(), m);
        assert!(ClientMessage::from_bytes(b"Xhi").is_err());
    }
}
