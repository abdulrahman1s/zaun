//! The binary form of a short link, used in the relay list of link data (Agent/Protocol.hs `Encoding ConnShortLink`, AGPL-3.0-only).
use super::contact_type::ContactConnType;
use super::short_link::{ConnShortLink, ShortLinkScheme};
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError};
use crate::smp::SmpServer;

/// The binary form, used in the relay list of link data.
impl Encode for ConnShortLink {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        match self {
            Self::Invitation {
                server,
                link_id,
                link_key,
                ..
            } => {
                out.push(b'I');
                (server, link_id, link_key).encode(out)
            }
            Self::Contact {
                contact_type,
                server,
                link_key,
                ..
            } => {
                out.push(b'C');
                (contact_type, server, link_key).encode(out)
            }
        }
    }
}

impl Decode for ConnShortLink {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let scheme = ShortLinkScheme::Server;
        match d.byte()? {
            b'I' => {
                let (server, link_id, link_key) = <(SmpServer, Vec<u8>, Vec<u8>)>::decode(d)?;
                Ok(Self::Invitation {
                    scheme,
                    server,
                    link_id,
                    link_key,
                })
            }
            b'C' => {
                let (contact_type, server, link_key) =
                    <(ContactConnType, SmpServer, Vec<u8>)>::decode(d)?;
                Ok(Self::Contact {
                    scheme,
                    contact_type,
                    server,
                    link_key,
                })
            }
            t => Err(DecodeError::BadTag("connection mode", t)),
        }
    }
}
