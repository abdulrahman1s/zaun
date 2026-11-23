//! X25519 on raw key bytes (Crypto.hs `X25519` instances, nook keys.ts).
use x25519_dalek::{PublicKey, StaticSecret};

pub(super) fn public(secret: &[u8; 32]) -> [u8; 32] {
    PublicKey::from(&StaticSecret::from(*secret)).to_bytes()
}

pub(super) fn dh(public: &[u8; 32], secret: &[u8; 32]) -> [u8; 32] {
    StaticSecret::from(*secret)
        .diffie_hellman(&PublicKey::from(*public))
        .to_bytes()
}
