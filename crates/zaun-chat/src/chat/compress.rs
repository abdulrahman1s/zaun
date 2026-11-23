//! The `X` container: `'X' count (tag len body)*` (simplexmq Compression.hs, Protocol.hs `parseChatMessages`).
use super::bin::{Reader, put_large, put_short};
use super::error::{ChatError, Result, parse_err};
use super::limits::{MAX_BATCH_ELEMENT_COUNT, MAX_DECOMPRESSED_MSG_LENGTH, MAX_LENGTH_PASSTHROUGH};
use super::zstd_frame::{compress, content_size, decompress_limited};

enum Part<'a> {
    Passthrough(&'a [u8]),
    Zstd(&'a [u8]),
}

/// `compressedBatchMsgBody_`: one part, passthrough up to 180 bytes, else a zstd frame.
pub fn compress_body(body: &[u8]) -> Result<Vec<u8>> {
    let mut out = vec![b'X', 1];
    if body.len() <= MAX_LENGTH_PASSTHROUGH {
        out.push(b'0');
        put_short(&mut out, body)?;
    } else {
        out.push(b'1');
        put_large(&mut out, &compress(body)?)?;
    }
    Ok(out)
}

/// `compressToLimit`: leave the body alone when it fits, else compress; fail when even that does not fit.
pub fn compress_to_limit(body: &[u8], max_len: usize) -> Result<Vec<u8>> {
    if body.len() <= max_len {
        return Ok(body.to_vec());
    }
    let compressed = compress_body(body)?;
    if compressed.len() <= max_len {
        Ok(compressed)
    } else {
        Err(ChatError::TooLarge)
    }
}

fn read_parts(body: &[u8]) -> Result<Vec<Part<'_>>> {
    let mut r = Reader(body);
    let count = r.u8()?;
    if count == 0 {
        return parse_err("empty list");
    }
    let mut parts = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        parts.push(match r.u8()? {
            b'0' => Part::Passthrough(r.short_bytes()?),
            b'1' => Part::Zstd(r.large()?),
            t => return parse_err(format!("unknown Compressed tag: {}", char::from(t))),
        });
    }
    r.finish()?;
    Ok(parts)
}

/// `decodeCompressed`: `body` is what follows the `X`. Sizes are checked from the headers, before any decoding.
pub fn decompress_container(body: &[u8]) -> Result<Vec<Vec<u8>>> {
    let parts = read_parts(body)?;
    let mut total = 0u64;
    for part in &parts {
        let size = match part {
            Part::Passthrough(b) => b.len() as u64,
            Part::Zstd(f) => content_size(f)
                .ok_or_else(|| ChatError::Compression("compressed size not specified".into()))?,
        };
        if size > MAX_DECOMPRESSED_MSG_LENGTH as u64 {
            return Err(ChatError::Limit("decompressed size exceeds limit"));
        }
        total += size;
    }
    if total > MAX_DECOMPRESSED_MSG_LENGTH as u64 || parts.len() > MAX_BATCH_ELEMENT_COUNT {
        return Err(ChatError::Limit("decompressed size exceeds limit"));
    }
    parts
        .iter()
        .map(|p| match p {
            Part::Passthrough(b) => Ok(b.to_vec()),
            Part::Zstd(f) => decompress_limited(f, MAX_DECOMPRESSED_MSG_LENGTH),
        })
        .collect()
}
