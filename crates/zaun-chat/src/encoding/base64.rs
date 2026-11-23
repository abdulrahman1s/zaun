//! base64url as Haskell `U.encode` / `strP` (Encoding/String.hs) and nook `base64urlEncode`.
use super::error::DecodeError;

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// `padded` writes trailing '=' as `U.encode` does; protocol links use the unpadded form.
#[must_use]
pub fn b64_encode(bytes: &[u8], padded: bool) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        let chars = chunk.len() + 1;
        for i in 0..4 {
            if i < chars {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
            } else if padded {
                out.push('=');
            }
        }
    }
    out
}

fn sextet(c: u8) -> Result<u32, DecodeError> {
    let v = match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'-' | b'+' => 62,
        b'_' | b'/' => 63,
        _ => return Err(DecodeError::Base64),
    };
    Ok(u32::from(v))
}

/// Accepts padded and unpadded input, and the standard alphabet's '+' and '/' as nook does.
pub fn b64_decode(s: &str) -> Result<Vec<u8>, DecodeError> {
    let digits = s.trim_end_matches('=').as_bytes();
    if digits.len() % 4 == 1 {
        return Err(DecodeError::Base64);
    }
    let mut out = Vec::with_capacity(digits.len() * 3 / 4);
    for chunk in digits.chunks(4) {
        let n = chunk
            .iter()
            .try_fold(0u32, |n, c| Ok::<_, DecodeError>(n << 6 | sextet(*c)?))?;
        let n = n << (6 * (4 - chunk.len()));
        out.extend_from_slice(&n.to_be_bytes()[1..chunk.len()]);
    }
    Ok(out)
}
