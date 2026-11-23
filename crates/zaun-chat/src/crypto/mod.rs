//! Cryptographic primitives SimpleX uses, on RustCrypto (ADR-0042). Ported from simplexmq src/Simplex/Messaging/Crypto.hs and nook src/sx/crypto (AGPL-3.0-only).

mod aes_gcm16;
mod alg;
mod cbox;
mod der;
mod ed25519;
mod ed448;
mod error;
mod hash;
mod hkdf;
mod key_codec;
mod keys;
mod pad;
mod poly;
mod random;
mod sb_chain;
mod sb_stream;
mod secretbox;
mod x25519;
mod x448;
mod xchacha;

pub use aes_gcm16::{
    AEAD_TAG_LEN, AesKey, aead_decrypt, aead_decrypt_no_pad, aead_encrypt, aead_encrypt_no_pad,
};
pub use alg::Alg;
pub use cbox::{
    CB_AUTHENTICATOR_LEN, cb_authenticate, cb_decrypt, cb_decrypt_no_pad, cb_encrypt,
    cb_encrypt_no_pad, cb_verify,
};
pub use der::{decode_private_der, decode_public_der, encode_private_der, encode_public_der};
pub use error::CryptoError;
pub use hash::{key_hash, sha3_256, sha3_384, sha256, sha512};
pub use hkdf::hkdf_sha512;
pub use keys::{DhSecret, KeyPair, PrivateKey, PublicKey, Signature, dh};
pub use pad::{MAX_MSG_LEN, can_pad, pad, pad_lazy, split_len, unpad, unpad_lazy};
pub use poly::Poly1305;
pub use random::{fill_random, random_array, random_vec};
pub use sb_chain::{sbc_hkdf, sbc_init};
pub use sb_stream::{
    SbState, sb_open_tail_tag, sb_open_tail_tag_no_pad, sb_seal_tail_tag, sb_seal_tail_tag_no_pad,
};
pub use secretbox::{
    CbNonce, SbKey, box_key, crypto_box, crypto_box_open, sb_decrypt, sb_decrypt_no_pad,
    sb_encrypt, sb_encrypt_no_pad, xor_nonce,
};
pub use xchacha::{xchacha_open, xchacha_seal, xchacha_seal_with_nonce};
