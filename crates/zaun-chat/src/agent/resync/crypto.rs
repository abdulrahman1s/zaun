//! A failed decryption moves the ratchet-sync state, a decrypted message ends it (Agent/Protocol.hs `cryptoErrToSyncState`; Client.hs `cryptoError`; Agent.hs `A_CRYPTO`, `resetRatchetSync`).
use crate::crypto::{CryptoError, sha256};
use crate::ratchet::RatchetError;

use super::state::SyncState;
use crate::agent::client::{AgentClient, AgentEvent, ConnId};
use crate::agent::records::ConnRecord;

/// `cryptoErrToSyncState` of the `A_CRYPTO` error `cryptoError` makes; `None` for the others.
#[must_use]
pub fn crypto_sync_state(e: &RatchetError) -> Option<SyncState> {
    match e {
        RatchetError::AesDecrypt
        | RatchetError::Crypto(CryptoError::CbDecrypt)
        | RatchetError::EarlierMessage(_) => Some(SyncState::Allowed),
        RatchetError::HeaderKey | RatchetError::TooManySkipped(_) => Some(SyncState::Required),
        RatchetError::Crypto(
            CryptoError::LargeMsg
            | CryptoError::InvalidMsg
            | CryptoError::AesDecrypt
            | CryptoError::Aead
            | CryptoError::BadKey(_)
            | CryptoError::Dh
            | CryptoError::Random
            | CryptoError::WrongAlg(_),
        )
        | RatchetError::DuplicateMessage
        | RatchetError::LargeMsg
        | RatchetError::Header(_)
        | RatchetError::State
        | RatchetError::KemState
        | RatchetError::Kem(_)
        | RatchetError::Version
        | RatchetError::BadState(_)
        | RatchetError::Decode(_)
        | RatchetError::Encode(_) => None,
    }
}

/// What a message that did not decrypt leads to.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DecryptFailure {
    /// A duplicate, or a message that failed before and came again: acked without a word.
    Ignore,
    /// Acked and reported; `Some` is a new state to tell the engine (`RSYNC`).
    Report(Option<SyncState>),
}

impl ConnRecord {
    /// Only a connection that is not re-synchronising counts the error (`notifySync`).
    pub(crate) fn decrypt_failed(&mut self, enc: &[u8], e: &RatchetError) -> DecryptFailure {
        if *e == RatchetError::DuplicateMessage {
            return DecryptFailure::Ignore;
        }
        let Some(to) = crypto_sync_state(e) else {
            return DecryptFailure::Report(None);
        };
        let sync = self.sync_mut();
        if !sync.remember_failed(sha256(enc).to_vec()) {
            return DecryptFailure::Ignore;
        }
        let from = sync.state;
        let counts = matches!(
            from,
            SyncState::Ok | SyncState::Allowed | SyncState::Required
        );
        if !counts || from == to {
            return DecryptFailure::Report(None);
        }
        sync.state = to;
        DecryptFailure::Report(Some(to))
    }

    /// `resetRatchetSync`: true when the state went back to `Ok`.
    pub(crate) fn decrypted(&mut self) -> bool {
        let Some(sync) = self.sync.as_mut() else {
            return false;
        };
        match sync.state {
            SyncState::Ok | SyncState::Started => false,
            SyncState::Allowed | SyncState::Required | SyncState::Agreed => {
                sync.state = SyncState::Ok;
                true
            }
        }
    }
}

impl AgentClient {
    /// A decrypted message ends a re-sync and any error state; the engine hears before the message.
    pub(crate) fn sync_decrypted(&mut self, rec: &mut ConnRecord) {
        if rec.decrypted() {
            self.emit(AgentEvent::RatchetSync {
                conn: rec.id,
                state: SyncState::Ok,
            });
        }
    }

