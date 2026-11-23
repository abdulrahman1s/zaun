//! What a requester keeps until its request is sent, and what an address keeps of each request it received (Agent.hs `joinConnSrv`, `storeInvitation`, AGPL-3.0-only).
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Large, Version};

use crate::agent::queue_info::SmpQueueInfo;

/// A request to an address: the queue it goes to, and the sealed body once it is built, so a retry sends the same bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactRequest {
    pub queue: SmpQueueInfo,
    /// The agent version agreed with the address, written into the request.
    pub agent_version: Version,
    /// Set when the address publishes ratchet keys: the request is ratchet-encrypted (`AgentContactRequest`).
    pub ratchet_key_id: Option<Vec<u8>>,
    pub body: Option<Vec<u8>>,
}

impl Encode for ContactRequest {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        let body = self.body.clone().map(Large);
        (&self.queue, self.agent_version, &self.ratchet_key_id, &body).encode(out)
    }
}

impl Decode for ContactRequest {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (queue, agent_version, ratchet_key_id, body) =
            <(SmpQueueInfo, Version, Option<Vec<u8>>, Option<Large>)>::decode(d)?;
        Ok(Self {
            queue,
            agent_version,
            ratchet_key_id,
            body: body.map(|l| l.0),
        })
    }
}

/// A request received on an address, kept until accepted or rejected (`Invitation` in Store).
/// Eviction: the engine answers each one; the agent never drops one by itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredInvitation {
    /// The requester's invitation link, as written.
    pub conn_req: String,
    pub conn_info: Vec<u8>,
}

impl Encode for StoredInvitation {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (
            Large(self.conn_req.clone().into_bytes()),
            Large(self.conn_info.clone()),
        )
            .encode(out)
    }
}

impl Decode for StoredInvitation {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (Large(req), Large(conn_info)) = <(Large, Large)>::decode(d)?;
        Ok(Self {
            conn_req: String::from_utf8(req).map_err(|_| DecodeError::Utf8)?,
            conn_info,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stored_request_with_a_large_conn_info_reads_back_the_same() {
        let inv = StoredInvitation {
            conn_req: "simplex:/invitation#/?v=6-8".into(),
            conn_info: vec![7; 11000],
        };
        assert_eq!(
            StoredInvitation::from_bytes(&inv.to_bytes().unwrap()).unwrap(),
            inv
        );
    }
}
