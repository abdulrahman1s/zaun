//! The ClientHello: TLS_CHACHA20_POLY1305_SHA256, x25519, Ed448 and Ed25519 signatures, no SNI (nook tls13.ts).
use super::super::key_schedule::Transcript;
use super::super::record::HANDSHAKE;
use super::super::verify::{SIG_ED448, SIG_ED25519};
use super::super::wire::{extension, handshake_message, put_u16};
use crate::crypto::{Alg, CryptoError, KeyPair, fill_random};

pub(super) const CLIENT_HELLO: u8 = 1;
pub(super) const SUITE_CHACHA20_POLY1305: u16 = 0x1303;
pub(super) const GROUP_X25519: u16 = 0x001d;
pub(super) const EXT_ALPN: u16 = 16;
pub(super) const EXT_SUPPORTED_VERSIONS: u16 = 43;
pub(super) const EXT_KEY_SHARE: u16 = 51;
const EXT_SUPPORTED_GROUPS: u16 = 10;
const EXT_SIGNATURE_ALGORITHMS: u16 = 13;

pub(super) struct ClientHello {
    /// The record to send.
    pub(super) record: Vec<u8>,
    pub(super) key: KeyPair,
}

pub(super) fn build(
    alpn: &[&str],
    transcript: &mut Transcript,
) -> Result<ClientHello, CryptoError> {
    let key = KeyPair::generate(Alg::X25519)?;
    let mut random = [0u8; 32];
    fill_random(&mut random)?;
    let u16s = |items: &[u16]| {
        let mut v = Vec::new();
        put_u16(&mut v, items.len() * 2);
        items
            .iter()
            .for_each(|i| v.extend_from_slice(&i.to_be_bytes()));
        v
    };
    let mut share = Vec::new();
    put_u16(&mut share, 36);
    share.extend_from_slice(&GROUP_X25519.to_be_bytes());
    put_u16(&mut share, 32);
    share.extend_from_slice(key.public.as_bytes());
    let mut exts = vec![
        extension(EXT_SUPPORTED_VERSIONS, &[2, 3, 4]),
        extension(EXT_SUPPORTED_GROUPS, &u16s(&[GROUP_X25519])),
        extension(EXT_SIGNATURE_ALGORITHMS, &u16s(&[SIG_ED448, SIG_ED25519])),
        extension(EXT_KEY_SHARE, &share),
    ];
    if !alpn.is_empty() {
        let names: Vec<u8> = alpn
            .iter()
            .flat_map(|p| [&[p.len() as u8][..], p.as_bytes()].concat())
            .collect();
        let mut list = Vec::new();
        put_u16(&mut list, names.len());
        list.extend_from_slice(&names);
        exts.push(extension(EXT_ALPN, &list));
    }
    let exts = exts.concat();
    let mut body = vec![3, 3];
    body.extend_from_slice(&random);
    body.push(0); // empty legacy session id
    body.extend_from_slice(&[0, 2]);
    body.extend_from_slice(&SUITE_CHACHA20_POLY1305.to_be_bytes());
    body.extend_from_slice(&[1, 0]); // null compression
    put_u16(&mut body, exts.len());
    body.extend_from_slice(&exts);
    let message = handshake_message(CLIENT_HELLO, &body);
    transcript.add(&message);
    let mut record = vec![HANDSHAKE, 3, 1];
    put_u16(&mut record, message.len());
    record.extend_from_slice(&message);
    Ok(ClientHello { record, key })
}
