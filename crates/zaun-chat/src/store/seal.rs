//! Value sealing: XChaCha20-Poly1305 over bucket-padded plaintext, HMAC lookup keys.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::{Sha256, Sha512};
use zeroize::Zeroizing;

use super::StoreError;

const INFO: &[u8] = b"zaun-chat store v1";
const NONCE: usize = 24;
const MIN_BUCKET: usize = 64;

/// The two keys derived from the store key: one seals values, one hashes lookup keys.
pub(super) struct Sealer {
    cipher: XChaCha20Poly1305,
    mac: Zeroizing<[u8; 32]>,
}

impl Sealer {
    pub(super) fn new(key: &str) -> Sealer {
        let mut okm = Zeroizing::new([0u8; 64]);
        // 64 bytes is far below HKDF-SHA512's limit, so this cannot fail.
        let _ = Hkdf::<Sha512>::new(None, key.as_bytes()).expand(INFO, &mut *okm);
        let mut enc = Zeroizing::new([0u8; 32]);
        let mut mac = Zeroizing::new([0u8; 32]);
        enc.copy_from_slice(&okm[..32]);
        mac.copy_from_slice(&okm[32..]);
        Sealer {
            cipher: XChaCha20Poly1305::new(&Key::from(*enc)),
            mac,
        }
    }

    /// `HMAC-SHA256(mac key, domain || 0 || data)`: a stable pseudonym for `data`.
    pub(super) fn lookup(&self, domain: &str, data: &[u8]) -> [u8; 32] {
        let mut m = <Hmac<Sha256> as KeyInit>::new_from_slice(&*self.mac)
            .unwrap_or_else(|_| unreachable!("HMAC takes any key length"));
        m.update(domain.as_bytes());
        m.update(&[0]);
        m.update(data);
        m.finalize().into_bytes().into()
    }

    /// `nonce || ciphertext`. The AAD binds the value to its table and row.
    pub(super) fn seal(&self, aad: &[&[u8]], plain: &[u8]) -> Result<Vec<u8>, StoreError> {
        let mut nonce = [0u8; NONCE];
        getrandom::fill(&mut nonce)
            .map_err(|e| StoreError::Io(std::io::Error::other(e.to_string())))?;
        let padded = pad(plain)?;
        let ct = self
            .cipher
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: &padded,
                    aad: &frame(aad),
                },
            )
            .map_err(|_| StoreError::Invalid("value cannot be sealed"))?;
        Ok([nonce.as_slice(), &ct].concat())
    }

    pub(super) fn open(&self, aad: &[&[u8]], sealed: &[u8]) -> Result<Vec<u8>, StoreError> {
        let (nonce, ct) = sealed
            .split_at_checked(NONCE)
            .ok_or_else(|| StoreError::Corrupt("sealed value too short".into()))?;
        let nonce = XNonce::try_from(nonce).map_err(|_| StoreError::Corrupt("bad nonce".into()))?;
        let padded = Zeroizing::new(
            self.cipher
                .decrypt(
                    &nonce,
                    Payload {
                        msg: ct,
                        aad: &frame(aad),
                    },
                )
                .map_err(|_| StoreError::Corrupt("sealed value does not open".into()))?,
        );
        unpad(&padded)
    }
}

/// Length-prefixed parts, so `("a", "bc")` and `("ab", "c")` differ.
fn frame(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for p in parts {
        out.extend_from_slice(&(p.len() as u32).to_le_bytes());
        out.extend_from_slice(p);
    }
    out
}

/// `len (u32 LE) || data || zeros`, up to the next power of two, at least 64 bytes.
fn pad(plain: &[u8]) -> Result<Zeroizing<Vec<u8>>, StoreError> {
    let len = u32::try_from(plain.len()).map_err(|_| StoreError::Invalid("value over 4 GiB"))?;
    let total = (plain.len() + 4).next_power_of_two().max(MIN_BUCKET);
    let mut out = Zeroizing::new(Vec::with_capacity(total));
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(plain);
    out.resize(total, 0);
    Ok(out)
}

fn unpad(padded: &[u8]) -> Result<Vec<u8>, StoreError> {
    let bad = || StoreError::Corrupt("bad padding".into());
    let (len, rest) = padded.split_first_chunk::<4>().ok_or_else(bad)?;
    let len = u32::from_le_bytes(*len) as usize;
    rest.get(..len).map(<[u8]>::to_vec).ok_or_else(bad)
}