    /// `A_CRYPTO` changed the state: the engine hears it before the error.
    pub(crate) fn tell_sync(&mut self, conn: ConnId, state: Option<SyncState>) {
        if let Some(state) = state {
            self.emit(AgentEvent::RatchetSync { conn, state });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::client::ConnId;
    use crate::agent::records::ConnStatus;
    use crate::encoding::Version;

    fn rec(state: SyncState) -> ConnRecord {
        let mut r = ConnRecord::new(ConnId(1), ConnStatus::Active, Version(8), false);
        r.sync_mut().state = state;
        r
    }

    #[test]
    fn decryption_errors_map_to_allowed_or_required_as_in_7_1_4() {
        use RatchetError as E;
        let allowed = [
            E::AesDecrypt,
            E::Crypto(CryptoError::CbDecrypt),
            E::EarlierMessage(2),
        ];
        let required = [E::HeaderKey, E::TooManySkipped(600)];
        for e in allowed {
            assert_eq!(crypto_sync_state(&e), Some(SyncState::Allowed), "{e:?}");
        }
        for e in required {
            assert_eq!(crypto_sync_state(&e), Some(SyncState::Required), "{e:?}");
        }
        for e in [E::DuplicateMessage, E::Header("x"), E::KemState, E::Version] {
            assert_eq!(crypto_sync_state(&e), None, "{e:?}");
        }
    }

    #[test]
    fn a_crypto_error_changes_the_state_and_tells_only_on_a_change() {
        let mut r = rec(SyncState::Ok);
        let f = r.decrypt_failed(b"one", &RatchetError::HeaderKey);
        assert_eq!(f, DecryptFailure::Report(Some(SyncState::Required)));
        let f = r.decrypt_failed(b"two", &RatchetError::TooManySkipped(9));
        assert_eq!(f, DecryptFailure::Report(None));
        // 7.1.4 sets the state of the latest error, even when it is the milder one.
        let f = r.decrypt_failed(b"three", &RatchetError::AesDecrypt);
        assert_eq!(f, DecryptFailure::Report(Some(SyncState::Allowed)));
    }

    #[test]
    fn a_failed_message_delivered_again_is_not_counted_again() {
        let mut r = rec(SyncState::Ok);
        r.decrypt_failed(b"same", &RatchetError::AesDecrypt);
        r.sync_mut().state = SyncState::Ok;
        assert_eq!(
            r.decrypt_failed(b"same", &RatchetError::HeaderKey),
            DecryptFailure::Ignore
        );
        assert_eq!(r.sync_state(), SyncState::Ok);
    }

    #[test]
    fn a_crypto_error_during_a_resync_leaves_the_state() {
        for state in [SyncState::Started, SyncState::Agreed] {
            let mut r = rec(state);
            let f = r.decrypt_failed(b"old ratchet", &RatchetError::HeaderKey);
            assert_eq!(f, DecryptFailure::Report(None));
            assert_eq!(r.sync_state(), state);
        }
    }

    #[test]
    fn a_duplicate_is_ignored_and_other_failures_are_reported_without_a_state() {
        let mut r = rec(SyncState::Ok);
        let dup = r.decrypt_failed(b"d", &RatchetError::DuplicateMessage);
        assert_eq!(dup, DecryptFailure::Ignore);
        let bad = r.decrypt_failed(b"h", &RatchetError::Header("short"));
        assert_eq!(bad, DecryptFailure::Report(None));
        assert_eq!(r.sync_state(), SyncState::Ok);
    }

    #[test]
    fn a_decrypted_message_resets_every_state_but_started_to_ok() {
        for (state, reset) in [
            (SyncState::Ok, false),
            (SyncState::Allowed, true),
            (SyncState::Required, true),
            (SyncState::Started, false),
            (SyncState::Agreed, true),
        ] {
            let mut r = rec(state);
            assert_eq!(r.decrypted(), reset, "{state:?}");
            let after = if reset { SyncState::Ok } else { state };
            assert_eq!(r.sync_state(), after);
        }
    }
}
