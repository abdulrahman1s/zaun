//! Message keys of skipped messages, found again by header key and number (Ratchet.hs `SkippedMsgKeys`, `skipMessageKeys`, `maxSkip`; Agent.hs loads the newest `maxSkippedMsgKeys`).
use std::collections::VecDeque;

use super::error::{RatchetError, Result};
use super::kdf::{MessageKey, chain_kdf};
use super::key::Key32;
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError};

/// The most messages one jump of the sender may skip.
pub const MAX_SKIP: u32 = 512;
/// Keys kept; when more arrive the oldest are dropped.
pub const MAX_SKIPPED_MSG_KEYS: usize = 2000;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SkippedKey {
    pub hk: Key32,
    pub n: u32,
    pub key: MessageKey,
}

/// Oldest first, so eviction takes from the front.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct SkippedKeys(VecDeque<SkippedKey>);

impl SkippedKeys {
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Each header key once, in order of first appearance.
    pub(crate) fn header_keys(&self) -> Vec<Key32> {
        let mut keys: Vec<Key32> = Vec::new();
        for k in &self.0 {
            if !keys.contains(&k.hk) {
                keys.push(k.hk.clone());
            }
        }
        keys
    }

    /// Removes and returns the key: it is used once, whether or not the message then authenticates.
    pub(crate) fn take(&mut self, hk: &Key32, n: u32) -> Option<MessageKey> {
        let i = self.0.iter().position(|k| k.n == n && k.hk == *hk)?;
        self.0.remove(i).map(|k| k.key)
    }

    pub(crate) fn add(&mut self, keys: impl IntoIterator<Item = SkippedKey>) {
        self.0.extend(keys);
        let extra = self.0.len().saturating_sub(MAX_SKIPPED_MSG_KEYS);
        self.0.drain(..extra);
    }
}

/// `skipMessageKeys` on one receiving chain: the keys of messages before `until`, and the new chain key and counter.
pub(crate) fn advance_chain(
    ckr: &Key32,
    hkr: &Key32,
    nr: u32,
    until: u32,
) -> Result<(Key32, u32, Vec<SkippedKey>)> {
    let (nr64, until64) = (u64::from(nr), u64::from(until));
    let too_far = |n: u64| u32::try_from(n).unwrap_or(u32::MAX);
    if nr64 > until64 + 1 {
        return Err(RatchetError::EarlierMessage(too_far(nr64 - until64 - 1)));
    }
    if nr64 == until64 + 1 {
        return Err(RatchetError::DuplicateMessage);
    }
    if nr64 + u64::from(MAX_SKIP) < until64 {
        return Err(RatchetError::TooManySkipped(too_far(until64 + 1 - nr64)));
    }
    let (mut ck, mut skipped) = (ckr.clone(), Vec::new());
    for n in nr..until {
        let step = chain_kdf(&ck)?;
        skipped.push(SkippedKey {
            hk: hkr.clone(),
            n,
            key: step.message,
        });
        ck = step.next;
    }
    Ok((ck, until, skipped))
}

impl Encode for SkippedKeys {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        let count = u16::try_from(self.0.len()).map_err(|_| EncodeError::TooLong {
            what: "skipped keys",
            len: self.0.len(),
            max: usize::from(u16::MAX),
        })?;
        count.encode(out)?;
        for k in &self.0 {
            (&k.hk, k.n, &k.key.key).encode(out)?;
            out.extend_from_slice(&k.key.iv);
        }
        Ok(())
    }
}

impl Decode for SkippedKeys {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let count = usize::from(u16::decode(d)?);
        if count > MAX_SKIPPED_MSG_KEYS {
            return Err(DecodeError::Invalid("too many skipped keys"));
        }
        let mut keys = VecDeque::with_capacity(count);
        for _ in 0..count {
            let (hk, n, key) = <(Key32, u32, Key32)>::decode(d)?;
            let iv = d.take_array()?;
            keys.push_back(SkippedKey {
                hk,
                n,
                key: MessageKey { key, iv },
            });
        }
        Ok(Self(keys))
    }
}
