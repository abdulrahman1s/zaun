//! The codec against JSON recorded from simplex-chat 7.0.0.12 (ADR-0033, Phase 0: two
//! helpers talking through a local relay).

use std::path::{Path, PathBuf};

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
        "new_items_reply" => include_str!("../fixtures/new_items_reply.json"),
        "item_updated_event" => include_str!("../fixtures/item_updated_event.json"),
        "update_item_resp" => include_str!("../fixtures/update_item_resp.json"),
        "reaction_event" => include_str!("../fixtures/reaction_event.json"),
        "reaction_resp" => include_str!("../fixtures/reaction_resp.json"),
        "delete_item_resp" => include_str!("../fixtures/delete_item_resp.json"),
        "items_deleted_event" => include_str!("../fixtures/items_deleted_event.json"),
        "get_code_resp" => include_str!("../fixtures/get_code_resp.json"),
        "verify_code_resp" => include_str!("../fixtures/verify_code_resp.json"),
        "verify_wrong_resp" => include_str!("../fixtures/verify_wrong_resp.json"),
        "profile_resp" => include_str!("../fixtures/profile_resp.json"),
        "members_resp" => include_str!("../fixtures/members_resp.json"),
        "members_resp_bob_blocked" => include_str!("../fixtures/members_resp_bob_blocked.json"),
        "new_items_mention" => include_str!("../fixtures/new_items_mention.json"),
        "new_items_blocked_member" => include_str!("../fixtures/new_items_blocked_member.json"),
        "new_items_file" => include_str!("../fixtures/new_items_file.json"),
        "send_file_resp" => include_str!("../fixtures/send_file_resp.json"),
        "rcv_file_complete" => include_str!("../fixtures/rcv_file_complete.json"),
        "settings_mute_resp" => include_str!("../fixtures/settings_mute_resp.json"),
        "history_deleted_reacted" => include_str!("../fixtures/history_deleted_reacted.json"),
        "api_chats_muted_verified" => include_str!("../fixtures/api_chats_muted_verified.json"),
        "api_chats_group_muted" => include_str!("../fixtures/api_chats_group_muted.json"),
        "host_disconnected" => include_str!("../fixtures/host_disconnected.json"),
        "host_connected" => include_str!("../fixtures/host_connected.json"),
        "send_offline_resp" => include_str!("../fixtures/send_offline_resp.json"),
        "new_items_space_mention" => include_str!("../fixtures/new_items_space_mention.json"),
        "active_user_about" => include_str!("../fixtures/active_user_about.json"),
        "group_reaction_event" => include_str!("../fixtures/group_reaction_event.json"),
        "file_not_approved" => include_str!("../fixtures/file_not_approved.json"),
        "address_created" => include_str!("../fixtures/address_created.json"),
        "show_address" => include_str!("../fixtures/show_address.json"),
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

/// The engine's data folder in these tests.
const DIR: &str = "/home/u/.local/share/easyplay/chat";

fn codec(name: &str, relays: Vec<String>) -> Codec {
    Codec::new(name, relays, Path::new(DIR))
}

fn ok() -> Value {
    serde_json::json!({ "result": { "type": "cmdOk" } })
}

