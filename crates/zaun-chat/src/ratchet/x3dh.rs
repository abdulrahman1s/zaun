//! Post-quantum X3DH: the root key, header keys, associated data and verify code both parties agree on (Ratchet.hs `pqX3dhSnd`, `pqX3dhRcv`, `pqX3dh`).
use zeroize::Zeroizing;

use super::error::{RatchetError, Result};
use super::generate::{PrivKem, PrivParams};
use super::kdf::hkdf3;
use super::kem::{KemKeyPair, kem_decapsulate};
use super::kem_params::KemParams;
use super::kem_state::KemAccepted;
use super::key::Key32;
use super::params::{E2eParams, RcvParams};
use crate::crypto::{DhSecret, PublicKey, dh, hkdf_sha512};

/// `RatchetInitParams`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InitParams {
    /// Both parties' first keys, raw: the same 112 bytes on each side. The security code hashes it.
    pub assoc_data: Vec<u8>,
    /// Covers every handshake key and the KEM secret.
    pub verify_code_pq: [u8; 32],
    pub ratchet_key: Key32,
    pub snd_hk: Key32,
    pub rcv_next_hk: Key32,
    pub kem_accepted: Option<KemAccepted>,
}

fn pq_x3dh(
    sk1: &PublicKey,
    rk1: &PublicKey,
    dhs: [DhSecret; 3],
    kem_accepted: Option<KemAccepted>,
) -> Result<InitParams> {
    let mut ikm = Zeroizing::new(Vec::new());
    for s in &dhs {
        ikm.extend_from_slice(s.as_bytes());
    }
    if let Some(a) = &kem_accepted {
        ikm.extend_from_slice(a.shared.as_ref());
    }
    let salt = [0u8; 64];
    let [hk, nhk, sk] = hkdf3(&salt, &ikm, b"SimpleXX3DH")?;
    let vc = hkdf_sha512::<32>(&salt, &ikm, b"SimpleXVerifyCode")?;
    Ok(InitParams {
        assoc_data: [sk1.as_bytes(), rk1.as_bytes()].concat(),
        verify_code_pq: *vc,
        ratchet_key: Key32::new(sk),
        snd_hk: Key32::new(hk),
        rcv_next_hk: Key32::new(nhk),
        kem_accepted,
    })
}

/// Used by the peer joining the connection, with the private params it made for its reply.
pub fn pq_x3dh_snd(own: &PrivParams, rcv: &RcvParams) -> Result<(InitParams, Option<KemKeyPair>)> {
    let rcv = rcv.params();
    let (keys, accepted) = match (&own.kem, &rcv.kem) {
        (Some(PrivKem::Accepted { ct, shared, keys }), Some(KemParams::Proposed(k))) => {
            let accepted = KemAccepted {
                peer_key: k.clone(),
                shared: shared.clone(),
                ct: ct.clone(),
            };
            (Some(keys.clone()), Some(accepted))
        }
        // Both parties can send a proposal when the ratchet is renegotiated.
        (Some(PrivKem::Proposed(keys)), _) => (Some(keys.clone()), None),
        (Some(PrivKem::Accepted { .. }), _) => return Err(RatchetError::KemState),
        (None, _) => (None, None),
    };
    let dhs = [
        dh(&rcv.k1, &own.pk2)?,
        dh(&rcv.k2, &own.pk1)?,
        dh(&rcv.k2, &own.pk2)?,
    ];
    let init = pq_x3dh(&own.pk1.public()?, &rcv.k1, dhs, accepted)?;
    Ok((init, keys))
}

/// Used by the peer that created the connection, with the params from the reply.
pub fn pq_x3dh_rcv(own: &PrivParams, snd: &E2eParams) -> Result<(InitParams, Option<KemKeyPair>)> {
    let kem = match &snd.kem {
        Some(KemParams::Accepted { ct, key }) => match &own.kem {
            Some(PrivKem::Proposed(keys)) => {
                let accepted = KemAccepted {
                    peer_key: key.clone(),
                    shared: kem_decapsulate(ct, &keys.secret),
                    ct: ct.clone(),
                };
                Some((keys.clone(), accepted))
            }
            Some(PrivKem::Accepted { .. }) | None => return Err(RatchetError::KemState),
        },
        Some(KemParams::Proposed(_)) | None => None,
    };
    let dhs = [
        dh(&snd.k2, &own.pk1)?,
        dh(&snd.k1, &own.pk2)?,
        dh(&snd.k2, &own.pk2)?,
    ];
    let (keys, accepted) = kem.unzip();
    let init = pq_x3dh(&snd.k1, &own.pk1.public()?, dhs, accepted)?;
    Ok((init, keys))
}
