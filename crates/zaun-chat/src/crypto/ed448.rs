//! Ed448 on raw key bytes, empty context (Crypto.hs `Ed448` instances, nook keys.ts).
use ed448_goldilocks::{Signature, SigningKey, VerifyingKey};

pub(super) fn public(seed: &[u8; 57]) -> [u8; 57] {
    SigningKey::from(&(*seed).into()).verifying_key().to_bytes()
}

pub(super) fn sign(seed: &[u8; 57], msg: &[u8]) -> [u8; 114] {
    SigningKey::from(&(*seed).into()).sign_raw(msg).to_bytes()
}

pub(super) fn verify(public: &[u8; 57], sig: &[u8; 114], msg: &[u8]) -> bool {
    VerifyingKey::from_bytes(public)
        .is_ok_and(|vk| vk.verify_raw(&Signature::from_bytes(sig), msg).is_ok())
}
