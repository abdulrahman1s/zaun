//! Ports of nook's protocol tests (src/sx/chat/protocol.test.ts) that apply to the Rust layer.
use serde_json::Value;
use zaun_chat::chat::version::VersionRange;
use zaun_chat::chat::{ChatEvent as E, parse_chat_elements, parse_chat_messages};

fn value(b: &[u8]) -> Value {
    serde_json::from_slice(b).unwrap()
}

#[test]
fn the_doc_example_without_a_version_reads_as_9() {
    let m = parse_chat_messages(br#"{"event":"x.msg.new","msgId":"abcd","params":{"content":{"type":"text","text":"hello!"}}}"#)
        .unwrap()
        .remove(0);
    assert_eq!(m.v, None);
    assert_eq!(m.version_range(), VersionRange::single(9));
    assert_eq!(m.msg_id.unwrap().as_bytes(), [0x69, 0xb7, 0x1d]);
}

#[test]
fn x_info_serializes_to_the_exact_bytes_nook_expects() {
    let j = r#"{"v":"1-22","event":"x.info","params":{"profile":{"displayName":"bob","fullName":"","preferences":{"voice":{"allow":"yes"}}}}}"#;
    let m = parse_chat_messages(j.as_bytes()).unwrap().remove(0);
    assert_eq!(m.v, Some(VersionRange::new(1, 22)));
    assert_eq!(String::from_utf8(m.to_json().unwrap()).unwrap(), j);
}

#[test]
fn file_description_with_expiry_round_trips() {
    let j = r#"{"v":"1-22","event":"x.msg.file.descr","params":{"msgId":"AAAAAAAAAAAAAAAA","fileDescr":{"fileDescrText":"abc","fileDescrPartNo":1,"fileDescrComplete":true},"fileExpires":"2026-12-01T00:00:00Z"}}"#;
    let m = parse_chat_messages(j.as_bytes()).unwrap().remove(0);
    let E::MsgFileDescr {
        file_descr,
        file_expires,
        ..
    } = &m.event
    else {
        panic!()
    };
    assert_eq!(
        (
            file_descr.file_descr_part_no,
            file_descr.file_descr_complete
        ),
        (1, true)
    );
    assert_eq!(file_expires.as_deref(), Some("2026-12-01T00:00:00Z"));
    assert_eq!(value(&m.to_json().unwrap()), value(j.as_bytes()));
}

#[test]
fn image_message_with_an_xftp_file_invitation_round_trips_exactly() {
    let j = r#"{"v":"1-22","msgId":"AAAAAAAAAAAAAAAA","event":"x.msg.new","params":{"content":{"type":"image","text":"","image":"data:image/jpg;base64,/9j/4AAQ"},"file":{"fileName":"a.jpg","fileSize":1000,"fileDescr":{"fileDescrText":"","fileDescrPartNo":0,"fileDescrComplete":false}}}}"#;
    let m = parse_chat_messages(j.as_bytes()).unwrap().remove(0);
    let E::MsgNew(c) = &m.event else { panic!() };
    assert_eq!(
        c.file,
        Some(zaun_chat::chat::file::FileInvitation::xftp("a.jpg", 1000))
    );
    assert_eq!(value(&m.to_json().unwrap()), value(j.as_bytes()));
}

#[test]
fn nooks_raw_binary_batch_has_a_signed_and_a_forwarded_element() {
    let e1 = br#"{"v":"1-22","event":"x.ok","params":{}}"#.to_vec();
    let mut signed = vec![0x2f, 0x44, 1, 0x4d, 64];
    signed.extend([0u8; 64]);
    signed.extend(&e1);
    let mut fwd = vec![0x3e, 0x43, 0, 0, 0, 0, 0, 0, 0, 1];
    fwd.extend(&e1);
    let els = [e1, signed, fwd];
    let mut body = vec![0x3d, els.len() as u8];
    for e in &els {
        body.extend((e.len() as u16).to_be_bytes());
        body.extend(e);
    }
    let r: Vec<_> = parse_chat_elements(&body)
        .into_iter()
        .map(Result::unwrap)
        .collect();
    assert!(r.iter().all(|p| p.message.event == E::Ok));
    assert_eq!(
        r[1].signed.as_ref().unwrap().signatures[0].signature,
        vec![0u8; 64]
    );
    assert_eq!(
        r[2].forward.as_ref().unwrap().broker_ts,
        "1970-01-01T00:00:01Z"
    );
}

#[test]
fn a_passthrough_container_and_a_zstd_container_from_a_peer_parse() {
    let small = br#"{"v":"1-22","event":"x.ok","params":{}}"#;
    let mut x = vec![b'X', 1, b'0', small.len() as u8];
    x.extend(small);
    assert_eq!(parse_chat_messages(&x).unwrap()[0].event, E::Ok);
}
