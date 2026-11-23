//! Algorithm-tagged keys, signatures and DH secrets (Crypto.hs `APublicVerifyKey`, `APrivateSignKey`, `ASignature`, `DhSecret`).
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use super::{Alg, CryptoError, ed448, ed25519, random_array, x448, x25519};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PublicKey {
    Ed25519([u8; 32]),
    Ed448([u8; 57]),
    X25519([u8; 32]),
    X448([u8; 56]),
}

/// Raw seed (Ed) or scalar (X). Zeroed when dropped; `Debug` shows nothing.
#[derive(Clone)]
pub enum PrivateKey {
    Ed25519(Zeroizing<[u8; 32]>),
    Ed448(Zeroizing<[u8; 57]>),
    X25519(Zeroizing<[u8; 32]>),
    X448(Zeroizing<[u8; 56]>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signature {
    Ed25519([u8; 64]),
    Ed448([u8; 114]),
}

/// Raw DH output, before the HSalsa20 step of the box.
#[derive(Clone)]
pub enum DhSecret {
    X25519(Zeroizing<[u8; 32]>),
    X448(Zeroizing<[u8; 56]>),
}

#[derive(Clone)]
pub struct KeyPair {
    pub public: PublicKey,
    pub private: PrivateKey,
}

fn exact<const N: usize>(b: &[u8], what: &'static str) -> Result<[u8; N], CryptoError> {
    b.try_into().map_err(|_| CryptoError::BadKey(what))
}

impl PublicKey {
    pub fn from_raw(alg: Alg, b: &[u8]) -> Result<Self, CryptoError> {
        let what = "public key size";
        Ok(match alg {
            Alg::Ed25519 => Self::Ed25519(exact(b, what)?),
            Alg::Ed448 => Self::Ed448(exact(b, what)?),
            Alg::X25519 => Self::X25519(exact(b, what)?),
            Alg::X448 => Self::X448(exact(b, what)?),
        })
    }

    #[must_use]
    pub fn alg(&self) -> Alg {
        match self {
            Self::Ed25519(_) => Alg::Ed25519,
            Self::Ed448(_) => Alg::Ed448,
            Self::X25519(_) => Alg::X25519,
            Self::X448(_) => Alg::X448,
        }
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Ed25519(k) | Self::X25519(k) => k,
            Self::Ed448(k) => k,
            Self::X448(k) => k,
        }
    }

    /// False for a mismatched algorithm, as `verify` in Crypto.hs.
    #[must_use]
    pub fn verify(&self, sig: &Signature, msg: &[u8]) -> bool {
        match (self, sig) {
            (Self::Ed25519(k), Signature::Ed25519(s)) => ed25519::verify(k, s, msg),
            (Self::Ed448(k), Signature::Ed448(s)) => ed448::verify(k, s, msg),
            (Self::Ed25519(_) | Self::Ed448(_) | Self::X25519(_) | Self::X448(_), _) => false,
        }
    }
}

impl PrivateKey {
    pub fn from_raw(alg: Alg, b: &[u8]) -> Result<Self, CryptoError> {
        let what = "private key size";
        Ok(match alg {
            Alg::Ed25519 => Self::Ed25519(Zeroizing::new(exact(b, what)?)),
            Alg::Ed448 => Self::Ed448(Zeroizing::new(exact(b, what)?)),
            Alg::X25519 => Self::X25519(Zeroizing::new(exact(b, what)?)),
            Alg::X448 => Self::X448(Zeroizing::new(exact(b, what)?)),
        })
    }

    #[must_use]
    pub fn alg(&self) -> Alg {
        match self {
            Self::Ed25519(_) => Alg::Ed25519,
            Self::Ed448(_) => Alg::Ed448,
            Self::X25519(_) => Alg::X25519,
            Self::X448(_) => Alg::X448,
        }
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Ed25519(k) | Self::X25519(k) => &k[..],
            Self::Ed448(k) => &k[..],
            Self::X448(k) => &k[..],
        }
    }

