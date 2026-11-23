//! JSON array batches and the binary `=` batch (Protocol.hs `parseChatMessages`, Messages/Batch.hs `encodeBatch`).
use super::bin::{Reader, put_large};
use super::element::{ParsedMsg, parse_element};
use super::error::{ChatError, Result};
use super::limits::MAX_BATCH_ELEMENT_COUNT;
use super::message::ChatMessage;
use serde_json::Value;

/// `[a,b]`; a single element goes out bare (`BMJson`). JSON batches carry no signatures.
pub fn encode_json_batch(elements: &[Vec<u8>]) -> Result<Vec<u8>> {
    if elements.len() > MAX_BATCH_ELEMENT_COUNT {
        return Err(ChatError::Limit("too many messages in batch"));
    }
    if let [one] = elements {
        return Ok(one.clone());
    }
    let mut out = vec![b'['];
    for (i, e) in elements.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        out.extend_from_slice(e);
    }
    out.push(b']');
    Ok(out)
}

/// `'='`, a count byte, then each element behind a two-byte length (`BMBinary`).
pub fn encode_binary_batch(elements: &[Vec<u8>]) -> Result<Vec<u8>> {
    let count =
        u8::try_from(elements.len()).map_err(|_| ChatError::Limit("too many messages in batch"))?;
    let mut out = vec![b'=', count];
    for e in elements {
        put_large(&mut out, e)?;
    }
    Ok(out)
}

/// The items of a `[` body, each parsed on its own; a bad item does not hide the others.
pub fn parse_json_batch(s: &[u8]) -> Vec<Result<ParsedMsg>> {
    match serde_json::from_slice::<Vec<Value>>(s) {
        Ok(items) => items.iter().map(item).collect(),
        Err(e) => vec![Err(e.into())],
    }
}

fn item(v: &Value) -> Result<ParsedMsg> {
    Ok(ParsedMsg {
        forward: None,
        signed: None,
        message: ChatMessage::from_value(v, 0)?,
    })
}

/// `s` is what follows the `=`.
pub fn parse_binary_batch(s: &[u8]) -> Vec<Result<ParsedMsg>> {
    let read = || -> Result<Vec<&[u8]>> {
        let mut r = Reader(s);
        let count = r.u8()?;
        let items = (0..count).map(|_| r.large()).collect::<Result<Vec<_>>>()?;
        r.finish()?;
        Ok(items)
    };
    match read() {
        Ok(items) => items.into_iter().map(parse_element).collect(),
        Err(e) => vec![Err(e)],
    }
}
