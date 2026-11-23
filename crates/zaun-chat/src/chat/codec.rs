//! Top level: encode a message for sending, parse anything that arrives (Protocol.hs `encodeChatMessage`, `parseChatMessages`).
use super::batch::{parse_binary_batch, parse_json_batch};
use super::compress::{compress_to_limit, decompress_container};
use super::element::{ParsedMsg, parse_element};
use super::error::{ChatError, Result, parse_err};
use super::limits::{MAX_BATCH_ELEMENT_COUNT, MAX_ENCODED_MSG_LENGTH, MAX_ENCODED_MSG_LENGTH_PQ};
use super::message::ChatMessage;

/// The body to hand to the agent: JSON, or an `X` container when it is over the limit
/// (13380 with PQ, else 15602). Fails with `TooLarge` when even the compressed body does not fit.
pub fn encode_chat_message(msg: &ChatMessage, pq: bool) -> Result<Vec<u8>> {
    let max = if pq {
        MAX_ENCODED_MSG_LENGTH_PQ
    } else {
        MAX_ENCODED_MSG_LENGTH
    };
    compress_to_limit(&msg.to_json()?, max)
}

/// Every element of a received body, each with its own result. The `X` container is unwrapped,
/// `[` and `=` batches are split, and the total is capped at 255 elements.
pub fn parse_chat_elements(msg: &[u8]) -> Vec<Result<ParsedMsg>> {
    let Some((&first, rest)) = msg.split_first() else {
        return vec![parse_err("empty string")];
    };
    let out = match first {
        b'X' => match decompress_container(rest) {
            Ok(bodies) => bodies.iter().flat_map(|b| parse_uncompressed(b)).collect(),
            Err(e) => vec![Err(e)],
        },
        _ => parse_uncompressed(msg),
    };
    if out.len() > MAX_BATCH_ELEMENT_COUNT {
        return vec![Err(ChatError::Limit("too many messages in batch"))];
    }
    out
}

fn parse_uncompressed(s: &[u8]) -> Vec<Result<ParsedMsg>> {
    match s.split_first() {
        None => vec![parse_err("empty string")],
        Some((b'[', _)) => parse_json_batch(s),
        Some((b'=', rest)) => parse_binary_batch(rest),
        Some(_) => vec![parse_element(s)],
    }
}

/// The messages of a received body; the first bad element fails the whole call. Use
/// [`parse_chat_elements`] to keep signatures and forward headers, or to survive a bad element.
pub fn parse_chat_messages(msg: &[u8]) -> Result<Vec<ChatMessage>> {
    parse_chat_elements(msg)
        .into_iter()
        .map(|e| e.map(|p| p.message))
        .collect()
}
