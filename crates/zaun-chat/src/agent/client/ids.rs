//! Connection and message ids the agent hands to the engine (Agent/Protocol.hs `ConnId`, `AgentMsgId`, AGPL-3.0-only).
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError};

/// A connection, from the store's `agent.conn` counter; never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConnId(pub i64);

/// A message of one connection, sent or received, numbered from 1 in one sequence (`InternalId`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AgentMsgId(pub i64);

impl Encode for ConnId {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        self.0.encode(out)
    }
}

impl Decode for ConnId {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        i64::decode(d).map(Self)
    }
}

impl Encode for AgentMsgId {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        self.0.encode(out)
    }
}

impl Decode for AgentMsgId {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        i64::decode(d).map(Self)
    }
}
