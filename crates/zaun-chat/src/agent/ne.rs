//! Non-empty lists with a one-byte count, held as plain vectors (Encoding.hs `NonEmpty`, AGPL-3.0-only).
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError};

pub(crate) fn encode_ne<T: Encode>(items: &[T], out: &mut Vec<u8>) -> Result<(), EncodeError> {
    if items.is_empty() {
        return Err(EncodeError::EmptyList);
    }
    let count = u8::try_from(items.len()).map_err(|_| EncodeError::TooLong {
        what: "list",
        len: items.len(),
        max: 255,
    })?;
    out.push(count);
    items.iter().try_for_each(|x| x.encode(out))
}

pub(crate) fn decode_ne<T: Decode>(d: &mut Decoder<'_>) -> Result<Vec<T>, DecodeError> {
    match d.byte()? {
        0 => Err(DecodeError::EmptyList),
        count => (0..count).map(|_| T::decode(d)).collect(),
    }
}
