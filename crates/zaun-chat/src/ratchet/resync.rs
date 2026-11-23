//! Ratchet-level pieces of the re-sync flow (EREADY, `AgentRatchetKey`): the key hash, who becomes the receiving side, and the new ratchet (Agent.hs `newRatchetKey`, `synchronizeRatchet'`).
use super::error::Result;
use super::generate::{PrivParams, generate_rcv_params};
use super::params::RcvParams;
use super::pq::PqSupport;
use super::state::Ratchet;
use super::version::RatchetVersions;
use super::x3dh::{pq_x3dh_rcv, pq_x3dh_snd};
use crate::crypto::{Alg, KeyPair, PublicKey, sha256};
use crate::encoding::Version;

/// `rkHash`: SHA-256 of the raw `k1` and `k2`. Duplicate `AgentRatchetKey` messages are told apart by it.
#[must_use]
pub fn ratchet_key_hash(k1: &PublicKey, k2: &PublicKey) -> [u8; 32] {
    sha256(&[k1.as_bytes(), k2.as_bytes()].concat())
}

/// New params for an `AgentRatchetKey` we start or answer: the same as for a new connection.
pub fn new_resync_params(version: Version, pq: PqSupport) -> Result<(PrivParams, RcvParams)> {
    generate_rcv_params(version, pq)
}

/// The ratchet after both sides have exchanged params, and whether we must send `EREADY`.
pub struct Resynced {
    pub ratchet: Ratchet,
    /// True on the sending side, which then enqueues `EREADY lastExternalSndId`.
    pub send_eready: bool,
}

impl Ratchet {
    /// Builds the new ratchet from our private params and the peer's `AgentRatchetKey` params. The side with the smaller key hash (equal counts) receives first.
    pub fn resync(
        versions: RatchetVersions,
        own: &PrivParams,
        peer: &RcvParams,
        pq: PqSupport,
    ) -> Result<Resynced> {
        let own_params = own.rcv_params(versions.current)?;
        let (own_p, peer_p) = (own_params.params(), peer.params());
        let own_hash = ratchet_key_hash(&own_p.k1, &own_p.k2);
        let peer_hash = ratchet_key_hash(&peer_p.k1, &peer_p.k2);
        if own_hash <= peer_hash {
            let init = pq_x3dh_rcv(own, peer.params())?;
            let ratchet = Self::init_rcv(versions, own.pk2.clone(), init, pq);
            return Ok(Resynced {
                ratchet,
                send_eready: false,
            });
        }
        let dhrs = KeyPair::generate(Alg::X448)?.private;
        let init = pq_x3dh_snd(own, peer)?;
        let ratchet = Self::init_snd(versions, peer.params().k2.clone(), dhrs, init)?;
        Ok(Resynced {
            ratchet,
            send_eready: true,
        })
    }
}
