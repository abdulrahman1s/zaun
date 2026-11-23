//! A received `A_MSG` the engine has not acked yet, kept to hand it out again after a restart, and the hashes receipts are checked against (Agent.hs `A_DUPLICATE`, `messagesRcvd`, AGPL-3.0-only).
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Large};

use crate::agent::client::AgentMsgId;
use crate::agent::integrity::{MsgErrorType, MsgIntegrity};

/// One per connection: the server holds the next message of a queue until this one is acked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unacked {
    pub msg_id: AgentMsgId,
    pub smp_msg_id: Vec<u8>,
    /// The sender's id and our hash of it, for the receipt.
    pub ext_snd_id: i64,
    pub hash: Vec<u8>,
    pub body: Vec<u8>,
    pub integrity: MsgIntegrity,
    pub broker_ts: i64,
    /// Times it was handed out again after redelivery by the server.
    pub attempts: u16,
    pub first_at: i64,
}

/// What a sent `A_MSG` is checked against when its receipt comes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentMsg {
    pub msg_id: AgentMsgId,
    pub hash: Vec<u8>,
}

struct Integrity(MsgIntegrity);

impl Encode for Integrity {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        match self.0 {
            MsgIntegrity::Ok => b'O'.encode(out),
            MsgIntegrity::Error(MsgErrorType::Skipped { from, to }) => (b'S', from, to).encode(out),
            MsgIntegrity::Error(MsgErrorType::BadId(id)) => (b'I', id).encode(out),
            MsgIntegrity::Error(MsgErrorType::BadHash) => b'H'.encode(out),
            MsgIntegrity::Error(MsgErrorType::Duplicate) => b'D'.encode(out),
        }
    }
}

impl Decode for Integrity {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let e = match d.byte()? {
            b'O' => return Ok(Self(MsgIntegrity::Ok)),
            b'S' => {
                let (from, to) = <_>::decode(d)?;
                MsgErrorType::Skipped { from, to }
            }
            b'I' => MsgErrorType::BadId(i64::decode(d)?),
            b'H' => MsgErrorType::BadHash,
            b'D' => MsgErrorType::Duplicate,
            t => return Err(DecodeError::BadTag("message integrity", t)),
        };
        Ok(Self(MsgIntegrity::Error(e)))
    }
}

impl Encode for Unacked {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (self.msg_id, &self.smp_msg_id, self.ext_snd_id, &self.hash).encode(out)?;
        let attempts = u32::from(self.attempts);
        let i = Integrity(self.integrity);
        (
            Large(self.body.clone()),
            i,
            self.broker_ts,
            attempts,
            self.first_at,
        )
            .encode(out)
    }
}

impl Decode for Unacked {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (msg_id, smp_msg_id, ext_snd_id, hash) = <_>::decode(d)?;
        let (Large(body), Integrity(integrity), broker_ts, attempts, first_at) =
            <(Large, Integrity, i64, u32, i64)>::decode(d)?;
        Ok(Self {
            msg_id,
            smp_msg_id,
            ext_snd_id,
            hash,
            body,
            integrity,
            broker_ts,
            attempts: u16::try_from(attempts).unwrap_or(u16::MAX),
            first_at,
        })
    }
}

impl Encode for SentMsg {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (self.msg_id, &self.hash).encode(out)
    }
}

impl Decode for SentMsg {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (msg_id, hash) = <_>::decode(d)?;
        Ok(Self { msg_id, hash })
    }
}
