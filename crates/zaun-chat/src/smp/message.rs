//! The server-to-recipient layer of MSG (Protocol.hs `clientRcvMsgBodyP`, Agent `decryptMessage`; nook smp/protocol.ts `decryptRcvMsg`).
use crate::crypto::{CbNonce, DhSecret, PrivateKey, PublicKey, cb_decrypt, dh};
use crate::encoding::{Decode, DecodeError, Decoder, SysTime};

use super::error::SmpError;

/// What the server put in a queue: a message, or the notice that the queue was full and is draining.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RcvMsg {
    Message {
        ts: SysTime,
        notification: bool,
        body: Vec<u8>,
    },
    Quota {
        ts: SysTime,
    },
}

/// The queue's DH secret: the server's key from IDS with our `NEW` DH private key.
pub fn rcv_dh_secret(
    server_dh_key: &PublicKey,
    rcv_dh_private: &PrivateKey,
) -> Result<DhSecret, SmpError> {
    Ok(dh(server_dh_key, rcv_dh_private)?)
}

/// Opens a MSG body: nonce is the 24-byte message id, the box is padded.
pub fn decrypt_message(secret: &DhSecret, msg_id: &[u8], body: &[u8]) -> Result<RcvMsg, SmpError> {
    let nonce = CbNonce::from_slice(msg_id)?;
    Ok(parse_rcv_body(&cb_decrypt(secret, &nonce, body)?)?)
}

/// `ts(Int64) ‖ flags('T'|'F' …) ‖ ' ' ‖ body`, or `QUOTA ‖ ' ' ‖ ts`.
pub fn parse_rcv_body(plain: &[u8]) -> Result<RcvMsg, DecodeError> {
    if let Some(rest) = plain.strip_prefix(b"QUOTA ") {
        return Ok(RcvMsg::Quota {
            ts: SysTime::from_bytes(rest)?,
        });
    }
    let mut d = Decoder::new(plain);
    let ts = SysTime::decode(&mut d)?;
    let notification = bool::decode(&mut d)?;
    // MsgFlags skips any further flag characters up to the separating space.
    while d.remaining() > 0 && plain[d.offset()] != b' ' {
        d.byte()?;
    }
    if d.byte()? != b' ' {
        return Err(DecodeError::Invalid("message separator"));
    }
    Ok(RcvMsg::Message {
        ts,
        notification,
        body: d.take_all().to_vec(),
    })
}
