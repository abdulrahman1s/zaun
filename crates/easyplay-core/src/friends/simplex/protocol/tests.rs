//! The codec against JSON recorded from simplex-chat 7.0.0.12 (ADR-0033, Phase 0: two
//! helpers talking through a local relay).

use serde_json::Value;

use super::*;

fn fixture(name: &str) -> Value {
    let text = match name {
        "active_user" => include_str!("../fixtures/active_user.json"),
        "chat_started" => include_str!("../fixtures/chat_started.json"),
        "invitation" => include_str!("../fixtures/invitation.json"),
        "sent_confirmation" => include_str!("../fixtures/sent_confirmation.json"),
        "contact_connected" => include_str!("../fixtures/contact_connected.json"),
        "group_created" => include_str!("../fixtures/group_created.json"),
        "received_group_invitation" => include_str!("../fixtures/received_group_invitation.json"),
        "user_joined_group" => include_str!("../fixtures/user_joined_group.json"),
        "api_chats" => include_str!("../fixtures/api_chats.json"),
        "call_invitation" => include_str!("../fixtures/call_invitation.json"),
        "statuses_updated" => include_str!("../fixtures/statuses_updated.json"),
        "new_items_direct" => include_str!("../fixtures/new_items_direct.json"),
        "new_items_group" => include_str!("../fixtures/new_items_group.json"),
        "new_items_arabic" => include_str!("../fixtures/new_items_arabic.json"),
        "send_direct_resp" => include_str!("../fixtures/send_direct_resp.json"),
        other => panic!("no fixture {other}"),
    };
    serde_json::from_str(text).unwrap()
}

fn resp(corr: u64, body: &Value) -> String {
    serde_json::json!({ "t": "resp", "corr": corr, "body": body }).to_string()
}

fn event(body: &Value) -> String {
    serde_json::json!({ "t": "event", "body": body }).to_string()
}

