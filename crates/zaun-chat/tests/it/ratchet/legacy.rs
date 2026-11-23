//! Envelopes of 7.0 and older: one-byte lengths, and headers without the KEM field (Ratchet.hs `versionedLengthP`, `legacyMsgHeaderP`).
use super::support::{PADDED, Pair, handshake, recv};
use zaun_chat::crypto::aead_encrypt;
use zaun_chat::encoding::Encode;
use zaun_chat::ratchet::CURRENT_E2E_VERSION;

#[test]
fn a_seven_oh_envelope_with_one_byte_lengths_and_no_kem_field_decrypts() {
    let Pair {
        mut alice,
        mut bob,
        bob_init,
        bob_dhrs,
        ..
    } = handshake(false, false);
    let mek = bob
        .encrypt_header(None, CURRENT_E2E_VERSION)
        .unwrap()
        .to_bytes()
        .unwrap();
    // (version, ad, key, iv, Large enc_header): each ByteString has a length byte.
    let ad = &mek[3..3 + 112];
    let key: [u8; 32] = mek[3 + 112 + 1..][..32].try_into().unwrap();
    let iv: [u8; 16] = mek[3 + 112 + 1 + 32 + 1..][..16].try_into().unwrap();
    let enc_header = &mek[3 + 112 + 1 + 32 + 1 + 16 + 2..];
    let header_iv: [u8; 16] = enc_header[2..18].try_into().unwrap();

    let mut header = CURRENT_E2E_VERSION.to_bytes().unwrap();
    bob_dhrs.encode(&mut header).unwrap();
    header.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
    let (hbody, htag) = aead_encrypt(&bob_init.snd_hk.aes(), &header_iv, 88, ad, &header).unwrap();
    let mut em_header = CURRENT_E2E_VERSION.to_bytes().unwrap();
    em_header.extend_from_slice(&header_iv);
    em_header.extend_from_slice(&htag);
    em_header.push(u8::try_from(hbody.len()).unwrap());
    em_header.extend_from_slice(&hbody);

    let mk = zaun_chat::ratchet::Key32::new(key);
    let msg_ad = [ad, &em_header].concat();
    let (mbody, mtag) = aead_encrypt(&mk.aes(), &iv, PADDED, &msg_ad, b"legacy").unwrap();
    let mut wire = vec![u8::try_from(em_header.len()).unwrap()];
    wire.extend_from_slice(&em_header);
    wire.extend_from_slice(&mtag);
    wire.extend_from_slice(&mbody);
    assert_eq!(wire[0], 123);
    assert_eq!(recv(&mut alice, &wire), "legacy");
}
