//! The client commands the in-memory server understands, read from their wire form (Protocol.hs `instance ProtocolEncoding (Command p)`).
use crate::crypto::PublicKey;
use crate::encoding::{Decode, DecodeError, Decoder, Tail};
use crate::smp::QueueLinkData;

pub enum Cmd {
    New {
        rcv_auth: PublicKey,
        rcv_dh: PublicKey,
        link: Option<(Vec<u8>, QueueLinkData)>,
    },
    Sub,
    Skey(PublicKey),
    Lkey(PublicKey),
    Send(Vec<u8>),
    Ack(Vec<u8>),
    Del,
    Ping,
}

pub fn parse(v: u16, bytes: &[u8]) -> Result<Cmd, DecodeError> {
    let at = bytes.iter().position(|b| *b == b' ').unwrap_or(bytes.len());
    let (tag, rest) = bytes.split_at(at);
    let mut d = Decoder::new(rest.get(1..).unwrap_or_default());
    Ok(match tag {
        b"NEW" => parse_new(v, &mut d)?,
        b"SUB" => Cmd::Sub,
        b"SKEY" | b"KEY" => Cmd::Skey(PublicKey::decode(&mut d)?),
        b"LKEY" => Cmd::Lkey(PublicKey::decode(&mut d)?),
        b"SEND" => {
            let _notification = bool::decode(&mut d)?;
            if d.byte()? != b' ' {
                return Err(DecodeError::Invalid("SEND"));
            }
            Cmd::Send(Tail::decode(&mut d)?.0)
        }
        b"ACK" => Cmd::Ack(Vec::decode(&mut d)?),
        b"DEL" => Cmd::Del,
        b"PING" => Cmd::Ping,
        _ => return Err(DecodeError::Invalid("command the fake does not know")),
    })
}

fn parse_new(_v: u16, d: &mut Decoder<'_>) -> Result<Cmd, DecodeError> {
    let (rcv_auth, rcv_dh) = <(PublicKey, PublicKey)>::decode(d)?;
    let _basic_auth = Option::<String>::decode(d)?;
    let _sub_mode = d.byte()?;
    let link = match d.byte()? {
        b'1' => match d.byte()? {
            b'M' => Option::<(Vec<u8>, QueueLinkData)>::decode(d)?,
            _ => return Err(DecodeError::Invalid("queue request data")),
        },
        _ => None,
    };
    Ok(Cmd::New {
        rcv_auth,
        rcv_dh,
        link,
    })
}
