//! Tuples of 2 to 8 encodable values: the fields back to back (Encoding.hs).
use super::{
    Decode, Decoder, Encode,
    error::{DecodeError, EncodeError},
};

macro_rules! tuple {
    ($($t:ident $i:tt),+) => {
        impl<$($t: Encode),+> Encode for ($($t,)+) {
            fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
                $(self.$i.encode(out)?;)+
                Ok(())
            }
        }
        impl<$($t: Decode),+> Decode for ($($t,)+) {
            fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
                Ok(($($t::decode(d)?,)+))
            }
        }
    };
}

tuple!(A 0, B 1);
tuple!(A 0, B 1, C 2);
tuple!(A 0, B 1, C 2, D 3);
tuple!(A 0, B 1, C 2, D 3, E 4);
tuple!(A 0, B 1, C 2, D 3, E 4, F 5);
tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6);
tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);
