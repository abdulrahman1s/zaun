//! Batches: JSON arrays, binary `=` batches with signed `/` and forwarded `>` elements, limits.
use super::chat_support::*;
use zaun_chat::chat::batch::{encode_binary_batch, encode_json_batch};
use zaun_chat::chat::element::{encode_forward_element, encode_signed_element, parse_element};
use zaun_chat::chat::forward::{FwdSender, GrpMsgForward};
use zaun_chat::chat::ids::MemberId;
use zaun_chat::chat::signed::{ChatBinding, MsgSignature, SignedMsg};
use zaun_chat::chat::{ChatEvent as E, ChatMessage, parse_chat_elements, parse_chat_messages};

fn ok_json() -> Vec<u8> {
    msg(E::Ok).to_json().unwrap()
}

fn text_json(t: &str) -> Vec<u8> {
    msg(E::MsgNew(zaun_chat::chat::container::MsgContainer::text(t)))
        .to_json()
        .unwrap()
}

#[test]
fn json_batch_of_two_parses_to_two_messages() {
    let b = encode_json_batch(&[text_json("a"), ok_json()]).unwrap();
    assert_eq!(b[0], b'[');
    let ms = parse_chat_messages(&b).unwrap();
    assert_eq!(ms.len(), 2);
    assert_eq!(ms[1].event, E::Ok);
}

#[test]
fn a_single_element_is_sent_bare() {
    let one = ok_json();
    assert_eq!(encode_json_batch(std::slice::from_ref(&one)).unwrap(), one);
}

fn signed_element() -> (Vec<u8>, SignedMsg) {
    let signed = SignedMsg {
        binding: ChatBinding::Direct,
        signatures: vec![MsgSignature {
            signature: vec![7; 64],
        }],
        signed_body: ok_json(),
    };
    (encode_signed_element(&signed).unwrap(), signed)
}

fn forward_header() -> GrpMsgForward {
    GrpMsgForward {
        sender: FwdSender::Member {
            member_id: MemberId(vec![1, 2, 3, 4]),
            member_name: "alice".into(),
        },
        broker_ts: "2026-11-23T10:20:30Z".into(),
    }
}

#[test]
fn binary_batch_with_plain_signed_and_forwarded_elements() {
    let (signed_bytes, signed) = signed_element();
    let fwd = encode_forward_element(&forward_header(), &text_json("hi")).unwrap();
    let batch = encode_binary_batch(&[ok_json(), signed_bytes, fwd]).unwrap();
    assert_eq!(&batch[..2], b"=\x03");
    let parsed = parse_chat_elements(&batch);
    assert_eq!(parsed.len(), 3);
    let parsed: Vec<_> = parsed.into_iter().map(|r| r.unwrap()).collect();
    assert!(parsed[0].signed.is_none() && parsed[0].forward.is_none());
    assert_eq!(parsed[1].signed.as_ref(), Some(&signed));
    assert_eq!(parsed[2].forward, Some(forward_header()));
    assert!(matches!(parsed[2].message.event, E::MsgNew(_)));
}

#[test]
fn signed_input_is_binding_then_binding_data_then_the_exact_body() {
    let (_, signed) = signed_element();
    let input = signed.signed_input(b"DATA");
    assert_eq!(&input[..5], b"DDATA");
    assert_eq!(&input[5..], signed.signed_body.as_slice());
}

#[test]
fn a_forward_inside_a_signed_element_and_a_signed_forward_both_parse() {
    let (signed_bytes, signed) = signed_element();
    let fwd = encode_forward_element(&forward_header(), &signed_bytes).unwrap();
    let p = parse_element(&fwd).unwrap();
    assert_eq!(p.forward, Some(forward_header()));
    assert_eq!(p.signed, Some(signed));
}

#[test]
fn a_nested_forward_element_is_refused() {
    let inner = encode_forward_element(&forward_header(), &ok_json()).unwrap();
    let outer = encode_forward_element(&forward_header(), &inner).unwrap();
    let e = parse_element(&outer).unwrap_err().to_string();
    assert!(e.contains("nested forward"), "{e}");
}

#[test]
fn a_signed_body_must_be_json() {
    let mut s = signed_element().1;
    s.signed_body = b"not json".to_vec();
    assert!(parse_element(&encode_signed_element(&s).unwrap()).is_err());
}

#[test]
fn bad_elements_are_reported_one_by_one_in_a_batch() {
    let batch = encode_binary_batch(&[ok_json(), b"?junk".to_vec(), ok_json()]).unwrap();
    let r = parse_chat_elements(&batch);
    assert_eq!(
        (r[0].is_ok(), r[1].is_ok(), r[2].is_ok()),
        (true, false, true)
    );
    assert!(parse_chat_messages(&batch).is_err());
}

#[test]
fn json_batch_at_the_element_limit_parses_and_one_above_is_refused() {
    let batch = |n: usize| format!("[{}]", vec!["{}"; n].join(","));
    assert_eq!(parse_chat_elements(batch(255).as_bytes()).len(), 255);
    let e = parse_chat_elements(batch(256).as_bytes());
    assert_eq!(e.len(), 1);
    assert!(
        e[0].as_ref()
            .unwrap_err()
            .to_string()
            .contains("too many messages in batch")
    );
}

#[test]
fn binary_batch_refuses_more_than_255_elements_when_encoding() {
    assert!(encode_binary_batch(&vec![ok_json(); 256]).is_err());
    assert!(encode_json_batch(&vec![ok_json(); 256]).is_err());
}

#[test]
fn compressed_blocks_that_together_exceed_the_element_limit_are_refused() {
    use zaun_chat::chat::compress::compress_body;
    let batch = format!("[{}]", vec!["{}"; 255].join(","));
    let mut two = compress_body(batch.as_bytes()).unwrap();
    let second = compress_body(batch.as_bytes()).unwrap();
    two[1] = 2;
    two.extend_from_slice(&second[2..]);
    let r = parse_chat_elements(&two);
    assert_eq!(r.len(), 1);
    assert!(
        r[0].as_ref()
            .unwrap_err()
            .to_string()
            .contains("too many messages in batch")
    );
}

#[test]
fn forward_depth_is_one_for_json_forwards() {
    let nested = |n: usize| -> ChatMessage {
        let mut m = msg(E::MsgNew(zaun_chat::chat::container::MsgContainer::text(
            "hello",
        )));
        for _ in 0..n {
            let fwd = GrpMsgForward {
                sender: FwdSender::Channel,
                broker_ts: "1970-01-01T00:00:01Z".into(),
            };
            m = msg(E::GrpMsgForward {
                fwd,
                msg: Box::new(m),
            });
        }
        m
    };
    let at_limit = nested(1);
    assert_eq!(
        parse_chat_messages(&at_limit.to_json().unwrap()).unwrap(),
        vec![at_limit]
    );
    let e = parse_chat_messages(&nested(2).to_json().unwrap())
        .unwrap_err()
        .to_string();
    assert!(e.contains("forward depth exceeds limit"), "{e}");
}

#[test]
fn binary_forward_time_keeps_whole_seconds() {
    let bytes = forward_header().encode().unwrap();
    assert_eq!(bytes[0], b'M');
    assert_eq!(&bytes[bytes.len() - 8..], &1_795_429_230i64.to_be_bytes());
}

#[test]
fn empty_and_garbage_input_is_refused() {
    assert!(parse_chat_messages(b"").is_err());
    assert!(parse_chat_messages(b"zzz").is_err());
    assert!(parse_chat_messages(b"[").is_err());
    assert!(parse_chat_messages(b"=\x02\x00").is_err());
}
