use super::support::*;
use crate::smp::{
    CommandError, ErrorType, QueueIds, QueueLinkData, QueueMode, Response, ServerNtfCreds,
};

fn ids(v: u16) -> QueueIds {
    QueueIds {
        rcv_id: b"rid".to_vec(),
        snd_id: b"sid".to_vec(),
        rcv_dh_key: sample_dh_key(),
        queue_mode: Some(QueueMode::Messaging),
        link_id: None,
        service_id: (v >= 16).then(|| b"svc".to_vec()),
        ntf_creds: (v >= 17).then(|| ServerNtfCreds {
            notifier_id: b"nid".to_vec(),
            dh_key: sample_dh_key(),
        }),
    }
}

#[test]
fn ids_round_trip_at_every_version_with_its_version_gated_fields() {
    for v in 15..=23 {
        let r = Response::Ids(ids(v));
        assert_eq!(
            Response::decode(v, &r.encode(v).unwrap()).unwrap(),
            r,
            "v{v}"
        );
    }
}

#[test]
fn ids_layout_has_the_queue_mode_then_link_then_service_then_notifier() {
    let b = Response::Ids(ids(23)).encode(23).unwrap();
    let n = b.len();
    // '1' 'M' mode, '0' no link, '1' bs"svc", then '1' bs"nid" and the 45-byte key
    assert_eq!(&b[n - 58..n - 56], b"1M");
    assert_eq!(b[n - 56], b'0');
    assert_eq!(&b[n - 55..n - 50], b"1\x03svc");
    assert_eq!(&b[n - 50..n - 50 + 5], b"1\x03nid");
}

#[test]
fn ids_before_v16_end_after_the_link_id() {
    let b = Response::Ids(ids(15)).encode(15).unwrap();
    assert!(b.ends_with(b"1M0"));
}

#[test]
fn sok_is_ok_below_v16_and_carries_the_service_id_from_v16() {
    assert_eq!(Response::Sok(None).encode(23).unwrap(), b"SOK 0");
    assert_eq!(
        Response::Sok(Some(vec![5])).encode(23).unwrap(),
        b"SOK 1\x01\x05"
    );
    assert_eq!(Response::Sok(None).encode(15).unwrap(), b"OK");
    assert_eq!(Response::decode(23, b"SOK 0").unwrap(), Response::Sok(None));
}

#[test]
fn msg_takes_the_rest_of_the_input_as_its_body() {
    let mut b = b"MSG \x18".to_vec();
    b.extend([7u8; 24]);
    b.extend([9, 9, b' ', 1]);
    assert_eq!(
        Response::decode(23, &b).unwrap(),
        Response::Msg {
            msg_id: vec![7; 24],
            body: vec![9, 9, b' ', 1]
        }
    );
}

#[test]
fn err_decodes_the_error_type() {
    assert_eq!(
        Response::decode(23, b"ERR AUTH").unwrap(),
        Response::Err(ErrorType::Auth)
    );
    assert_eq!(
        Response::decode(23, b"ERR CMD SYNTAX").unwrap(),
        Response::Err(ErrorType::Cmd(CommandError::Syntax))
    );
    assert_eq!(
        Response::decode(23, b"ERR QUOTA").unwrap(),
        Response::Err(ErrorType::Quota)
    );
}

#[test]
fn bare_responses_decode_and_trailing_bytes_are_rejected() {
    assert_eq!(Response::decode(23, b"OK").unwrap(), Response::Ok);
    assert_eq!(Response::decode(23, b"PONG").unwrap(), Response::Pong);
    assert_eq!(Response::decode(23, b"END").unwrap(), Response::End);
    assert_eq!(Response::decode(23, b"DELD").unwrap(), Response::Deld);
    assert!(Response::decode(23, b"OK extra").is_err());
    assert!(Response::decode(23, b"NOPE").is_err());
}

#[test]
fn lnk_and_nid_round_trip() {
    let lnk = Response::Lnk {
        sender_id: vec![1, 2, 3],
        data: QueueLinkData {
            fixed: vec![4; 300],
            user: vec![5],
        },
    };
    assert_eq!(Response::decode(23, &lnk.encode(23).unwrap()).unwrap(), lnk);
    let nid = Response::Nid {
        notifier_id: vec![1],
        dh_key: sample_dh_key(),
    };
    assert_eq!(Response::decode(23, &nid.encode(23).unwrap()).unwrap(), nid);
}

#[test]
fn info_keeps_its_bytes() {
    assert_eq!(
        Response::decode(23, b"INFO {\"a\":1}").unwrap(),
        Response::Info(b"{\"a\":1}".to_vec())
    );
}
