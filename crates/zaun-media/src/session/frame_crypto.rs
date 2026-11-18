//! The apps' frame encryption (`callCryptoFunction` in `call.ts`, ADR-0035 §4): each voice
//! frame is AES-256-GCM under the engine's key, its first byte (Opus' TOC) left plain, the
//! random 12-byte IV after the ciphertext and tag: `toc ‖ ciphertext ‖ tag ‖ iv`.
use aes_gcm::aead::{Aead, Generate, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use zaun_core::chat::media::CallKey;

/// Bytes left plain at the front of an audio frame (`initialPlainTextRequired`, no type).
const PLAIN: usize = 1;
const IV: usize = 12;

pub(super) struct FrameCrypto {
    cipher: Aes256Gcm,
}

impl FrameCrypto {
    pub fn new(key: &CallKey) -> Self {
        Self {
            cipher: Aes256Gcm::new(&Key::<Aes256Gcm>::from(key.0)),
        }
    }

    /// One encoded frame, sealed. `None` only if the cipher refuses (it does not for frames
    /// this size).
    pub fn seal(&self, frame: &[u8]) -> Option<Vec<u8>> {
        let n = PLAIN.min(frame.len());
        let (plain, rest) = frame.split_at(n);
        let iv = Nonce::generate();
        let sealed = if rest.is_empty() {
            Vec::new()
        } else {
            self.cipher.encrypt(&iv, rest).ok()?
        };
        let mut out = Vec::with_capacity(n + sealed.len() + IV);
        out.extend_from_slice(plain);
        out.extend_from_slice(&sealed);
        out.extend_from_slice(&iv);
        Some(out)
    }

    /// One of the friend's frames, opened; `None` for a frame that does not authenticate.
    pub fn open(&self, data: &[u8]) -> Option<Vec<u8>> {
        let end = data.len().checked_sub(IV)?;
        let n = PLAIN.min(end);
        let (plain, sealed) = data[..end].split_at(n);
        let iv = Nonce::try_from(&data[end..]).ok()?;
        let mut out = plain.to_vec();
        if !sealed.is_empty() {
            out.extend(self.cipher.decrypt(&iv, sealed).ok()?);
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crypto() -> FrameCrypto {
        FrameCrypto::new(&CallKey([3; 32]))
    }

    #[test]
    fn a_sealed_frame_keeps_its_first_byte_and_opens_to_the_same_frame() {
        let c = crypto();
        let frame = [0x78, 1, 2, 3, 4, 5];
        let sealed = c.seal(&frame).unwrap_or_default();
        assert_eq!(sealed.len(), 1 + 5 + 16 + 12, "toc, ciphertext, tag, iv");
        assert_eq!(sealed[0], 0x78);
        assert_eq!(c.open(&sealed).as_deref(), Some(&frame[..]));
    }

    #[test]
    fn a_frame_under_another_key_or_tampered_does_not_open() {
        let sealed = crypto().seal(&[0x78, 9, 9, 9]).unwrap_or_default();
        assert_eq!(FrameCrypto::new(&CallKey([4; 32])).open(&sealed), None);
        let mut bad = sealed.clone();
        bad[2] ^= 1;
        assert_eq!(crypto().open(&bad), None);
        assert_eq!(crypto().open(&[1, 2, 3]), None, "shorter than an iv");
    }

    #[test]
    fn a_one_byte_frame_carries_only_its_iv() {
        let c = crypto();
        let sealed = c.seal(&[0xfc]).unwrap_or_default();
        assert_eq!(sealed.len(), 1 + 12);
        assert_eq!(c.open(&sealed).as_deref(), Some(&[0xfc][..]));
    }
}
