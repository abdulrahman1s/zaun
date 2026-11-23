//! The forward header of a `>` element and of `x.grp.msg.forward` (Protocol.hs `GrpMsgForward`, `FwdSender`).
use super::bin::{Reader, put_short};
use super::error::{Result, parse_err};
use super::ids::MemberId;
use super::iso::{iso_to_secs, secs_to_iso};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FwdSender {
    Member {
        member_id: MemberId,
        member_name: String,
    },
    Channel,
}

/// `broker_ts` is the original broker time as ISO text (JSON `msgTs`); the binary form keeps whole seconds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrpMsgForward {
    pub sender: FwdSender,
    pub broker_ts: String,
}

impl GrpMsgForward {
    /// `smpEncode`: `'M' memberId memberName | 'C'`, then the broker time as an Int64 of seconds.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        match &self.sender {
            FwdSender::Member {
                member_id,
                member_name,
            } => {
                out.push(b'M');
                put_short(&mut out, member_id.as_bytes())?;
                put_short(&mut out, member_name.as_bytes())?;
            }
            FwdSender::Channel => out.push(b'C'),
        }
        let Some(secs) = iso_to_secs(&self.broker_ts) else {
            return parse_err("bad forward time");
        };
        out.extend_from_slice(&secs.to_be_bytes());
        Ok(out)
    }

    pub(crate) fn read(r: &mut Reader<'_>) -> Result<Self> {
        let sender = match r.u8()? {
            b'M' => {
                let member_id = MemberId(r.short_bytes()?.to_vec());
                let name = String::from_utf8(r.short_bytes()?.to_vec());
                FwdSender::Member {
                    member_id,
                    member_name: name.or_else(|_| parse_err("bad member name"))?,
                }
            }
            b'C' => FwdSender::Channel,
            c => return parse_err(format!("invalid FwdSender tag: {}", char::from(c))),
        };
        Ok(Self {
            sender,
            broker_ts: secs_to_iso(r.i64()?),
        })
    }
}
