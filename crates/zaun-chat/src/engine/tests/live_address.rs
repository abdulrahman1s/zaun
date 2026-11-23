//! Addresses against the official CLI 7.1.4: each side's address used by the other, and a deleted
//! address that stops working. Same servers and variables as `live_cli`.

use serde_json::Value;
use zaun_chat_wire::model::{ChatRef, MessageAuthor, MessageBody};
use zaun_chat_wire::{Event, Request};

use super::Dir;
use super::live_cli::{Native, engine_msg, until, var};
use crate::agent::tests::cli::{Cli, connected, kind, received_text};

fn ready_bobs(n: &Native) -> Vec<i64> {
    let f = n.e.state.friends.values();
    f.filter(|f| f.name == "bob" && f.ready)
        .map(|f| f.id)
        .collect()
}

#[test]
#[ignore = "needs a local smp-server, xftp-server and CLI; see live_cli"]
fn the_engine_and_the_official_cli_connect_through_each_others_address() {
    let dir = Dir::new();
    let mut n = Native::start(&dir);
    let mut c = Cli::open(var("ZAUN_TEST_CLI").parse().unwrap());

    // The CLI asks through the engine's address; the user accepts.
    n.req(Request::CreateAddress);
    let link = until(&mut n, &mut c, "the address", |n, _| {
        n.events.iter().find_map(|e| match e {
            Event::Address(Some(l)) => Some(l.clone()),
            _ => None,
        })
    });
    let r = c.cmd(&format!("/connect {link}"));
    assert!(!kind(&r).contains("Error"), "{r}");
    let request = until(&mut n, &mut c, "bob's request", |n, _| {
        n.events.iter().rev().find_map(|e| match e {
            Event::Chats { requests, .. } => {
                requests.iter().find(|r| r.name == "bob").map(|r| r.id)
            }
            _ => None,
        })
    });
    n.req(Request::Accept(request));
    let cid = until(&mut n, &mut c, "the CLI's contact", |_, c| {
        c.take(|e| connected(e, "alice"))
    });
    let first = until(&mut n, &mut c, "the engine's friend", |n, _| {
        ready_bobs(n).first().copied()
    });
    n.req(Request::Send {
        chat: ChatRef::Direct(first),
        text: "through my address".into(),
        reply_to: None,
        mentions: Vec::new(),
    });
    until(&mut n, &mut c, "bob reads it", |_, c| {
        c.take(|e| received_text(e, cid).filter(|t| t == "through my address"))
    });

    // A deleted address stops working.
    n.req(Request::DeleteAddress);
    until(&mut n, &mut c, "the address gone", |n, _| {
        n.events
            .iter()
            .any(|e| matches!(e, Event::Address(None)))
            .then_some(())
    });
    let r = c.cmd(&format!("/connect {link}"));
    assert!(
        kind(&r).contains("Error") || kind(&r).contains("error"),
        "{r}"
    );

    // The engine asks through the CLI's address; the CLI accepts.
    let r = c.cmd("/_address 1");
    assert_eq!(kind(&r), "userContactLinkCreated", "{r}");
    let theirs = r
        .pointer("/connLinkContact/connShortLink")
        .or_else(|| r.pointer("/connLinkContact/connFullLink"))
        .and_then(Value::as_str)
        .unwrap()
        .to_owned();
    n.req(Request::Connect(theirs));
    let req_id = until(&mut n, &mut c, "the CLI's request", |_, c| {
        c.take(|e| {
            (kind(e) == "receivedContactRequest")
                .then(|| e.pointer("/contactRequest/contactRequestId")?.as_i64())?
        })
    });
    let r = c.cmd(&format!("/_accept {req_id}"));
    assert!(!kind(&r).contains("Error"), "{r}");
    let cid2 = until(&mut n, &mut c, "the CLI's second contact", |_, c| {
        c.take(|e| connected(e, "alice"))
    });
    let second = until(&mut n, &mut c, "the engine's second friend", |n, _| {
        ready_bobs(n).into_iter().find(|&f| f != first)
    });
    let r = c.cmd(&format!(
        "/_send @{cid2} json {}",
        serde_json::json!([{"msgContent": {"type": "text", "text": "through mine"}}])
    ));
    assert_eq!(kind(&r), "newChatItems", "{r}");
    until(&mut n, &mut c, "alice reads it", |n, _| {
        engine_msg(n, |m| {
            m.chat == ChatRef::Direct(second)
                && m.author == MessageAuthor::Friend
                && m.body == MessageBody::Text("through mine".into())
        })
    });
    let r = c.cmd("/_delete_address 1");
    assert!(!kind(&r).contains("Error"), "{r}");
}
