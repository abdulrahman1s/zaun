//! Crypto failures (Crypto.hs `CryptoError`).
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CryptoError {
    #[error("message too large to pad")]
    LargeMsg,
    #[error("invalid padded message")]
    InvalidMsg,
    #[error("box decryption failed")]
    CbDecrypt,
    #[error("AES-GCM decryption failed")]
    AesDecrypt,
    #[error("AEAD seal or open failed")]
    Aead,
    #[error("invalid {0}")]
    BadKey(&'static str),
    #[error("Diffie-Hellman failed")]
    Dh,
    #[error("the system random source failed")]
    Random,
    #[error("unsupported key algorithm for {0}")]
    WrongAlg(&'static str),
}
