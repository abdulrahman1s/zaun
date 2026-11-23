//! Decoding of group events.
use super::Obj;
use crate::chat::error::Result;
use crate::chat::event::{ChatEvent as E, EventKind as K};
use crate::chat::json::{opt, req};

pub(super) fn decode_join(kind: K, o: &Obj) -> Result<E> {
    Ok(match kind {
        K::GrpInv => E::GrpInv(req(o, "groupInvitation")?),
        K::GrpAcpt => E::GrpAcpt {
            member_id: req(o, "memberId")?,
            member_key: opt(o, "memberKey")?,
            profile: opt(o, "profile")?,
        },
        K::GrpLinkInv => E::GrpLinkInv(req(o, "groupLinkInvitation")?),
        K::GrpLinkReject => E::GrpLinkReject(req(o, "groupLinkRejection")?),
        K::GrpLinkMem => E::GrpLinkMem {
            profile: req(o, "profile")?,
            member_key: opt(o, "memberKey")?,
        },
        _ => E::GrpLinkAcpt {
            acceptance: req(o, "acceptance")?,
            role: req(o, "role")?,
            member_id: req(o, "memberId")?,
        },
    })
}

pub(super) fn decode_member(kind: K, o: &Obj) -> Result<E> {
    Ok(match kind {
        K::GrpMemNew => E::GrpMemNew {
            member_info: req(o, "memberInfo")?,
            scope: opt(o, "scope")?,
        },
        K::GrpMemIntro => E::GrpMemIntro {
            member_info: req(o, "memberInfo")?,
            restrictions: opt(o, "memberRestrictions")?,
        },
        K::GrpMemInv => E::GrpMemInv {
            member_id: req(o, "memberId")?,
            intro: req(o, "memberIntro")?,
        },
        K::GrpMemFwd => E::GrpMemFwd {
            member_info: req(o, "memberInfo")?,
            intro: req(o, "memberIntro")?,
        },
        K::GrpMemInfo => E::GrpMemInfo {
            member_id: req(o, "memberId")?,
            profile: req(o, "profile")?,
        },
        K::GrpMemRole => E::GrpMemRole {
            member_id: req(o, "memberId")?,
            role: req(o, "role")?,
            member_key: opt(o, "memberKey")?,
            roster_version: opt(o, "rosterVersion")?,
        },
        _ => E::GrpMemRestrict {
            member_id: req(o, "memberId")?,
            restrictions: req(o, "memberRestrictions")?,
        },
    })
}

pub(super) fn decode_group(kind: K, o: &Obj) -> Result<E> {
    Ok(match kind {
        K::GrpMemCon => E::GrpMemCon {
            member_id: req(o, "memberId")?,
        },
        K::GrpMemConAll => E::GrpMemConAll {
            member_id: req(o, "memberId")?,
        },
        K::GrpMemDel => E::GrpMemDel {
            member_id: req(o, "memberId")?,
            messages: req(o, "messages").unwrap_or(false),
            roster_version: opt(o, "rosterVersion")?,
        },
        K::GrpLeave => E::GrpLeave,
        K::GrpInfo => E::GrpInfo(req(o, "groupProfile")?),
        K::GrpPrefs => E::GrpPrefs(req(o, "groupPreferences")?),
        K::GrpDirectInv => E::GrpDirectInv {
            conn_req: req(o, "connReq")?,
            content: opt(o, "content")?,
            scope: opt(o, "scope")?,
        },
        _ => E::GrpDel,
    })
}
