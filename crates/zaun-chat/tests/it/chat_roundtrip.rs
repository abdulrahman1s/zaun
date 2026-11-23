//! Every event survives encode and parse; unknown events and content are kept; ids, reactions, images, flags.
use super::chat_support::*;
use serde_json::{Map, json};
use zaun_chat::chat::call::*;
use zaun_chat::chat::container::*;
use zaun_chat::chat::content::MsgContent;
use zaun_chat::chat::event::FileChunk;
use zaun_chat::chat::file::{FileDescr, FileInvitation};
use zaun_chat::chat::forward::{FwdSender, GrpMsgForward};
use zaun_chat::chat::group::*;
use zaun_chat::chat::group_prefs::GroupPreferences;
use zaun_chat::chat::group_profile::{GroupProfile, MemberAdmission};
use zaun_chat::chat::ids::*;
use zaun_chat::chat::image::ImageData;
use zaun_chat::chat::reaction::MsgReaction;
use zaun_chat::chat::version::{SUPPORTED_CHAT_VRANGE, VersionRange};
use zaun_chat::chat::{
    ChatEvent as E, ChatMessage, EventKind, encode_chat_message, parse_chat_messages,
};

fn mid() -> MemberId {
    MemberId(b"MTIzNDU2Nzg5MDEy".to_vec())
}

fn member_info() -> MemberInfo {
    MemberInfo {
        member_id: mid(),
        member_role: GroupMemberRole::Member,
        v: Some(SUPPORTED_CHAT_VRANGE),
        profile: test_profile(),
        member_key: Some("MCowBQYDK2VwAyEA".into()),
    }
}

fn gprofile() -> GroupProfile {
    GroupProfile {
        display_name: "g".into(),
        full_name: "".into(),
        short_descr: Some("s".into()),
        description: Some("welcome".into()),
        image: None,
        public_group: None,
        group_preferences: Some(GroupPreferences::default_full()),
        member_admission: Some(MemberAdmission {
            review: Some("all".into()),
        }),
        chat_layout: None,
    }
}

fn role_id(r: GroupMemberRole) -> MemberIdRole {
    MemberIdRole {
        member_id: mid(),
        member_role: r,
    }
}

fn session() -> WebRtcSession {
    WebRtcSession {
        rtc_session: "abc".into(),
        rtc_ice_candidates: "def".into(),
    }
}

