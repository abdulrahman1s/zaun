//! The sntrup761 KEM as simplexmq uses it: wire encodings and seeded keygen (Crypto/SNTRUP761/Bindings.hs; nook ratchet/sntrup761/kem.ts).
// TODO(crypto): move to `crypto` if other modules need the KEM.
use sntrup761::{Ciphertext, DecapsulationKey, EncapsulationKey, SharedSecret};
use zeroize::Zeroizing;

use super::error::{RatchetError, Result};
use crate::crypto::random_array;
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Large};

pub const KEM_PUBLIC_KEY_LEN: usize = sntrup761::PUBLIC_KEY_SIZE;
pub const KEM_CIPHERTEXT_LEN: usize = sntrup761::CIPHERTEXT_SIZE;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct KemPublicKey(Box<EncapsulationKey>);

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct KemSecretKey(Box<DecapsulationKey>);

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct KemCiphertext(Box<Ciphertext>);

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct KemSharedSecret(Box<SharedSecret>);

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct KemKeyPair {
    pub public: KemPublicKey,
    pub secret: KemSecretKey,
}

macro_rules! kem_bytes {
    ($t:ident, $inner:ty, $what:literal) => {
        impl $t {
            pub fn from_bytes(b: &[u8]) -> Result<Self> {
                <$inner>::try_from(b)
                    .map(|k| Self(Box::new(k)))
                    .map_err(|_| RatchetError::Kem($what))
            }
        }

        impl AsRef<[u8]> for $t {
            fn as_ref(&self) -> &[u8] {
                (*self.0).as_ref()
            }
        }
    };
}

kem_bytes!(KemPublicKey, EncapsulationKey, "public key size");
kem_bytes!(KemSecretKey, DecapsulationKey, "secret key size");
kem_bytes!(KemCiphertext, Ciphertext, "ciphertext size");
kem_bytes!(KemSharedSecret, SharedSecret, "shared secret size");

/// `sntrup761Keypair`.
pub fn kem_keypair() -> Result<KemKeyPair> {
    let seed = Zeroizing::new(random_array::<32>()?);
    let (public, secret) = sntrup761::generate_key_from_seed(*seed);
    Ok(KemKeyPair {
        public: KemPublicKey(Box::new(public)),
        secret: KemSecretKey(Box::new(secret)),
    })
}

/// `sntrup761Enc`: the ciphertext for the peer and the shared secret.
pub fn kem_encapsulate(pk: &KemPublicKey) -> Result<(KemCiphertext, KemSharedSecret)> {
    let seed = Zeroizing::new(random_array::<32>()?);
    let (ct, ss) = pk.0.encapsulate_deterministic(*seed);
    Ok((KemCiphertext(Box::new(ct)), KemSharedSecret(Box::new(ss))))
}

/// `sntrup761Dec`. A bad ciphertext gives an unrelated secret (implicit rejection), not an error.
#[must_use]
pub fn kem_decapsulate(ct: &KemCiphertext, sk: &KemSecretKey) -> KemSharedSecret {
    KemSharedSecret(Box::new(sk.0.decapsulate(&ct.0)))
}

fn encode_large(b: &[u8], out: &mut Vec<u8>) -> std::result::Result<(), EncodeError> {
    Large(b.to_vec()).encode(out)
}

fn decode_with<T>(
    d: &mut Decoder<'_>,
    make: fn(&[u8]) -> Result<T>,
    what: &'static str,
) -> std::result::Result<T, DecodeError> {
    make(&Large::decode(d)?.0).map_err(|_| DecodeError::Invalid(what))
}

macro_rules! large_codec {
    ($t:ident, $what:literal) => {
        impl Encode for $t {
            fn encode(&self, out: &mut Vec<u8>) -> std::result::Result<(), EncodeError> {
                encode_large(self.as_ref(), out)
            }
        }

        impl Decode for $t {
            fn decode(d: &mut Decoder<'_>) -> std::result::Result<Self, DecodeError> {
                decode_with(d, Self::from_bytes, $what)
            }
        }
    };
}

large_codec!(KemPublicKey, "KEM public key");
large_codec!(KemSecretKey, "KEM secret key");
large_codec!(KemCiphertext, "KEM ciphertext");

/// A plain ByteString (1-byte length), as Bindings.hs encodes the shared secret.
impl Encode for KemSharedSecret {
    fn encode(&self, out: &mut Vec<u8>) -> std::result::Result<(), EncodeError> {
        self.as_ref().encode(out)
    }
}

impl Decode for KemSharedSecret {
    fn decode(d: &mut Decoder<'_>) -> std::result::Result<Self, DecodeError> {
        let b = Zeroizing::new(Vec::<u8>::decode(d)?);
        Self::from_bytes(&b).map_err(|_| DecodeError::Invalid("KEM shared secret"))
    }
}

impl Encode for KemKeyPair {
    fn encode(&self, out: &mut Vec<u8>) -> std::result::Result<(), EncodeError> {
        (&self.public, &self.secret).encode(out)
    }
}

impl Decode for KemKeyPair {
    fn decode(d: &mut Decoder<'_>) -> std::result::Result<Self, DecodeError> {
        let (public, secret) = <(KemPublicKey, KemSecretKey)>::decode(d)?;
        Ok(Self { public, secret })
    }
}
