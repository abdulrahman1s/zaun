//! Which keys meet the peer's `AgentRatchetKey`: ours from the re-sync we started, fresh ones sent back, or none (Agent.hs 7.1.4 `newRatchetKey.getSendRatchetKeys`).
use crate::encoding::Version;
use crate::ratchet::{PqSupport, PrivParams, RcvParams, new_resync_params, ratchet_key_hash};

use super::state::SyncState;
use crate::agent::error::{AgentError, Result};
use crate::agent::records::ConnRecord;

pub(crate) enum Keys {
    /// Build the new ratchet from `own`; `reply` is the `AgentRatchetKey` to send back.
    Use {
        own: Box<PrivParams>,
        reply: Option<RcvParams>,
    },
    /// Only remember the peer's key: it answers a key that is not ours.
    Skip,
    /// An answer after our ratchet was agreed: the ratchets cannot meet (`RATCHET_SYNC`).
    Refuse,
}

fn own_hash(own: &PrivParams) -> Result<Vec<u8>> {
    Ok(ratchet_key_hash(&own.pk1.public()?, &own.pk2.public()?).to_vec())
}

/// `version` is the peer's e2e version, which our reply uses too.
pub(crate) fn choose_keys(
    rec: &ConnRecord,
    version: Version,
    answered: Option<&[u8]>,
) -> Result<Keys> {
    let stored = || {
        rec.sync
            .as_ref()
            .and_then(|s| s.keys.clone())
            .ok_or(AgentError::Prohibited(
                "re-sync started without stored keys",
            ))
    };
    Ok(match (rec.sync_state(), answered) {
        (SyncState::Started, None) => Keys::Use {
            own: Box::new(stored()?),
            reply: None,
        },
        (SyncState::Started, Some(h)) => {
            let own = stored()?;
            if own_hash(&own)? == h {
                Keys::Use {
                    own: Box::new(own),
                    reply: None,
                }
            } else {
                Keys::Skip
            }
        }
        (SyncState::Ok | SyncState::Allowed | SyncState::Required | SyncState::Agreed, None) => {
            let (own, params) = new_resync_params(version, PqSupport(rec.pq))?;
            Keys::Use {
                own: Box::new(own),
                reply: Some(params),
            }
        }
        (SyncState::Agreed, Some(_)) => Keys::Refuse,
        (SyncState::Ok | SyncState::Allowed | SyncState::Required, Some(_)) => Keys::Skip,
    })
}
