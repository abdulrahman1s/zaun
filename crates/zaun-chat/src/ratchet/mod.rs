//! SimpleX's post-quantum double ratchet: X3DH on X448, header encryption, sntrup761, skipped keys (ported from simplexmq Crypto/Ratchet.hs and nook src/sx/ratchet, AGPL-3.0-only).

mod codec;
mod decrypt;
mod encrypt;
mod envelope;
mod error;
mod generate;
mod header;
mod init;
mod kdf;
mod kem;
mod kem_params;
mod kem_state;
mod key;
mod lengths;
mod params;
mod pq;
mod query;
mod resync;
mod skipped;
mod state;
mod step;
mod uri;
mod version;
mod x3dh;

pub use encrypt::MsgEncryptKey;
pub use envelope::{EncMessageHeader, EncRatchetMessage};
pub use error::{RatchetError, Result};
pub use generate::{PrivKem, PrivParams, generate_rcv_params, generate_snd_params};
pub use header::MsgHeader;
pub use kdf::MessageKey;
pub use kem::{
    KEM_CIPHERTEXT_LEN, KEM_PUBLIC_KEY_LEN, KemCiphertext, KemKeyPair, KemPublicKey, KemSecretKey,
    KemSharedSecret, kem_decapsulate, kem_encapsulate, kem_keypair,
};
pub use kem_params::{KemParams, UseKem, reply_kem};
pub use kem_state::{KemAccepted, RatchetKem};
pub use key::Key32;
pub use lengths::{
    e2e_enc_agent_msg_length, e2e_enc_conn_info_length, full_header_len, padded_header_len,
};
pub use params::{E2eParams, RcvParams};
pub use pq::{PqEncryption, PqSupport};
pub use resync::{Resynced, new_resync_params, ratchet_key_hash};
pub use skipped::{MAX_SKIP, MAX_SKIPPED_MSG_KEYS, SkippedKey, SkippedKeys};
pub use state::Ratchet;
pub use uri::ParamsUri;
pub use version::{
    CURRENT_E2E_VERSION, MIN_SUPPORTED_E2E_VERSION, RatchetVersions, adjust_e2e_range,
    compatible_e2e_version, supported_e2e_range,
};
pub use x3dh::{InitParams, pq_x3dh_rcv, pq_x3dh_snd};
