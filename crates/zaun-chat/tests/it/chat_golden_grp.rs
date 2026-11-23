//! Golden JSON of x.contact, x.grp.*, x.info.probe*, x.call.* and x.ok from tests/ProtocolTests.hs.
use super::chat_support::*;
use zaun_chat::chat::ChatEvent as E;
use zaun_chat::chat::ChatMessage;
use zaun_chat::chat::call::{
    CallExtraInfo, CallInvitation, CallOffer, CallType, WebRtcExtraInfo, WebRtcSession,
};
use zaun_chat::chat::container::MsgContainer;
use zaun_chat::chat::content::MsgContent;
use zaun_chat::chat::forward::{FwdSender, GrpMsgForward};
use zaun_chat::chat::group::{
    GroupInvitation, GroupMemberRole as R, IntroInvitation, MemberIdRole, MemberInfo,
    MemberRestrictionStatus, MemberRestrictions,
};
use zaun_chat::chat::group_prefs::{FeatureEnabled, GroupFeature, GroupPreferences};
use zaun_chat::chat::group_profile::GroupProfile;
use zaun_chat::chat::ids::{CallId, GroupLinkId, MemberId, Probe, ProbeHash, XContactId};
use zaun_chat::chat::version::{SUPPORTED_CHAT_VRANGE, VersionRange};

fn contact(req_id: Option<&[u8]>, request: Option<MsgContent>) -> ChatMessage {
    msg(E::Contact {
        profile: test_profile(),
        member_key: None,
        contact_req_id: req_id.map(|b| XContactId(b.to_vec())),
        welcome_msg_id: None,
        request_msg: request.map(|c| (id(&[1, 2, 3, 4]), c)),
    })
}

