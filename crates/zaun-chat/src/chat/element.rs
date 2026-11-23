//! One batch element: `{` JSON, `/` signed, `>` forwarded, `F` file chunk (Protocol.hs `elementP`).
use super::bin::Reader;
use super::error::{Result, parse_err};
use super::event::{FileChunk, encode_chunk, parse_chunk};
use super::forward::GrpMsgForward;
use super::ids::SharedMsgId;
use super::message::ChatMessage;
use super::signed::SignedMsg;

/// A parsed element with whatever envelope it came in.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedMsg {
    pub forward: Option<GrpMsgForward>,
    /// Present for a `/` element; its `signed_body` holds the exact bytes the signature covers.
    pub signed: Option<SignedMsg>,
    pub message: ChatMessage,
}

pub fn parse_element(s: &[u8]) -> Result<ParsedMsg> {
    element(s, None)
}

fn element(s: &[u8], forward: Option<GrpMsgForward>) -> Result<ParsedMsg> {
    let Some((&first, rest)) = s.split_first() else {
        return parse_err("empty string");
    };
    match first {
        b'/' => {
            let signed = SignedMsg::read(&mut Reader(rest))?;
            let message = ChatMessage::from_json(&signed.signed_body)?;
            Ok(ParsedMsg {
                forward,
                signed: Some(signed),
                message,
            })
        }
        b'>' => {
            if forward.is_some() {
                return parse_err("nested forward elements not supported");
            }
            let mut r = Reader(rest);
            let fwd = GrpMsgForward::read(&mut r)?;
            element(r.rest(), Some(fwd))
        }
        b'{' => Ok(ParsedMsg {
            forward,
            signed: None,
            message: ChatMessage::from_json(s)?,
        }),
        b'F' => Ok(ParsedMsg {
            forward,
            signed: None,
            message: ChatMessage {
                v: None,
                msg_id: None,
                event: parse_chunk(s)?,
            },
        }),
        c => parse_err(format!("invalid element prefix: {:?}", char::from(c))),
    }
}

/// `'/'` + the signature envelope and its body.
pub fn encode_signed_element(signed: &SignedMsg) -> Result<Vec<u8>> {
    let mut out = vec![b'/'];
    out.extend_from_slice(&signed.encode()?);
    Ok(out)
}

/// `'>'` + the forward header + the element it forwards.
pub fn encode_forward_element(fwd: &GrpMsgForward, element: &[u8]) -> Result<Vec<u8>> {
    let mut out = vec![b'>'];
    out.extend_from_slice(&fwd.encode()?);
    out.extend_from_slice(element);
    Ok(out)
}

/// An inline file chunk element (`F`).
pub fn encode_file_chunk_element(msg_id: &SharedMsgId, chunk: &FileChunk) -> Result<Vec<u8>> {
    encode_chunk(msg_id, chunk)
}
