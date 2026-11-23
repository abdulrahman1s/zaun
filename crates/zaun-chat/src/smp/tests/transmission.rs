use crate::smp::{Incoming, authorized, encode_transmission, pack, parse_payload};

#[test]
fn authorized_bytes_are_session_id_corr_id_entity_and_command() {
    let a = authorized(&[1, 2], &[3], &[4, 5], b"SUB").unwrap();
    assert_eq!(a, [2, 1, 2, 1, 3, 2, 4, 5, b'S', b'U', b'B']);
}

#[test]
fn unauthorised_transmission_has_an_empty_authenticator_and_no_service_byte() {
    let t = encode_transmission(23, &[], &[7; 24], &[], b"PING").unwrap();
    assert_eq!(t[0], 0);
    assert_eq!(t[1], 24);
    assert_eq!(&t[t.len() - 5..], b"\0PING");
}

#[test]
fn authorised_transmission_adds_the_no_service_signature_byte_from_v16() {
    let t = encode_transmission(23, &[9; 64], &[7; 24], b"ent", b"SUB").unwrap();
    assert_eq!((t[0], t[65]), (64, b'0'));
    let old = encode_transmission(15, &[9; 64], &[7; 24], b"ent", b"SUB").unwrap();
    assert_eq!((old[0], old[65]), (64, 24));
}

#[test]
fn a_block_payload_parses_back_into_its_transmissions() {
    let ts: Vec<Vec<u8>> = (0..3u8)
        .map(|i| encode_transmission(23, &[], &[i; 24], &[i], b"OK").unwrap())
        .collect();
    let (payloads, too_large) = pack(&ts, 16382);
    assert_eq!((payloads.len(), too_large.len()), (1, 0));
    let parsed = parse_payload(23, &payloads[0]).unwrap();
    assert_eq!(parsed.len(), 3);
    assert_eq!(
        parsed[1],
        Incoming {
            auth: vec![],
            corr_id: vec![1; 24],
            entity_id: vec![1],
            command: b"OK".to_vec()
        }
    );
}

#[test]
fn parsing_skips_a_service_signature_after_an_authenticator() {
    let mut t = vec![2, 9, 9, b'1', 3, 1, 2, 3];
    t.extend([1, 5, 1, 6]);
    t.extend(b"OK");
    let mut payload = vec![1, 0, t.len() as u8];
    payload.extend(t);
    let p = parse_payload(23, &payload).unwrap();
    assert_eq!(
        p[0],
        Incoming {
            auth: vec![9, 9],
            corr_id: vec![5],
            entity_id: vec![6],
            command: b"OK".to_vec()
        }
    );
}

#[test]
fn batches_fill_a_block_then_start_another() {
    let t = vec![0u8; 5000];
    let (payloads, too_large) = pack(&[t.clone(), t.clone(), t.clone(), t], 16382);
    assert_eq!(payloads.iter().map(|p| p[0]).collect::<Vec<_>>(), [3, 1]);
    assert!(payloads.iter().all(|p| p.len() <= 16382));
    assert!(too_large.is_empty());
}

#[test]
fn a_transmission_too_big_for_any_block_is_reported_by_index() {
    let (payloads, too_large) = pack(&[vec![0; 10], vec![0; 20000], vec![0; 10]], 16382);
    assert_eq!(too_large, [1]);
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0][0], 2);
}

#[test]
fn encrypted_blocks_leave_sixteen_bytes_less_room() {
    assert_eq!(crate::smp::max_payload(false), 16382);
    assert_eq!(crate::smp::max_payload(true), 16366);
}
