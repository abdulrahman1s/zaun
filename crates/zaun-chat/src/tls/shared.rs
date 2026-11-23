//! State both halves of a split connection see: the handshake facts and a KeyUpdate request.
use std::sync::atomic::AtomicBool;

use super::info::TlsInfo;

pub(super) struct Shared {
    pub(super) info: TlsInfo,
    /// The peer sent KeyUpdate(update_requested); the writer answers before its next record.
    pub(super) key_update_requested: AtomicBool,
}
