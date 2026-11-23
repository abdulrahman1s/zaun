//! Signature envelope of a `/` element; verification is left to the caller (Protocol.hs `SignedMsg`).
use super::bin::{Reader, put_short};
use super::error::{ChatError, Result, parse_err};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatBinding {
    Group,
    Direct,
    Channel,
}

impl ChatBinding {
    pub const fn tag(self) -> u8 {
        match self {
            Self::Group => b'G',
            Self::Direct => b'D',
            Self::Channel => b'C',
        }
    }

    fn from_tag(c: u8) -> Result<Self> {
        match c {
            b'G' => Ok(Self::Group),
            b'D' => Ok(Self::Direct),
            b'C' => Ok(Self::Channel),
            c => parse_err(format!("invalid ChatBinding: {}", char::from(c))),
        }
    }
}

/// `KRMember`, the only key reference in 7.1.4, then the raw 64-byte Ed25519 signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MsgSignature {
    pub signature: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedMsg {
    pub binding: ChatBinding,
    pub signatures: Vec<MsgSignature>,
    /// The exact bytes that were signed after `binding_data`; forward and store these, never a re-encoding.
    pub signed_body: Vec<u8>,
}

/// Checks a signature against the member's key. The caller owns the keys and the binding data
/// (`smpEncode (memberId, memberKey)` for groups).
pub trait SignatureVerifier {
    fn verify(&self, signed: &SignedMsg) -> bool;
}

impl SignedMsg {
    /// The bytes the signature covers: the binding tag, the binding data, the body (`encodeChatBinding`).
    pub fn signed_input(&self, binding_data: &[u8]) -> Vec<u8> {
        let mut out = vec![self.binding.tag()];
        out.extend_from_slice(binding_data);
        out.extend_from_slice(&self.signed_body);
        out
    }

    /// Wire form after the `/`: binding, signature count, `'M'` + short signature each, body.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let count = u8::try_from(self.signatures.len())
            .map_err(|_| ChatError::Limit("too many signatures"))?;
        let mut out = vec![self.binding.tag(), count];
        for s in &self.signatures {
            out.push(b'M');
            put_short(&mut out, &s.signature)?;
        }
        out.extend_from_slice(&self.signed_body);
        Ok(out)
    }

    pub(crate) fn read(r: &mut Reader<'_>) -> Result<Self> {
        let binding = ChatBinding::from_tag(r.u8()?)?;
        let count = r.u8()?;
        if count == 0 {
            return parse_err("empty list");
        }
        let mut signatures = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            match r.u8()? {
                b'M' => signatures.push(MsgSignature {
                    signature: r.short_bytes()?.to_vec(),
                }),
                c => return parse_err(format!("invalid KeyRef tag: {}", char::from(c))),
            }
        }
        Ok(Self {
            binding,
            signatures,
            signed_body: r.rest().to_vec(),
        })
    }
}
