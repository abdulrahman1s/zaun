//! Where a connection is in the invitation handshake; each step that waits on a server is re-sent from here after a restart (HANDSHAKE.md §1–§5).
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnStatus {
    /// Inviter: NEW of the invitation queue is outstanding.
    Creating,
    /// Inviter: the link is out; waiting for the joiner's confirmation.
    Invited,
    /// Inviter: `Conf` was emitted; waiting for `allow`.
    Confirmed,
    /// Inviter: SKEY on the joiner's reply queue is outstanding.
    Allowing,
    /// Inviter: our confirmation is queued; `Con` when the server takes it.
    Accepting,
    /// Joiner: LKEY of the short link is outstanding.
    Resolving,
    /// Joiner: SKEY on the inviter's queue is outstanding.
    Securing,
    /// Joiner: NEW of our reply queue is outstanding.
    Replying,
    /// Joiner: our confirmation is queued or sent; waiting for the inviter's.
    Joined,
    Active,
}

impl ConnStatus {
    const ALL: [Self; 10] = [
        Self::Creating,
        Self::Invited,
        Self::Confirmed,
        Self::Allowing,
        Self::Accepting,
        Self::Resolving,
        Self::Securing,
        Self::Replying,
        Self::Joined,
        Self::Active,
    ];

    const fn code(self) -> u8 {
        match self {
            Self::Creating => b'c',
            Self::Invited => b'i',
            Self::Confirmed => b'f',
            Self::Allowing => b'l',
            Self::Accepting => b'p',
            Self::Resolving => b'r',
            Self::Securing => b's',
            Self::Replying => b'y',
            Self::Joined => b'j',
            Self::Active => b'a',
        }
    }
}

impl Encode for ConnStatus {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        out.push(self.code());
        Ok(())
    }
}

impl Decode for ConnStatus {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let b = d.byte()?;
        Self::ALL
            .into_iter()
            .find(|s| s.code() == b)
            .ok_or(DecodeError::BadTag("connection status", b))
    }
}
