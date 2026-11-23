//! Wire encoding of keys and signatures: a ByteString of the DER, or of the raw signature (Crypto.hs `Encoding` instances).
use zeroize::Zeroizing;

use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError};

use super::{
    Alg, CryptoError, PrivateKey, PublicKey, Signature, decode_private_der, decode_public_der,
    encode_private_der, encode_public_der,
};

impl PublicKey {
    /// X.509 SubjectPublicKeyInfo.
    #[must_use]
    pub fn to_der(&self) -> Vec<u8> {
        encode_public_der(self.alg(), self.as_bytes())
    }

    pub fn from_der(der: &[u8]) -> Result<Self, CryptoError> {
        let (alg, key) = decode_public_der(der)?;
        Self::from_raw(alg, key)
    }
}

impl PrivateKey {
    /// PKCS#8.
    #[must_use]
    pub fn to_der(&self) -> Zeroizing<Vec<u8>> {
        encode_private_der(self.alg(), self.as_bytes())
    }

    pub fn from_der(der: &[u8]) -> Result<Self, CryptoError> {
        let (alg, key) = decode_private_der(der)?;
        Self::from_raw(alg, &key)
    }
}

impl Encode for PublicKey {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        self.to_der().encode(out)
    }
}

impl Decode for PublicKey {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        Self::from_der(&Vec::<u8>::decode(d)?).map_err(|_| DecodeError::Invalid("public key"))
    }
}

/// For storing keys only, never for sending.
impl Encode for PrivateKey {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        self.to_der().encode(out)
    }
}

impl Decode for PrivateKey {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let der = Zeroizing::new(Vec::<u8>::decode(d)?);
        Self::from_der(&der).map_err(|_| DecodeError::Invalid("private key"))
    }
}

impl Encode for Signature {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        self.as_bytes().encode(out)
    }
}

/// The algorithm is told by the length: 64 for Ed25519, 114 for Ed448.
impl Decode for Signature {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let bytes = Vec::<u8>::decode(d)?;
        let alg = match bytes.len() {
            64 => Alg::Ed25519,
            114 => Alg::Ed448,
            _ => return Err(DecodeError::Invalid("signature")),
        };
        Signature::from_raw(alg, &bytes).map_err(|_| DecodeError::Invalid("signature"))
    }
}
