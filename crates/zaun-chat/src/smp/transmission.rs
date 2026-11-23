//! Transmissions and batches inside a block (Protocol.hs `tEncode*`, `batchTransmissions`, `transmissionP`; nook smp/transmission.ts).
//!
//! A block's payload is `count(1) ‖ Large(transmission) × count`. A client transmission is
//! `bs(authenticator) ‖ ['0' no service signature, v ≥ 16, only if authenticator ≠ ∅] ‖ bs(corrId) ‖ bs(entityId) ‖ command`.
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Large};

use super::version::SERVICE_CERTS;

/// The bytes a command's authenticator covers: `bs(sessionId) ‖ bs(corrId) ‖ bs(entityId) ‖ command`.
pub fn authorized(
    session_id: &[u8],
    corr_id: &[u8],
    entity_id: &[u8],
    command: &[u8],
) -> Result<Vec<u8>, EncodeError> {
    let mut out =
        Vec::with_capacity(session_id.len() + corr_id.len() + entity_id.len() + command.len() + 3);
    session_id.encode(&mut out)?;
    corr_id.encode(&mut out)?;
    entity_id.encode(&mut out)?;
    out.extend_from_slice(command);
    Ok(out)
}

/// One client transmission, the authenticator already computed (empty for unauthorised commands).
pub fn encode_transmission(
    v: u16,
    auth: &[u8],
    corr_id: &[u8],
    entity_id: &[u8],
    command: &[u8],
) -> Result<Vec<u8>, EncodeError> {
    let mut out =
        Vec::with_capacity(auth.len() + corr_id.len() + entity_id.len() + command.len() + 5);
    auth.encode(&mut out)?;
    if v >= SERVICE_CERTS && !auth.is_empty() {
        out.push(b'0');
    }
    corr_id.encode(&mut out)?;
    entity_id.encode(&mut out)?;
    out.extend_from_slice(command);
    Ok(out)
}

/// `Large` header bytes added per transmission, and the count byte of the batch.
const ENTRY_OVERHEAD: usize = 2;
const MAX_BATCH: usize = 255;

/// Greedy `batchTransmissions`: block payloads of at most `max` bytes, in order. A transmission that
/// cannot fit a block alone comes back as an index in the second list.
#[must_use]
pub fn pack(transmissions: &[Vec<u8>], max: usize) -> (Vec<Vec<u8>>, Vec<usize>) {
    let (mut payloads, mut too_large) = (Vec::new(), Vec::new());
    let mut current: Vec<u8> = vec![0];
    for (i, t) in transmissions.iter().enumerate() {
        let need = ENTRY_OVERHEAD + t.len();
        if 1 + need > max || t.len() > usize::from(u16::MAX) {
            too_large.push(i);
            continue;
        }
        if current.len() + need > max || usize::from(current[0]) == MAX_BATCH {
            payloads.push(std::mem::replace(&mut current, vec![0]));
        }
        current[0] += 1;
        current.extend_from_slice(&u16::try_from(t.len()).unwrap_or(u16::MAX).to_be_bytes());
        current.extend_from_slice(t);
    }
    if current[0] > 0 {
        payloads.push(current);
    }
    (payloads, too_large)
}

/// A transmission as the server sent it; `command` is the undecoded response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incoming {
    /// Empty in what a server sends; a client's authenticator, for a test server that checks it.
    pub auth: Vec<u8>,
    pub corr_id: Vec<u8>,
    pub entity_id: Vec<u8>,
    pub command: Vec<u8>,
}

/// `tParse` for a block's payload.
pub fn parse_payload(v: u16, payload: &[u8]) -> Result<Vec<Incoming>, DecodeError> {
    let mut d = Decoder::new(payload);
    let count = d.byte()?;
    let mut out = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let body = Large::decode(&mut d)?.0;
        let mut t = Decoder::new(&body);
        let auth = Vec::<u8>::decode(&mut t)?;
        if v >= SERVICE_CERTS && !auth.is_empty() {
            match t.byte()? {
                b'0' => {}
                b'1' => drop(Vec::<u8>::decode(&mut t)?),
                tag => return Err(DecodeError::BadTag("service signature", tag)),
            }
        }
        out.push(Incoming {
            auth,
            corr_id: Vec::decode(&mut t)?,
            entity_id: Vec::decode(&mut t)?,
            command: t.take_all().to_vec(),
        });
    }
    Ok(out)
}
