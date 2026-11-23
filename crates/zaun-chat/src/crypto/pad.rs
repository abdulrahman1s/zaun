//! Message padding: 2-byte length, message, '#' fill; and the 8-byte-length lazy form (Crypto.hs `pad`, Crypto/Lazy.hs `pad`).
use super::CryptoError;

const FILL: u8 = b'#';

/// Largest message `pad` accepts (2^16 - 3).
pub const MAX_MSG_LEN: usize = 65533;

#[must_use]
pub fn can_pad(msg_len: usize, padded_len: usize) -> bool {
    msg_len <= MAX_MSG_LEN && padded_len >= msg_len + 2
}

pub fn pad(msg: &[u8], padded_len: usize) -> Result<Vec<u8>, CryptoError> {
    if !can_pad(msg.len(), padded_len) {
        return Err(CryptoError::LargeMsg);
    }
    let len = u16::try_from(msg.len()).map_err(|_| CryptoError::LargeMsg)?;
    let mut out = Vec::with_capacity(padded_len);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(msg);
    out.resize(padded_len, FILL);
    Ok(out)
}

pub fn unpad(padded: &[u8]) -> Result<&[u8], CryptoError> {
    let (len, rest) = padded
        .split_first_chunk::<2>()
        .ok_or(CryptoError::InvalidMsg)?;
    rest.get(..usize::from(u16::from_be_bytes(*len)))
        .ok_or(CryptoError::InvalidMsg)
}

/// Lazy form for files: an 8-byte Int64 length, the first `msg_len` bytes of `msg`, '#' fill.
pub fn pad_lazy(msg: &[u8], msg_len: usize, padded_len: usize) -> Result<Vec<u8>, CryptoError> {
    let content = msg.get(..msg_len).ok_or(CryptoError::InvalidMsg)?;
    if padded_len < msg_len + 8 {
        return Err(CryptoError::LargeMsg);
    }
    let len = i64::try_from(msg_len).map_err(|_| CryptoError::LargeMsg)?;
    let mut out = Vec::with_capacity(padded_len);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(content);
    out.resize(padded_len, FILL);
    Ok(out)
}

/// `splitLen`: the declared length and the content. Content shorter than declared is not an
/// error, so a chunk of a longer stream can be split.
pub fn split_len(padded: &[u8]) -> Result<(u64, &[u8]), CryptoError> {
    let (len, rest) = padded
        .split_first_chunk::<8>()
        .ok_or(CryptoError::InvalidMsg)?;
    let len = u64::try_from(i64::from_be_bytes(*len)).map_err(|_| CryptoError::InvalidMsg)?;
    let take = usize::try_from(len).map_or(rest.len(), |n| n.min(rest.len()));
    Ok((len, &rest[..take]))
}

pub fn unpad_lazy(padded: &[u8]) -> Result<&[u8], CryptoError> {
    split_len(padded).map(|(_, content)| content)
}
