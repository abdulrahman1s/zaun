//! A connection's ratchet-sync state and what a re-sync keeps (Agent/Protocol.hs `RatchetSyncState`; Agent/Store.hs `ratchetSyncSendProhibited`; AgentStore.hs `ratchet_x3dh_keys`, `processed_ratchet_key_hashes`).
use crate::agent::error::{AgentError, Result};
use crate::agent::records::ConnRecord;
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, List};
use crate::ratchet::PrivParams;

/// Agent.hs `maxRatchetKeyHashes`.
const MAX_KEY_HASHES: usize = 100;
/// The ack of a failed message is lost at most a few times before it is gone from the server.
const MAX_FAILED_HASHES: usize = 8;

/// `RatchetSyncState`; a connection with none stored is `Ok`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyncState {
    #[default]
    Ok,
    Allowed,
    Required,
    Started,
    Agreed,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RatchetSync {
    pub state: SyncState,
    /// `ratchet_x3dh_keys`: our keys while a re-sync we started waits for the peer's.
    pub keys: Option<PrivParams>,
    /// `processed_ratchet_key_hashes`: the peer keys already handled. Eviction: the newest
    /// `MAX_KEY_HASHES` stay. They outlive the state, or a repeated key would replace a working ratchet.
    pub key_hashes: Vec<Vec<u8>>,
    /// `encrypted_rcv_message_hashes` of the messages that failed to decrypt, so that one delivered
    /// again is not counted again. Eviction: the newest `MAX_FAILED_HASHES` stay.
    pub failed: Vec<Vec<u8>>,
}

fn push_bounded(v: &mut Vec<Vec<u8>>, hash: Vec<u8>, max: usize) {
    v.push(hash);
    if v.len() > max {
        v.drain(..v.len() - max);
    }
}

impl RatchetSync {
    #[must_use]
    pub fn knows_key(&self, hash: &[u8]) -> bool {
        self.key_hashes.iter().any(|h| h == hash)
    }

    pub fn remember_key(&mut self, hash: Vec<u8>) {
        push_bounded(&mut self.key_hashes, hash, MAX_KEY_HASHES);
    }

    /// False when this message failed before.
    pub fn remember_failed(&mut self, hash: Vec<u8>) -> bool {
        if self.failed.contains(&hash) {
            return false;
        }
        push_bounded(&mut self.failed, hash, MAX_FAILED_HASHES);
        true
    }
}

impl ConnRecord {
    #[must_use]
    pub fn sync_state(&self) -> SyncState {
        self.sync.as_ref().map_or(SyncState::Ok, |s| s.state)
    }

    pub fn sync_mut(&mut self) -> &mut RatchetSync {
        self.sync.get_or_insert_with(RatchetSync::default)
    }

    /// `ratchetSyncSendProhibited`: until the ratchets agree, a message would not decrypt.
    pub fn ensure_can_send(&self) -> Result<()> {
        match self.sync_state() {
            SyncState::Ok | SyncState::Allowed => Ok(()),
            SyncState::Required | SyncState::Started | SyncState::Agreed => Err(
                AgentError::Prohibited("send prohibited: the ratchet is being re-synchronised"),
            ),
        }
    }
}

impl SyncState {
    const fn tag(self) -> u8 {
        match self {
            Self::Ok => b'O',
            Self::Allowed => b'L',
            Self::Required => b'R',
            Self::Started => b'S',
            Self::Agreed => b'A',
        }
    }
}

impl Encode for RatchetSync {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        self.state.tag().encode(out)?;
        let (keys, failed) = (List(self.key_hashes.clone()), List(self.failed.clone()));
        (&self.keys, keys, failed).encode(out)
    }
}

impl Decode for RatchetSync {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let state = match u8::decode(d)? {
            b'O' => SyncState::Ok,
            b'L' => SyncState::Allowed,
            b'R' => SyncState::Required,
            b'S' => SyncState::Started,
            b'A' => SyncState::Agreed,
            t => return Err(DecodeError::BadTag("ratchet sync state", t)),
        };
        let (keys, List(key_hashes), List(failed)) = <_>::decode(d)?;
        Ok(Self {
            state,
            keys,
            key_hashes,
            failed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::Version;
    use crate::ratchet::{PqSupport, new_resync_params};

    #[test]
    fn a_sync_record_with_keys_and_hashes_reads_back_the_same() {
        let (keys, _) = new_resync_params(Version(3), PqSupport(true)).unwrap();
        let s = RatchetSync {
            state: SyncState::Started,
            keys: Some(keys),
            key_hashes: vec![vec![1; 32], vec![2; 32]],
            failed: vec![vec![3; 32]],
        };
        assert_eq!(RatchetSync::from_bytes(&s.to_bytes().unwrap()).unwrap(), s);
    }

    #[test]
    fn processed_key_hashes_keep_the_newest_hundred() {
        let mut s = RatchetSync::default();
        for i in 0..=100u8 {
            s.remember_key(vec![i; 32]);
        }
        assert_eq!(s.key_hashes.len(), 100);
        assert!(!s.knows_key(&[0; 32]));
        assert!(s.knows_key(&[100; 32]));
    }

    #[test]
    fn sending_is_prohibited_while_required_started_or_agreed() {
        use crate::agent::client::ConnId;
        use crate::agent::records::ConnStatus;
        let mut rec = ConnRecord::new(ConnId(1), ConnStatus::Active, Version(8), false);
        for (state, ok) in [
            (SyncState::Ok, true),
            (SyncState::Allowed, true),
            (SyncState::Required, false),
            (SyncState::Started, false),
            (SyncState::Agreed, false),
        ] {
            rec.sync_mut().state = state;
            assert_eq!(rec.ensure_can_send().is_ok(), ok, "{state:?}");
        }
    }
}
