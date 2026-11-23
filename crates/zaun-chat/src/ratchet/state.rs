//! The double ratchet state of one connection (Ratchet.hs `Ratchet`, `SndRatchet`, `RcvRatchet`).
use super::kem_state::RatchetKem;
use super::key::Key32;
use super::pq::{PqEncryption, PqSupport};
use super::skipped::SkippedKeys;
use super::version::RatchetVersions;
use crate::crypto::{PrivateKey, PublicKey};

#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct SndRatchet {
    /// The peer's latest ratchet key (`rcDHRr`).
    pub dhr: PublicKey,
    pub cks: Key32,
    pub hks: Key32,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct RcvRatchet {
    pub ckr: Key32,
    pub hkr: Key32,
}

/// Persist it after every `encrypt` and `decrypt`, including the ones that fail: some failures still advance it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ratchet {
    pub(crate) versions: RatchetVersions,
    /// Must be equal on both sides.
    pub(crate) ad: Vec<u8>,
    /// Absent in states made before it was added.
    pub(crate) vc_pq: Option<Key32>,
    pub(crate) dhrs: PrivateKey,
    pub(crate) kem: Option<RatchetKem>,
    /// Sets the header size; can only be turned on.
    pub(crate) support_kem: PqSupport,
    /// Turns the KEM on at the next ratchet step.
    pub(crate) enable_kem: PqEncryption,
    pub(crate) snd_kem: PqEncryption,
    pub(crate) rcv_kem: PqEncryption,
    pub(crate) rk: Key32,
    pub(crate) snd: Option<SndRatchet>,
    pub(crate) rcv: Option<RcvRatchet>,
    pub(crate) ns: u32,
    pub(crate) nr: u32,
    pub(crate) pn: u32,
    pub(crate) nhks: Key32,
    pub(crate) nhkr: Key32,
    /// At most `MAX_SKIPPED_MSG_KEYS`, oldest dropped first.
    pub(crate) skipped: SkippedKeys,
}

impl Ratchet {
    /// The associated data both sides share: our and their first X448 keys, raw. The security code is its SHA-256.
    #[must_use]
    pub fn associated_data(&self) -> &[u8] {
        &self.ad
    }

    /// `rcVCPQ`, covering all handshake keys and the KEM secret.
    #[must_use]
    pub fn verify_code_pq(&self) -> Option<&[u8; 32]> {
        self.vc_pq.as_ref().map(Key32::as_bytes)
    }

    #[must_use]
    pub fn versions(&self) -> RatchetVersions {
        self.versions
    }

    /// Header size follows it, so it also tells the agent which padded lengths to use.
    #[must_use]
    pub fn pq_support(&self) -> PqSupport {
        self.support_kem
    }

    #[must_use]
    pub fn pq_enabled(&self) -> PqEncryption {
        self.enable_kem
    }

    /// Whether the last ratchet step we sent used a KEM secret.
    #[must_use]
    pub fn pq_sending(&self) -> PqEncryption {
        self.snd_kem
    }

    #[must_use]
    pub fn pq_receiving(&self) -> PqEncryption {
        self.rcv_kem
    }

    /// False on the party that has not sent yet; it answers `EREADY` in that case.
    #[must_use]
    pub fn has_sending_chain(&self) -> bool {
        self.snd.is_some()
    }

    #[must_use]
    pub fn skipped_len(&self) -> usize {
        self.skipped.len()
    }
}
