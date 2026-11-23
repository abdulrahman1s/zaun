//! Real SMP servers (ignored by default): the handshake, the pinned CA and the session id.
use std::io::Read;
use std::net::TcpStream;
use std::time::Duration;

use crate::crypto::key_hash;
use crate::encoding::b64_decode;
use crate::net::connect_tcp;
use crate::tls::{TlsStream, verify_server_chain};

fn check_server(host: &str, port: u16, fingerprint: &str) {
    let key_hash_bytes = b64_decode(fingerprint).unwrap();
    let sock = connect_tcp(host, port, Duration::from_secs(10)).unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    let mut tls: TlsStream<TcpStream> = TlsStream::connect(sock, &["smp/1"]).unwrap();
    assert_eq!(tls.info().alpn(), Some("smp/1"));
    let chain = tls.peer_cert_chain().to_vec();
    verify_server_chain(&chain, &key_hash_bytes).unwrap();
    assert_eq!(key_hash(&chain[1]).to_vec(), key_hash_bytes);

    let mut block = vec![0u8; 16384];
    tls.read_exact(&mut block).unwrap();
    let hello_len = usize::from(u16::from_be_bytes([block[0], block[1]]));
    let hello = &block[2..2 + hello_len];
    let session_len = usize::from(hello[4]);
    assert_eq!(session_len, 32);
    assert_eq!(
        &hello[5..5 + session_len],
        tls.tls_unique(),
        "SMP sessionId is tls_unique"
    );
    tls.close().unwrap();
}

#[test]
#[ignore = "needs a local smp-server: ZAUN_TEST_SMP=smp://FINGERPRINT@127.0.0.1:5223"]
fn local_smp_server_handshake_pins_the_ca_and_session_id_is_tls_unique() {
    let address = std::env::var("ZAUN_TEST_SMP").unwrap();
    let rest = address.strip_prefix("smp://").unwrap();
    let (fingerprint, host_port) = rest.split_once('@').unwrap();
    let (host, port) = host_port.rsplit_once(':').unwrap();
    check_server(host, port.parse().unwrap(), fingerprint);
}

#[test]
#[ignore = "needs the internet: smp5.simplex.im:5223"]
fn smp5_simplex_im_handshake_pins_the_published_ca() {
    check_server(
        "smp5.simplex.im",
        5223,
        "hpq7_4gGJiilmz5Rf-CswuU5kZGkm_zOIooSw6yALRg",
    );
}
