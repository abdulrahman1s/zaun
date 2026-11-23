//! What a ratchet-encrypted agent message carries (Agent/Protocol.hs `AgentMessage`, `RatchetInfo`, `RequestSignature`, AGPL-3.0-only).
use crate::crypto::{Alg, PublicKey, Signature};
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Tail};

use super::amessage::AMessage;
use super::priv_header::APrivHeader;
use crate::agent::keys::expect_alg;
use crate::agent::ne::{decode_ne, encode_ne};
use crate::agent::queue_info::SmpQueueInfo;

/// What an agent answers when asked to re-sync the ratchet: the hash of the key it answers, if any.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RatchetInfo {
    pub answered_key_hash: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestSignature {
    /// Ed25519.
    pub key: PublicKey,
    pub sig: Signature,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentMessage {
    /// The initiator confirms with its connection info.
    ConnInfo(Vec<u8>),
    /// The accepting side confirms and names its reply queues.
    ConnInfoReply {
        queues: Vec<SmpQueueInfo>,
        conn_info: Vec<u8>,
    },
    /// Sent with per-queue encryption only, never by the ratchet.
    RatchetInfo(RatchetInfo),
    Message {
        header: APrivHeader,
        message: AMessage,
    },
    /// An RPC request to a contact address (agent v8).
    ServiceRequest {
        queues: Vec<SmpQueueInfo>,
        signature: Option<RequestSignature>,
        body: Vec<u8>,
    },
    ServiceResponse(Vec<u8>),
    Rejection(Vec<u8>),
}

impl Encode for RequestSignature {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (&self.key, &self.sig).encode(out)
    }
}

impl Decode for RequestSignature {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (key, sig) = <(PublicKey, Signature)>::decode(d)?;
        Ok(Self {
            key: expect_alg(key, Alg::Ed25519)?,
            sig,
        })
    }
}

impl Encode for RatchetInfo {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        self.answered_key_hash.encode(out)
    }
}

/// An unreadable value is no value, and anything after it is ignored.
impl Decode for RatchetInfo {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let answered_key_hash = Option::<Vec<u8>>::decode(&mut d.clone()).unwrap_or(None);
        d.take_all();
        Ok(Self { answered_key_hash })
    }
}

impl Encode for AgentMessage {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        match self {
            Self::ConnInfo(info) => (b'I', Tail(info.clone())).encode(out),
            Self::ConnInfoReply { queues, conn_info } => {
                out.push(b'D');
                encode_ne(queues, out)?;
                Tail(conn_info.clone()).encode(out)
            }
            Self::RatchetInfo(info) => (b'R', info).encode(out),
            Self::Message { header, message } => (b'M', header, message).encode(out),
            Self::ServiceRequest {
                queues,
                signature,
                body,
            } => {
                out.push(b'A');
                encode_ne(queues, out)?;
                (signature, Tail(body.clone())).encode(out)
            }
            Self::ServiceResponse(body) => (b'P', Tail(body.clone())).encode(out),
            Self::Rejection(reason) => (b'J', Tail(reason.clone())).encode(out),
        }
    }
}

impl Decode for AgentMessage {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        match d.byte()? {
            b'I' => Ok(Self::ConnInfo(Tail::decode(d)?.0)),
            b'D' => {
                let queues = decode_ne(d)?;
                Ok(Self::ConnInfoReply {
                    queues,
                    conn_info: Tail::decode(d)?.0,
                })
            }
            b'R' => RatchetInfo::decode(d).map(Self::RatchetInfo),
            b'M' => {
                let (header, message) = <(APrivHeader, AMessage)>::decode(d)?;
                Ok(Self::Message { header, message })
            }
            b'A' => {
                let queues = decode_ne(d)?;
                let (signature, Tail(body)) = <(Option<RequestSignature>, Tail)>::decode(d)?;
                Ok(Self::ServiceRequest {
                    queues,
                    signature,
                    body,
                })
            }
            b'P' => Ok(Self::ServiceResponse(Tail::decode(d)?.0)),
            b'J' => Ok(Self::Rejection(Tail::decode(d)?.0)),
            t => Err(DecodeError::BadTag("AgentMessage", t)),
        }
    }
}
