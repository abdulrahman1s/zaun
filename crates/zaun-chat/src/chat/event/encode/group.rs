//! Params of group events.
use crate::chat::event::ChatEvent as E;
use crate::chat::json::Params;
use serde_json::{Map, Value};

pub(super) fn params(ev: &E) -> Map<String, Value> {
    let p = Params::new();
    match ev {
        E::GrpInv(i) => p.put("groupInvitation", i),
        E::GrpAcpt {
            member_id,
            member_key,
            profile,
        } => p
            .put("memberId", member_id)
            .put_opt("memberKey", member_key)
            .put_opt("profile", profile),
        E::GrpLinkInv(i) => p.put("groupLinkInvitation", i),
        E::GrpLinkReject(r) => p.put("groupLinkRejection", r),
        E::GrpLinkMem {
            profile,
            member_key,
        } => p.put("profile", profile).put_opt("memberKey", member_key),
        E::GrpLinkAcpt {
            acceptance,
            role,
            member_id,
        } => p
            .put("acceptance", acceptance)
            .put("role", role)
            .put("memberId", member_id),
        E::GrpMemNew { member_info, scope } => {
            p.put("memberInfo", member_info).put_opt("scope", scope)
        }
        E::GrpMemIntro {
            member_info,
            restrictions,
        } => p
            .put("memberInfo", member_info)
            .put_opt("memberRestrictions", restrictions),
        E::GrpMemInv { member_id, intro } => p.put("memberId", member_id).put("memberIntro", intro),
        E::GrpMemFwd { member_info, intro } => {
            p.put("memberInfo", member_info).put("memberIntro", intro)
        }
        E::GrpMemInfo { member_id, profile } => {
            p.put("memberId", member_id).put("profile", profile)
        }
        E::GrpMemRole {
            member_id,
            role,
            member_key,
            roster_version,
        } => p
            .put("memberId", member_id)
            .put("role", role)
            .put_opt("memberKey", member_key)
            .put_opt("rosterVersion", roster_version),
        E::GrpMemRestrict {
            member_id,
            restrictions,
        } => p
            .put("memberId", member_id)
            .put("memberRestrictions", restrictions),
        E::GrpMemCon { member_id } | E::GrpMemConAll { member_id } => p.put("memberId", member_id),
        E::GrpMemDel {
            member_id,
            messages,
            roster_version,
        } => p
            .put("memberId", member_id)
            .put_if("messages", *messages)
            .put_opt("rosterVersion", roster_version),
        E::GrpInfo(g) => p.put("groupProfile", g),
        E::GrpPrefs(g) => p.put("groupPreferences", g),
        E::GrpDirectInv {
            conn_req,
            content,
            scope,
        } => p
            .put("connReq", conn_req)
            .put_opt("content", content)
            .put_opt("scope", scope),
        _ => p,
    }
    .done()
}
