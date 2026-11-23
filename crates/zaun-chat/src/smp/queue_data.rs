//! Queue creation and short-link data types of NEW, LSET, LNK (Protocol.hs `QueueReqData`, `QueueMode`, `SubscriptionMode`, `QueueLinkData`).
use crate::crypto::PublicKey;
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Large};

/// `SubscriptionMode`: whether NEW also subscribes the new queue on this connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionMode {
    Subscribe,
    OnlyCreate,
}

impl Encode for SubscriptionMode {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        out.push(match self {
            Self::Subscribe => b'S',
            Self::OnlyCreate => b'C',
        });
        Ok(())
    }
}

/// `QueueMode`: a queue the sender may secure (messaging, SKEY) or a contact-address queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueMode {
    Messaging,
    Contact,
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

/// `(EncFixedDataBytes, EncUserDataBytes)`: two `Large` blobs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueLinkData {
    pub fixed: Vec<u8>,
    pub user: Vec<u8>,
}

impl Encode for QueueLinkData {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        Large(self.fixed.clone()).encode(out)?;
        Large(self.user.clone()).encode(out)
    }
}

impl Decode for QueueLinkData {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            fixed: Large::decode(d)?.0,
            user: Large::decode(d)?.0,
        })
    }
}

/// `QRMessaging (Maybe (SenderId, QueueLinkData))`: with a link, the short link of a one-time invitation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessagingLink {
    pub sender_id: Vec<u8>,
    pub data: QueueLinkData,
}

/// `QRContact (Maybe (LinkId, (SenderId, QueueLinkData)))`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactLink {
    pub link_id: Vec<u8>,
    pub sender_id: Vec<u8>,
    pub data: QueueLinkData,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueReqData {
    Messaging(Option<MessagingLink>),
    Contact(Option<ContactLink>),
}

impl Encode for QueueReqData {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        match self {
            Self::Messaging(link) => {
                out.push(b'M');
                link.as_ref().map(|l| (&l.sender_id, &l.data)).encode(out)
            }
            Self::Contact(link) => {
                out.push(b'C');
                link.as_ref()
                    .map(|l| (&l.link_id, &l.sender_id, &l.data))
                    .encode(out)
            }
        }
    }
}

/// Notifier credentials of NEW (v17+): key that authorises the notifier and its DH key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewNtfCreds {
    pub auth_key: PublicKey,
    pub dh_key: PublicKey,
}

impl Encode for NewNtfCreds {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        self.auth_key.encode(out)?;
        self.dh_key.encode(out)
    }
}