#[test]
fn x_contact_variants() {
    let p = test_profile_json();
    let s = format!(
        r#"{{"v":"9","event":"x.contact","params":{{"contactReqId":"AQIDBA==","profile":{p}}}}}"#
    );
    both(&s, &contact(Some(&[1, 2, 3, 4]), None));
    let s = format!(r#"{{"v":"9","event":"x.contact","params":{{"profile":{p}}}}}"#);
    both(&s, &contact(None, None));
    let s = format!(r#"{{"v":"9","event":"x.contact","params":{{"content":null,"profile":{p}}}}}"#);
    parses_to(&s, &contact(None, None));
    let s = format!(
        r#"{{"v":"9","event":"x.contact","params":{{"msgId":"AQIDBA==","content":{{"text":"hello","type":"text"}},"profile":{p}}}}}"#
    );
    parses_to(
        &s,
        &contact(
            None,
            Some(MsgContent::Text {
                text: "hello".into(),
            }),
        ),
    );
}

fn group_profile() -> GroupProfile {
    let on = || Some(GroupFeature::new(FeatureEnabled::On));
    GroupProfile {
        display_name: "team".into(),
        full_name: "Team".into(),
        short_descr: None,
        description: None,
        image: None,
        public_group: None,
        group_preferences: Some(GroupPreferences {
            reactions: on(),
            voice: on(),
            ..GroupPreferences::default()
        }),
        member_admission: None,
        chat_layout: None,
    }
}

const GROUP_PROFILE: &str = r#"{"fullName":"Team","displayName":"team","groupPreferences":{"reactions":{"enable":"on"},"voice":{"enable":"on"}}}"#;

fn member_info(v: Option<VersionRange>) -> MemberInfo {
    MemberInfo {
        member_id: MemberId(vec![1, 2, 3, 4]),
        member_role: R::Admin,
        v,
        profile: test_profile(),
        member_key: None,
    }
}

fn member_info_json(v: &str) -> String {
    format!(
        r#"{{"memberRole":"admin","memberId":"AQIDBA=="{v},"profile":{}}}"#,
        test_profile_json()
    )
}

#[test]
fn x_grp_inv_with_and_without_group_link_id() {
    let s = format!(
        r#"{{"v":"9","event":"x.grp.inv","params":{{"groupInvitation":{{"connRequest":"{CONN_REQ}","invitedMember":{{"memberRole":"member","memberId":"BQYHCA=="}},"groupProfile":{GROUP_PROFILE},"fromMember":{{"memberRole":"admin","memberId":"AQIDBA=="}}}}}}}}"#
    );
    let inv = GroupInvitation {
        from_member: MemberIdRole {
            member_id: MemberId(vec![1, 2, 3, 4]),
            member_role: R::Admin,
        },
        from_member_key: None,
        invited_member: MemberIdRole {
            member_id: MemberId(vec![5, 6, 7, 8]),
            member_role: R::Member,
        },
        conn_request: CONN_REQ.into(),
        group_profile: group_profile(),
        business: None,
        group_link_id: None,
        group_size: None,
    };
    both(&s, &msg(E::GrpInv(inv.clone())));
    let s = s.replace(r#"}}}}"#, r#"}, "groupLinkId":"AQIDBA=="}}}"#);
    let with_link = GroupInvitation {
        group_link_id: Some(GroupLinkId(vec![1, 2, 3, 4])),
        ..inv
    };
    both(&s, &msg(E::GrpInv(with_link)));
}

#[test]
fn x_grp_info_keeps_known_and_unknown_chat_layout() {
    for layout in ["terminal", "wide"] {
        let gp = r#"{"fullName":"Team","displayName":"team","groupPreferences":{"reactions":{"enable":"on"},"voice":{"enable":"on"}},"chatLayout":"LAYOUT"}"#.replace("LAYOUT", layout);
        let s = format!(r#"{{"v":"9","event":"x.grp.info","params":{{"groupProfile":{gp}}}}}"#);
        both(
            &s,
            &msg(E::GrpInfo(GroupProfile {
                chat_layout: Some(layout.into()),
                ..group_profile()
            })),
        );
    }
}

#[test]
fn x_grp_acpt_with_and_without_profile() {
    let s = r#"{"v":"9","event":"x.grp.acpt","params":{"memberId":"AQIDBA=="}}"#;
    both(
        s,
        &msg(E::GrpAcpt {
            member_id: MemberId(vec![1, 2, 3, 4]),
            member_key: None,
            profile: None,
        }),
    );
    let s = format!(
        r#"{{"v":"9","event":"x.grp.acpt","params":{{"memberId":"AQIDBA==","profile":{}}}}}"#,
        test_profile_json()
    );
    both(
        &s,
        &msg(E::GrpAcpt {
            member_id: MemberId(vec![1, 2, 3, 4]),
            member_key: None,
            profile: Some(test_profile()),
        }),
    );
}

#[test]
fn x_grp_mem_new_intro_with_member_version_range_and_restrictions() {
    let v = Some(SUPPORTED_CHAT_VRANGE);
    for (name, with_v) in [
        ("x.grp.mem.new", false),
        ("x.grp.mem.new", true),
        ("x.grp.mem.intro", false),
        ("x.grp.mem.intro", true),
    ] {
        let vj = if with_v { r#","v":"9-22""# } else { "" };
        let s = format!(
            r#"{{"v":"9","event":"{name}","params":{{"memberInfo":{}}}}}"#,
            member_info_json(vj)
        );
        let mi = member_info(if with_v { v } else { None });
        let ev = if name.ends_with("new") {
            E::GrpMemNew {
                member_info: mi,
                scope: None,
            }
        } else {
            E::GrpMemIntro {
                member_info: mi,
                restrictions: None,
            }
        };
        both(&s, &msg(ev));
    }
    let s = format!(
        r#"{{"v":"9","event":"x.grp.mem.intro","params":{{"memberRestrictions":{{"restriction":"blocked"}},"memberInfo":{}}}}}"#,
        member_info_json("")
    );
    let r = MemberRestrictions {
        restriction: MemberRestrictionStatus::Blocked,
    };
    both(
        &s,
        &msg(E::GrpMemIntro {
            member_info: member_info(None),
            restrictions: Some(r),
        }),
    );
}

#[test]
fn x_grp_mem_inv_fwd_info_con_del() {
    let intro = || IntroInvitation {
        group_conn_req: CONN_REQ.into(),
    };
    let s = format!(
        r#"{{"v":"9","event":"x.grp.mem.inv","params":{{"memberId":"AQIDBA==","memberIntro":{{"groupConnReq":"{CONN_REQ}"}}}}}}"#
    );
    both(
        &s,
        &msg(E::GrpMemInv {
            member_id: MemberId(vec![1, 2, 3, 4]),
            intro: intro(),
        }),
    );
    let s = format!(
        r#"{{"v":"9","event":"x.grp.mem.fwd","params":{{"memberIntro":{{"groupConnReq":"{CONN_REQ}"}},"memberInfo":{}}}}}"#,
        member_info_json(r#","v":"9-22""#)
    );
    both(
        &s,
        &msg(E::GrpMemFwd {
            member_info: member_info(Some(SUPPORTED_CHAT_VRANGE)),
            intro: intro(),
        }),
    );
    let s = format!(
        r#"{{"v":"9","event":"x.grp.mem.info","params":{{"memberId":"AQIDBA==","profile":{}}}}}"#,
        test_profile_json()
    );
    both(
        &s,
        &msg(E::GrpMemInfo {
            member_id: MemberId(vec![1, 2, 3, 4]),
            profile: test_profile(),
        }),
    );
    let m = || MemberId(vec![1, 2, 3, 4]);
    both(
        r#"{"v":"9","event":"x.grp.mem.con","params":{"memberId":"AQIDBA=="}}"#,
        &msg(E::GrpMemCon { member_id: m() }),
    );
    both(
        r#"{"v":"9","event":"x.grp.mem.con.all","params":{"memberId":"AQIDBA=="}}"#,
        &msg(E::GrpMemConAll { member_id: m() }),
    );
    both(
        r#"{"v":"9","event":"x.grp.mem.del","params":{"memberId":"AQIDBA=="}}"#,
        &msg(E::GrpMemDel {
            member_id: m(),
            messages: false,
            roster_version: None,
        }),
    );
    both(
        r#"{"v":"9","event":"x.grp.leave","params":{}}"#,
        &msg(E::GrpLeave),
    );
    both(
        r#"{"v":"9","event":"x.grp.del","params":{}}"#,
        &msg(E::GrpDel),
    );
}

#[test]
fn x_grp_direct_inv_with_and_without_content() {
    let s = format!(
        r#"{{"v":"9","event":"x.grp.direct.inv","params":{{"connReq":"{CONN_REQ}", "content":{{"text":"hello","type":"text"}}}}}}"#
    );
    let c = Some(MsgContent::Text {
        text: "hello".into(),
    });
    both(
        &s,
        &msg(E::GrpDirectInv {
            conn_req: CONN_REQ.into(),
            content: c,
            scope: None,
        }),
    );
    let s =
        format!(r#"{{"v":"9","event":"x.grp.direct.inv","params":{{"connReq":"{CONN_REQ}"}}}}"#);
    both(
        &s,
        &msg(E::GrpDirectInv {
            conn_req: CONN_REQ.into(),
            content: None,
            scope: None,
        }),
    );
}

#[test]
fn x_grp_msg_forward_wraps_the_forwarded_message() {
    let s = r#"{"v":"9","event":"x.grp.msg.forward","params":{"memberId":"AQIDBA==","memberName":"alice","msg":{"v":"9","msgId":"AQIDBA==","event":"x.msg.new","params":{"content":{"text":"hello","type":"text"}}},"msgTs":"1970-01-01T00:00:01.000000001Z"}}"#;
    let inner = msg_with_id(&[1, 2, 3, 4], E::MsgNew(MsgContainer::text("hello")));
    let fwd = GrpMsgForward {
        sender: FwdSender::Member {
            member_id: MemberId(vec![1, 2, 3, 4]),
            member_name: "alice".into(),
        },
        broker_ts: "1970-01-01T00:00:01.000000001Z".into(),
    };
    both(
        s,
        &msg(E::GrpMsgForward {
            fwd,
            msg: Box::new(inner),
        }),
    );
}

#[test]
fn x_info_probe_and_ok() {
    both(
        r#"{"v":"9","event":"x.info.probe","params":{"probe":"AQIDBA=="}}"#,
        &msg(E::InfoProbe(Probe(vec![1, 2, 3, 4]))),
    );
    both(
        r#"{"v":"9","event":"x.info.probe.check","params":{"probeHash":"AQIDBA=="}}"#,
        &msg(E::InfoProbeCheck(ProbeHash(vec![1, 2, 3, 4]))),
    );
    both(
        r#"{"v":"9","event":"x.info.probe.ok","params":{"probe":"AQIDBA=="}}"#,
        &msg(E::InfoProbeOk(Probe(vec![1, 2, 3, 4]))),
    );
    both(r#"{"v":"9","event":"x.ok","params":{}}"#, &msg(E::Ok));
}

fn cid() -> CallId {
    CallId(vec![1, 2, 3, 4])
}

fn rtc() -> WebRtcSession {
    WebRtcSession {
        rtc_session: "{}".into(),
        rtc_ice_candidates: "[]".into(),
    }
}

const CALL_TYPE: &str = r#""callType":{"media":"video","capabilities":{"encryption":true}}"#;

#[test]
fn x_call_inv_with_and_without_version_range() {
    let s = format!(
        r#"{{"v":"9","event":"x.call.inv","params":{{"callId":"AQIDBA==","invitation":{{{CALL_TYPE}}}}}}}"#
    );
    let inv = CallInvitation {
        call_type: CallType::DEFAULT,
        call_dh_pub_key: None,
        call_v_range: None,
    };
    both(
        &s,
        &msg(E::CallInv {
            call_id: cid(),
            invitation: inv.clone(),
        }),
    );
    let s = format!(
        r#"{{"v":"9","event":"x.call.inv","params":{{"callId":"AQIDBA==","invitation":{{{CALL_TYPE},"callVRange":"1-2"}}}}}}"#
    );
    let with_range = CallInvitation {
        call_v_range: Some(VersionRange::new(1, 2)),
        ..inv
    };
    both(
        &s,
        &msg(E::CallInv {
            call_id: cid(),
            invitation: with_range,
        }),
    );
}

#[test]
fn x_call_offer_with_and_without_call_version() {
    let session = r#""rtcSession":{"rtcSession":"{}","rtcIceCandidates":"[]"}"#;
    let s = format!(
        r#"{{"v":"9","event":"x.call.offer","params":{{"callId":"AQIDBA==","offer":{{{CALL_TYPE},{session}}}}}}}"#
    );
    let offer = CallOffer {
        call_type: CallType::DEFAULT,
        rtc_session: rtc(),
        call_dh_pub_key: None,
        call_version: None,
    };
    both(
        &s,
        &msg(E::CallOffer {
            call_id: cid(),
            offer: offer.clone(),
        }),
    );
    let s = format!(
        r#"{{"v":"9","event":"x.call.offer","params":{{"callId":"AQIDBA==","offer":{{{CALL_TYPE},{session},"callVersion":2}}}}}}"#
    );
    both(
        &s,
        &msg(E::CallOffer {
            call_id: cid(),
            offer: CallOffer {
                call_version: Some(2),
                ..offer
            },
        }),
    );
}

#[test]
fn x_call_extra_with_and_without_ice_restart_session() {
    let s = r#"{"v":"9","event":"x.call.extra","params":{"callId":"AQIDBA==","extra":{"rtcExtraInfo":{"rtcIceCandidates":"[]"}}}}"#;
    let info = |rtc_session| CallExtraInfo {
        rtc_extra_info: WebRtcExtraInfo {
            rtc_session,
            rtc_ice_candidates: "[]".into(),
        },
    };
    both(
        s,
        &msg(E::CallExtra {
            call_id: cid(),
            extra: info(None),
        }),
    );
    let s = r#"{"v":"9","event":"x.call.extra","params":{"callId":"AQIDBA==","extra":{"rtcExtraInfo":{"rtcSession":"{}","rtcIceCandidates":"[]"}}}}"#;
    both(
        s,
        &msg(E::CallExtra {
            call_id: cid(),
            extra: info(Some("{}".into())),
        }),
    );
}