fn every_event() -> Vec<E> {
    let m = || id(&[1, 2, 3, 4]);
    let content = || MsgContent::Voice {
        text: "t".into(),
        duration: 3,
    };
    let scope = || Some(MsgScope::Member { member_id: mid() });
    let mut mentions = MsgMentions::new();
    mentions.insert("bob".into(), MsgMention { member_id: mid() });
    vec![
        E::MsgNew(MsgContainer {
            mentions: mentions.clone(),
            scope: scope(),
            as_group: Some(true),
            ..MsgContainer::text("hi @bob")
        }),
        E::MsgFileDescr {
            msg_id: m(),
            file_descr: FileDescr {
                file_descr_text: "x".into(),
                file_descr_part_no: 1,
                file_descr_complete: true,
            },
            file_expires: Some("2026-12-01T00:00:00Z".into()),
            file_badge: None,
        },
        E::MsgUpdate {
            msg_id: m(),
            content: content(),
            mentions,
            ttl: Some(5),
            live: Some(true),
            scope: scope(),
            as_group: None,
        },
        E::MsgDel {
            msg_id: m(),
            member_id: Some(mid()),
            scope: scope(),
            only_history: true,
        },
        E::MsgDeleted,
        E::MsgReact {
            msg_id: m(),
            member_id: None,
            scope: None,
            reaction: MsgReaction::Emoji('👍'),
            add: false,
        },
        E::File(FileInvitation::xftp("a.bin", 10)),
        E::FileAcpt {
            file_name: "a".into(),
        },
        E::FileAcptInv {
            msg_id: m(),
            file_conn_req: None,
            file_name: "a".into(),
        },
        E::FileCancel { msg_id: m() },
        E::Info {
            profile: test_profile(),
            member_key: Some("k".into()),
        },
        E::Contact {
            profile: test_profile(),
            member_key: None,
            contact_req_id: Some(XContactId(vec![9])),
            welcome_msg_id: Some(m()),
            request_msg: Some((m(), content())),
        },
        E::Member {
            profile: test_profile(),
            new_member_id: mid(),
            new_member_key: "k".into(),
            via_relay: Some(mid()),
        },
        E::DirectDel,
        E::GrpInv(GroupInvitation {
            from_member: role_id(GroupMemberRole::Owner),
            from_member_key: Some("k".into()),
            invited_member: role_id(GroupMemberRole::Member),
            conn_request: CONN_REQ.into(),
            group_profile: gprofile(),
            business: Some(json!({"chatType":"business"})),
            group_link_id: None,
            group_size: Some(3),
        }),
        E::GrpAcpt {
            member_id: mid(),
            member_key: Some("k".into()),
            profile: Some(test_profile()),
        },
        E::GrpLinkInv(GroupLinkInvitation {
            from_member: role_id(GroupMemberRole::Admin),
            from_member_name: "h".into(),
            from_member_key: None,
            invited_member: role_id(GroupMemberRole::Member),
            group_profile: gprofile(),
            accepted: Some(GroupAcceptance::PendingReview),
            business: None,
            group_size: None,
        }),
        E::GrpLinkReject(GroupLinkRejection {
            from_member: role_id(GroupMemberRole::Observer),
            invited_member: role_id(GroupMemberRole::Member),
            group_profile: gprofile(),
            rejection_reason: GroupRejectionReason::LongName,
        }),
        E::GrpLinkMem {
            profile: test_profile(),
            member_key: Some("k".into()),
        },
        E::GrpLinkAcpt {
            acceptance: GroupAcceptance::Accepted,
            role: GroupMemberRole::Moderator,
            member_id: mid(),
        },
        E::GrpMemNew {
            member_info: member_info(),
            scope: scope(),
        },
        E::GrpMemIntro {
            member_info: member_info(),
            restrictions: Some(MemberRestrictions {
                restriction: MemberRestrictionStatus::Blocked,
            }),
        },
        E::GrpMemInv {
            member_id: mid(),
            intro: IntroInvitation {
                group_conn_req: CONN_REQ.into(),
            },
        },
        E::GrpMemFwd {
            member_info: member_info(),
            intro: IntroInvitation {
                group_conn_req: CONN_REQ.into(),
            },
        },
        E::GrpMemInfo {
            member_id: mid(),
            profile: test_profile(),
        },
        E::GrpMemRole {
            member_id: mid(),
            role: GroupMemberRole::Admin,
            member_key: None,
            roster_version: Some(4),
        },
        E::GrpMemRestrict {
            member_id: mid(),
            restrictions: MemberRestrictions {
                restriction: MemberRestrictionStatus::Unrestricted,
            },
        },
        E::GrpMemCon { member_id: mid() },
        E::GrpMemConAll { member_id: mid() },
        E::GrpMemDel {
            member_id: mid(),
            messages: true,
            roster_version: None,
        },
        E::GrpLeave,
        E::GrpDel,
        E::GrpInfo(gprofile()),
        E::GrpPrefs(GroupPreferences::default_full()),
        E::GrpDirectInv {
            conn_req: CONN_REQ.into(),
            content: Some(content()),
            scope: scope(),
        },
        E::GrpMsgForward {
            fwd: GrpMsgForward {
                sender: FwdSender::Channel,
                broker_ts: "2026-11-23T10:00:00Z".into(),
            },
            msg: Box::new(msg_with_id(&[1], E::GrpLeave)),
        },
        E::InfoProbe(Probe(vec![1])),
        E::InfoProbeCheck(ProbeHash(vec![2])),
        E::InfoProbeOk(Probe(vec![3])),
        E::CallInv {
            call_id: CallId(vec![7; 16]),
            invitation: CallInvitation {
                call_type: CallType::DEFAULT,
                call_dh_pub_key: Some("MCow".into()),
                call_v_range: Some(VersionRange::new(1, 2)),
            },
        },
        E::CallOffer {
            call_id: CallId(vec![7; 16]),
            offer: CallOffer {
                call_type: CallType::DEFAULT,
                rtc_session: session(),
                call_dh_pub_key: None,
                call_version: Some(2),
            },
        },
        E::CallAnswer {
            call_id: CallId(vec![7; 16]),
            answer: CallAnswer {
                rtc_session: session(),
            },
        },
        E::CallExtra {
            call_id: CallId(vec![7; 16]),
            extra: CallExtraInfo {
                rtc_extra_info: WebRtcExtraInfo {
                    rtc_session: None,
                    rtc_ice_candidates: "[]".into(),
                },
            },
        },
        E::CallEnd {
            call_id: CallId(vec![7; 16]),
        },
        E::Ok,
    ]
}

