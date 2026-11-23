//! The immutable, signed part of link data: root key and the connection request (Agent/Protocol.hs `FixedLinkData`, AGPL-3.0-only).
use crate::crypto::{Alg, PublicKey};
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, VersionRange};

use crate::agent::keys::expect_alg;
use crate::agent::links::BinaryConnReq;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedLinkData {
    pub agent_v_range: VersionRange,
    /// Ed25519; signs all link data.
    pub root_key: PublicKey,
    pub link_conn_req: BinaryConnReq,
    /// An empty ID is read as absent.
    pub link_entity_id: Option<Vec<u8>>,
}

impl Encode for FixedLinkData {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (self.agent_v_range, &self.root_key, &self.link_conn_req).encode(out)?;
        self.link_entity_id
            .as_ref()
            .map_or(Ok(()), |id| id.encode(out))
    }
}

impl Decode for FixedLinkData {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (agent_v_range, root_key, link_conn_req) =
            <(VersionRange, PublicKey, BinaryConnReq)>::decode(d)?;
        let link_entity_id = Vec::<u8>::decode(&mut d.clone())
            .ok()
            .filter(|id| !id.is_empty());
        d.take_all();
        Ok(Self {
            agent_v_range,
            root_key: expect_alg(root_key, Alg::Ed25519)?,
            link_conn_req,
            link_entity_id,
        })
    }
}
