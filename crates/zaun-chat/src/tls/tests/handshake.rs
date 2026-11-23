//! Handshake shapes the client must accept, with an echo of 40000 bytes under the traffic keys.
use std::io::{Read, Write};

use super::{connect, opts_for, start};
use crate::crypto::Alg;
use crate::tls::testing::MockOpts;

fn echo_roundtrip(opts: MockOpts, chunk: usize) {
    let cert_request = opts.cert_request;
    let (client, server) = start(opts, chunk);
    let mut tls = connect(client).unwrap();
    assert_eq!(tls.info().alpn(), Some("smp/1"));
    assert_eq!(tls.peer_cert_chain().len(), 1);
    let payload: Vec<u8> = (0..40000u32).map(|i| (i * 7) as u8).collect();
    tls.write_all(&payload).unwrap();
    let mut back = vec![0u8; payload.len()];
    tls.read_exact(&mut back).unwrap();
    assert_eq!(back, payload);
    let unique = *tls.tls_unique();
    tls.close().unwrap();
    let r = server.join().unwrap();
    assert_eq!(r.error, None);
    assert_eq!(
        r.client_finished, unique,
        "tls_unique is the client Finished verify_data"
    );
    assert_eq!(r.received, payload);
    assert_eq!(r.client_cert_empty, cert_request);
}

#[test]
fn ed25519_leaf_with_one_record_per_message() {
    echo_roundtrip(opts_for(Alg::Ed25519), 0);
}

#[test]
fn ed448_leaf_is_accepted() {
    echo_roundtrip(opts_for(Alg::Ed448), 0);
}

#[test]
fn handshake_messages_coalesced_in_one_record_are_split_again() {
    echo_roundtrip(
        MockOpts {
            coalesce: true,
            ..opts_for(Alg::Ed25519)
        },
        0,
    );
}

#[test]
fn handshake_messages_fragmented_across_tiny_records_are_reassembled() {
    echo_roundtrip(
        MockOpts {
            fragment: 7,
            ..opts_for(Alg::Ed25519)
        },
        0,
    );
}

#[test]
fn tcp_delivery_in_three_byte_chunks_is_reassembled() {
    echo_roundtrip(opts_for(Alg::Ed25519), 3);
}

#[test]
fn compat_change_cipher_spec_after_server_hello_is_ignored() {
    echo_roundtrip(
        MockOpts {
            compat_ccs: true,
            ..opts_for(Alg::Ed25519)
        },
        0,
    );
}

#[test]
fn certificate_request_is_answered_with_an_empty_certificate() {
    echo_roundtrip(
        MockOpts {
            cert_request: true,
            ..opts_for(Alg::Ed25519)
        },
        0,
    );
}

#[test]
fn client_offers_no_alpn_when_none_is_given_and_reports_none() {
    let (client, server) = start(
        MockOpts {
            alpn: None,
            ..MockOpts::default()
        },
        0,
    );
    let mut tls = crate::tls::TlsStream::connect(client, &[]).unwrap();
    assert_eq!(tls.info().alpn(), None);
    tls.close().unwrap();
    assert_eq!(server.join().unwrap().error, None);
}
