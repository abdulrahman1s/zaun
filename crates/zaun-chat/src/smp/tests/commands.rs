use super::support::*;
use crate::smp::{Command, NewQueue, QueueLinkData, QueueReqData, SubscriptionMode};

fn encode(v: u16, c: &Command) -> Vec<u8> {
    c.encode(v).unwrap()
}

fn ascii(b: &[u8]) -> String {
    String::from_utf8(b.to_vec()).unwrap()
}

#[test]
fn new_at_v17_and_later_matches_the_haskell_server_test_bytes() {
    // ServerTests.hs: (NEW_, ' ', samplePubKey, sampleDhPubKey) ‖ ('0', SMSubscribe, Just (QRMessaging Nothing), '0')
    let new = NewQueue::messaging(
        sample_pub_key(),
        sample_dh_key(),
        SubscriptionMode::Subscribe,
    );
    let b = encode(23, &Command::New(Box::new(new)));
    let mut expected = b"NEW ".to_vec();
    expected.extend(
        sample_pub_key()
            .to_der()
            .iter()
            .fold(vec![44u8], |mut v, x| {
                v.push(*x);
                v
            }),
    );
    expected.extend([44u8].into_iter().chain(sample_dh_key().to_der()));
    expected.extend(b"0S1M00");
    assert_eq!(b, expected);
    assert!(hex(&b[4..]).starts_with(ED_KEY_PREFIX));
    assert_eq!(b.len(), 4 + 45 + 45 + 6);
}

#[test]
fn new_before_v17_leaves_out_the_notifier_credentials() {
    let new = NewQueue::messaging(
        sample_pub_key(),
        sample_dh_key(),
        SubscriptionMode::OnlyCreate,
    );
    let b = encode(16, &Command::New(Box::new(new)));
    assert_eq!(ascii(&b[4 + 90..]), "0C1M0");
}

#[test]
fn new_without_queue_data_writes_nothing_for_it() {
    let mut new = NewQueue::messaging(
        sample_pub_key(),
        sample_dh_key(),
        SubscriptionMode::Subscribe,
    );
    new.queue_req_data = None;
    new.basic_auth = Some("pw".into());
    let b = encode(23, &Command::New(Box::new(new)));
    assert_eq!(&b[4 + 90..], b"1\x02pwS00");
}

#[test]
fn new_for_a_contact_queue_with_a_link_writes_link_id_sender_id_and_data() {
    let mut new = NewQueue::messaging(
        sample_pub_key(),
        sample_dh_key(),
        SubscriptionMode::Subscribe,
    );
    let data = QueueLinkData {
        fixed: vec![1, 2],
        user: vec![3],
    };
    new.queue_req_data = Some(QueueReqData::Contact(Some(crate::smp::ContactLink {
        link_id: vec![9],
        sender_id: vec![8, 8],
        data,
    })));
    let b = encode(23, &Command::New(Box::new(new)));
    assert_eq!(
        &b[4 + 90..],
        b"0S1C1\x01\x09\x02\x08\x08\x00\x02\x01\x02\x00\x01\x030"
    );
}

#[test]
fn send_matches_the_haskell_server_test_bytes() {
    // ServerTests.hs: (SEND_, ' ', noMsgFlags, ' ', "hello")
    assert_eq!(
        encode(
            23,
            &Command::Send {
                notification: false,
                body: b"hello".to_vec()
            }
        ),
        b"SEND F hello"
    );
    assert_eq!(
        encode(
            23,
            &Command::Send {
                notification: true,
                body: b"hi".to_vec()
            }
        ),
        b"SEND T hi"
    );
    assert_eq!(
        encode(
            23,
            &Command::Send {
                notification: false,
                body: Vec::new()
            }
        ),
        b"SEND F "
    );
}

#[test]
fn ack_matches_the_haskell_server_test_bytes() {
    // ServerTests.hs: (ACK_, ' ', "123456789012345678901234")
    let mut expected = b"ACK \x18".to_vec();
    expected.extend(b"123456789012345678901234");
    assert_eq!(
        encode(
            23,
            &Command::Ack {
                msg_id: b"123456789012345678901234".to_vec()
            }
        ),
        expected
    );
}

#[test]
fn key_commands_carry_one_encoded_key() {
    let mut expected = b"KEY ".to_vec();
    expected.push(44);
    expected.extend(sample_pub_key().to_der());
    assert_eq!(encode(23, &Command::Key(sample_pub_key())), expected);
    assert_eq!(&encode(23, &Command::Skey(sample_pub_key()))[..5], b"SKEY ");
    assert_eq!(&encode(23, &Command::Lkey(sample_pub_key()))[..5], b"LKEY ");
}

#[test]
fn lset_carries_link_id_and_two_large_blobs() {
    let c = Command::Lset {
        link_id: vec![7; 3],
        data: QueueLinkData {
            fixed: vec![1; 300],
            user: vec![2],
        },
    };
    let b = encode(23, &c);
    assert_eq!(&b[..9], b"LSET \x03\x07\x07\x07");
    assert_eq!(&b[9..11], &[0x01, 0x2c]);
    assert_eq!(b.len(), 9 + 2 + 300 + 2 + 1);
}

#[test]
fn commands_without_parameters_are_their_tag_alone() {
    for (c, tag) in [
        (Command::Sub, "SUB"),
        (Command::Ldel, "LDEL"),
        (Command::Lget, "LGET"),
        (Command::Get, "GET"),
        (Command::Off, "OFF"),
        (Command::Del, "DEL"),
        (Command::Que, "QUE"),
        (Command::Ping, "PING"),
    ] {
        assert_eq!(ascii(&encode(23, &c)), tag);
        assert_eq!(c.name(), tag);
    }
}

#[test]
fn oversized_fields_fail_instead_of_wrapping() {
    assert!(
        Command::Ack {
            msg_id: vec![0; 300]
        }
        .encode(23)
        .is_err()
    );
}