#[test]
fn every_event_round_trips_through_encode_and_parse() {
    for event in every_event() {
        let m = msg_with_id(&[1, 2, 3], event);
        let bytes = encode_chat_message(&m, false).unwrap();
        assert_eq!(
            parse_chat_messages(&bytes).unwrap(),
            vec![m.clone()],
            "{:?}",
            m.event.kind()
        );
    }
}

#[test]
fn the_round_trip_list_covers_every_event_kind_except_channel_and_roster_ones() {
    let covered: Vec<_> = every_event().iter().filter_map(E::kind).collect();
    for kind in EventKind::ALL {
        let kept_unknown =
            kind.as_str().starts_with("x.grp.relay.") || kind.as_str().starts_with("x.grp.roster");
        assert!(
            covered.contains(kind) || kept_unknown,
            "{} is not covered",
            kind.as_str()
        );
    }
}

#[test]
fn channel_and_roster_events_are_kept_as_unknown_with_their_params() {
    for kind in EventKind::ALL.iter().filter(|k| {
        k.as_str().starts_with("x.grp.relay.") || k.as_str().starts_with("x.grp.roster")
    }) {
        let s = format!(
            r#"{{"v":"9-22","event":"{}","params":{{"a":[1,2],"b":{{"c":null}}}}}}"#,
            kind.as_str()
        );
        let parsed = parse_chat_messages(s.as_bytes()).unwrap().remove(0);
        let E::Unknown { event, params } = &parsed.event else {
            panic!("{} was decoded", kind.as_str())
        };
        assert_eq!(event, kind.as_str());
        assert_eq!(parsed.event.kind(), Some(*kind));
        assert_eq!(
            serde_json::to_value(params).unwrap(),
            json!({"a":[1,2],"b":{"c":null}})
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&parsed.to_json().unwrap()).unwrap(),
            serde_json::from_str::<serde_json::Value>(&s).unwrap()
        );
    }
}