    pub fn public(&self) -> Result<PublicKey, CryptoError> {
        Ok(match self {
            Self::Ed25519(k) => PublicKey::Ed25519(ed25519::public(k)),
            Self::Ed448(k) => PublicKey::Ed448(ed448::public(k)),
            Self::X25519(k) => PublicKey::X25519(x25519::public(k)),
            Self::X448(k) => PublicKey::X448(x448::public(k)?),
        })
    }

    /// Only Ed25519 and Ed448 keys sign.
    pub fn sign(&self, msg: &[u8]) -> Result<Signature, CryptoError> {
        match self {
            Self::Ed25519(k) => Ok(Signature::Ed25519(ed25519::sign(k, msg))),
            Self::Ed448(k) => Ok(Signature::Ed448(ed448::sign(k, msg))),
            Self::X25519(_) | Self::X448(_) => Err(CryptoError::WrongAlg("signing")),
        }
    }
}

impl std::fmt::Debug for PrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PrivateKey::{:?}(..)", self.alg())
    }
}

impl PartialEq for PrivateKey {
    fn eq(&self, other: &Self) -> bool {
        self.alg() == other.alg() && bool::from(self.as_bytes().ct_eq(other.as_bytes()))
    }
}

impl Eq for PrivateKey {}

impl Signature {
    pub fn from_raw(alg: Alg, b: &[u8]) -> Result<Self, CryptoError> {
        let what = "signature size";
        match alg {
            Alg::Ed25519 => Ok(Self::Ed25519(exact(b, what)?)),
            Alg::Ed448 => Ok(Self::Ed448(exact(b, what)?)),
            Alg::X25519 | Alg::X448 => Err(CryptoError::WrongAlg("signature")),
        }
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Ed25519(s) => s,
            Self::Ed448(s) => s,
        }
    }
}

impl DhSecret {
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::X25519(s) => &s[..],
            Self::X448(s) => &s[..],
        }
    }

    /// The secret the boxes take; SimpleX boxes only use X25519.
    pub fn as_x25519(&self) -> Result<&[u8; 32], CryptoError> {
        match self {
            Self::X25519(s) => Ok(s),
            Self::X448(_) => Err(CryptoError::WrongAlg("box")),
        }
    }
}

impl std::fmt::Debug for DhSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DhSecret(..)")
    }
}

impl KeyPair {
    /// `generateKeyPair`: a random seed or scalar, as the Haskell does.
    pub fn generate(alg: Alg) -> Result<Self, CryptoError> {
        let private = match alg {
            Alg::Ed25519 => PrivateKey::Ed25519(Zeroizing::new(random_array()?)),
            Alg::Ed448 => PrivateKey::Ed448(Zeroizing::new(random_array()?)),
            Alg::X25519 => PrivateKey::X25519(Zeroizing::new(random_array()?)),
            Alg::X448 => PrivateKey::X448(Zeroizing::new(random_array()?)),
        };
        Self::from_private(private)
    }

    pub fn from_private(private: PrivateKey) -> Result<Self, CryptoError> {
        Ok(Self {
            public: private.public()?,
            private,
        })
    }
}

/// `dh'`: the public and private keys must be of the same X algorithm.
pub fn dh(public: &PublicKey, private: &PrivateKey) -> Result<DhSecret, CryptoError> {
    match (public, private) {
        (PublicKey::X25519(p), PrivateKey::X25519(s)) => {
            Ok(DhSecret::X25519(Zeroizing::new(x25519::dh(p, s))))
        }
        (PublicKey::X448(p), PrivateKey::X448(s)) => {
            Ok(DhSecret::X448(Zeroizing::new(x448::dh(p, s)?)))
        }
        (
            PublicKey::Ed25519(_) | PublicKey::Ed448(_) | PublicKey::X25519(_) | PublicKey::X448(_),
            _,
        ) => Err(CryptoError::WrongAlg("dh")),
    }
}
