//! One command or response as a padded block (Protocol.hs `xftpEncodeAuthTransmission`, `xftpDecodeTClient`; nook xftp/transmission.ts).
use subtle::ConstantTimeEq;

use crate::crypto::{PrivateKey, pad, unpad};
use crate::encoding::{Decode, Decoder, Encode, Large};

use super::error::XftpError;
use super::error_type::XftpErrorType;
use super::version::BLOCK_SIZE;

/// `[1] | Large(authenticator | sessionId | corrId | entityId | command)`, padded. The authenticator is an Ed25519
/// signature over everything after it, or empty. The correlation id is empty: one request is in flight at a time.
pub fn encode_transmission(
    session_id: &[u8],
    entity_id: &[u8],
    command: &[u8],
    key: Option<&PrivateKey>,
) -> Result<Vec<u8>, XftpError> {
    let mut for_auth = Vec::new();
    session_id.encode(&mut for_auth)?;
    [0u8; 0].encode(&mut for_auth)?;
    entity_id.encode(&mut for_auth)?;
    for_auth.extend_from_slice(command);
    let auth = match key {
        Some(k) => k.sign(&for_auth)?.as_bytes().to_vec(),
        None => Vec::new(),
    };
    let mut payload = Vec::new();
    auth.encode(&mut payload)?;
    payload.extend_from_slice(&for_auth);
    let mut batch = vec![1u8];
    Large(payload).encode(&mut batch)?;
    Ok(pad(&batch, BLOCK_SIZE)?)
}

/// The entity id and command of a response block. The server may answer with only an error word
/// (`HANDSHAKE`, `SESSION`), which comes back as `Server`.
pub fn decode_transmission(
    session_id: &[u8],
    block: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), XftpError> {
    let raw = unpad(block)?;
    if raw.len() < 20
        && raw.iter().all(|b| b.is_ascii_uppercase() || *b == b'_')
        && let Some(e) = XftpErrorType::parse(raw)
    {
        return Err(XftpError::Server(e));
    }
    let mut d = Decoder::new(raw);
    if d.byte()? != 1 {
        return Err(XftpError::Unexpected("response batch size"));
    }
    let payload = Large::decode(&mut d)?.0;
    let mut t = Decoder::new(&payload);
    let _authenticator = Vec::<u8>::decode(&mut t)?;
    let session = Vec::<u8>::decode(&mut t)?;
    if !bool::from(session.as_slice().ct_eq(session_id)) {
        return Err(XftpError::Unexpected("session id in response"));
    }
    let _corr_id = Vec::<u8>::decode(&mut t)?;
    let entity = Vec::<u8>::decode(&mut t)?;
    Ok((entity, t.take_all().to_vec()))
}
