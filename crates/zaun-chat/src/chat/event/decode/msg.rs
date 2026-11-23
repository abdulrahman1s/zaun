//! Decoding of message and connection events.
use super::Obj;
use crate::chat::error::Result;
use crate::chat::event::{ChatEvent as E, EventKind as K};
use crate::chat::json::{opt, req};
use serde::de::DeserializeOwned;
use serde_json::Value;

pub(super) fn whole<T: DeserializeOwned>(o: &Obj) -> Result<T> {
    Ok(serde_json::from_value(Value::Object(o.clone()))?)
}

pub(super) fn decode_msg(kind: K, o: &Obj) -> Result<E> {
    Ok(match kind {
        K::MsgNew => E::MsgNew(whole(o)?),
        K::MsgFileDescr => E::MsgFileDescr {
            msg_id: req(o, "msgId")?,
            file_descr: req(o, "fileDescr")?,
            file_expires: opt(o, "fileExpires")?,
            file_badge: opt(o, "fileBadge")?,
        },
        K::MsgUpdate => E::MsgUpdate {
            msg_id: req(o, "msgId")?,
            content: req(o, "content")?,
            mentions: opt(o, "mentions")?.unwrap_or_default(),
            ttl: opt(o, "ttl")?,
            live: opt(o, "live")?,
            scope: opt(o, "scope")?,
            as_group: opt(o, "asGroup")?,
        },
        K::MsgDel => E::MsgDel {
            msg_id: req(o, "msgId")?,
            member_id: opt(o, "memberId")?,
            scope: opt(o, "scope")?,
            only_history: opt(o, "onlyHistory")?.unwrap_or(false),
        },
        K::MsgReact => E::MsgReact {
            msg_id: req(o, "msgId")?,
            member_id: opt(o, "memberId")?,
            scope: opt(o, "scope")?,
            reaction: req(o, "reaction")?,
            add: req(o, "add")?,
        },
        _ => E::MsgDeleted,
    })
}

pub(super) fn decode_conn(kind: K, o: &Obj) -> Result<E> {
    Ok(match kind {
        K::File => E::File(req(o, "file")?),
        K::FileAcpt => E::FileAcpt {
            file_name: req(o, "fileName")?,
        },
        K::FileAcptInv => E::FileAcptInv {
            msg_id: req(o, "msgId")?,
            file_conn_req: opt(o, "fileConnReq")?,
            file_name: req(o, "fileName")?,
        },
        K::FileCancel => E::FileCancel {
            msg_id: req(o, "msgId")?,
        },
        K::Info => E::Info {
            profile: req(o, "profile")?,
            member_key: opt(o, "memberKey")?,
        },
        K::Contact => E::Contact {
            profile: req(o, "profile")?,
            member_key: opt(o, "memberKey")?,
            contact_req_id: opt(o, "contactReqId")?,
            welcome_msg_id: opt(o, "welcomeMsgId")?,
            request_msg: opt(o, "msgId")?.zip(opt(o, "content")?),
        },
        K::Member => E::Member {
            profile: req(o, "profile")?,
            new_member_id: req(o, "newMemberId")?,
            new_member_key: req(o, "newMemberKey")?,
            via_relay: opt(o, "viaRelay")?,
        },
        K::DirectDel => E::DirectDel,
        _ => E::Ok,
    })
}
