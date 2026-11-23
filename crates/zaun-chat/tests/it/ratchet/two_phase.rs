//! Header first and body later, as the agent queues messages.
use super::support::{PADDED, Pair, handshake, recv};
use zaun_chat::encoding::{Decode, Encode};
use zaun_chat::ratchet::{
    CURRENT_E2E_VERSION, EncRatchetMessage, MsgEncryptKey, PqSupport, e2e_enc_agent_msg_length,
    e2e_enc_conn_info_length,
};

#[test]
fn a_stored_message_key_encrypts_later_into_the_same_wire_format() {
    let Pair {
        mut alice, mut bob, ..
    } = handshake(false, false);
    let mek = bob.encrypt_header(None, CURRENT_E2E_VERSION).unwrap();
    let mek = MsgEncryptKey::from_bytes(&mek.to_bytes().unwrap()).unwrap();
    let wire = mek
        .encrypt_msg(e2e_enc_conn_info_length(PqSupport::OFF), b"conn info")
        .unwrap();
    assert_eq!(wire.len(), 54 + 88 + 14832);
    assert_eq!(recv(&mut alice, &wire), "conn info");
    assert_eq!(e2e_enc_agent_msg_length(PqSupport::OFF), 15840);
    assert_eq!(
        (
            e2e_enc_conn_info_length(PqSupport::ON),
            e2e_enc_agent_msg_length(PqSupport::ON)
        ),
        (11106, 13618)
    );
    let parsed = EncRatchetMessage::from_bytes(&wire).unwrap();
    assert_eq!(parsed.tag.len(), 16);
    assert_eq!(parsed.body.len(), 14832);
}

#[test]
fn the_next_message_after_a_two_phase_one_still_decrypts() {
    let Pair {
        mut alice, mut bob, ..
    } = handshake(false, false);
    let mek = bob.encrypt_header(None, CURRENT_E2E_VERSION).unwrap();
    let second = bob.encrypt(b"second", PADDED, None).unwrap();
    assert_eq!(recv(&mut alice, &second), "second");
    let first = mek.encrypt_msg(PADDED, b"first").unwrap();
    assert_eq!(recv(&mut alice, &first), "first");
}