/// A codec taken through start-up with the recorded answers; returns it ready.
fn started() -> Codec {
    let mut c = codec("alice", Vec::new());
    c.on_line(r#"{"t":"ready"}"#);
    c.on_line(&resp(1, &fixture("active_user")));
    c.on_line(&resp(2, &ok()));
    let out = c.on_line(&resp(3, &fixture("chat_started")));
    assert!(c.started());
    assert_eq!(cmds(&out).len(), 2);
    c
}

/// Started, with the start-up list (4) and address (5) answered: nothing in flight.
fn settled() -> Codec {
    settled_with("api_chats")
}

/// Started, with the list answered by `list` and no address: nothing in flight.
fn settled_with(list: &str) -> Codec {
    let mut c = started();
    c.on_line(&resp(4, &fixture(list)));
    let none = serde_json::json!({ "error": { "type": "error", "errorType": { "type": "noContactLink" } } });
    let out = c.on_line(&resp(5, &none));
    assert_eq!(out.events, vec![ChatEvent::Address(None)]);
    c
}

#[test]
fn start_up_reads_the_user_then_starts_then_reads_the_list_and_address() {
    let mut c = codec("alice", Vec::new());
    let out = c.on_line(r#"{"t":"ready"}"#);
    assert_eq!(cmds(&out), vec![(1, "/user".to_owned())]);

    let out = c.on_line(&resp(1, &fixture("active_user")));
    let (corr, cmd) = cmds(&out)[0].clone();
    assert_eq!(corr, 2);
    // Files land in the Chat folder: the only folder the helper's sandbox can write.
    let paths: Value = serde_json::from_str(cmd.strip_prefix("/set file paths ").unwrap()).unwrap();
    assert_eq!(paths["appFilesFolder"], format!("{DIR}/files"));
    assert_eq!(paths["appTempFolder"], format!("{DIR}/tmp"));
    assert!(out.events.is_empty());

    let out = c.on_line(&resp(2, &ok()));
    assert_eq!(cmds(&out), vec![(3, "/_start".to_owned())]);

    let out = c.on_line(&resp(3, &fixture("chat_started")));
    let [
        ChatEvent::Features {
            features,
            reactions,
        },
        ChatEvent::Ready(profile),
    ] = out.events.as_slice()
    else {
        panic!("{:?}", out.events)
    };
    assert_eq!(profile.name, "alice");
    assert!(features.reactions && features.replies && features.edit && features.files);
    assert!(features.mute && features.block && features.verify && features.mentions);
    assert!(!features.calls && !features.presence && !features.share && !features.voice);
    assert_eq!(features.gifs, GIFS_SEND_WORKS);
    assert_eq!(reactions.as_deref().unwrap().len(), 8);
    let sent = cmds(&out);
    assert_eq!(sent[0].0, 4);
    assert!(
        sent[0].1.starts_with("/_get chats 1 count=200 {"),
        "{}",
        sent[0].1
    );
    assert_eq!(sent[1], (5, "/_show_address 1".to_owned()));
}

#[test]
fn a_first_start_creates_the_user_with_the_given_name() {
    let mut c = codec("Rin", Vec::new());
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
    let mut c = codec(
        "alice",
        vec!["smp://fp@relay.example".into(), "smp://fp2@r2".into()],
    );
    c.on_line(r#"{"t":"ready"}"#);
    c.on_line(&resp(1, &fixture("active_user")));
    c.on_line(&resp(2, &ok()));
    let out = c.on_line(&resp(3, &fixture("chat_started")));
    assert!(out.events.is_empty());
    assert_eq!(
        cmds(&out),
        vec![(4, "/smp smp://fp@relay.example smp://fp2@r2".to_owned())]
    );
    let out = c.on_line(&resp(4, &ok()));
    let ChatEvent::Features { features, .. } = &out.events[0] else {
        panic!("{:?}", out.events)
    };
    // Custom relays without a file server of their own: files would reach the preset file
    // servers, so they are off.
    assert!(!features.files);
    assert!(matches!(out.events[1], ChatEvent::Ready(_)));
    assert!(c.started());
}

#[test]
fn rejected_custom_relays_never_fall_back_to_the_presets() {
    let mut c = codec("alice", vec!["smp://bad".into()]);
    c.on_line(r#"{"t":"ready"}"#);
    c.on_line(&resp(1, &fixture("active_user")));
    c.on_line(&resp(2, &ok()));
    c.on_line(&resp(3, &fixture("chat_started")));
    let refused = serde_json::json!({ "error": { "type": "error", "errorType": { "type": "commandError" } } });
    let out = c.on_line(&resp(4, &refused));
    assert!(!c.started());
    assert!(out.lines.is_empty());
    assert!(matches!(
        out.events[0],
        ChatEvent::Failed {
            request: RequestKind::Start,
            ..
        }
    ));
}

#[test]
fn requests_made_while_starting_wait_for_the_start() {
    let mut c = codec("alice", Vec::new());
    assert!(c.request(Request::CreateInvite).lines.is_empty());
    c.on_line(r#"{"t":"ready"}"#);
    c.on_line(&resp(1, &fixture("active_user")));
    c.on_line(&resp(2, &ok()));
    let out = c.on_line(&resp(3, &fixture("chat_started")));
    assert_eq!(cmds(&out).last().unwrap().1, "/_connect 1");
}

#[test]
fn an_invitation_answer_gives_the_link() {
    let mut c = started();
    let out = c.request(Request::CreateInvite);
    let corr = cmds(&out)[0].0;
    let out = c.on_line(&resp(corr, &fixture("invitation")));
    let [ChatEvent::Invite(link)] = out.events.as_slice() else {
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
        ChatEvent::Failed {
            request: RequestKind::Connect,
            code: ErrorCode::ChtBadLink,
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
        ChatEvent::Failed {
            code: ErrorCode::ChtBadLink,
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
        ChatEvent::Failed {
            code: ErrorCode::ChtBadLink,
            ..
        }
    ));

    let corr = cmds(&c.request(Request::CreateInvite))[0].0;
    let net = serde_json::json!({ "error": { "type": "errorAgent", "agentError": { "type": "BROKER", "brokerErr": { "type": "NETWORK" } } } });
    let out = c.on_line(&resp(corr, &net));
    assert!(matches!(
        out.events[0],
        ChatEvent::Failed {
            request: RequestKind::CreateInvite,
            code: ErrorCode::ChtNoConnection,
            ..
        }
    ));
}

#[test]
fn a_received_message_is_text_from_the_friend_and_settings_items_are_hidden() {
    let mut c = started();
    let out = c.on_line(&event(&fixture("new_items_direct")));
    let [ChatEvent::Messages(m)] = out.events.as_slice() else {
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
    let [ChatEvent::Messages(m)] = out.events.as_slice() else {
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
    let [ChatEvent::Messages(m)] = out.events.as_slice() else {
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
        reply_to: None,
        mentions: Vec::new(),
    });
    let (corr, cmd) = cmds(&out)[0].clone();
    let json: Value = serde_json::from_str(cmd.strip_prefix("/_send @3 json ").unwrap()).unwrap();
    assert_eq!(json[0]["msgContent"]["text"], "hi \"there\"\nsecond line");
    assert_eq!(json[0]["msgContent"]["type"], "text");

    let out = c.on_line(&resp(corr, &fixture("send_direct_resp")));
    let [ChatEvent::Messages(m)] = out.events.as_slice() else {
        panic!("{:?}", out.events)
    };
    assert_eq!(m[0].author, MessageAuthor::Me);
    assert_eq!(m[0].body, MessageBody::Text("hello bob".into()));
    assert!(!m[0].unread);
}

#[test]
fn the_list_has_friends_groups_and_unread_counts() {
    let mut c = started();
    let out = c.on_line(&resp(4, &fixture("api_chats")));
    let ChatEvent::Chats {
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
    let mut c = started(); // corr 4 (list) and 5 (address) are in flight
    let out = c.on_line(&event(&fixture("contact_connected")));
    assert!(out.lines.is_empty(), "a list read is already in flight");
    let out = c.on_line(&event(&fixture("user_joined_group")));
    assert!(out.lines.is_empty());
    let out = c.on_line(&resp(4, &fixture("api_chats")));
    assert!(matches!(out.events[0], ChatEvent::Chats { .. }));
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
    let [ChatEvent::Updated(m)] = out.events.as_slice() else {
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
        ChatEvent::History {
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
    let mut c = codec("alice", Vec::new());
    let out = c.on_line(r#"{"t":"init_error","body":{"type":"errorNotADatabase","dbFile":"x"}}"#);
    assert!(matches!(
        out.events[0],
        ChatEvent::Failed {
            request: RequestKind::Start,
            code: ErrorCode::ChtDatabase,
            ..
        }
    ));
    let out = c.on_line(r#"{"t":"fatal","msg":"loading /x/libsimplex.so: not found"}"#);
    assert!(matches!(
        out.events[0],
        ChatEvent::Failed {
            request: RequestKind::Start,
            code: ErrorCode::ChtUnavailable,
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

// ---- replies, mentions, edits, deletions, reactions (chat model v2) ---------------------

/// The one message of an `Updated` or `Messages` event.
fn one(out: &Output) -> ChatMessage {
    match out.events.as_slice() {
        [ChatEvent::Updated(m) | ChatEvent::Messages(m)] if m.len() == 1 => m[0].clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_reply_quotes_by_item_and_a_received_reply_names_what_it_answers() {
    let mut c = started();
    let out = c.request(Request::Send {
        chat: ChatRef::Direct(3),
        text: "hi alice, replying".into(),
        reply_to: Some(8),
        mentions: vec![("ignored".into(), 1)],
    });
    let cmd = cmds(&out)[0].1.clone();
    let json: Value = serde_json::from_str(cmd.strip_prefix("/_send @3 json ").unwrap()).unwrap();
    assert_eq!(json[0]["quotedItemId"], 8);
    // Mentions are a group's: none in a direct chat.
    assert_eq!(json[0]["mentions"], serde_json::json!({}));

    let m = one(&c.on_line(&event(&fixture("new_items_reply"))));
    assert_eq!(m.body, MessageBody::Text("hi alice, replying".into()));
    assert_eq!(
        m.reply,
        Some(Quote {
            id: Some(8),
            author: MessageAuthor::Me,
            text: "hello bob".into(),
        })
    );
}

#[test]
fn mentions_go_by_member_number_and_a_mention_of_the_user_is_marked() {
    let mut c = started();
    let out = c.request(Request::Send {
        chat: ChatRef::Group(1),
        text: "@bob ready?".into(),
        reply_to: None,
        mentions: vec![("bob".into(), 2)],
    });
    let cmd = cmds(&out)[0].1.clone();
    let json: Value = serde_json::from_str(cmd.strip_prefix("/_send #1 json ").unwrap()).unwrap();
    assert_eq!(json[0]["mentions"], serde_json::json!({ "bob": 2 }));
    assert_eq!(json[0]["msgContent"]["text"], "@bob ready?");
    assert!(json[0].get("quotedItemId").is_none());

    // Bob's side: the mention names his own membership in the group.
    let m = one(&c.on_line(&event(&fixture("new_items_mention"))));
    assert_eq!(
        m.mentions,
        vec![Mention {
            name: "bob".into(),
            member: 2,
            me: true,
        }]
    );
}

#[test]
fn a_name_of_two_words_is_quoted_for_the_engine_and_unquoted_for_the_page() {
    let mut c = started();
    let out = c.request(Request::Send {
        chat: ChatRef::Group(1),
        text: "@Bob Smith quoted".into(),
        reply_to: None,
        mentions: vec![("Bob Smith".into(), 2)],
    });
    let cmd = cmds(&out)[0].1.clone();
    let json: Value = serde_json::from_str(cmd.strip_prefix("/_send #1 json ").unwrap()).unwrap();
    // The engine refuses `@Bob Smith` (invalidMention) and takes `@'Bob Smith'`.
    assert_eq!(json[0]["msgContent"]["text"], "@'Bob Smith' quoted");

    let m = one(&c.on_line(&event(&fixture("new_items_space_mention"))));
    assert_eq!(m.body, MessageBody::Text("@Bob Smith quoted".into()));
    assert_eq!(m.mentions[0].name, "Bob Smith");
    assert!(m.mentions[0].me);
}

#[test]
fn an_edit_updates_the_item_and_marks_it_edited() {
    let mut c = started();
    let out = c.request(Request::Edit {
        chat: ChatRef::Direct(3),
        item: 8,
        text: "hello bob (edited)".into(),
    });
    let (corr, cmd) = cmds(&out)[0].clone();
    let (head, json) = cmd.split_once(" json ").unwrap();
    assert_eq!(head, "/_update item @3 8");
    let json: Value = serde_json::from_str(json).unwrap();
    assert_eq!(json["msgContent"]["text"], "hello bob (edited)");

    let m = one(&c.on_line(&resp(corr, &fixture("update_item_resp"))));
    assert!(m.edited);
    assert_eq!(m.author, MessageAuthor::Me);
    // The friend's side.
    let m = one(&c.on_line(&event(&fixture("item_updated_event"))));
    assert!(m.edited);
    assert_eq!(m.body, MessageBody::Text("hello bob (edited)".into()));
    assert_eq!(m.author, MessageAuthor::Friend);
}

#[test]
fn deleting_for_everyone_broadcasts_and_both_sides_show_it_deleted() {
    let mut c = started();
    let out = c.request(Request::Delete {
        chat: ChatRef::Direct(3),
        item: 10,
    });
    let (corr, cmd) = cmds(&out)[0].clone();
    assert_eq!(cmd, "/_delete item @3 10 broadcast");
    let m = one(&c.on_line(&resp(corr, &fixture("delete_item_resp"))));
    assert_eq!((m.id, m.body), (10, MessageBody::Deleted));
    let m = one(&c.on_line(&event(&fixture("items_deleted_event"))));
    assert_eq!((m.id, m.body.clone()), (10, MessageBody::Deleted));
    assert!(!m.unread);

    // Read back later, the deleted item has no text and the reactions stay where they are.
    let corr = cmds(&c.request(Request::History {
        chat: ChatRef::Direct(3),
        before: None,
        count: 20,
        tag: 1,
    }))[0]
        .0;
    let out = c.on_line(&resp(corr, &fixture("history_deleted_reacted")));
    let [ChatEvent::History { messages, .. }] = out.events.as_slice() else {
        panic!("{:?}", out.events)
    };
    let deleted = messages.iter().find(|m| m.id == 10).unwrap();
    assert_eq!(deleted.body, MessageBody::Deleted);
    let hello = messages.iter().find(|m| m.id == 8).unwrap();
    assert!(hello.edited);
    assert_eq!(
        hello.reactions,
        vec![Reaction {
            emoji: "👍".into(),
            count: 1,
            mine: true,
            who: Vec::new(),
        }]
    );
}

#[test]
fn reactions_carry_only_the_engines_emoji_and_report_the_new_counts() {
    let mut c = started();
    let out = c.request(Request::React {
        chat: ChatRef::Direct(3),
        item: 8,
        emoji: "👍".into(),
        on: true,
    });
    let (corr, cmd) = cmds(&out)[0].clone();
    assert_eq!(cmd, r#"/_reaction @3 8 on {"emoji":"👍","type":"emoji"}"#);
    let m = one(&c.on_line(&resp(corr, &fixture("reaction_resp"))));
    assert_eq!(m.reactions[0].emoji, "👍");
    assert!(m.reactions[0].mine);
    let m = one(&c.on_line(&event(&fixture("reaction_event"))));
    assert!(!m.reactions[0].mine);
    assert_eq!(m.reactions[0].count, 1);

    // ❤️ goes without its variation selector (the engine refuses it with one) and comes
    // back in display form.
    let cmd = cmds(&c.request(Request::React {
        chat: ChatRef::Group(1),
        item: 25,
        emoji: "❤️".into(),
        on: false,
    }))[0]
        .1
        .clone();
    assert_eq!(
        cmd,
        "/_reaction #1 25 off {\"emoji\":\"\u{2764}\",\"type\":\"emoji\"}"
    );
    let m = one(&c.on_line(&event(&fixture("group_reaction_event"))));
    assert_eq!(m.reactions[0].emoji, "❤️");

    // Anything else never reaches the engine.
    let out = c.request(Request::React {
        chat: ChatRef::Direct(3),
        item: 8,
        emoji: "🎮".into(),
        on: true,
    });
    assert!(out.lines.is_empty());
    assert!(matches!(
        out.events[0],
        ChatEvent::Failed {
            request: RequestKind::React,
            ..
        }
    ));
}

// ---- mute, block, codes, profile, members ------------------------------------------------

#[test]
fn the_list_says_who_is_muted_verified_blocked_and_since_when() {
    let c_out = {
        let mut c = started();
        c.on_line(&resp(4, &fixture("api_chats_muted_verified")))
    };
    let ChatEvent::Chats { friends, chats, .. } = &c_out.events[0] else {
        panic!("{:?}", c_out.events)
    };
    let bob = friends.iter().find(|f| f.name == "bob").unwrap();
    assert!(bob.muted && bob.verified && bob.blocked);
    assert!(bob.since.is_some_and(|t| t.as_millis() > 1_700_000_000_000));
    // Blocked: reachable from the friends list, gone from the conversations.
    assert!(!chats.iter().any(|p| p.chat == ChatRef::Direct(bob.id)));

    let mut c = started();
    let out = c.on_line(&resp(4, &fixture("api_chats_group_muted")));
    let ChatEvent::Chats {
        friends,
        groups,
        chats,
        ..
    } = &out.events[0]
    else {
        panic!("{:?}", out.events)
    };
    let alice = friends.iter().find(|f| f.name == "alice").unwrap();
    assert_eq!(alice.about, "Speedrunner. Mostly Hades.");
    assert!(!alice.muted && !alice.verified && !alice.blocked);
    assert!(groups[0].muted);
    assert!(
        chats
            .iter()
            .any(|p| p.chat == ChatRef::Group(groups[0].id) && p.muted)
    );
}

#[test]
fn muting_keeps_the_other_settings_and_a_muted_group_still_hears_mentions() {
    let mut c = settled_with("api_chats_group_muted");
    let out = c.request(Request::SetMuted {
        chat: ChatRef::Direct(3),
        on: true,
    });
    let (corr, cmd) = cmds(&out)[0].clone();
    let (head, json) = cmd.rsplit_once(' ').unwrap();
    assert_eq!(head, "/_settings @3");
    let json: Value = serde_json::from_str(json).unwrap();
    assert_eq!(
        json,
        serde_json::json!({ "enableNtfs": "none", "favorite": false })
    );
    let out = c.on_line(&resp(corr, &fixture("settings_mute_resp")));
    assert!(cmds(&out)[0].1.starts_with("/_get chats"));

    let cmd = cmds(&c.request(Request::SetMuted {
        chat: ChatRef::Group(1),
        on: true,
    }))[0]
        .1
        .clone();
    assert!(cmd.contains(r#""enableNtfs":"mentions""#), "{cmd}");
    let cmd = cmds(&c.request(Request::SetMuted {
        chat: ChatRef::Group(1),
        on: false,
    }))[0]
        .1
        .clone();
    assert!(cmd.contains(r#""enableNtfs":"all""#), "{cmd}");
}

#[test]
fn a_blocked_friend_is_kept_in_local_data_and_their_new_messages_are_deleted_unseen() {
    let mut c = settled_with("api_chats_group_muted"); // alice is contact 3
    let cmd = cmds(&c.request(Request::SetBlocked {
        friend: 3,
        on: true,
    }))[0]
        .1
        .clone();
    assert_eq!(cmd, r#"/_set custom @3 {"easyplay":{"blocked":true}}"#);

    // What alice sends now is deleted on this computer and never reported.
    let out = c.on_line(&event(&fixture("new_items_reply")));
    assert!(out.events.is_empty(), "{:?}", out.events);
    assert_eq!(cmds(&out)[0].1, "/_delete item @3 9 internal");

    let cmd = cmds(&c.request(Request::SetBlocked {
        friend: 3,
        on: false,
    }))[0]
        .1
        .clone();
    assert_eq!(cmd, "/_set custom @3");
    assert!(matches!(
        c.on_line(&event(&fixture("new_items_reply"))).events[0],
        ChatEvent::Messages(_)
    ));
}

#[test]
fn a_blocked_member_is_hidden_locally_and_their_messages_collapse_without_counting() {
    let mut c = started();
    let out = c.request(Request::SetMemberBlocked {
        group: 1,
        member: 1,
        on: true,
    });
    let (corr, cmd) = cmds(&out)[0].clone();
    assert_eq!(cmd, r#"/_member settings #1 1 {"showMessages":false}"#);
    let out = c.on_line(&resp(corr, &ok()));
    let (corr, cmd) = cmds(&out)[0].clone();
    assert_eq!(cmd, "/_members #1");
    let out = c.on_line(&resp(corr, &fixture("members_resp_bob_blocked")));
    let [ChatEvent::Members { group: 1, members }] = out.events.as_slice() else {
        panic!("{:?}", out.events)
    };
    let alice = members.iter().find(|m| m.name == "alice").unwrap();
    assert!(alice.blocked && alice.owner && !alice.me);

    let m = one(&c.on_line(&event(&fixture("new_items_blocked_member"))));
    assert_eq!(m.body, MessageBody::Text("you should not see this".into()));
    assert!(
        !m.unread,
        "a blocked member's message does not count as unread"
    );
}

#[test]
fn members_list_the_user_first_with_owner_and_friend() {
    let mut c = settled();
    let (corr, cmd) = cmds(&c.request(Request::Members(1)))[0].clone();
    assert_eq!(cmd, "/_members #1");
    let out = c.on_line(&resp(corr, &fixture("members_resp")));
    let [ChatEvent::Members { members, .. }] = out.events.as_slice() else {
        panic!("{:?}", out.events)
    };
    assert_eq!(members.len(), 2);
    assert!(members[0].me && members[0].owner && members[0].name == "alice");
    assert_eq!(members[1].name, "bob");
    assert_eq!(members[1].friend, Some(3));
    assert!(!members[1].owner && !members[1].blocked);
    // A group whose members were read is read again when they change.
    let out = c.on_line(&event(&fixture("user_joined_group")));
    let sent: Vec<String> = cmds(&out).into_iter().map(|(_, c)| c).collect();
    assert!(sent[0].starts_with("/_get chats"), "{sent:?}");
    assert_eq!(sent[1], "/_members #1");
}

#[test]
fn a_security_code_is_shown_and_a_match_verifies_the_friend() {
    let mut c = started();
    let (corr, cmd) = cmds(&c.request(Request::ShowCode(3)))[0].clone();
    assert_eq!(cmd, "/_get code @3");
    let out = c.on_line(&resp(corr, &fixture("get_code_resp")));
    let [ChatEvent::Code { friend: 3, code }] = out.events.as_slice() else {
        panic!("{:?}", out.events)
    };
    assert!(code.starts_with("31318 25417"), "{code}");

    let (corr, cmd) = cmds(&c.request(Request::Verify {
        friend: 3,
        code: format!(" {code}\n"),
    }))[0]
        .clone();
    assert_eq!(cmd, format!("/_verify code @3 {code}"));
    let out = c.on_line(&resp(corr, &fixture("verify_code_resp")));
    assert_eq!(
        out.events[0],
        ChatEvent::Verified {
            friend: 3,
            matched: true
        }
    );
    let corr = cmds(&c.request(Request::Verify {
        friend: 3,
        code: "12345 67890".into(),
    }))[0]
        .0;
    let out = c.on_line(&resp(corr, &fixture("verify_wrong_resp")));
    assert_eq!(
        out.events[0],
        ChatEvent::Verified {
            friend: 3,
            matched: false
        }
    );
}

#[test]
fn the_profile_carries_the_line_about_the_user_and_keeps_the_rest() {
    let mut c = codec("alice", Vec::new());
    c.on_line(r#"{"t":"ready"}"#);
    c.on_line(&resp(1, &fixture("active_user_about")));
    c.on_line(&resp(2, &ok()));
    let out = c.on_line(&resp(3, &fixture("chat_started")));
    let ChatEvent::Ready(p) = &out.events[1] else {
        panic!("{:?}", out.events)
    };
    assert_eq!(p.about, "Mostly co-op.");

    let out = c.request(Request::SetProfile(ChatProfile {
        name: "alice".into(),
        about: "Speedrunner. Mostly Hades.".into(),
        ..ChatProfile::default()
    }));
    let (corr, cmd) = cmds(&out).last().unwrap().clone();
    let json: Value = serde_json::from_str(cmd.strip_prefix("/_profile 1 ").unwrap()).unwrap();
    assert_eq!(json["displayName"], "alice");
    assert_eq!(json["shortDescr"], "Speedrunner. Mostly Hades.");
    assert!(json.get("profileId").is_none() && json.get("localAlias").is_none());
    let out = c.on_line(&resp(corr, &fixture("profile_resp")));
    let [ChatEvent::Profile(p)] = out.events.as_slice() else {
        panic!("{:?}", out.events)
    };
    assert_eq!(p.about, "Speedrunner. Mostly Hades.");

    // An empty line removes it.
    let cmd = cmds(&c.request(Request::SetProfile(ChatProfile {
        name: "alice".into(),
        ..ChatProfile::default()
    })))[0]
        .1
        .clone();
    assert!(!cmd.contains("shortDescr"), "{cmd}");
}

// ---- files and connectivity ---------------------------------------------------------------

#[test]
fn a_sent_picture_goes_as_an_image_with_its_file_and_shows_the_local_copy() {
    let mut c = started();
    let path = PathBuf::from(format!("{DIR}/files/party.gif"));
    let out = c.request(Request::SendFile {
        chat: ChatRef::Direct(3),
        path: path.clone(),
        kind: FileKind::Gif,
    });
    let (corr, cmd) = cmds(&out)[0].clone();
    let json: Value = serde_json::from_str(cmd.strip_prefix("/_send @3 json ").unwrap()).unwrap();
    assert_eq!(json[0]["fileSource"]["filePath"], path.to_str().unwrap());
    assert_eq!(json[0]["msgContent"]["type"], "image");
    let m = one(&c.on_line(&resp(corr, &fixture("send_file_resp"))));
    assert_eq!(
        m.body,
        MessageBody::Gif(path.to_string_lossy().into_owned())
    );
}

#[test]
fn a_small_received_picture_is_fetched_and_shows_its_file_once_here() {
    let mut c = started();
    let out = c.on_line(&event(&fixture("new_items_file")));
    let m = one(&out);
    assert_eq!(m.body, MessageBody::Gif("party.gif".into()));
    assert_eq!(cmds(&out)[0].1, "/freceive 1 encrypt=off");
    let m = one(&c.on_line(&event(&fixture("rcv_file_complete"))));
    assert_eq!(m.body, MessageBody::Gif(format!("{DIR}/files/party.gif")));

    // A file server the user's list does not know is refused by the engine: nothing breaks.
    let out = c.on_line(&event(&fixture("file_not_approved")));
    assert!(out.events.is_empty() && out.lines.is_empty());
}

#[test]
fn a_large_picture_or_files_turned_off_fetch_nothing() {
    let mut big = fixture("new_items_file");
    big["result"]["chatItems"][0]["chatItem"]["file"]["fileSize"] = (2 * 1024 * 1024).into();
    let mut c = started();
    assert!(c.on_line(&event(&big)).lines.is_empty());

    let mut c = codec("alice", vec!["smp://fp@relay.example".into()]);
    c.on_line(r#"{"t":"ready"}"#);
    c.on_line(&resp(1, &fixture("active_user")));
    c.on_line(&resp(2, &ok()));
    c.on_line(&resp(3, &fixture("chat_started")));
    c.on_line(&resp(4, &ok()));
    assert!(c.started());
    assert!(
        c.on_line(&event(&fixture("new_items_file")))
            .lines
            .is_empty()
    );
    let out = c.request(Request::SendFile {
        chat: ChatRef::Direct(3),
        path: PathBuf::from(format!("{DIR}/files/x.gif")),
        kind: FileKind::Gif,
    });
    assert!(out.lines.is_empty());
    assert!(matches!(
        out.events[0],
        ChatEvent::Failed {
            request: RequestKind::SendFile,
            ..
        }
    ));
}

#[test]
fn custom_file_servers_go_to_the_engine_after_the_relays_and_turn_files_on() {
    let mut c = codec(
        "alice",
        vec![
            "smp://fp@relay.example".into(),
            "xftp://fp@files.example".into(),
        ],
    );
    c.on_line(r#"{"t":"ready"}"#);
    c.on_line(&resp(1, &fixture("active_user")));
    c.on_line(&resp(2, &ok()));
    let out = c.on_line(&resp(3, &fixture("chat_started")));
    assert_eq!(cmds(&out)[0].1, "/smp smp://fp@relay.example");
    let out = c.on_line(&resp(4, &ok()));
    assert_eq!(
        cmds(&out),
        vec![(5, "/xftp xftp://fp@files.example".to_owned())]
    );
    let out = c.on_line(&resp(5, &ok()));
    let ChatEvent::Features { features, .. } = &out.events[0] else {
        panic!("{:?}", out.events)
    };
    assert!(features.files);
}

#[test]
fn the_relays_going_away_and_coming_back_is_reported_once_each() {
    let mut c = started();
    let out = c.on_line(&event(&fixture("host_connected")));
    assert!(out.events.is_empty(), "online from the start");
    let out = c.on_line(&event(&fixture("host_disconnected")));
    assert_eq!(out.events, vec![ChatEvent::Offline(true)]);
    assert!(
        c.on_line(&event(&fixture("host_disconnected")))
            .events
            .is_empty()
    );
    // A message written meanwhile waits.
    let corr = cmds(&c.request(Request::Send {
        chat: ChatRef::Direct(3),
        text: "while offline".into(),
        reply_to: None,
        mentions: Vec::new(),
    }))[0]
        .0;
    let m = one(&c.on_line(&resp(corr, &fixture("send_offline_resp"))));
    assert_eq!(m.delivery, Delivery::Sending);
    let out = c.on_line(&event(&fixture("host_connected")));
    assert_eq!(out.events, vec![ChatEvent::Offline(false)]);

    // One relay of several down is not offline.
    let other = serde_json::json!({ "result": { "type": "hostConnected", "protocol": "smp", "transportHost": "relay2.example" } });
    c.on_line(&event(&other));
    assert!(
        c.on_line(&event(&fixture("host_disconnected")))
            .events
            .is_empty()
    );
}

// ---- short links (a QR code that scans at the page's size) -------------------------------

#[test]
fn an_invitation_is_shared_as_its_short_link_and_falls_back_to_the_full_one() {
    let mut c = started();
    let corr = cmds(&c.request(Request::CreateInvite))[0].0;
    let out = c.on_line(&resp(corr, &fixture("invitation")));
    let [ChatEvent::Invite(link)] = out.events.as_slice() else {
        panic!("{:?}", out.events)
    };
    let short = fixture("invitation")["result"]["connLinkInvitation"]["connShortLink"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(*link, short);
    assert!(
        link.starts_with("https://") && link.contains("/i#"),
        "{link}"
    );
    assert!(link.len() < 160, "{} characters", link.len());

    let mut full_only = fixture("invitation");
    full_only["result"]["connLinkInvitation"]["connShortLink"] = Value::Null;
    let corr = cmds(&c.request(Request::CreateInvite))[0].0;
    let out = c.on_line(&resp(corr, &full_only));
    let [ChatEvent::Invite(link)] = out.events.as_slice() else {
        panic!("{:?}", out.events)
    };
    assert!(link.starts_with("simplex:/invitation#"), "{link}");
}

#[test]
fn the_address_is_shared_as_its_short_link_when_made_and_when_read_back() {
    let mut c = settled();
    let (corr, cmd) = cmds(&c.request(Request::CreateAddress))[0].clone();
    assert_eq!(cmd, "/_address 1");
    let out = c.on_line(&resp(corr, &fixture("address_created")));
    let [ChatEvent::Address(Some(made))] = out.events.as_slice() else {
        panic!("{:?}", out.events)
    };
    assert!(
        made.starts_with("https://") && made.contains("/a#"),
        "{made}"
    );
    assert!(made.len() < 120, "{} characters", made.len());

    // Start-up reads it back (corr 5 of `started`).
    let mut c = started();
    c.on_line(&resp(4, &fixture("api_chats")));
    let out = c.on_line(&resp(5, &fixture("show_address")));
    assert_eq!(out.events, vec![ChatEvent::Address(Some(made.clone()))]);
}
