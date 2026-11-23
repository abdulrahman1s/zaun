//! Maybe ('0' / '1'+value), lists and NonEmpty with a 1-byte count (Encoding.hs `smpEncodeList`, `EncList`).
use super::{
    Decode, Decoder, Encode,
    error::{DecodeError, EncodeError},
};

/// `EncList`: a 1-byte count, then the items. A plain `Vec<u8>` is a ByteString, so lists get a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct List<T>(pub Vec<T>);

/// A list that is never empty; decoding a zero count fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonEmpty<T>(Vec<T>);

impl<T> NonEmpty<T> {
    #[must_use]
    pub fn new(items: Vec<T>) -> Option<Self> {
        (!items.is_empty()).then_some(Self(items))
    }

    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }

    #[must_use]
    pub fn into_vec(self) -> Vec<T> {
        self.0
    }
}

fn encode_items<T: Encode>(items: &[T], out: &mut Vec<u8>) -> Result<(), EncodeError> {
    let count = u8::try_from(items.len()).map_err(|_| EncodeError::TooLong {
        what: "list",
        len: items.len(),
        max: 255,
    })?;
    out.push(count);
    items.iter().try_for_each(|x| x.encode(out))
}

fn decode_items<T: Decode>(d: &mut Decoder<'_>, count: u8) -> Result<Vec<T>, DecodeError> {
    (0..count).map(|_| T::decode(d)).collect()
}

impl<T: Encode> Encode for List<T> {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        encode_items(&self.0, out)
    }
}

impl<T: Decode> Decode for List<T> {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let count = d.byte()?;
        decode_items(d, count).map(List)
    }
}

impl<T: Encode> Encode for NonEmpty<T> {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        encode_items(&self.0, out)
    }
}

impl<T: Decode> Decode for NonEmpty<T> {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        match d.byte()? {
            0 => Err(DecodeError::EmptyList),
            count => decode_items(d, count).map(NonEmpty),
        }
    }
}

impl<T: Encode> Encode for Option<T> {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        match self {
            None => out.push(b'0'),
            Some(v) => {
                out.push(b'1');
                v.encode(out)?;
            }
        }
        Ok(())
    }
}

impl<T: Decode> Decode for Option<T> {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        match d.byte()? {
            b'0' => Ok(None),
            b'1' => T::decode(d).map(Some),
            t => Err(DecodeError::BadTag("Maybe", t)),
        }
    }
}
