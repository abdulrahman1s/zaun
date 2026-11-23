//! Params of message and connection events.
use crate::chat::event::ChatEvent as E;
use crate::chat::json::Params;
use serde_json::{Map, Value};

pub(super) fn params(ev: &E) -> Map<String, Value> {
    let p = Params::new();
    match ev {
        E::MsgFileDescr {
            msg_id,
            file_descr,
            file_expires,
            file_badge,
        } => p
            .put("msgId", msg_id)
            .put("fileDescr", file_descr)
            .put_opt("fileExpires", file_expires)
            .put_opt("fileBadge", file_badge),
        E::MsgUpdate {
            msg_id,
            content,
            mentions,
            ttl,
            live,
            scope,
            as_group,
        } => p
            .put("msgId", msg_id)
            .put("content", content)
            .put_opt("mentions", &(!mentions.is_empty()).then_some(mentions))
            .put_opt("ttl", ttl)
            .put_opt("live", live)
            .put_opt("scope", scope)
            .put_opt("asGroup", as_group),
        E::MsgDel {
            msg_id,
            member_id,
            scope,
            only_history,
        } => p
            .put("msgId", msg_id)
            .put_opt("memberId", member_id)
            .put_opt("scope", scope)
            .put_if("onlyHistory", *only_history),
        E::MsgReact {
            msg_id,
            member_id,
            scope,
            reaction,
            add,
        } => p
            .put("msgId", msg_id)
            .put_opt("memberId", member_id)
            .put_opt("scope", scope)
            .put("reaction", reaction)
            .put("add", add),
        E::File(f) => p.put("file", f),
        E::FileAcpt { file_name } => p.put("fileName", file_name),
        E::FileAcptInv {
            msg_id,
            file_conn_req,
            file_name,
        } => p
            .put("msgId", msg_id)
            .put("fileName", file_name)
            .put_opt("fileConnReq", file_conn_req),
        E::FileCancel { msg_id } => p.put("msgId", msg_id),
        E::Info {
            profile,
            member_key,
        } => p.put("profile", profile).put_opt("memberKey", member_key),
        E::Contact {
            profile,
            member_key,
            contact_req_id,
            welcome_msg_id,
            request_msg,
        } => p
            .put("profile", profile)
            .put_opt("memberKey", member_key)
            .put_opt("contactReqId", contact_req_id)
            .put_opt("welcomeMsgId", welcome_msg_id)
            .put_opt("msgId", &request_msg.as_ref().map(|r| &r.0))
            .put_opt("content", &request_msg.as_ref().map(|r| &r.1)),
        E::Member {
            profile,
            new_member_id,
            new_member_key,
            via_relay,
        } => p
            .put("profile", profile)
            .put("newMemberId", new_member_id)
            .put("newMemberKey", new_member_key)
            .put_opt("viaRelay", via_relay),
        _ => p,
    }
    .done()
}
