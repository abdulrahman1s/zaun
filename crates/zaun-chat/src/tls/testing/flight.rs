//! The mock server's encrypted flight: EncryptedExtensions through Finished, with faults on request.
use super::options::MockOpts;
use crate::crypto::KeyPair;
use crate::tls::error::TlsError;
use crate::tls::key_schedule::{
    Secret, Transcript, certificate_verify_input, finished_verify_data,
};
use crate::tls::verify::{SIG_ED448, SIG_ED25519};
use crate::tls::wire::{extension, handshake_message, put_u16, put_u24};

pub(super) fn server_flight(
    o: &MockOpts,
    leaf: &KeyPair,
    chain: &[Vec<u8>],
    server_hs: &Secret,
    transcript: &mut Transcript,
) -> Result<Vec<Vec<u8>>, TlsError> {
    let mut msgs = Vec::new();
    let mut add = |m: Vec<u8>, t: &mut Transcript| {
        t.add(&m);
        msgs.push(m);
    };
    let mut ee = Vec::new();
    if let Some(name) = o.alpn {
        let mut list = vec![name.len() as u8];
        list.extend_from_slice(name.as_bytes());
        let mut data = Vec::new();
        put_u16(&mut data, list.len());
        data.extend(list);
        ee = extension(16, &data);
    }
    let mut body = Vec::new();
    put_u16(&mut body, ee.len());
    body.extend(ee);
    add(handshake_message(8, &body), transcript);
    if o.cert_request {
        add(handshake_message(13, &[1, 0xaa, 0, 0]), transcript);
    }
    let chain = if o.empty_chain { &[][..] } else { chain };
    let mut list = Vec::new();
    for cert in chain {
        put_u24(&mut list, cert.len());
        list.extend_from_slice(cert);
        put_u16(&mut list, 0);
    }
    let mut body = vec![0];
    put_u24(&mut body, list.len());
    body.extend(list);
    add(handshake_message(11, &body), transcript);

    let input = certificate_verify_input(&transcript.hash(), true);
    let mut sig = leaf.private.sign(&input)?.as_bytes().to_vec();
    if o.bad_sig {
        sig[3] ^= 1;
    }
    let scheme = if leaf.public.as_bytes().len() == 57 {
        SIG_ED448
    } else {
        SIG_ED25519
    };
    let mut body = scheme.to_be_bytes().to_vec();
    put_u16(&mut body, sig.len());
    body.extend(sig);
    add(handshake_message(15, &body), transcript);

    let mut verify_data = finished_verify_data(server_hs, &transcript.hash());
    if o.bad_finished {
        verify_data[0] ^= 1;
    }
    add(handshake_message(20, &verify_data), transcript);
    Ok(msgs)
}
