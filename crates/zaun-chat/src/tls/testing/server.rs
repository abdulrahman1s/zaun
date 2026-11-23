//! A TLS 1.3 server in a thread, built from the client's own key schedule, with fault injection (port of nook sx/tls/scripts/mockServer.ts).
use std::io::Write;
use std::thread::{self, JoinHandle};

use super::app::app_phase;
use super::cert::make_cert;
use super::client_hello::client_key_share;
use super::flight::server_flight;
use super::options::{MockOpts, MockResult};
use super::pipe::Pipe;
use crate::crypto::{Alg, KeyPair, PublicKey, dh, fill_random};
use crate::net::Socket;
use crate::tls::error::TlsError;
use crate::tls::key_schedule::{
    Transcript, derive_secret, finished_verify_data, handshake_secrets,
};
use crate::tls::record::{ALERT, HANDSHAKE, Inbound, Protection};
use crate::tls::wire::{extension, handshake_message, put_u16};

const HELLO_RETRY_REQUEST_RANDOM: [u8; 32] = [
    0xcf, 0x21, 0xad, 0x74, 0xe5, 0x9a, 0x61, 0x11, 0xbe, 0x1d, 0x8c, 0x02, 0x1e, 0x65, 0xb8, 0x91,
    0xc2, 0xa2, 0x11, 0x16, 0x7a, 0xbb, 0x8c, 0x5e, 0x07, 0x9e, 0x09, 0xe2, 0xc8, 0xa8, 0x33, 0x9c,
];

pub(in crate::tls) fn spawn_server(mut sock: Pipe, o: MockOpts) -> JoinHandle<MockResult> {
    thread::spawn(move || {
        let mut res = MockResult::default();
        if let Err(e) = serve(&mut sock, &o, &mut res) {
            res.error = Some(e.to_string());
        }
        sock.shutdown(std::net::Shutdown::Both).ok();
        res
    })
}

fn serve(sock: &mut Pipe, o: &MockOpts, res: &mut MockResult) -> Result<(), TlsError> {
    let mut inbound = Inbound::default();
    let hello = inbound.next_record(sock)?.body;
    let client_pub = PublicKey::from_raw(Alg::X25519, &client_key_share(&hello)?)?;
    let mut transcript = Transcript::new();
    transcript.add(&hello);

    let eph = KeyPair::generate(Alg::X25519)?;
    let mut random = [0u8; 32];
    fill_random(&mut random)?;
    if o.hrr {
        random = HELLO_RETRY_REQUEST_RANDOM;
    }
    let mut share = 0x001du16.to_be_bytes().to_vec();
    put_u16(&mut share, 32);
    share.extend_from_slice(eph.public.as_bytes());
    let exts = [extension(43, &[3, 4]), extension(51, &share)].concat();
    let mut body = vec![3, 3];
    body.extend_from_slice(&random);
    body.extend_from_slice(&[0, 0x13, 0x03, 0]);
    put_u16(&mut body, exts.len());
    body.extend(exts);
    let server_hello = handshake_message(2, &body);
    transcript.add(&server_hello);
    let mut record = vec![HANDSHAKE, 3, 3];
    put_u16(&mut record, server_hello.len());
    sock.write_all(&[record, server_hello].concat())?;
    if o.hrr {
        return Ok(());
    }
    if o.compat_ccs {
        sock.write_all(&[20, 3, 3, 0, 1, 1])?;
    }

    let secrets = handshake_secrets(dh(&client_pub, &eph.private)?.as_bytes());
    let th = transcript.hash();
    let client_hs = derive_secret(&secrets.handshake, "c hs traffic", &th);
    let server_hs = derive_secret(&secrets.handshake, "s hs traffic", &th);
    let mut tx = Protection::new(&server_hs);
    let mut rx = Protection::new(&client_hs);
    if o.alert_in_handshake {
        sock.write_all(&tx.seal(ALERT, &[2, 40])?)?;
        return Ok(());
    }

    let leaf = KeyPair::generate(o.key)?;
    let chain = vec![make_cert(&leaf.public, &leaf, 1)];
    let msgs = server_flight(o, &leaf, &chain, &server_hs, &mut transcript)?;
    send_flight(sock, &mut tx, &msgs, o)?;

    let done = transcript.hash();
    let client_ap = derive_secret(&secrets.master, "c ap traffic", &done);
    let server_ap = derive_secret(&secrets.master, "s ap traffic", &done);
    read_client_flight(
        sock,
        &mut inbound,
        &mut rx,
        &mut transcript,
        &client_hs,
        o,
        res,
    )?;
    app_phase(sock, inbound, o, res, server_ap, client_ap)
}

fn send_flight(
    sock: &mut Pipe,
    tx: &mut Protection,
    msgs: &[Vec<u8>],
    o: &MockOpts,
) -> Result<(), TlsError> {
    let flight = msgs.concat();
    let step = if o.coalesce { flight.len() } else { o.fragment };
    let mut records = Vec::new();
    if step > 0 {
        for part in flight.chunks(step) {
            records.push(tx.seal(HANDSHAKE, part)?);
        }
    } else {
        for m in msgs {
            records.push(tx.seal(HANDSHAKE, m)?);
        }
    }
    if o.corrupt_handshake_record {
        let r = &mut records[1];
        let at = r.len() - 3;
        r[at] ^= 1;
    }
    sock.write_all(&records.concat())?;
    Ok(())
}

fn read_client_flight(
    sock: &mut Pipe,
    inbound: &mut Inbound,
    rx: &mut Protection,
    transcript: &mut Transcript,
    client_hs: &[u8; 32],
    o: &MockOpts,
    res: &mut MockResult,
) -> Result<(), TlsError> {
    let mut next = |t: &mut Transcript, what: &'static str, ty: u8| -> Result<Vec<u8>, TlsError> {
        let r = inbound.next_record(sock)?;
        let (inner, content) = rx.open(&r.header, &r.body)?;
        if inner != HANDSHAKE || content.first() != Some(&ty) {
            return Err(TlsError::Protocol(what));
        }
        t.add(&content);
        Ok(content)
    };
    if o.cert_request {
        let cert = next(transcript, "expected client Certificate", 11)?;
        res.client_cert_empty = cert == [11, 0, 0, 5, 1, 0xaa, 0, 0, 0];
    }
    let want = finished_verify_data(client_hs, &transcript.hash());
    let fin = next(transcript, "expected client Finished", 20)?;
    if fin[4..] != want {
        return Err(TlsError::BadFinished);
    }
    res.client_finished = fin[4..].to_vec();
    Ok(())
}
