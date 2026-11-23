//! The Finished messages and the client's closing flight (RFC 8446 section 4.4.4).
use subtle::ConstantTimeEq;

use super::super::error::TlsError;
use super::super::key_schedule::{Secret, Transcript, finished_verify_data};
use super::super::record::{HANDSHAKE, Protection};
use super::super::wire::{handshake_message, put_u24};
use super::server_flight::{CERTIFICATE, FINISHED};

pub(super) fn check_server_finished(
    server_secret: &Secret,
    transcript: &[u8; 32],
    body: &[u8],
) -> Result<(), TlsError> {
    let expected = finished_verify_data(server_secret, transcript);
    if bool::from(body.ct_eq(&expected)) {
        Ok(())
    } else {
        Err(TlsError::BadFinished)
    }
}

/// Records for [empty Certificate if requested] and Finished, plus the Finished verify_data.
pub(super) fn client_flight(
    request_context: Option<&[u8]>,
    transcript: &mut Transcript,
    client_secret: &Secret,
    tx: &mut Protection,
) -> Result<(Vec<u8>, [u8; 32]), TlsError> {
    let mut out = Vec::new();
    if let Some(context) = request_context {
        let mut body = vec![context.len() as u8];
        body.extend_from_slice(context);
        put_u24(&mut body, 0); // an empty certificate_list
        let certificate = handshake_message(CERTIFICATE, &body);
        transcript.add(&certificate);
        out.extend(tx.seal(HANDSHAKE, &certificate)?);
    }
    let verify_data = finished_verify_data(client_secret, &transcript.hash());
    let finished = handshake_message(FINISHED, &verify_data);
    transcript.add(&finished);
    out.extend(tx.seal(HANDSHAKE, &finished)?);
    Ok((out, verify_data))
}
