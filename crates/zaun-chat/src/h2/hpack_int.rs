//! HPACK prefix integers (RFC 7541 section 5.1) and the decoding error.
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("hpack: {0}")]
pub struct HpackError(pub &'static str);

/// Appends `value` with an `prefix_bits`-bit prefix; `flags` are the bits above the prefix.
pub fn encode_int(value: usize, prefix_bits: u8, flags: u8, out: &mut Vec<u8>) {
    let max = (1usize << prefix_bits) - 1;
    if value < max {
        out.push(flags | value as u8);
        return;
    }
    out.push(flags | max as u8);
    let mut rest = value - max;
    while rest >= 128 {
        out.push((rest % 128) as u8 | 0x80);
        rest /= 128;
    }
    out.push(rest as u8);
}

pub fn decode_int(buf: &[u8], pos: &mut usize, prefix_bits: u8) -> Result<usize, HpackError> {
    let max = (1usize << prefix_bits) - 1;
    let first = *buf.get(*pos).ok_or(HpackError("truncated integer"))?;
    *pos += 1;
    let mut value = usize::from(first) & max;
    if value < max {
        return Ok(value);
    }
    let mut shift = 0u32;
    loop {
        let b = *buf.get(*pos).ok_or(HpackError("truncated integer"))?;
        *pos += 1;
        value += usize::from(b & 0x7f) << shift;
        shift += 7;
        if value > 1 << 31 {
            return Err(HpackError("integer too large"));
        }
        if b & 0x80 == 0 {
            return Ok(value);
        }
        if shift > 35 {
            return Err(HpackError("integer too long"));
        }
    }
}
