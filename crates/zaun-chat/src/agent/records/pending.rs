//! A message waiting to be sent: the SEND body is built and its ratchet step stored before the first attempt, so a retry sends the same bytes (Agent/Store.hs `PendingMsgData`, AGPL-3.0-only).
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Large};

use crate::agent::client::AgentMsgId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendKind {
    /// The handshake confirmation, sent with the queue's e2e key in the public header.
    Confirmation,
    /// `A_MSG` from `send`: the only kind the engine hears about (`Sent`, `Merr`).
    Msg,
    Receipt,
    Qcont,
    /// Our `AgentRatchetKey` or `EREADY` during a ratchet re-sync.
    Resync,
    /// `QADD`, `QKEY`, `QUSE`, `QTEST`: a dropped one is reported (`qError`).
    Switch,
    /// `QEND`, and a message sent again on the new queue after a switch: never reported, as the engine heard of it already.
    Quiet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSend {
    pub id: AgentMsgId,
    pub kind: SendKind,
    /// The `ClientMsgEnvelope` to SEND.
    pub body: Vec<u8>,
    /// Wall-clock ms; the delivery deadlines count from here.
    pub created_at: i64,
}

impl SendKind {
    /// `MsgFlags.notification`: wake the peer's device for what a user would see.
    #[must_use]
    pub const fn notifies(self) -> bool {
        match self {
            Self::Confirmation | Self::Msg | Self::Resync => true,
            Self::Receipt | Self::Qcont | Self::Switch | Self::Quiet => false,
        }
    }

    const fn code(self) -> u8 {
        match self {
            Self::Confirmation => b'C',
            Self::Msg => b'M',
            Self::Receipt => b'V',
            Self::Qcont => b'Q',
            Self::Resync => b'R',
            Self::Switch => b'W',
            Self::Quiet => b'D',
        }
    }
}

impl Encode for PendingSend {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (
            self.id,
            self.kind.code(),
            Large(self.body.clone()),
            self.created_at,
        )
            .encode(out)
    }
}

impl Decode for PendingSend {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (id, code, Large(body), created_at) = <(AgentMsgId, u8, Large, i64)>::decode(d)?;
        let kind = [
            SendKind::Confirmation,
            SendKind::Msg,
            SendKind::Receipt,
            SendKind::Qcont,
            SendKind::Resync,
            SendKind::Switch,
            SendKind::Quiet,
        ]
        .into_iter()
        .find(|k| k.code() == code)
        .ok_or(DecodeError::BadTag("send kind", code))?;
        Ok(Self {
            id,
            kind,
            body,
            created_at,
        })
    }
}
