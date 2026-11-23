//! The call's media key (Call.hs `callMediaKey`): X25519 between the two sides' keys, then HKDF from version 2.

use zaun_chat_wire::model::CallKey;

use crate::chat::ids::{b64_decode, b64_encode};
use crate::crypto::{Alg, KeyPair, PrivateKey, PublicKey, dh, hkdf_sha512};
use crate::engine::error::{Code, Result, fail};

/// `callMediaKdfVersion`: below it the DH secret itself is the key.
const KDF_VERSION: u16 = crate::chat::version::CALL_MEDIA_KDF_VERSION;

pub(super) fn new_pair() -> Result<KeyPair> {
    KeyPair::generate(Alg::X25519).map_err(|e| fail(Code::CallFailed, format!("call key: {e}")))
}

/// `callDhPubKey`: padded base64url of the X.509 key.
#[must_use]
pub(crate) fn public_b64(key: &PublicKey) -> String {
    b64_encode(&key.to_der())
}

pub(super) fn parse_public(text: &str) -> Option<PublicKey> {
    let key = PublicKey::from_der(&b64_decode(text).ok()?).ok()?;
    (key.alg() == Alg::X25519).then_some(key)
}

/// `None` when the keys do not agree (not both X25519).
#[must_use]
pub(crate) fn media_key(
    version: u16,
    call_id: &[u8],
    peer: &PublicKey,
    own: &PrivateKey,
) -> Option<CallKey> {
    let secret = dh(peer, own).ok()?;
    let secret = secret.as_x25519().ok()?;
    if version < KDF_VERSION {
        return Some(CallKey(*secret));
    }
    let key = hkdf_sha512::<32>(call_id, secret, b"SimpleXCallMediaKey").ok()?;
    Some(CallKey(*key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_sides_derive_the_same_key_and_version_two_is_not_the_raw_secret() {
        let (a, b) = (new_pair().unwrap(), new_pair().unwrap());
        let id = [7u8; 16];
        for v in [1, 2] {
            let ka = media_key(v, &id, &b.public, &a.private).unwrap();
            let kb = media_key(v, &id, &a.public, &b.private).unwrap();
            assert_eq!(ka, kb);
        }
        let raw = media_key(1, &id, &b.public, &a.private).unwrap();
        let kdf = media_key(2, &id, &b.public, &a.private).unwrap();
        assert_ne!(raw, kdf);
        let other = media_key(2, &[8u8; 16], &b.public, &a.private).unwrap();
        assert_ne!(kdf, other, "the call id is the salt");
    }

    #[test]
    fn a_public_key_is_the_padded_base64url_of_its_x509_form() {
        let k = new_pair().unwrap();
        let text = public_b64(&k.public);
        assert_eq!(text.len(), 60);
        assert_eq!(parse_public(&text), Some(k.public));
        assert_eq!(parse_public("nonsense"), None);
    }
}
