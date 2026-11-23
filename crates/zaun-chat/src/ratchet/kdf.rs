//! Root and chain KDFs of the double ratchet with header encryption (Ratchet.hs `rootKdf`, `chainKdf`, `hkdf3`).
use zeroize::Zeroizing;

use super::error::Result;
use super::kem::KemSharedSecret;
use super::key::Key32;
use crate::crypto::{PrivateKey, PublicKey, dh, hkdf_sha512};

/// A one-time AES key and its 16-byte IV (Ratchet.hs `MessageKey`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MessageKey {
    pub key: Key32,
    pub iv: [u8; 16],
}

/// Three 32-byte outputs of one HKDF-SHA512 call.
pub(crate) fn hkdf3(salt: &[u8], ikm: &[u8], info: &[u8]) -> Result<[[u8; 32]; 3]> {
    let out = hkdf_sha512::<96>(salt, ikm, info)?;
    let part = |i: usize| -> [u8; 32] {
        let mut b = [0u8; 32];
        b.copy_from_slice(&out[i * 32..(i + 1) * 32]);
        b
    };
    Ok([part(0), part(1), part(2)])
}

/// `rootKdf`: the new root key, the chain key and the next header key.
pub(crate) fn root_kdf(
    rk: &Key32,
    peer: &PublicKey,
    own: &PrivateKey,
    kem: Option<&KemSharedSecret>,
) -> Result<(Key32, Key32, Key32)> {
    let mut ikm = Zeroizing::new(dh(peer, own)?.as_bytes().to_vec());
    if let Some(ss) = kem {
        ikm.extend_from_slice(ss.as_ref());
    }
    let [rk, ck, nhk] = hkdf3(rk.as_bytes(), &ikm, b"SimpleXRootRatchet")?;
    Ok((Key32::new(rk), Key32::new(ck), Key32::new(nhk)))
}

pub(crate) struct ChainStep {
    pub next: Key32,
    pub message: MessageKey,
    pub header_iv: [u8; 16],
}

/// `chainKdf`: the next chain key, the message key with its IV, and the header IV.
pub(crate) fn chain_kdf(ck: &Key32) -> Result<ChainStep> {
    let [next, mk, ivs] = hkdf3(&[], ck.as_bytes(), b"SimpleXChainRatchet")?;
    let (mut iv, mut header_iv) = ([0u8; 16], [0u8; 16]);
    iv.copy_from_slice(&ivs[..16]);
    header_iv.copy_from_slice(&ivs[16..]);
    Ok(ChainStep {
        next: Key32::new(next),
        message: MessageKey {
            key: Key32::new(mk),
            iv,
        },
        header_iv,
    })
}
