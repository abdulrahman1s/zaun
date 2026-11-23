//! Application data after the handshake: KeyUpdate, close_notify and the split halves.
use std::io::{Read, Write};
use std::thread;

use super::{connect, opts_for, start};
use crate::tls::testing::MockOpts;

#[test]
fn server_key_update_rotates_both_directions() {
    let opts = MockOpts {
        key_update: true,
        send_data: vec![1, 2, 3],
        ..opts_for(crate::crypto::Alg::Ed25519)
    };
    let (client, server) = start(opts, 0);
    let mut tls = connect(client).unwrap();
    tls.write_all(&[9, 9]).unwrap();
    let mut b = [0u8; 3];
    tls.read_exact(&mut b).unwrap();
    assert_eq!(b, [1, 2, 3]);
    let mut b = [0u8; 2];
    tls.read_exact(&mut b).unwrap();
    assert_eq!(b, [9, 9]);
    tls.write_all(&[7]).unwrap();
    let mut b = [0u8; 1];
    tls.read_exact(&mut b).unwrap();
    assert_eq!(b, [7], "echo under the rotated keys");
    tls.close().unwrap();
    assert_eq!(server.join().unwrap().error, None);
}

#[test]
fn buffered_data_is_served_before_close_notify_reads_as_eof() {
    let opts = MockOpts {
        send_data: vec![1, 2, 3],
        close_notify_after_data: true,
        ..MockOpts::default()
    };
    let (client, _server) = start(opts, 0);
    let mut tls = connect(client).unwrap();
    let mut b = [0u8; 3];
    tls.read_exact(&mut b).unwrap();
    assert_eq!(b, [1, 2, 3]);
    assert_eq!(tls.read(&mut b).unwrap(), 0);
}

#[test]
fn halves_work_from_two_threads_with_their_own_keys() {
    let (client, server) = start(opts_for(crate::crypto::Alg::Ed448), 0);
    let (mut reader, mut writer) = connect(client).unwrap().split();
    let payload: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
    let expected = payload.clone();
    let reading = thread::spawn(move || {
        let mut back = vec![0u8; expected.len()];
        reader.read_exact(&mut back).unwrap();
        assert_eq!(back, expected);
    });
    writer.write_all(&payload).unwrap();
    reading.join().unwrap();
    writer.close().unwrap();
    assert_eq!(server.join().unwrap().error, None);
}

#[test]
fn client_key_update_request_is_sent_and_the_server_follows() {
    let (client, server) = start(opts_for(crate::crypto::Alg::Ed25519), 0);
    let (mut reader, mut writer) = connect(client).unwrap().split();
    writer.write_all(&[1]).unwrap();
    writer.request_key_update().unwrap();
    writer.write_all(&[2]).unwrap();
    let mut b = [0u8; 2];
    reader.read_exact(&mut b).unwrap();
    assert_eq!(b, [1, 2]);
    writer.close().unwrap();
    assert_eq!(server.join().unwrap().error, None);
}
