//! Shared fixtures: the keys of the Haskell `ServerTests.hs`, hex helpers.
use crate::crypto::{Alg, KeyPair, PrivateKey, PublicKey};
use crate::encoding::b64_decode;

/// `samplePubKey` (Ed25519) of ServerTests.hs.
pub fn sample_pub_key() -> PublicKey {
    PublicKey::from_der(
        &b64_decode("MCowBQYDK2VwAyEAfAOflyvbJv1fszgzkQ6buiZJVgSpQWsucXq7U6zjMgY=").unwrap(),
    )
    .unwrap()
}

/// `sampleDhPubKey` (X25519) of ServerTests.hs.
pub fn sample_dh_key() -> PublicKey {
    PublicKey::from_der(
        &b64_decode("MCowBQYDK2VuAyEAriy+HcARIhqsgSjVnjKqoft+y6pxrxdY68zn4+LjYhQ=").unwrap(),
    )
    .unwrap()
}

pub fn keypair(alg: Alg) -> KeyPair {
    KeyPair::generate(alg).unwrap()
}

pub fn private(alg: Alg) -> PrivateKey {
    keypair(alg).private
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// DER prefix of an encoded Ed25519 key as a ByteString: length 44, SPKI header.
pub const ED_KEY_PREFIX: &str = "2c302a300506032b6570032100";
