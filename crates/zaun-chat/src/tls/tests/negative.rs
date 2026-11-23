//! Handshakes and records the client must reject.
use std::io::Read;

use super::{connect, start};
use crate::tls::testing::MockOpts;
use crate::tls::{Alert, TlsError};

fn rejected(opts: MockOpts) -> TlsError {
    let (client, _server) = start(opts, 0);
    connect(client).err().unwrap()
}

#[test]
fn wrong_server_finished_is_rejected() {
    let e = rejected(MockOpts {
        bad_finished: true,
        ..MockOpts::default()
    });
    assert!(matches!(e, TlsError::BadFinished), "{e}");
}

#[test]
fn wrong_certificate_verify_signature_is_rejected() {
    let e = rejected(MockOpts {
        bad_sig: true,
        ..MockOpts::default()
    });
    assert!(matches!(e, TlsError::BadCertificateVerify), "{e}");
}

#[test]
fn empty_certificate_chain_is_rejected() {
    let e = rejected(MockOpts {
        empty_chain: true,
        ..MockOpts::default()
    });
    assert!(matches!(e, TlsError::EmptyChain), "{e}");
}

#[test]
fn hello_retry_request_is_rejected() {
    let e = rejected(MockOpts {
        hrr: true,
        ..MockOpts::default()
    });
    assert!(matches!(e, TlsError::HelloRetryRequest), "{e}");
}

#[test]
fn corrupted_handshake_record_fails_authentication() {
    let e = rejected(MockOpts {
        corrupt_handshake_record: true,
        ..MockOpts::default()
    });
    assert!(matches!(e, TlsError::BadRecordMac), "{e}");
}

#[test]
fn alert_during_the_handshake_is_reported_by_name() {
    let e = rejected(MockOpts {
        alert_in_handshake: true,
        ..MockOpts::default()
    });
    assert!(
        matches!(
            e,
            TlsError::Alert(Alert {
                description: 40,
                ..
            })
        ),
        "{e}"
    );
    assert!(e.to_string().contains("handshake_failure"));
}

#[test]
fn alpn_the_client_did_not_offer_is_rejected() {
    let e = rejected(MockOpts {
        alpn: Some("h2"),
        ..MockOpts::default()
    });
    assert!(matches!(e, TlsError::Alpn), "{e}");
}

#[test]
fn stream_truncated_mid_handshake_is_an_io_error() {
    let (client, server) = crate::tls::testing::Pipe::pair(0);
    drop(server_closing(server));
    assert!(matches!(connect(client), Err(TlsError::Io(_))));
}

fn server_closing(server: crate::tls::testing::Pipe) -> crate::tls::testing::Pipe {
    use crate::net::Socket;
    server.shutdown(std::net::Shutdown::Both).ok();
    server
}

#[test]
fn corrupted_application_record_fails_authentication() {
    let opts = MockOpts {
        send_data: vec![1, 2, 3],
        corrupt_app_record: true,
        ..MockOpts::default()
    };
    let (client, _server) = start(opts, 0);
    let mut tls = connect(client).unwrap();
    let e = tls.read_exact(&mut [0u8; 3]).unwrap_err();
    assert!(e.to_string().contains("bad_record_mac"), "{e}");
}
