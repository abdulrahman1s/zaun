//! HKDF-SHA512 as `Crypto.hs hkdf salt ikm info n` (nook bytes.ts `hkdfSha512`).
use hkdf::Hkdf;
use sha2::Sha512;
use zeroize::Zeroizing;

use super::CryptoError;

/// Derives exactly `N` bytes (at most 255 * 64).
pub fn hkdf_sha512<const N: usize>(
    salt: &[u8],
    ikm: &[u8],
    info: &[u8],
) -> Result<Zeroizing<[u8; N]>, CryptoError> {
    let mut out = Zeroizing::new([0u8; N]);
    Hkdf::<Sha512>::new(Some(salt), ikm)
        .expand(info, &mut *out)
        .map_err(|_| CryptoError::BadKey("hkdf output length"))?;
    Ok(out)
}
