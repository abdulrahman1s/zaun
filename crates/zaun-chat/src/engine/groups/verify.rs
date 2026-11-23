//! A signed (`/`) element checked against its author's member key (Subscriber.hs `withVerifiedMsg`, `verifyGroupSig`; Internal.hs `groupBindingData`).

use crate::chat::ids::{MemberId, b64_decode};
use crate::chat::signed::{ChatBinding, SignedMsg};
use crate::crypto::{PublicKey, Signature};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;

/// `groupBindingData` of a classic group: `smpEncode (memberId, memberKey)`, each a
/// ByteString behind its one-byte length, the key as its DER.
fn binding_data(member_id: &MemberId, der: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for part in [member_id.as_bytes(), der] {
        out.push(u8::try_from(part.len()).ok()?);
        out.extend_from_slice(part);
    }
    Some(out)
}

/// `verifyMemberSig`: every signature verifies over `'G' ‖ binding data ‖ signed body`.
#[must_use]
pub(crate) fn signed_by(member_id: &MemberId, key_b64: &str, signed: &SignedMsg) -> bool {
    let Ok(der) = b64_decode(key_b64) else {
        return false;
    };
    let Ok(key @ PublicKey::Ed25519(_)) = PublicKey::from_der(&der) else {
        return false;
    };
    let Some(data) = binding_data(member_id, &der) else {
        return false;
    };
    let input = signed.signed_input(&data);
    signed.signatures.iter().all(|s| {
        <[u8; 64]>::try_from(s.signature.as_slice())
            .is_ok_and(|raw| key.verify(&Signature::Ed25519(raw), &input))
    })
}

impl<A: AgentPort> Engine<A> {
    /// `withVerifiedMsg` in a classic group, where a signature is optional: an unsigned
    /// element, or one whose author has no key here, is taken; a group signature from a
    /// member whose key is known must verify, or the element is dropped. 7.1.4 also adds a
    /// "bad signature" item; the page has no notice for it, so it is only logged.
    pub(crate) fn check_signature(
        &self,
        group: i64,
        author: i64,
        signed: Option<&SignedMsg>,
    ) -> Result<()> {
        let Some(sm) = signed else {
            return Ok(());
        };
        let m = self.member(group, author)?;
        let (Some(key), ChatBinding::Group) = (&m.key, sm.binding) else {
            return Ok(());
        };
        if signed_by(&m.member_id, key, sm) {
            return Ok(());
        }
        Err(fail(
            Code::RequestFailed,
            "bad signature: the element is dropped",
        ))
    }
}
