//! The operating system's random source (Crypto.hs `randomBytes`, nook `randomBytes`).
use super::CryptoError;

pub fn fill_random(buf: &mut [u8]) -> Result<(), CryptoError> {
    getrandom::fill(buf).map_err(|_| CryptoError::Random)
}

pub fn random_array<const N: usize>() -> Result<[u8; N], CryptoError> {
    let mut out = [0u8; N];
    fill_random(&mut out)?;
    Ok(out)
}

pub fn random_vec(n: usize) -> Result<Vec<u8>, CryptoError> {
    let mut out = vec![0u8; n];
    fill_random(&mut out)?;
    Ok(out)
}
