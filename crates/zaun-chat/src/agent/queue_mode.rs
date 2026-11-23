//! Queue mode: messaging (sender secures) or contact (Protocol.hs `QueueMode`, `queueModeP`, AGPL-3.0-only).
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QueueMode {
    Messaging,
    Contact,
}

/// `senderCanSecure`.
#[must_use]
pub fn sender_can_secure(mode: Option<QueueMode>) -> bool {
    mode == Some(QueueMode::Messaging)
}

impl Encode for QueueMode {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        out.push(match self {
            Self::Messaging => b'M',
            Self::Contact => b'C',
        });
        Ok(())
    }
}

impl Decode for QueueMode {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        match d.byte()? {
            b'M' => Ok(Self::Messaging),
            b'C' => Ok(Self::Contact),
            t => Err(DecodeError::BadTag("QueueMode", t)),
        }
    }
}

/// `queueModeP`: the optional tail of a queue address, as `M`/`C` or as the older Bool `T`/`F`.
pub(super) fn decode_tail(d: &mut Decoder<'_>) -> Option<QueueMode> {
    let mode = match d.clone().byte() {
        Ok(b'M' | b'T') => QueueMode::Messaging,
        Ok(b'C' | b'F') => QueueMode::Contact,
        _ => return None,
    };
    d.byte().ok()?;
    Some(mode)
}