#[test]
fn unknown_events_are_kept_and_non_x_events_are_refused() {
    let s = r#"{"v":"9","event":"x.nook.pin","params":{"k":1}}"#;
    let m = parse_chat_messages(s.as_bytes()).unwrap().remove(0);
    assert_eq!(
        m.event,
        E::Unknown {
            event: "x.nook.pin".into(),
            params: Map::from_iter([("k".to_owned(), json!(1))])
        }
    );
    assert!(parse_chat_messages(br#"{"v":"9","event":"y.msg.new","params":{}}"#).is_err());
    assert!(
        parse_chat_messages(br#"{"v":"9","event":"x.msg.new"}"#).is_err(),
        "params are required"
    );
}

#[test]
fn unknown_content_type_keeps_its_json_and_defaults_the_text() {
    let s = r#"{"event":"x.msg.new","params":{"content":{"type":"future","extra":1}}}"#;
    let m = parse_chat_messages(s.as_bytes()).unwrap().remove(0);
    let E::MsgNew(c) = &m.event else { panic!() };
    assert_eq!(c.content.text(), "unknown message type");
    let again = serde_json::from_slice::<serde_json::Value>(&m.to_json().unwrap()).unwrap();
    assert_eq!(
        again["params"]["content"],
        json!({"type":"future","extra":1})
    );
    assert_eq!(
        m.version_range(),
        VersionRange::single(9),
        "an absent v is 9"
    );
}

#[test]
fn a_message_in_each_content_type_round_trips() {
    for c in [
        json!({"type":"voice","text":"","duration":3}),
        json!({"type":"video","text":"x","image":"data:image/jpg;base64,/9j/4AAQ","duration":4}),
        json!({"type":"file","text":""}),
        json!({"type":"chat","text":"t","chatLink":{"type":"contact"},"ownerSig":{"a":1}}),
    ] {
        let s = json!({"event":"x.msg.new","params":{"content":c}}).to_string();
        let m = parse_chat_messages(s.as_bytes()).unwrap().remove(0);
        let out = serde_json::from_slice::<serde_json::Value>(&m.to_json().unwrap()).unwrap();
        assert_eq!(out["params"]["content"], c);
    }
}

#[test]
fn images_that_are_not_jpeg_or_png_are_cleared_on_encode() {
    assert_eq!(ImageData("data:image/jpg;base64,AA".into()).safe(), "");
    assert_eq!(
        ImageData("data:image/webp;base64,/9j/4AAQ".into()).safe(),
        ""
    );
    assert_eq!(
        ImageData("data:image/jpg;base64,/9j/4AAQ".into()).safe(),
        "data:image/jpg;base64,/9j/4AAQ"
    );
    assert_eq!(ImageData(IMG.into()).safe(), IMG);
}

#[test]
fn reactions_outside_the_whitelist_stay_unknown() {
    let parse = |s: &str| serde_json::from_str::<MsgReaction>(s).unwrap();
    assert_eq!(
        parse(r#"{"type":"emoji","emoji":"👍"}"#),
        MsgReaction::Emoji('👍')
    );
    assert_eq!(
        parse(r#"{"type":"emoji","emoji":"❤"}"#),
        MsgReaction::Emoji('\u{2764}')
    );
    assert!(matches!(
        parse(r#"{"type":"emoji","emoji":"🎉"}"#),
        MsgReaction::Unknown(_)
    ));
    assert!(matches!(
        parse(r#"{"type":"emoji","emoji":"👍👍"}"#),
        MsgReaction::Unknown(_)
    ));
    assert!(matches!(
        parse(r#"{"type":"sticker","x":1}"#),
        MsgReaction::Unknown(_)
    ));
}

#[test]
fn only_history_and_messages_are_sent_only_when_true() {
    let del = msg(E::MsgDel {
        msg_id: id(&[1]),
        member_id: None,
        scope: None,
        only_history: false,
    });
    assert!(
        !String::from_utf8(del.to_json().unwrap())
            .unwrap()
            .contains("onlyHistory")
    );
    let mem = msg(E::GrpMemDel {
        member_id: mid(),
        messages: false,
        roster_version: None,
    });
    assert!(
        !String::from_utf8(mem.to_json().unwrap())
            .unwrap()
            .contains("messages")
    );
    let lenient = br#"{"event":"x.grp.mem.del","params":{"memberId":"AQIDBA==","messages":"yes"}}"#;
    let E::GrpMemDel { messages, .. } = parse_chat_messages(lenient).unwrap().remove(0).event
    else {
        panic!()
    };
    assert!(!messages);
}

#[test]
fn x_contact_request_message_needs_both_msg_id_and_content() {
    let p = test_profile_json();
    let only_id =
        format!(r#"{{"event":"x.contact","params":{{"msgId":"AQIDBA==","profile":{p}}}}}"#);
    let E::Contact { request_msg, .. } = parse_chat_messages(only_id.as_bytes())
        .unwrap()
        .remove(0)
        .event
    else {
        panic!()
    };
    assert_eq!(request_msg, None);
}

#[test]
fn file_chunks_round_trip_as_binary_elements() {
    use zaun_chat::chat::element::{encode_file_chunk_element, parse_element};
    for chunk in [
        FileChunk::Chunk {
            chunk_no: 3,
            bytes: vec![1, 2, 3],
        },
        FileChunk::Cancel,
    ] {
        let bytes = encode_file_chunk_element(&id(b"abc"), &chunk).unwrap();
        let p = parse_element(&bytes).unwrap();
        assert_eq!(
            p.message.event,
            E::FileChunk {
                msg_id: id(b"abc"),
                chunk
            }
        );
    }
}

#[test]
fn minted_ids_are_double_encoded_like_the_reference() {
    let a = SharedMsgId::mint().unwrap();
    assert_eq!(
        a.as_bytes().len(),
        16,
        "12 random bytes as standard base64 text"
    );
    assert!(
        a.as_bytes()
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'+' || *b == b'/')
    );
    let json = serde_json::to_string(&a).unwrap();
    assert_eq!(json.len(), 24 + 2, "24 characters in JSON");
    assert!(json.ends_with("==\""));
    assert_ne!(a, SharedMsgId::mint().unwrap());
    assert_eq!(serde_json::from_str::<SharedMsgId>(&json).unwrap(), a);
    assert_eq!(MemberId::mint().unwrap().as_bytes().len(), 16);
    let call = CallId::mint().unwrap();
    assert_eq!(call.as_bytes().len(), 16);
    assert_eq!(
        serde_json::to_string(&call).unwrap().len(),
        24 + 2,
        "raw 16 bytes, padded base64url"
    );
}

#[test]
fn ids_decode_padded_and_unpadded_and_keep_the_bytes() {
    let padded: SharedMsgId = serde_json::from_str(r#""AQIDBA==""#).unwrap();
    let bare: SharedMsgId = serde_json::from_str(r#""AQIDBA""#).unwrap();
    assert_eq!(padded, bare);
    assert_eq!(padded.as_bytes(), [1, 2, 3, 4]);
    assert_eq!(serde_json::to_string(&bare).unwrap(), r#""AQIDBA==""#);
}

#[test]
fn sent_default_preferences_and_flags_match_7_1_4() {
    let p = zaun_chat::chat::profile::Profile::new("a", "");
    let v = serde_json::to_value(&p).unwrap();
    assert_eq!(
        v["preferences"],
        json!({"timedMessages":{"allow":"yes"},"fullDelete":{"allow":"no"},"reactions":{"allow":"yes"},"voice":{"allow":"yes"},"files":{"allow":"always"},"calls":{"allow":"yes"},"sessions":{"allow":"no"},"commands":[]})
    );
    assert_eq!(v["fullName"], "");
    assert!(EventKind::MsgNew.has_notification() && EventKind::MsgNew.has_delivery_receipt());
    assert!(!EventKind::MsgReact.has_notification());
    assert!(EventKind::GrpMemRole.requires_signature() && !EventKind::MsgNew.requires_signature());
    let inline = FileInvitation {
        file_inline: Some("offer".into()),
        ..FileInvitation::xftp("a", 1)
    };
    assert!(
        !E::MsgNew(MsgContainer {
            file: Some(inline),
            ..MsgContainer::text("x")
        })
        .is_forwarded_group_msg()
    );
    assert!(E::MsgNew(MsgContainer::text("x")).is_forwarded_group_msg());
    assert!(
        !E::GrpMemInv {
            member_id: mid(),
            intro: IntroInvitation {
                group_conn_req: String::new()
            }
        }
        .is_forwarded_group_msg()
    );
}

#[test]
fn unknown_preferences_survive_a_round_trip() {
    let s = r#"{"voice":{"allow":"yes"},"polls":{"allow":"yes","x":1}}"#;
    let p: zaun_chat::chat::preferences::Preferences = serde_json::from_str(s).unwrap();
    assert_eq!(
        serde_json::to_value(&p).unwrap(),
        serde_json::from_str::<serde_json::Value>(s).unwrap()
    );
}

#[test]
fn version_ranges_are_written_like_the_reference() {
    assert_eq!(
        serde_json::to_string(&VersionRange::new(9, 9)).unwrap(),
        r#""9""#
    );
    assert_eq!(
        serde_json::to_string(&SUPPORTED_CHAT_VRANGE).unwrap(),
        r#""9-22""#
    );
    assert_eq!(
        VersionRange::new(9, 22).compatible(&VersionRange::new(1, 19)),
        Some(19)
    );
    assert_eq!(
        VersionRange::new(9, 22).compatible(&VersionRange::new(1, 8)),
        None
    );
    assert!(serde_json::from_str::<VersionRange>(r#""22-9""#).is_err());
    let m = ChatMessage::new(E::Ok).unwrap();
    assert_eq!(m.v, Some(SUPPORTED_CHAT_VRANGE));
    assert!(m.msg_id.is_some());
}
