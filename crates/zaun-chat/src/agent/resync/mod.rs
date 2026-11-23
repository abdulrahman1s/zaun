//! Ratchet re-synchronisation, starting one or answering a peer's: its `AgentRatchetKey`, a new ratchet, `EREADY`; and the state decryption errors set (simplexmq 7.1.4 Agent.hs `newRatchetKey`, `ereadyMsg`, Agent/Protocol.hs `RatchetSyncState`, AGPL-3.0-only).
mod crypto;
mod keys;
mod receive;
mod send;
mod start;
mod state;

pub(crate) use crypto::DecryptFailure;
pub use crypto::crypto_sync_state;
pub(crate) use receive::BeforeDecrypt;
#[cfg(test)]
pub(crate) use send::ratchet_info;
pub use state::{RatchetSync, SyncState};
