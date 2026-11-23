use zaun_chat_wire::model::*;
use zaun_chat_wire::{EvLine, Event, FileKind, ReqLine, RequestKind, decode_line, encode_line};

use crate::samples;

#[test]
fn every_request_survives_a_json_line() {
    for req in samples::requests() {
        let line = encode_line(&ReqLine { req: req.clone() }).unwrap();
        assert!(!line.contains('\n'), "{line}");
        let back: ReqLine = decode_line(&line).unwrap();
        assert_eq!(back.req, req, "{line}");
    }
}

#[test]
fn every_event_survives_a_json_line() {
    for ev in samples::events() {
        let line = encode_line(&EvLine { ev: ev.clone() }).unwrap();
        assert!(!line.contains('\n'), "{line}");
        let back: EvLine = decode_line(&line).unwrap();
        assert_eq!(back.ev, ev, "{line}");
    }
}

#[test]
fn every_message_body_notice_and_file_state_survives_a_json_line() {
    for body in samples::bodies() {
        let msg = samples::message(body);
        let back: ChatMessage = decode_line(&encode_line(&msg).unwrap()).unwrap();
        assert_eq!(back, msg);
    }
    for notice in samples::notices() {
        let back: ChatNotice = decode_line(&encode_line(&notice).unwrap()).unwrap();
        assert_eq!(back, notice);
    }
    for state in samples::states() {
        let back: FileState = decode_line(&encode_line(&state).unwrap()).unwrap();
        assert_eq!(back, state);
    }
}

#[test]
fn every_request_kind_and_file_kind_survives_a_json_line() {
    use RequestKind::*;
    let kinds = [
        Start,
        SetProfile,
        CreateInvite,
        Connect,
        Answer,
        Address,
        Send,
        MarkRead,
        History,
        Refresh,
        Group,
        RemoveFriend,
        SendFile,
        Edit,
        Delete,
        React,
        Mute,
        Block,
        Code,
        Members,
        File,
        GroupLink,
        Call,
    ];
    for kind in kinds {
        let back: RequestKind = decode_line(&encode_line(&kind).unwrap()).unwrap();
        assert_eq!(back, kind);
    }
    for kind in [
        FileKind::Image,
        FileKind::Gif,
        FileKind::Video,
        FileKind::File,
    ] {
        let back: FileKind = decode_line(&encode_line(&kind).unwrap()).unwrap();
        assert_eq!(back, kind);
    }
}

#[test]
fn picture_bytes_travel_as_base64_text() {
    let line = encode_line(&samples::picture()).unwrap();
    assert_eq!(line, r#"{"bytes":"/9gAAQI=","mime":"image/jpeg"}"#);
}

#[test]
fn failed_carries_the_stable_code_string() {
    let ev = Event::Failed {
        request: RequestKind::Send,
        code: "ZN-CHT-005".into(),
        detail: "d".into(),
    };
    let line = encode_line(&EvLine { ev }).unwrap();
    assert_eq!(
        line,
        r#"{"ev":{"t":"failed","c":{"request":"send","code":"ZN-CHT-005","detail":"d"}}}"#
    );
}
