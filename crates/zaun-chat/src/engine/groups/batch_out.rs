//! Several events to one member in as few bodies as fit (Messages/Batch.hs `batchMessages`, `batchLen`): a JSON array, or `=` from chat v18 (Internal.hs `batchSendConnMessages`).

use crate::chat::batch::{encode_binary_batch, encode_json_batch};
use crate::chat::limits::{MAX_BATCH_ELEMENT_COUNT, MAX_ENCODED_MSG_LENGTH};
use crate::engine::error::Result;

const fn batch_len(binary: bool, len: usize, n: usize) -> usize {
    match n {
        0 => 0,
        1 => len,
        _ if binary => len + n * 2 + 2,
        _ => len + n + 1,
    }
}

fn close(binary: bool, elements: &[Vec<u8>]) -> Result<Vec<u8>> {
    match elements {
        [one] => Ok(one.clone()),
        _ if binary => Ok(encode_binary_batch(elements)?),
        _ => Ok(encode_json_batch(elements)?),
    }
}

/// The bodies to send, in order, and how many elements were too large for any.
pub(crate) fn batch_bodies(elements: Vec<Vec<u8>>, binary: bool) -> Result<(Vec<Vec<u8>>, usize)> {
    let (mut bodies, mut open, mut len, mut dropped) = (Vec::new(), Vec::new(), 0, 0);
    for e in elements {
        let n = open.len() + 1;
        if n <= MAX_BATCH_ELEMENT_COUNT
            && batch_len(binary, len + e.len(), n) <= MAX_ENCODED_MSG_LENGTH
        {
            len += e.len();
            open.push(e);
        } else if e.len() <= MAX_ENCODED_MSG_LENGTH {
            if !open.is_empty() {
                bodies.push(close(binary, &open)?);
            }
            len = e.len();
            open = vec![e];
        } else {
            dropped += 1;
        }
    }
    if !open.is_empty() {
        bodies.push(close(binary, &open)?);
    }
    Ok((bodies, dropped))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_element_goes_bare_and_more_go_in_the_peers_format() {
        let e = |c: u8| vec![b'{', c, b'}'];
        let (one, _) = batch_bodies(vec![e(b'a')], true).unwrap();
        assert_eq!(one, [e(b'a')]);
        let (json, _) = batch_bodies(vec![e(b'a'), e(b'b')], false).unwrap();
        assert_eq!(json, [b"[{a},{b}]".to_vec()]);
        let (bin, _) = batch_bodies(vec![e(b'a'), e(b'b')], true).unwrap();
        assert_eq!(bin[0][..2], [b'=', 2]);
    }

    #[test]
    fn a_batch_closes_before_it_passes_the_message_limit_and_a_too_large_element_is_dropped() {
        let big = vec![b'x'; MAX_ENCODED_MSG_LENGTH / 2];
        let huge = vec![b'x'; MAX_ENCODED_MSG_LENGTH + 1];
        let (bodies, dropped) =
            batch_bodies(vec![big.clone(), big.clone(), huge, big], false).unwrap();
        assert_eq!(dropped, 1);
        assert_eq!(bodies.len(), 3);
        assert!(bodies.iter().all(|b| b.len() <= MAX_ENCODED_MSG_LENGTH));
    }
}
