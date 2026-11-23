//! TLS 1.3 key schedule for SHA-256 suites (RFC 8446 section 7; nook sx/tls/keySchedule.ts).
use hkdf::Hkdf;
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

pub(super) type Secret = [u8; 32];

/// SHA-256 of all handshake messages so far.
#[derive(Clone)]
pub(super) struct Transcript(Sha256);

impl Transcript {
    pub(super) fn new() -> Self {
        Self(Sha256::new())
    }

    pub(super) fn add(&mut self, message: &[u8]) {
        self.0.update(message);
    }

    pub(super) fn hash(&self) -> [u8; 32] {
        self.0.clone().finalize().into()
    }
}

#[allow(
    clippy::expect_used,
    reason = "a 32-byte PRK and a 32 or 12-byte output cannot fail"
)]
pub(super) fn expand_label<const N: usize>(secret: &[u8], label: &str, context: &[u8]) -> [u8; N] {
    let full = [b"tls13 ", label.as_bytes()].concat();
    let mut info = (N as u16).to_be_bytes().to_vec();
    info.push(full.len() as u8);
    info.extend_from_slice(&full);
    info.push(context.len() as u8);
    info.extend_from_slice(context);
    let mut out = [0u8; N];
    Hkdf::<Sha256>::from_prk(secret)
        .expect("PRK is 32 bytes")
        .expand(&info, &mut out)
        .expect("output within HKDF limits");
    out
}

pub(super) fn derive_secret(secret: &Secret, label: &str, transcript: &[u8; 32]) -> Secret {
    expand_label(secret, label, transcript)
}

fn extract(salt: &[u8], ikm: &[u8]) -> Secret {
    let (prk, _) = Hkdf::<Sha256>::extract(Some(salt), ikm);
    prk.into()
}

pub(super) struct Secrets {
    pub(super) handshake: Secret,
    pub(super) master: Secret,
}

/// Early secret (no PSK) to handshake secret and master secret.
pub(super) fn handshake_secrets(shared: &[u8]) -> Secrets {
    let zeros = [0u8; 32];
    let empty: [u8; 32] = Sha256::digest([]).into();
    let early = extract(&zeros, &zeros);
    let handshake = extract(&derive_secret(&early, "derived", &empty), shared);
    let master = extract(&derive_secret(&handshake, "derived", &empty), &zeros);
    Secrets { handshake, master }
}

#[allow(clippy::expect_used, reason = "HMAC accepts keys of any length")]
pub(super) fn finished_verify_data(base: &Secret, transcript: &[u8; 32]) -> [u8; 32] {
    let key: [u8; 32] = expand_label(base, "finished", &[]);
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(&key).expect("any key length");
    mac.update(transcript);
    mac.finalize().into_bytes().into()
}

/// KeyUpdate: the next application traffic secret of one direction.
pub(super) fn next_traffic_secret(secret: &Secret) -> Secret {
    expand_label(secret, "traffic upd", &[])
}

/// What a CertificateVerify signature covers (RFC 8446 section 4.4.3).
pub(super) fn certificate_verify_input(transcript: &[u8; 32], server: bool) -> Vec<u8> {
    let label: &[u8] = if server {
        b"TLS 1.3, server CertificateVerify"
    } else {
        b"TLS 1.3, client CertificateVerify"
    };
    [&[0x20u8; 64][..], label, &[0], transcript].concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len() / 2)
            .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn rfc8448_simple_handshake_secrets_match() {
        // RFC 8448 section 3 uses SHA-256 and the same schedule as the ChaCha suite.
        let shared = hex("8bd4054fb55b9d63fdfbacf9f04b9f0d35e6d63f537563efd46272900f89492d");
        let s = handshake_secrets(&shared);
        assert_eq!(
            s.handshake.to_vec(),
            hex("1dc826e93606aa6fdc0aadc12f741b01046aa6b99f691ed221a9f0ca043fbeac")
        );
        assert_eq!(
            s.master.to_vec(),
            hex("18df06843d13a08bf2a449844c5f8a478001bc4d4c627984d5a41da8d0402919")
        );
    }

    #[test]
    fn traffic_keys_follow_the_label_lengths() {
        let secret = [7u8; 32];
        let key: [u8; 32] = expand_label(&secret, "key", &[]);
        let iv: [u8; 12] = expand_label(&secret, "iv", &[]);
        assert_ne!(key[..12], iv);
        assert_ne!(next_traffic_secret(&secret), secret);
    }
}
