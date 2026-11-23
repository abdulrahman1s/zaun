//! `ChatEvent` to its JSON name and params (Protocol.hs `chatToAppMessage`).
mod group;
mod misc;
mod msg;

use super::types::ChatEvent as E;
use crate::chat::error::{ChatError, Result};
use crate::chat::forward::{FwdSender, GrpMsgForward};
use crate::chat::json::{Params, val};
use crate::chat::message::ChatMessage;
use serde_json::{Map, Value};

pub(crate) fn encode(ev: &E) -> Result<(String, Map<String, Value>)> {
    if let E::Unknown { event, params } = ev {
        return Ok((event.clone(), params.clone()));
    }
    let Some(kind) = ev.kind() else {
        return Err(ChatError::Parse("a binary event has no JSON form".into()));
    };
    Ok((kind.as_str().to_owned(), params(ev)))
}

fn params(ev: &E) -> Map<String, Value> {
    match ev {
        E::MsgNew(c) => match val(c) {
            Value::Object(o) => o,
            _ => Map::new(),
        },
        E::GrpMsgForward { fwd, msg } => forward_params(fwd, msg),
        E::MsgFileDescr { .. }
        | E::MsgUpdate { .. }
        | E::MsgDel { .. }
        | E::MsgReact { .. }
        | E::File(_)
        | E::FileAcpt { .. }
        | E::FileAcptInv { .. }
        | E::FileCancel { .. }
        | E::Info { .. }
        | E::Contact { .. }
        | E::Member { .. } => msg::params(ev),
        E::GrpInv(_)
        | E::GrpAcpt { .. }
        | E::GrpLinkInv(_)
        | E::GrpLinkReject(_)
        | E::GrpLinkMem { .. }
        | E::GrpLinkAcpt { .. }
        | E::GrpMemNew { .. }
        | E::GrpMemIntro { .. }
        | E::GrpMemInv { .. }
        | E::GrpMemFwd { .. }
        | E::GrpMemInfo { .. }
        | E::GrpMemRole { .. }
        | E::GrpMemRestrict { .. }
        | E::GrpMemCon { .. }
        | E::GrpMemConAll { .. }
        | E::GrpMemDel { .. }
        | E::GrpInfo(_)
        | E::GrpPrefs(_)
        | E::GrpDirectInv { .. } => group::params(ev),
        E::InfoProbe(_)
        | E::InfoProbeCheck(_)
        | E::InfoProbeOk(_)
        | E::CallInv { .. }
        | E::CallOffer { .. }
        | E::CallAnswer { .. }
        | E::CallExtra { .. }
        | E::CallEnd { .. } => misc::params(ev),
        E::MsgDeleted
        | E::DirectDel
        | E::GrpLeave
        | E::GrpDel
        | E::Ok
        | E::FileChunk { .. }
        | E::Unknown { .. } => Map::new(),
    }
}

fn forward_params(fwd: &GrpMsgForward, msg: &ChatMessage) -> Map<String, Value> {
    let p = match &fwd.sender {
        FwdSender::Member {
            member_id,
            member_name,
        } => Params::new()
            .put("memberId", member_id)
            .put("memberName", member_name),
        FwdSender::Channel => Params::new(),
    };
    p.put("msg", &msg.to_value())
        .put("msgTs", &fwd.broker_ts)
        .done()
}
