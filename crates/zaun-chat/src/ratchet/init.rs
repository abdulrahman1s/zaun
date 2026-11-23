//! Starting a ratchet from X3DH output (Ratchet.hs `initSndRatchet`, `initRcvRatchet`; Agent.hs `createRatchet_`).
use super::error::Result;
use super::generate::{PrivParams, generate_snd_params};
use super::kdf::root_kdf;
use super::kem::KemKeyPair;
use super::kem_params::reply_kem;
use super::kem_state::RatchetKem;
use super::key::Key32;
use super::params::{E2eParams, RcvParams};
use super::pq::{PqEncryption, PqSupport};
use super::skipped::SkippedKeys;
use super::state::{Ratchet, SndRatchet};
use super::version::RatchetVersions;
use super::x3dh::{InitParams, pq_x3dh_snd};
use crate::crypto::{Alg, KeyPair, PrivateKey, PublicKey};

impl Ratchet {
    /// The joining side, which sends first. `dhrs` is a fresh X448 key; `dhr` the inviter's second key.
    pub fn init_snd(
        versions: RatchetVersions,
        dhr: PublicKey,
        dhrs: PrivateKey,
        (init, kem_keys): (InitParams, Option<KemKeyPair>),
    ) -> Result<Self> {
        let shared = init.kem_accepted.as_ref().map(|a| &a.shared);
        let (rk, cks, nhks) = root_kdf(&init.ratchet_key, &dhr, &dhrs, shared)?;
        let pq_on = kem_keys.is_some();
        Ok(Self {
            versions,
            ad: init.assoc_data,
            vc_pq: Some(Key32::new(init.verify_code_pq)),
            dhrs,
            snd_kem: PqEncryption(init.kem_accepted.is_some()),
            kem: kem_keys.map(|keys| RatchetKem {
                keys,
                accepted: init.kem_accepted,
            }),
            support_kem: PqSupport(pq_on),
            enable_kem: PqEncryption(pq_on),
            rcv_kem: PqEncryption::OFF,
            rk,
            snd: Some(SndRatchet {
                dhr,
                cks,
                hks: init.snd_hk,
            }),
            rcv: None,
            ns: 0,
            nr: 0,
            pn: 0,
            nhks,
            nhkr: init.rcv_next_hk,
            skipped: SkippedKeys::default(),
        })
    }

    /// The inviting side, which receives first. `dhrs` is the private key of `k2` from its params.
    #[must_use]
    pub fn init_rcv(
        versions: RatchetVersions,
        dhrs: PrivateKey,
        (init, kem_keys): (InitParams, Option<KemKeyPair>),
        pq: PqSupport,
    ) -> Self {
        Self {
            versions,
            ad: init.assoc_data,
            vc_pq: Some(Key32::new(init.verify_code_pq)),
            dhrs,
            kem: kem_keys.map(|keys| RatchetKem {
                keys,
                accepted: init.kem_accepted,
            }),
            support_kem: pq,
            enable_kem: pq.to_enc(),
            snd_kem: PqEncryption::OFF,
            rcv_kem: PqEncryption::OFF,
            rk: init.ratchet_key,
            snd: None,
            rcv: None,
            ns: 0,
            nr: 0,
            pn: 0,
            nhks: init.rcv_next_hk,
            nhkr: init.snd_hk,
            skipped: SkippedKeys::default(),
        }
    }

    /// Joining from the inviter's params (`createRatchet_`): the ratchet to send with, and the reply params to send.
    pub fn join(
        versions: RatchetVersions,
        rcv: &RcvParams,
        pq: PqSupport,
    ) -> Result<(Self, E2eParams)> {
        let params = rcv.params();
        let use_kem = reply_kem(rcv.kem_key(), pq);
        let (private, reply): (PrivParams, E2eParams) =
            generate_snd_params(params.version, use_kem.as_ref())?;
        let dhrs = KeyPair::generate(Alg::X448)?.private;
        let init = pq_x3dh_snd(&private, rcv)?;
        let rc = Self::init_snd(versions, params.k2.clone(), dhrs, init)?;
        Ok((rc, reply))
    }
}
