//! SHA-2 and SHA-3 digests (Crypto.hs `sha256Hash`, `sha512Hash`, `sha3_256`, `sha3_384`; nook bytes.ts).
use sha2::{Digest, Sha256, Sha512};
use sha3::{Sha3_256, Sha3_384};

#[must_use]
pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

#[must_use]
pub fn sha512(data: &[u8]) -> [u8; 64] {
    Sha512::digest(data).into()
}

#[must_use]
pub fn sha3_256(data: &[u8]) -> [u8; 32] {
    Sha3_256::digest(data).into()
}

/// Short links derive the sender ID from it.
#[must_use]
pub fn sha3_384(data: &[u8]) -> [u8; 48] {
    Sha3_384::digest(data).into()
}

/// `KeyHash`: SHA-256 of a DER structure (a certificate or a key).
#[must_use]
pub fn key_hash(der: &[u8]) -> [u8; 32] {
    sha256(der)
}
