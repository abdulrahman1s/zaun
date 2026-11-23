//! Command authorisation: an Ed25519/Ed448 signature, or the X25519 `cbAuthenticate` form (Client.hs `authTransmission`; nook smp/transmission.ts).
use crate::crypto::{Alg, CbNonce, PrivateKey, PublicKey, cb_authenticate};

use super::error::SmpError;
use super::transmission::authorized;

/// What authorising needs of the session: the session id and the server's signed X25519 key.
pub struct Authorizer<'a> {
    pub session_id: &'a [u8],
    pub server_key: &'a PublicKey,
}

impl Authorizer<'_> {
    /// The authenticator for one command, empty when `key` is `None` (PING, LGET, an unsecured SEND).
    /// X25519 keys need a 24-byte `corr_id`, which is the box nonce.
    pub fn authenticate(
        &self,
        key: Option<&PrivateKey>,
        corr_id: &[u8],
        entity_id: &[u8],
        command: &[u8],
    ) -> Result<Vec<u8>, SmpError> {
        let Some(key) = key else {
            return Ok(Vec::new());
        };
        let msg = authorized(self.session_id, corr_id, entity_id, command)?;
        match key.alg() {
            Alg::Ed25519 | Alg::Ed448 => Ok(key.sign(&msg)?.as_bytes().to_vec()),
            Alg::X25519 => {
                let nonce = CbNonce::from_slice(corr_id)?;
                Ok(cb_authenticate(self.server_key, key, &nonce, &msg)?.to_vec())
            }
            Alg::X448 => Err(SmpError::Unexpected("X448 authorisation key")),
        }
    }
}
