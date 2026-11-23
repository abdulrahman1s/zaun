//! The forward batch and header name (Messages/Batch.hs `batchElements`; Protocol.hs `fwdMemberName`, `maxForwardBatchLength`).

use crate::chat::batch::encode_binary_batch;
use crate::chat::limits::{MAX_BATCH_ELEMENT_COUNT, MAX_FORWARD_BATCH_LENGTH};

/// Protocol.hs `fwdMemberName`: at most 16 characters, then an ellipsis.
#[must_use]
pub(crate) fn fwd_member_name(name: &str) -> String {
    if name.chars().count() <= 16 {
        return name.to_owned();
    }
    let mut short: String = name.chars().take(16).collect();
    short.push('…');
    short
}

/// Messages/Batch.hs `batchElements`: in order, each batch within `MAX_FORWARD_BATCH_LENGTH`
/// and 255 elements; an element too large for a batch of its own is dropped and counted.
#[must_use]
pub(crate) fn forward_batches(elements: &[Vec<u8>]) -> (Vec<Vec<u8>>, usize) {
    let max = MAX_FORWARD_BATCH_LENGTH;
    let (mut batches, mut open, mut len, mut dropped) = (Vec::new(), Vec::new(), 0, 0);
    for el in elements {
        if el.len() + 4 > max {
            dropped += 1;
            continue;
        }
        let n = open.len() + 1;
        if n > MAX_BATCH_ELEMENT_COUNT || len + el.len() + n * 2 + 2 > max {
            batches.push(std::mem::take(&mut open));
            len = 0;
        }
        len += el.len();
        open.push(el.clone());
    }
    if !open.is_empty() {
        batches.push(open);
    }
    let encoded = batches
        .iter()
        .filter_map(|b| encode_binary_batch(b).ok())
        .collect();
    (encoded, dropped)
}
