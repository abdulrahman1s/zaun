//! Messages the agents send each other once a queue is secured (Agent/Protocol.hs `AMessage`, `AMsgType`, AGPL-3.0-only).
use crate::crypto::PublicKey;
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Tail};

use super::receipt::AMessageReceipt;
use super::snd_addr::SndQAddr;
use crate::agent::ne::{decode_ne, encode_ne};
use crate::agent::queue_info::SmpQueueInfo;
use crate::agent::queue_uri::SmpQueueUri;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AMessage {
    /// The first message of a queue, to show it is secured.
    Hello,
    /// The chat layer's message.
    AMsg(Vec<u8>),
    ARcvd(Vec<AMessageReceipt>),
    /// Continue sending after a quota error.
    AQcont(SndQAddr),
    /// The recipient adds queues, each optionally replacing one.
    Qadd(Vec<(SmpQueueUri, Option<SndQAddr>)>),
    /// The sender's key for the added queues; below agent v8.
    Qkey(Vec<(SmpQueueInfo, PublicKey)>),
    /// The queues are ready; the flag marks the primary.
    Quse(Vec<(SndQAddr, bool)>),
    Qtest(Vec<SndQAddr>),
    /// Remove queues from the connection: fast rotation at agent v8.
    Qend(Vec<SndQAddr>),
    /// Ratchet re-synchronisation is done, with the last sender message ID decrypted.
    Eready(i64),
}

impl Encode for AMessage {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        match self {
            Self::Hello => out.push(b'H'),
            Self::AMsg(body) => {
                out.push(b'M');
                Tail(body.clone()).encode(out)?;
            }
            Self::ARcvd(rs) => {
                out.push(b'V');
                encode_ne(rs, out)?;
            }
            Self::AQcont(a) => {
                out.extend_from_slice(b"QC");
                a.encode(out)?;
            }
            Self::Qadd(qs) => {
                out.extend_from_slice(b"QA");
                encode_ne(qs, out)?;
            }
            Self::Qkey(qs) => {
                out.extend_from_slice(b"QK");
                encode_ne(qs, out)?;
            }
            Self::Quse(qs) => {
                out.extend_from_slice(b"QU");
                encode_ne(qs, out)?;
            }
            Self::Qtest(qs) => {
                out.extend_from_slice(b"QT");
                encode_ne(qs, out)?;
            }
            Self::Qend(qs) => {
                out.extend_from_slice(b"QE");
                encode_ne(qs, out)?;
            }
            Self::Eready(id) => {
                out.push(b'E');
                id.encode(out)?;
            }
        }
        Ok(())
    }
}

impl Decode for AMessage {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        match d.byte()? {
            b'H' => Ok(Self::Hello),
            b'M' => Ok(Self::AMsg(d.take_all().to_vec())),
            b'V' => decode_ne(d).map(Self::ARcvd),
            b'E' => i64::decode(d).map(Self::Eready),
            b'Q' => match d.byte()? {
                b'C' => SndQAddr::decode(d).map(Self::AQcont),
                b'A' => decode_ne(d).map(Self::Qadd),
                b'K' => decode_ne(d).map(Self::Qkey),
                b'U' => decode_ne(d).map(Self::Quse),
                b'T' => decode_ne(d).map(Self::Qtest),
                b'E' => decode_ne(d).map(Self::Qend),
                t => Err(DecodeError::BadTag("AMsgType", t)),
            },
            t => Err(DecodeError::BadTag("AMsgType", t)),
        }
    }
}
