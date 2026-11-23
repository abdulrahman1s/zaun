//! The connection request inside link data: `I`/`C`, the common data and, for an invitation, its E2E params (Agent/Protocol.hs `BinaryConnectionRequestUri`, AGPL-3.0-only).
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError};

use super::conn_req_data::ConnReqUriData;
use crate::agent::e2e::E2eParamsUri;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinaryConnReq {
    Invitation {
        data: ConnReqUriData,
        e2e: E2eParamsUri,
    },
    Contact {
        data: ConnReqUriData,
    },
}

impl Encode for BinaryConnReq {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        match self {
            Self::Invitation { data, e2e } => {
                out.push(b'I');
                (data, e2e).encode(out)
            }
            Self::Contact { data } => {
                out.push(b'C');
                data.encode(out)
            }
        }
    }
}

impl Decode for BinaryConnReq {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        match d.byte()? {
            b'I' => {
                let (data, e2e) = <(ConnReqUriData, E2eParamsUri)>::decode(d)?;
                Ok(Self::Invitation { data, e2e })
            }
            b'C' => Ok(Self::Contact {
                data: ConnReqUriData::decode(d)?,
            }),
            t => Err(DecodeError::BadTag("connection mode", t)),
        }
    }
}
