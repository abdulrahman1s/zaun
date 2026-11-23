//! Link ID and secretbox key from the link key (Crypto/ShortLink.hs `contactShortLinkKdf`, `invShortLinkKdf`, AGPL-3.0-only).
use crate::crypto::{SbKey, hkdf_sha512};

use crate::agent::error::Result;

/// A contact link's queue ID (24 bytes) and the key that encrypts its data.
pub fn contact_short_link_kdf(link_key: &[u8]) -> Result<(Vec<u8>, SbKey)> {
    let out = hkdf_sha512::<56>(&[], link_key, b"SimpleXContactLink")?;
    let (link_id, key) = out.split_at(24);
    Ok((link_id.to_vec(), SbKey::from_slice(key)?))
}

/// An invitation link carries its ID in the link, so only the key is derived.
pub fn inv_short_link_kdf(link_key: &[u8]) -> Result<SbKey> {
    let out = hkdf_sha512::<32>(&[], link_key, b"SimpleXInvLink")?;
    Ok(SbKey::new(*out))
}