/// The commands of `out`, as (corr, command).
fn cmds(out: &Output) -> Vec<(u64, String)> {
    out.lines
        .iter()
        .map(|l| {
            let v: Value = serde_json::from_str(l).unwrap();
            (
                v["corr"].as_u64().unwrap(),
                v["cmd"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

/// A codec taken through start-up with the recorded answers; returns it ready.
fn started() -> Codec {
    let mut c = Codec::new("alice", Vec::new());
    c.on_line(r#"{"t":"ready"}"#);
    c.on_line(&resp(1, &fixture("active_user")));
    let out = c.on_line(&resp(2, &fixture("chat_started")));
    assert!(c.started());
    assert_eq!(cmds(&out).len(), 2);
    c
}

/// Started, with the start-up list (3) and address (4) answered: nothing in flight.
fn settled() -> Codec {
    let mut c = started();
    c.on_line(&resp(3, &fixture("api_chats")));
    let none = serde_json::json!({ "error": { "type": "error", "errorType": { "type": "noContactLink" } } });
    let out = c.on_line(&resp(4, &none));
    assert_eq!(out.events, vec![FriendsEvent::Address(None)]);
    c
}

#[test]
fn start_up_reads_the_user_then_starts_then_reads_the_list_and_address() {
    let mut c = Codec::new("alice", Vec::new());
    let out = c.on_line(r#"{"t":"ready"}"#);
    assert_eq!(cmds(&out), vec![(1, "/user".to_owned())]);

    let out = c.on_line(&resp(1, &fixture("active_user")));
    assert_eq!(cmds(&out), vec![(2, "/_start".to_owned())]);
    assert!(out.events.is_empty());

    let out = c.on_line(&resp(2, &fixture("chat_started")));
    assert_eq!(
        out.events,
        vec![FriendsEvent::Ready(FriendProfile {
            name: "alice".into()
        })]
    );
    let sent = cmds(&out);
    assert_eq!(sent[0].0, 3);
    assert!(
        sent[0].1.starts_with("/_get chats 1 count=200 {"),
        "{}",
        sent[0].1
    );
    assert_eq!(sent[1], (4, "/_show_address 1".to_owned()));
}

#[test]
fn a_first_start_creates_the_user_with_the_given_name() {
    let mut c = Codec::new("Rin", Vec::new());
    c.on_line(r#"{"t":"ready"}"#);
    let no_user = serde_json::json!({ "error": { "type": "error", "errorType": { "type": "noActiveUser" } } });
    let out = c.on_line(&resp(1, &no_user));
    let (corr, cmd) = &cmds(&out)[0];
    assert_eq!(*corr, 2);
    let json: Value = serde_json::from_str(cmd.strip_prefix("/_create user ").unwrap()).unwrap();
    assert_eq!(json["profile"]["displayName"], "Rin");
    assert_eq!(json["pastTimestamp"], false);
}

#[test]
fn custom_relays_are_set_after_the_start_and_before_ready() {
    let mut c = Codec::new(
        "alice",
        vec!["smp://fp@relay.example".into(), "smp://fp2@r2".into()],
    );
    c.on_line(r#"{"t":"ready"}"#);
    c.on_line(&resp(1, &fixture("active_user")));
    let out = c.on_line(&resp(2, &fixture("chat_started")));
    assert!(out.events.is_empty());
    assert_eq!(
        cmds(&out),
        vec![(3, "/smp smp://fp@relay.example smp://fp2@r2".to_owned())]
    );
    let ok = serde_json::json!({ "result": { "type": "cmdOk" } });
    let out = c.on_line(&resp(3, &ok));
    assert!(matches!(out.events[0], FriendsEvent::Ready(_)));
    assert!(c.started());
}

#[test]
fn rejected_custom_relays_never_fall_back_to_the_presets() {
    let mut c = Codec::new("alice", vec!["smp://bad".into()]);
    c.on_line(r#"{"t":"ready"}"#);
    c.on_line(&resp(1, &fixture("active_user")));
    c.on_line(&resp(2, &fixture("chat_started")));
    let refused = serde_json::json!({ "error": { "type": "error", "errorType": { "type": "commandError" } } });
    let out = c.on_line(&resp(3, &refused));
    assert!(!c.started());
    assert!(out.lines.is_empty());
    assert!(matches!(
        out.events[0],
        FriendsEvent::Failed { request: RequestKind::Start, .. }
    ));
}

#[test]
fn requests_made_while_starting_wait_for_the_start() {
    let mut c = Codec::new("alice", Vec::new());
    assert!(c.request(Request::CreateInvite).lines.is_empty());
    c.on_line(r#"{"t":"ready"}"#);
    c.on_line(&resp(1, &fixture("active_user")));
    let out = c.on_line(&resp(2, &fixture("chat_started")));
    assert_eq!(cmds(&out).last().unwrap().1, "/_connect 1");
}

#[test]
fn an_invitation_answer_gives_the_link() {
    let mut c = started();
    let out = c.request(Request::CreateInvite);
    let corr = cmds(&out)[0].0;
    let out = c.on_line(&resp(corr, &fixture("invitation")));
    let [FriendsEvent::Invite(link)] = out.events.as_slice() else {
        panic!("{:?}", out.events)
    };
    assert!(
        link.starts_with("simplex:/invitation#") || link.starts_with("https://"),
        "{link}"
    );
}

#[test]
fn connecting_reads_the_list_again_and_a_bad_link_never_reaches_the_engine() {
    let mut c = settled();
    let out = c.request(Request::Connect("https://relay.example/i#abc".into()));
    let (corr, cmd) = cmds(&out)[0].clone();
    assert_eq!(cmd, "/_connect plan 1 https://relay.example/i#abc");
    let plan = serde_json::json!({ "result": {
        "type": "connectionPlan",
        "connLink": { "connFullLink": "simplex:/invitation#/?v=2-7", "connShortLink": "https://relay.example/i#abc" },
        "connectionPlan": { "type": "invitationLink", "invitationLinkPlan": { "type": "ok" } },
    }});
    let out = c.on_line(&resp(corr, &plan));
    let (corr, cmd) = cmds(&out)[0].clone();
    assert_eq!(
        cmd,
        "/_connect 1 simplex:/invitation#/?v=2-7 https://relay.example/i#abc"
    );
    let out = c.on_line(&resp(corr, &fixture("sent_confirmation")));
    assert!(cmds(&out)[0].1.starts_with("/_get chats"));

    let out = c.request(Request::Connect("not a link".into()));
    assert!(out.lines.is_empty());
    assert!(matches!(
        out.events[0],
        FriendsEvent::Failed {
            request: RequestKind::Connect,
            code: ErrorCode::FrnBadLink,
            ..
        }
    ));
}

#[test]
fn the_users_own_link_and_a_known_one_connect_nothing() {
    let mut c = settled();
    let plan = |sub: &str| {
        serde_json::json!({ "result": {
            "type": "connectionPlan",
            "connLink": { "connFullLink": "simplex:/contact#/x" },
            "connectionPlan": { "type": "contactAddress", "contactAddressPlan": { "type": sub } },
        }})
    };
    let corr = cmds(&c.request(Request::Connect("simplex:/contact#/x".into())))[0].0;
    let out = c.on_line(&resp(corr, &plan("ownLink")));
    assert!(out.lines.is_empty());
    assert!(matches!(
        out.events[0],
        FriendsEvent::Failed {
            code: ErrorCode::FrnBadLink,
            ..
        }
    ));

    let corr = cmds(&c.request(Request::Connect("simplex:/contact#/x".into())))[0].0;
    let out = c.on_line(&resp(corr, &plan("known")));
    assert!(cmds(&out)[0].1.starts_with("/_get chats"));
}

#[test]
fn an_engine_error_for_a_link_is_a_bad_link_and_a_network_error_is_no_connection() {
    let mut c = started();
    let corr = cmds(&c.request(Request::Connect("simplex:/x".into())))[0].0;
    let bad = serde_json::json!({ "error": { "type": "error", "errorType": { "type": "invalidConnReq" } } });
    let out = c.on_line(&resp(corr, &bad));
    assert!(matches!(
        out.events[0],
        FriendsEvent::Failed {
            code: ErrorCode::FrnBadLink,
            ..
        }
    ));

    let corr = cmds(&c.request(Request::CreateInvite))[0].0;
    let net = serde_json::json!({ "error": { "type": "errorAgent", "agentError": { "type": "BROKER", "brokerErr": { "type": "NETWORK" } } } });
    let out = c.on_line(&resp(corr, &net));
    assert!(matches!(
        out.events[0],
        FriendsEvent::Failed {
            request: RequestKind::CreateInvite,
            code: ErrorCode::FrnNoConnection,
            ..
        }
    ));
}

#[test]
fn a_received_message_is_text_from_the_friend_and_settings_items_are_hidden() {
    let mut c = started();
    let out = c.on_line(&event(&fixture("new_items_direct")));
    let [FriendsEvent::Messages(m)] = out.events.as_slice() else {
        panic!("{:?}", out.events)
    };
    let hello = m
        .iter()
        .find(|m| m.body == MessageBody::Text("hello bob".into()))
        .unwrap();
    assert_eq!(hello.author, MessageAuthor::Friend);
    assert!(matches!(hello.chat, ChatRef::Direct(_)));
    assert_eq!(hello.delivery, Delivery::Received);
    assert!(hello.unread);
    assert!(hello.at.as_millis() > 1_700_000_000_000);
}

#[test]
fn arabic_text_arrives_unchanged() {
    let mut c = started();
    let out = c.on_line(&event(&fixture("new_items_arabic")));
    let [FriendsEvent::Messages(m)] = out.events.as_slice() else {
        panic!("{:?}", out.events)
    };
    assert!(
        m.iter()
            .any(|m| m.body == MessageBody::Text("مرحبا alice".into()))
    );
}

#[test]
fn a_group_message_names_its_author() {
    let mut c = started();
    let out = c.on_line(&event(&fixture("new_items_group")));
    let [FriendsEvent::Messages(m)] = out.events.as_slice() else {
        panic!("{:?}", out.events)
    };
    let gg = m
        .iter()
        .find(|m| m.body == MessageBody::Text("gg in the group".into()))
        .unwrap();
    assert_eq!(gg.author, MessageAuthor::Member("alice".into()));
    assert!(matches!(gg.chat, ChatRef::Group(_)));
}

#[test]
fn sending_renders_the_message_as_json_and_the_answer_is_the_sent_message() {
    let mut c = started();
    let out = c.request(Request::Send {
        chat: ChatRef::Direct(3),
        text: "hi \"there\"\nsecond line".into(),
    });
    let (corr, cmd) = cmds(&out)[0].clone();
    let json: Value = serde_json::from_str(cmd.strip_prefix("/_send @3 json ").unwrap()).unwrap();
    assert_eq!(json[0]["msgContent"]["text"], "hi \"there\"\nsecond line");
    assert_eq!(json[0]["msgContent"]["type"], "text");

    let out = c.on_line(&resp(corr, &fixture("send_direct_resp")));
    let [FriendsEvent::Messages(m)] = out.events.as_slice() else {
        panic!("{:?}", out.events)
    };
    assert_eq!(m[0].author, MessageAuthor::Me);
    assert_eq!(m[0].body, MessageBody::Text("hello bob".into()));
    assert!(!m[0].unread);
}

#[test]
fn the_list_has_friends_groups_and_unread_counts() {
    let mut c = started();
    let out = c.on_line(&resp(3, &fixture("api_chats")));
    let FriendsEvent::Chats {
        friends,
        groups,
        requests,
        chats,
    } = &out.events[0]
    else {
        panic!("{:?}", out.events)
    };
    assert!(
        friends.iter().any(|f| f.name == "alice" && f.ready),
        "{friends:?}"
    );
    // The engine's built-in, never connected contact is not a friend.
    assert_eq!(friends.len(), 1, "{friends:?}");
    let squad = groups.iter().find(|g| g.name == "squad").unwrap();
    assert_eq!(squad.membership, Membership::Member);
    assert!(requests.is_empty());
    // The notes-to-self chat is not a conversation the page shows.
    assert!(
        chats
            .iter()
            .all(|c| matches!(c.chat, ChatRef::Direct(_) | ChatRef::Group(_)))
    );
    let direct = chats
        .iter()
        .find(|p| p.name == "alice" && matches!(p.chat, ChatRef::Direct(_)))
        .unwrap();
    assert_eq!(direct.unread, 3);
    let group = chats.iter().find(|p| p.name == "squad").unwrap();
    assert_eq!(group.unread, 1);
}

#[test]
fn changes_to_who_the_user_talks_to_read_the_list_once_more_at_most() {
    let mut c = started(); // corr 3 (list) and 4 (address) are in flight
    let out = c.on_line(&event(&fixture("contact_connected")));
    assert!(out.lines.is_empty(), "a list read is already in flight");
    let out = c.on_line(&event(&fixture("user_joined_group")));
    assert!(out.lines.is_empty());
    let out = c.on_line(&resp(3, &fixture("api_chats")));
    assert!(matches!(out.events[0], FriendsEvent::Chats { .. }));
    assert_eq!(cmds(&out).len(), 1, "one more read for both changes");
    assert!(cmds(&out)[0].1.starts_with("/_get chats"));
}

#[test]
fn a_call_invitation_is_not_a_message_yet() {
    let mut c = started();
    let out = c.on_line(&event(&fixture("call_invitation")));
    assert!(out.events.is_empty() && out.lines.is_empty());
}

#[test]
fn delivery_updates_are_reported_as_updates() {
    let mut c = started();
    let out = c.on_line(&event(&fixture("statuses_updated")));
    let [FriendsEvent::Updated(m)] = out.events.as_slice() else {
        panic!("{:?}", out.events)
    };
    assert!(m.iter().all(|m| m.author == MessageAuthor::Me));
    assert!(
        m.iter()
            .any(|m| matches!(m.delivery, Delivery::Sent | Delivery::Delivered))
    );
}

#[test]
fn history_asks_for_a_page_and_returns_its_messages_oldest_first() {
    let mut c = started();
    let out = c.request(Request::History {
        chat: ChatRef::Direct(3),
        before: Some(40),
        count: 50,
        tag: 7,
    });
    let (corr, cmd) = cmds(&out)[0].clone();
    assert_eq!(cmd, "/_get chat @3 before=40 count=50");
    let items: Vec<Value> = fixture("new_items_direct")["result"]["chatItems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["chatItem"].clone())
        .collect();
    let body =
        serde_json::json!({ "result": { "type": "apiChat", "chat": { "chatItems": items } } });
    let out = c.on_line(&resp(corr, &body));
    let [
        FriendsEvent::History {
            tag: 7,
            chat: ChatRef::Direct(3),
            messages,
        },
    ] = out.events.as_slice()
    else {
        panic!("{:?}", out.events)
    };
    assert!(messages.windows(2).all(|w| w[0].id < w[1].id));
}

#[test]
fn a_database_that_does_not_open_and_a_missing_library_are_reported() {
    let mut c = Codec::new("alice", Vec::new());
    let out = c.on_line(r#"{"t":"init_error","body":{"type":"errorNotADatabase","dbFile":"x"}}"#);
    assert!(matches!(
        out.events[0],
        FriendsEvent::Failed {
            request: RequestKind::Start,
            code: ErrorCode::FrnDatabase,
            ..
        }
    ));
    let out = c.on_line(r#"{"t":"fatal","msg":"loading /x/libsimplex.so: not found"}"#);
    assert!(matches!(
        out.events[0],
        FriendsEvent::Failed {
            request: RequestKind::Start,
            code: ErrorCode::FrnUnavailable,
            ..
        }
    ));
}

#[test]
fn lines_that_are_not_json_or_answer_nothing_are_ignored() {
    let mut c = started();
    assert_eq!(c.on_line("garbage"), Output::default());
    assert_eq!(
        c.on_line(&resp(999, &fixture("chat_started"))),
        Output::default()
    );
    assert_eq!(
        c.on_line(r#"{"t":"bad_request","msg":"x"}"#),
        Output::default()
    );
}
