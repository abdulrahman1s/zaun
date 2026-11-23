//! Additional owners of a contact link and their authorisation chain (Agent/Protocol.hs `OwnerAuth`, `validateLinkOwners`, AGPL-3.0-only).
use crate::crypto::{Alg, PublicKey, Signature};
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, b64_encode};

use crate::agent::error::{AgentError, Result};
use crate::agent::keys::expect_alg;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerAuth {
    /// Unique in the list, set by the application (a member ID in chat).
    pub owner_id: Vec<u8>,
    /// Ed25519.
    pub owner_key: PublicKey,
    /// `sig(ownerId || ownerKey)` by the root key or an earlier owner.
    pub auth_owner_sig: Signature,
}

impl OwnerAuth {
    fn signed_by(&self, key: &PublicKey) -> bool {
        key.verify(
            &self.auth_owner_sig,
            &[self.owner_id.as_slice(), &self.owner_key.to_der()].concat(),
        )
    }
}

/// An owner is a ByteString of its own fields, so that the fields can grow later.
impl Encode for OwnerAuth {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (&self.owner_id, &self.owner_key, &self.auth_owner_sig)
            .to_bytes()?
            .encode(out)
    }
}

impl Decode for OwnerAuth {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let inner = Vec::<u8>::decode(d)?;
        let (owner_id, owner_key, auth_owner_sig) =
            <(Vec<u8>, PublicKey, Signature)>::decode(&mut Decoder::new(&inner))?; // trailing fields are ignored
        Ok(Self {
            owner_id,
            owner_key: expect_alg(owner_key, Alg::Ed25519)?,
            auth_owner_sig,
        })
    }
}

fn link_err(what: &str, id: &[u8]) -> AgentError {
    AgentError::Link(what.replace("{}", &b64_encode(id, false)))
}

/// `validateLinkOwners`: each owner is signed by the root or by an owner before it, and keys and IDs are unique.
pub fn validate_link_owners(root: &PublicKey, owners: &[OwnerAuth]) -> Result<()> {
    for (i, o) in owners.iter().enumerate() {
        let id = &o.owner_id;
        if &o.owner_key == root {
            return Err(link_err("owner key for ID {} matches root key", id));
        }
        let prev = &owners[..i];
        if prev
            .iter()
            .any(|p| &p.owner_id == id || p.owner_key == o.owner_key)
        {
            return Err(link_err("duplicate owner key or ID {}", id));
        }
        if !(o.signed_by(root) || prev.iter().any(|p| o.signed_by(&p.owner_key))) {
            return Err(link_err("invalid authorization of owner ID {}", id));
        }
    }
    Ok(())
}
