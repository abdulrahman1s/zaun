//! A delivery receipt inside `A_RCVD` (Agent/Protocol.hs `AMessageReceipt`, AGPL-3.0-only).
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Large};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AMessageReceipt {
    /// The sender's own ID of the message that was received.
    pub agent_msg_id: i64,
    pub msg_hash: Vec<u8>,
    /// Chat's receipt body.
    pub rcpt_info: Vec<u8>,
}

impl Encode for AMessageReceipt {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (
            self.agent_msg_id,
            &self.msg_hash,
            Large(self.rcpt_info.clone()),
        )
            .encode(out)
    }
}

impl Decode for AMessageReceipt {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (agent_msg_id, msg_hash, Large(rcpt_info)) = <(i64, Vec<u8>, Large)>::decode(d)?;
        Ok(Self {
            agent_msg_id,
            msg_hash,
            rcpt_info,
        })
    }
}
