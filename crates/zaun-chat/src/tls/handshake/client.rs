//! The client side of the TLS 1.3 handshake, from ClientHello to the first application keys.
use std::io::{Read, Write};

use super::super::error::TlsError;
use super::super::info::TlsInfo;
use super::super::key_schedule::{
    Secret, Transcript, certificate_verify_input, derive_secret, handshake_secrets,
};
use super::super::record::{Inbound, Protection};
use super::super::verify::check_certificate_verify;

use super::client_hello;
use super::finished::{check_server_finished, client_flight};
use super::messages::Messages;
use super::server_flight::{
    CERTIFICATE, CERTIFICATE_REQUEST, CERTIFICATE_VERIFY, ENCRYPTED_EXTENSIONS, FINISHED,
    SERVER_HELLO, parse_certificate, parse_certificate_request, parse_certificate_verify,
    parse_encrypted_extensions, parse_server_hello,
};
use crate::crypto::{Alg, PublicKey, dh};

/// Everything the record layer needs after the handshake.
pub(in crate::tls) struct Established {
    pub(in crate::tls) inbound: Inbound,
    pub(in crate::tls) pending: Vec<u8>,
    pub(in crate::tls) rx: Protection,
    pub(in crate::tls) rx_secret: Secret,
    pub(in crate::tls) tx: Protection,
    pub(in crate::tls) tx_secret: Secret,
    pub(in crate::tls) info: TlsInfo,
}

pub(in crate::tls) fn perform<S: Read + Write>(
    sock: &mut S,
    alpn: &[&str],
) -> Result<Established, TlsError> {
    let mut transcript = Transcript::new();
    let hello = client_hello::build(alpn, &mut transcript)?;
    sock.write_all(&hello.record)?;
    sock.flush()?;
    let mut io = Messages::new(sock);

    let server_hello = io.expect(SERVER_HELLO, "expected ServerHello")?;
    let peer = PublicKey::from_raw(Alg::X25519, &parse_server_hello(server_hello.body())?)?;
    let shared = dh(&peer, &hello.key.private)?;
    if shared.as_bytes().iter().all(|b| *b == 0) {
        return Err(TlsError::Illegal("all-zero shared secret"));
    }
    transcript.add(&server_hello.raw);
    let secrets = handshake_secrets(shared.as_bytes());
    let hello_hash = transcript.hash();
    let client_hs = derive_secret(&secrets.handshake, "c hs traffic", &hello_hash);
    let server_hs = derive_secret(&secrets.handshake, "s hs traffic", &hello_hash);
    io.rx = Some(Protection::new(&server_hs));

    let ee = io.expect(ENCRYPTED_EXTENSIONS, "expected EncryptedExtensions")?;
    let negotiated = parse_encrypted_extensions(ee.body(), alpn)?;
    transcript.add(&ee.raw);

    let mut next = io.next_message()?;
    let request = if next.ty == CERTIFICATE_REQUEST {
        let context = parse_certificate_request(next.body())?;
        transcript.add(&next.raw);
        next = io.next_message()?;
        Some(context)
    } else {
        None
    };
    if next.ty != CERTIFICATE {
        return Err(TlsError::Protocol("expected Certificate"));
    }
    let chain = parse_certificate(next.body())?;
    transcript.add(&next.raw);

    let cv = io.expect(CERTIFICATE_VERIFY, "expected CertificateVerify")?;
    let (scheme, signature) = parse_certificate_verify(cv.body())?;
    let signed = certificate_verify_input(&transcript.hash(), true);
    check_certificate_verify(&chain[0], scheme, signature, &signed)?;
    transcript.add(&cv.raw);

    let fin = io.expect(FINISHED, "expected Finished")?;
    check_server_finished(&server_hs, &transcript.hash(), fin.body())?;
    transcript.add(&fin.raw);
    let server_done = transcript.hash();
    let client_ap = derive_secret(&secrets.master, "c ap traffic", &server_done);
    let server_ap = derive_secret(&secrets.master, "s ap traffic", &server_done);

    let (pending, inbound) = (
        std::mem::take(&mut io.pending),
        std::mem::take(&mut io.inbound),
    );
    let mut tx_hs = Protection::new(&client_hs);
    let (flight, tls_unique) =
        client_flight(request.as_deref(), &mut transcript, &client_hs, &mut tx_hs)?;
    sock.write_all(&flight)?;
    sock.flush()?;

    Ok(Established {
        inbound,
        pending,
        rx: Protection::new(&server_ap),
        rx_secret: server_ap,
        tx: Protection::new(&client_ap),
        tx_secret: client_ap,
        info: TlsInfo {
            alpn: negotiated,
            peer_cert_chain: chain,
            tls_unique,
            certificate_requested: request.is_some(),
        },
    })
}
