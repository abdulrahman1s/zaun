//! The pinned server identity: CA fingerprint and chain signatures (simplexmq Transport/Shared.hs `chainIdCaCerts`, `x509validate`).
use thiserror::Error;

use super::der::CertError;
use super::verify::certificate_signed_by;
use crate::crypto::key_hash;

/// The roles of a server chain, as in simplexmq: leaf and identity (CA) certificate first, then
/// optionally an operator certificate and a network certificate.
#[derive(Debug, PartialEq, Eq)]
pub enum ChainCertificates<'a> {
    Empty,
    SelfSigned(&'a [u8]),
    Valid { leaf: &'a [u8], id: &'a [u8] },
    Long,
}

#[must_use]
pub fn chain_id_ca_certs<C: AsRef<[u8]>>(chain: &[C]) -> ChainCertificates<'_> {
    match chain {
        [] => ChainCertificates::Empty,
        [cert] => ChainCertificates::SelfSigned(cert.as_ref()),
        [leaf, id] | [leaf, id, _] | [leaf, id, _, _] => ChainCertificates::Valid {
            leaf: leaf.as_ref(),
            id: id.as_ref(),
        },
        _ => ChainCertificates::Long,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IdentityError {
    #[error("bad certificate chain: {0} certificates")]
    ChainShape(usize),
    #[error("the CA fingerprint does not match the server address")]
    Fingerprint,
    #[error("certificate {0} is not signed by the next one")]
    Signature(usize),
    #[error("the top certificate of a short chain is not self-signed")]
    NotSelfSigned,
    #[error(transparent)]
    Certificate(#[from] CertError),
}

/// Checks a server chain against the key hash from its address: the identity certificate must
/// hash to `key_hash`, every certificate must be signed by the next, and the top of a chain of
/// two must sign itself. Validity dates are not checked.
pub fn verify_server_chain<C: AsRef<[u8]>>(
    chain: &[C],
    expected_key_hash: &[u8],
) -> Result<(), IdentityError> {
    let ChainCertificates::Valid { id, .. } = chain_id_ca_certs(chain) else {
        return Err(IdentityError::ChainShape(chain.len()));
    };
    if key_hash(id).as_slice() != expected_key_hash {
        return Err(IdentityError::Fingerprint);
    }
    for (i, pair) in chain.windows(2).enumerate() {
        if !certificate_signed_by(pair[0].as_ref(), pair[1].as_ref()) {
            return Err(IdentityError::Signature(i));
        }
    }
    let top = chain[chain.len() - 1].as_ref();
    if chain.len() == 2 && !certificate_signed_by(top, top) {
        return Err(IdentityError::NotSelfSigned);
    }
    Ok(())
}
