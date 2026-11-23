//! The DH ratchet step on a message with a new ratchet key, with the KEM half (Ratchet.hs `ratchetStep`, `pqRatchetStep`).
use super::error::{RatchetError, Result};
use super::header::MsgHeader;
use super::kdf::root_kdf;
use super::kem::{KemSharedSecret, kem_decapsulate, kem_encapsulate, kem_keypair};
use super::kem_params::KemParams;
use super::kem_state::{KemAccepted, RatchetKem};
use super::pq::PqEncryption;
use super::state::{Ratchet, RcvRatchet, SndRatchet};
use crate::crypto::{Alg, KeyPair};

struct PqStep {
    /// Secret from the peer's ciphertext, for the receiving chain.
    rcv: Option<KemSharedSecret>,
    /// Secret from our new ciphertext, for the sending chain.
    snd: Option<KemSharedSecret>,
    kem: Option<RatchetKem>,
}

impl Ratchet {
    /// Performs the step for `hdr`: new keys on both chains, and the previous header keys move up.
    pub(super) fn ratchet_step(&mut self, hdr: &MsgHeader) -> Result<()> {
        let pq = self.pq_step(hdr.kem.as_ref())?;
        let new_dhrs = KeyPair::generate(Alg::X448)?.private;
        let (rk, ckr, nhkr) = root_kdf(&self.rk, &hdr.dhrs, &self.dhrs, pq.rcv.as_ref())?;
        let (rk, cks, nhks) = root_kdf(&rk, &hdr.dhrs, &new_dhrs, pq.snd.as_ref())?;
        let enable = PqEncryption(pq.snd.is_some() || pq.rcv.is_some() || pq.kem.is_some());
        self.support_kem = self.support_kem.enable_with(enable);
        self.enable_kem = enable;
        self.snd_kem = PqEncryption(pq.snd.is_some());
        self.rcv_kem = PqEncryption(pq.rcv.is_some());
        self.kem = pq.kem;
        self.dhrs = new_dhrs;
        self.rk = rk;
        self.snd = Some(SndRatchet {
            dhr: hdr.dhrs.clone(),
            cks,
            hks: std::mem::replace(&mut self.nhks, nhks),
        });
        self.rcv = Some(RcvRatchet {
            ckr,
            hkr: std::mem::replace(&mut self.nhkr, nhkr),
        });
        self.pn = self.ns;
        self.ns = 0;
        self.nr = 0;
        Ok(())
    }

    fn pq_step(&self, header_kem: Option<&KemParams>) -> Result<PqStep> {
        let enabled = self.enable_kem.0;
        let Some(params) = header_kem else {
            // No KEM in the header, but we enabled it when sending: start proposing.
            let kem = match (&self.kem, enabled) {
                (None, true) => Some(RatchetKem {
                    keys: kem_keypair()?,
                    accepted: None,
                }),
                _ => None,
            };
            return Ok(PqStep {
                rcv: None,
                snd: None,
                kem,
            });
        };
        let (rcv, peer_key) = match params {
            KemParams::Proposed(key) => (None, key),
            KemParams::Accepted { ct, key } => {
                let own = self.kem.as_ref().ok_or(RatchetError::KemState)?;
                (Some(kem_decapsulate(ct, &own.keys.secret)), key)
            }
        };
        if !enabled {
            return Ok(PqStep {
                rcv,
                snd: None,
                kem: None,
            });
        }
        let (ct, shared) = kem_encapsulate(peer_key)?;
        let kem = RatchetKem {
            keys: kem_keypair()?,
            accepted: Some(KemAccepted {
                peer_key: peer_key.clone(),
                shared: shared.clone(),
                ct,
            }),
        };
        Ok(PqStep {
            rcv,
            snd: Some(shared),
            kem: Some(kem),
        })
    }
}
