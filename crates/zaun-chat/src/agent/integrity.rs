//! The check of message order and the hash chain (Agent.hs `checkMsgIntegrity`, Agent/Protocol.hs `MsgIntegrity`, AGPL-3.0-only).
use std::fmt;

use crate::crypto::sha256;

/// `MsgErrorType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgErrorType {
    /// Messages `from..=to` never arrived.
    Skipped {
        from: i64,
        to: i64,
    },
    BadId(i64),
    BadHash,
    Duplicate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgIntegrity {
    Ok,
    Error(MsgErrorType),
}

/// The hash a message chain links with: `sha256` of the decrypted agent message.
#[must_use]
pub fn msg_hash(agent_msg_body: &[u8]) -> [u8; 32] {
    sha256(agent_msg_body)
}

/// `prev_ext_snd_id` is the sender message ID last received (0 at the start), and `prev_rcv_msg_hash` the hash of that message.
#[must_use]
pub fn check_msg_integrity(
    prev_ext_snd_id: i64,
    ext_snd_id: i64,
    prev_rcv_msg_hash: &[u8],
    received_prev_msg_hash: &[u8],
) -> MsgIntegrity {
    use MsgErrorType::{BadHash, BadId, Duplicate, Skipped};
    let same_hash = prev_rcv_msg_hash == received_prev_msg_hash;
    let next = prev_ext_snd_id.saturating_add(1);
    if ext_snd_id == next && same_hash {
        MsgIntegrity::Ok
    } else if ext_snd_id < prev_ext_snd_id {
        MsgIntegrity::Error(BadId(ext_snd_id))
    } else if ext_snd_id == prev_ext_snd_id {
        MsgIntegrity::Error(Duplicate)
    } else if ext_snd_id > next {
        MsgIntegrity::Error(Skipped {
            from: next,
            to: ext_snd_id - 1,
        })
    } else {
        // Only the hash differs: `ext_snd_id == next` here.
        MsgIntegrity::Error(BadHash)
    }
}

/// `strEncode`: `OK`, `ERR ID 3`, `ERR NO_ID 2 4`, `ERR HASH`, `ERR DUPLICATE`.
impl fmt::Display for MsgIntegrity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok => f.write_str("OK"),
            Self::Error(MsgErrorType::Skipped { from, to }) => write!(f, "ERR NO_ID {from} {to}"),
            Self::Error(MsgErrorType::BadId(id)) => write!(f, "ERR ID {id}"),
            Self::Error(MsgErrorType::BadHash) => f.write_str("ERR HASH"),
            Self::Error(MsgErrorType::Duplicate) => f.write_str("ERR DUPLICATE"),
        }
    }
}
